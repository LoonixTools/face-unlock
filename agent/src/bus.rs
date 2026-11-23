//! What the agent hears and says on D-Bus: logind (is the session locked,
//! is the machine going to sleep, unlock it), the screen saver, GNOME's idle
//! monitor, and the bubble's state for face-unlock's GNOME Shell extension.
//! Every wait runs on a thread of its own and reports to the main loop.

use crate::app::Event;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use zbus::blocking::{Connection, Proxy};
use zbus::proxy::{CacheProperties, MethodFlags};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

type Tx = smithay_client_toolkit::reexports::calloop::channel::Sender<Event>;

const LOGIN1: &str = "org.freedesktop.login1";
const LOGIN1_PATH: &str = "/org/freedesktop/login1";
const MANAGER: &str = "org.freedesktop.login1.Manager";
const SESSION: &str = "org.freedesktop.login1.Session";
const AUTO: &str = "/org/freedesktop/login1/session/auto";

fn proxy<'a>(c: &Connection, dest: &'a str, path: &'a str, iface: &'a str) -> zbus::Result<Proxy<'a>> {
    zbus::blocking::proxy::Builder::new(c).destination(dest)?.path(path)?.interface(iface)?.cache_properties(CacheProperties::No).build()
}

/// This session's object path at logind. Niri and some others hand their
/// session on to user services; without it, "auto" is the session on the
/// display. The signals come from the real path, not from "auto".
fn find_session(c: &Connection) -> String {
    let id = std::env::var("XDG_SESSION_ID").ok().filter(|s| !s.is_empty()).or_else(|| proxy(c, LOGIN1, AUTO, SESSION).ok()?.get_property::<String>("Id").ok());
    let Some(id) = id else {
        eprintln!("logind knows no session for this agent");
        return AUTO.into();
    };
    proxy(c, LOGIN1, LOGIN1_PATH, MANAGER)
        .and_then(|m| m.call::<_, _, OwnedObjectPath>("GetSession", &(id,)))
        .map(|p| p.to_string())
        .unwrap_or_else(|_| AUTO.into())
}

/// Watches the session's LockedHint (set by KWin, GNOME, niri) and the
/// machine going to sleep and waking up.
pub fn watch_logind(tx: Tx) -> Option<Connection> {
    let conn = Connection::system().map_err(|e| eprintln!("no system bus: {e}")).ok()?;
    let c = conn.clone();
    thread::spawn(move || {
        let path = find_session(&c);
        let _ = tx.send(Event::Session(path.clone()));
        let (c2, tx2) = (c.clone(), tx.clone());
        thread::spawn(move || {
            let Ok(p) = proxy(&c2, LOGIN1, LOGIN1_PATH, MANAGER) else { return };
            let Ok(signals) = p.receive_signal("PrepareForSleep") else { return };
            for m in signals {
                if let Ok(sleeping) = m.body().deserialize::<bool>() {
                    let _ = tx2.send(Event::Sleep(sleeping));
                }
            }
        });
        let Ok(props) = zbus::blocking::fdo::PropertiesProxy::builder(&c).destination(LOGIN1).and_then(|b| b.path(path.as_str())).and_then(|b| b.build()) else { return };
        let Ok(changes) = props.receive_properties_changed() else { return };
        let read = || proxy(&c, LOGIN1, &path, SESSION).and_then(|s| s.get_property::<bool>("LockedHint"));
        if let Ok(locked) = read() {
            let _ = tx.send(Event::LockedHint(locked));
        }
        for change in changes {
            let Ok(args) = change.args() else { continue };
            if &*args.interface_name != SESSION {
                continue;
            }
            if let Some(v) = args.changed_properties.get("LockedHint") {
                if let Ok(locked) = bool::try_from(v) {
                    let _ = tx.send(Event::LockedHint(locked));
                }
            } else if args.invalidated_properties.contains(&"LockedHint")
                && let Ok(locked) = read()
            {
                let _ = tx.send(Event::LockedHint(locked));
            }
        }
    });
    Some(conn)
}

/// Opens the session: logind's Unlock, which Plasma and GNOME follow, else
/// loginctl.
pub fn unlock(conn: Option<Connection>, session: String) {
    thread::spawn(move || {
        let path = if session.is_empty() { AUTO.to_string() } else { session };
        let done = conn.ok_or(()).and_then(|c| proxy(&c, LOGIN1, &path, SESSION).and_then(|s| s.call::<_, _, ()>("Unlock", &())).map_err(|e| eprintln!("logind would not unlock: {e}; trying loginctl")));
        if done.is_err() {
            let mut cmd = std::process::Command::new("loginctl");
            cmd.arg("unlock-session");
            if let Ok(id) = std::env::var("XDG_SESSION_ID")
                && !id.is_empty()
            {
                cmd.arg(id);
            }
            let _ = cmd.status();
        }
    });
}

/// Plasma and GNOME lock with their own lock screen when logind asks them
/// to, as `loginctl lock-session` does.
pub fn lock_session() -> bool {
    let id = std::env::var("XDG_SESSION_ID").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "auto".into());
    let r = Connection::system().and_then(|c| proxy(&c, LOGIN1, LOGIN1_PATH, MANAGER)?.call::<_, _, ()>("LockSession", &(id,)));
    if let Err(e) = &r {
        eprintln!("logind would not lock: {e}");
    }
    r.is_ok()
}

/// The screen saver's ActiveChanged, and whether it is active already (the
/// agent restarted while locked).
pub fn watch_screensaver(tx: Tx) {
    thread::spawn(move || {
        let Ok(c) = Connection::session() else { return };
        let Ok(p) = proxy(&c, "org.freedesktop.ScreenSaver", "/ScreenSaver", "org.freedesktop.ScreenSaver") else { return };
        let Ok(signals) = p.receive_signal("ActiveChanged") else { return };
        if let Ok(reply) = p.call_with_flags::<_, _, bool>("GetActive", MethodFlags::NoAutoStart.into(), &())
            && reply == Some(true)
        {
            let _ = tx.send(Event::ScreenSaver(true));
        }
        for m in signals {
            if let Ok(active) = m.body().deserialize::<bool>() {
                let _ = tx.send(Event::ScreenSaver(active));
            }
        }
    });
}

// ---------------------------------------------------------------------------
// GNOME's idle monitor
// ---------------------------------------------------------------------------

pub enum Idle {
    /// Tell once somebody touches something after `calm` ms of quiet.
    Watch(u32),
    Stop,
    Fired(u32),
}

const MUTTER: &str = "org.gnome.Mutter.IdleMonitor";
const MUTTER_PATH: &str = "/org/gnome/Mutter/IdleMonitor/Core";

/// GNOME's way of telling when somebody comes back: a watch that fires once
/// nothing was touched for the calm, then one that fires at the next input.
///
/// Mutter also counts the charger going in or out as activity. Nobody came
/// back then, so that does not count here. It does this because of UPower's
/// own signal, so by the time the watch fires UPower has the new value.
pub fn mutter_idle(tx: Tx) -> Option<mpsc::Sender<Idle>> {
    let c = Connection::session().ok()?;
    let dbus = zbus::blocking::fdo::DBusProxy::new(&c).ok()?;
    if !dbus.name_has_owner(MUTTER.try_into().ok()?).unwrap_or(false) {
        return None;
    }
    let (cmd, cmds) = mpsc::channel::<Idle>();
    let fired = cmd.clone();
    let c2 = c.clone();
    thread::spawn(move || {
        let Ok(p) = proxy(&c2, MUTTER, MUTTER_PATH, MUTTER) else { return };
        let Ok(signals) = p.receive_signal("WatchFired") else { return };
        for m in signals {
            if let Ok(id) = m.body().deserialize::<u32>()
                && fired.send(Idle::Fired(id)).is_err()
            {
                return;
            }
        }
    });
    thread::spawn(move || {
        let Ok(p) = proxy(&c, MUTTER, MUTTER_PATH, MUTTER) else { return };
        let system = Connection::system().ok();
        let on_battery = || -> Option<bool> {
            let s = system.as_ref()?;
            proxy(s, "org.freedesktop.UPower", "/org/freedesktop/UPower", "org.freedesktop.UPower").ok()?.get_property::<bool>("OnBattery").ok()
        };
        let remove = |id: u32| {
            if id != 0 {
                let _ = p.call::<_, _, ()>("RemoveWatch", &(id,));
            }
        };
        let (mut idle, mut active, mut battery) = (0u32, 0u32, None);
        let watch_active = |battery: &mut Option<bool>| -> u32 {
            *battery = on_battery();
            p.call::<_, _, u32>("AddUserActiveWatch", &()).unwrap_or(0)
        };
        for c in cmds {
            match c {
                Idle::Watch(calm) => {
                    remove(idle);
                    remove(active);
                    (idle, active) = (0, 0);
                    // Already calm for that long (the scan itself took a
                    // while): the idle watch would wait for the next calm.
                    let quiet = p.call::<_, _, u64>("GetIdletime", &()).unwrap_or(0);
                    if calm == 0 || quiet >= calm as u64 {
                        active = watch_active(&mut battery);
                    } else {
                        idle = p.call::<_, _, u32>("AddIdleWatch", &(calm as u64,)).unwrap_or(0);
                    }
                }
                Idle::Stop => {
                    remove(idle);
                    remove(active);
                    (idle, active) = (0, 0);
                }
                Idle::Fired(id) if id != 0 && id == idle => {
                    remove(idle);
                    idle = 0;
                    active = watch_active(&mut battery);
                }
                Idle::Fired(id) if id != 0 && id == active => {
                    active = 0;
                    let now = on_battery();
                    if now.is_some() && battery.is_some() && now != battery {
                        active = watch_active(&mut battery);
                    } else {
                        let _ = tx.send(Event::Input);
                    }
                }
                Idle::Fired(_) => {}
            }
        }
    });
    Some(cmd)
}

// ---------------------------------------------------------------------------
// The bubble for GNOME
// ---------------------------------------------------------------------------

const BUS_NAME: &str = "io.github.loonixtools.FaceUnlock";
const OBJECT_PATH: &str = "/io/github/loonixtools/FaceUnlock";
const BUBBLE_IFACE: &str = "io.github.loonixtools.FaceUnlock.Bubble";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BubbleState {
    pub phase: String,
    pub message: String,
    pub face_seen: bool,
    pub style: String,
    pub pace: f64,
    pub screen: String,
}

impl BubbleState {
    fn map(&self) -> HashMap<&'static str, Value<'_>> {
        HashMap::from([
            ("phase", Value::from(self.phase.as_str())),
            ("message", Value::from(self.message.as_str())),
            ("faceSeen", Value::from(self.face_seen)),
            ("style", Value::from(self.style.as_str())),
            ("pace", Value::from(self.pace)),
            ("screen", Value::from(self.screen.as_str())),
        ])
    }
}

struct BubbleIface {
    state: Arc<Mutex<BubbleState>>,
}

#[zbus::interface(name = "io.github.loonixtools.FaceUnlock.Bubble")]
impl BubbleIface {
    fn get_state(&self) -> HashMap<String, OwnedValue> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.map().into_iter().filter_map(|(k, v)| Some((k.to_string(), OwnedValue::try_from(v).ok()?))).collect()
    }
}

/// GNOME lets no program draw above its windows: face-unlock's GNOME Shell
/// extension draws the bubble there, from what this tells it.
pub struct BubbleService {
    conn: Connection,
    state: Arc<Mutex<BubbleState>>,
}

impl BubbleService {
    pub fn start() -> Option<BubbleService> {
        let state = Arc::new(Mutex::new(BubbleState::default()));
        let conn = zbus::blocking::connection::Builder::session()
            .and_then(|b| b.name(BUS_NAME))
            .and_then(|b| b.serve_at(OBJECT_PATH, BubbleIface { state: state.clone() }))
            .and_then(|b| b.build())
            .map_err(|e| eprintln!("cannot offer the bubble on the session bus: {e}"))
            .ok()?;
        Some(BubbleService { conn, state })
    }

    pub fn update(&self, new: BubbleState) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if *state == new {
            return;
        }
        *state = new;
        let _ = self.conn.emit_signal(None::<&str>, OBJECT_PATH, BUBBLE_IFACE, "StateChanged", &(state.map(),));
    }
}
