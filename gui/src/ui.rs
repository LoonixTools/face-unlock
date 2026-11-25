//! The window's state and what its buttons do. Anything that waits (the
//! daemon, a password dialog) runs on a thread and hands its answer back
//! through the app handle.

use crate::setup::Enroll;
use crate::trial::Trial;
use face_unlock::actions::{self, Notice, Notices};
use face_unlock::client::{self, Camera, Face, Status};
use face_unlock::config::{self, Settings, UserConfig};
use face_unlock::i18n::tr;
use face_unlock::keyvalue::KeyValue;
use face_unlock::pam::PamFiles;
use face_unlock::paths;
use face_unlock::root::{self, Ask};
use face_unlock::system::{self, Desktop};
use rune::{AppHandle, TextEdit};
use std::sync::Mutex;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Pane {
    #[default]
    Overview,
    Faces,
    LockScreen,
    Prompts,
    Recognition,
    Bubble,
    Try,
}

impl Pane {
    pub const ALL: [Pane; 7] = [Pane::Overview, Pane::Faces, Pane::LockScreen, Pane::Prompts, Pane::Recognition, Pane::Bubble, Pane::Try];

    pub fn title(self) -> &'static str {
        match self {
            Pane::Overview => tr("Overview"),
            Pane::Faces => tr("Faces"),
            Pane::LockScreen => tr("Lock screen"),
            Pane::Prompts => tr("Password prompts"),
            Pane::Recognition => tr("Recognition"),
            Pane::Bubble => tr("Bubble"),
            Pane::Try => tr("Try it"),
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Pane::Overview => "nav-overview",
            Pane::Faces => "nav-faces",
            Pane::LockScreen => "nav-lock",
            Pane::Prompts => "nav-prompts",
            Pane::Recognition => "nav-recognition",
            Pane::Bubble => "nav-bubble",
            Pane::Try => "nav-try",
        }
    }

    pub fn icon(self) -> &'static rune::Icon {
        use crate::icons::*;
        match self {
            Pane::Overview => &SCAN_FACE,
            Pane::Faces => &USERS,
            Pane::LockScreen => &LOCK,
            Pane::Prompts => &SHIELD,
            Pane::Recognition => &EYE,
            Pane::Bubble => &BUBBLE,
            Pane::Try => &PLAY,
        }
    }

    pub fn from_name(name: &str) -> Pane {
        Pane::ALL.into_iter().find(|p| p.key() == format!("nav-{name}")).unwrap_or_default()
    }
}

/// What the window is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// The settings, with the sidebar.
    Main,
    /// The first start: one question at a time.
    Onboarding,
    /// Only setting up a face (`face-unlock setup`).
    Setup,
    /// Only the question which lock screen (`--choose-lock-screen`).
    ChooseLock,
    /// Only the question which screen the bubble uses (`--choose-screen`).
    ChooseScreen,
}

/// The steps of the first start.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Welcome,
    Service,
    Face,
    LockScreen,
    Places,
    Done,
}

/// The PAM side, as read from the files.
#[derive(Clone, Debug, Default)]
pub struct Pam {
    pub sudo_available: bool,
    pub sudo: bool,
    pub polkit_available: bool,
    pub polkit: bool,
    pub lockers: Vec<&'static str>,
    pub lockers_on: bool,
}

/// What a background thread read: the files, the services, the daemon.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub user: KeyValue,
    pub sys: KeyValue,
    pub cfg: UserConfig,
    pub settings: Settings,
    pub status: Option<Status>,
    pub faces: Vec<Face>,
    pub cameras: Vec<Camera>,
    pub auto_camera: String,
    pub pam: Pam,
    pub socket_enabled: bool,
    pub agent_available: bool,
    pub agent_running: bool,
    pub polkit_missing: bool,
    pub polkit_unit: Option<&'static str>,
    pub desktop: Desktop,
    pub screens: Vec<String>,
}

impl Snapshot {
    /// Only the files: quick, for the first frame and the scenes.
    pub fn files() -> Snapshot {
        let user = KeyValue::load(&paths::user_config());
        let sys = KeyValue::load(&paths::sysconfig());
        Snapshot {
            cfg: UserConfig::from(&user),
            settings: Settings::from(&sys),
            user,
            sys,
            status: None,
            faces: vec![],
            cameras: vec![],
            auto_camera: String::new(),
            pam: Pam::default(),
            socket_enabled: false,
            agent_available: false,
            agent_running: false,
            polkit_missing: false,
            polkit_unit: None,
            desktop: system::desktop(),
            screens: vec![],
        }
    }

    /// Everything, asking the daemon and systemd. Not in a frame.
    pub fn read() -> Snapshot {
        let mut s = Snapshot::files();
        s.status = client::status();
        if s.status.is_some() {
            s.faces = client::faces().unwrap_or_default();
            if let Some((list, auto)) = client::cameras() {
                s.cameras = list;
                s.auto_camera = auto;
            }
        }
        let files = PamFiles::system();
        let lockers = if s.desktop.own_locker() { files.lockers() } else { vec![] };
        s.pam = Pam {
            sudo_available: files.available("sudo"),
            sudo: files.enabled("sudo"),
            polkit_available: files.available("polkit-1"),
            polkit: files.enabled("polkit-1"),
            lockers_on: !lockers.is_empty() && files.lockers_enabled(),
            lockers,
        };
        s.socket_enabled = system::socket_enabled();
        s.agent_available = system::agent_available();
        s.agent_running = s.agent_available && system::agent_running();
        s.polkit_missing = system::polkit_agent_missing();
        if s.polkit_missing {
            s.polkit_unit = system::polkit_agent_unit();
        }
        s.screens = system::screens();
        s
    }

    pub fn reachable(&self) -> bool {
        self.status.is_some()
    }
}

/// The running app around the state: none in headless scenes.
#[derive(Clone)]
pub struct Host {
    pub app: AppHandle<Ui>,
}

/// What the window answers the command that opened it, once it closes.
pub static ANSWER: Mutex<Option<(i32, String)>> = Mutex::new(None);

pub fn answer(code: i32, text: &str) {
    *ANSWER.lock().unwrap_or_else(|e| e.into_inner()) = Some((code, text.into()));
}

pub struct Ui {
    pub mode: Mode,
    pub step: Step,
    pub pane: Pane,
    pub snap: Snapshot,
    /// The first full snapshot came.
    pub loaded: bool,
    /// The select that is open.
    pub open: Option<&'static str>,
    /// Something waits for a password or the daemon: which setting.
    pub busy: Option<String>,
    /// What the last action said.
    pub notices: Notices,
    pub host: Option<Host>,
    pub enroll: Enroll,
    pub trial: Trial,
    /// A face being renamed: its id and the field.
    pub rename: Option<(String, TextEdit)>,
    /// A face about to be deleted.
    pub confirm: Option<String>,
    pub wallpaper: TextEdit,
    /// The lock screen picked on the way, before it is written down.
    pub lock_pick: String,
    /// The screen picked for the bubble, "auto" for the mouse.
    pub screen_pick: String,
    /// The screens as the compositor has them, for drawing them to scale.
    pub displays: Vec<rune::Display>,
    /// Places picked during the first start.
    pub places: [bool; 3],
    /// Shown inside the main window instead of a pane.
    pub sub: Option<Mode>,
    pub images: Option<rune::ImageCache>,
    pub logo: Option<rune::Image>,
    /// The app icon, large, for the welcome.
    pub logo_big: Option<rune::Image>,
    pub wallpapers: Vec<(String, rune::Image)>,
    /// What the setting changed last does, played above the pane.
    pub preview: crate::preview::Preview,
    /// That setting.
    pub focus: Option<&'static str>,
}

impl Ui {
    pub fn new(mode: Mode, host: Option<Host>, snap: Snapshot) -> Ui {
        let wallpaper = TextEdit::new(snap.cfg.lock_wallpaper.clone());
        let lock_pick = if snap.cfg.lock_screen_style.is_empty() { "own".into() } else { snap.cfg.lock_screen_style.clone() };
        let screen_pick = if snap.cfg.bubble_screen.is_empty() { "auto".into() } else { snap.cfg.bubble_screen.clone() };
        let places = [snap.cfg.sudo, snap.cfg.polkit, snap.cfg.lock_screens];
        let preview = crate::preview::Preview::new(host.clone());
        Ui {
            mode,
            step: Step::Welcome,
            pane: Pane::Overview,
            snap,
            loaded: false,
            open: None,
            busy: None,
            notices: vec![],
            host,
            enroll: Enroll::default(),
            trial: Trial::default(),
            rename: None,
            confirm: None,
            wallpaper,
            lock_pick,
            screen_pick,
            displays: vec![],
            places,
            sub: None,
            images: None,
            logo: None,
            logo_big: None,
            wallpapers: vec![],
            preview,
            focus: None,
        }
    }

    /// Reads everything again on a thread, and shows it when it comes.
    pub fn refresh(&self) {
        let Some(host) = self.host.clone() else { return };
        std::thread::spawn(move || {
            let snap = Snapshot::read();
            host.app.update(move |s, _| s.apply(snap));
        });
    }

    pub fn apply(&mut self, snap: Snapshot) {
        let first = !self.loaded;
        self.loaded = true;
        if self.wallpaper.text.as_str() == self.snap.cfg.lock_wallpaper {
            self.wallpaper = TextEdit::new(snap.cfg.lock_wallpaper.clone());
        }
        self.snap = snap;
        // The first start: nothing set up and nothing switched on yet.
        if first && self.mode == Mode::Main && !self.snap.cfg.enabled && self.snap.faces.is_empty() && !self.snap.user.raw("Enabled").is_some_and(|v| !v.is_empty()) {
            self.mode = Mode::Onboarding;
            self.step = Step::Welcome;
        }
        if first && self.mode == Mode::Main && self.snap.status.is_some() {
            self.load_wallpapers();
        }
    }

    pub fn show_pane(&mut self, pane: Pane) {
        self.pane = pane;
        self.focus = None;
        self.open = None;
        self.rename = None;
        self.confirm = None;
        self.sub = None;
        if pane != Pane::Try {
            self.trial.stop();
        }
    }

    /// Runs `work` on a thread with `key` marked busy, then reads everything
    /// again. Its error, if any, is shown.
    pub fn run(&mut self, key: &str, work: impl FnOnce() -> Result<Notices, String> + Send + 'static) {
        if self.busy.is_some() {
            return;
        }
        self.open = None;
        self.busy = Some(key.to_string());
        self.notices.clear();
        let Some(host) = self.host.clone() else {
            self.busy = None;
            return;
        };
        std::thread::spawn(move || {
            let result = work();
            let snap = Snapshot::read();
            host.app.update(move |s, _| {
                s.busy = None;
                match result {
                    Ok(n) => s.notices = n,
                    Err(e) => s.notices = vec![Notice::Bad(e)],
                }
                s.apply(snap);
            });
        });
    }

    pub fn is_busy(&self, key: &str) -> bool {
        self.busy.as_deref() == Some(key)
    }

    /// One of this user's settings. Written at once: the file is theirs.
    pub fn set_user(&mut self, key: &'static str, value: String) {
        self.open = None;
        self.focus = Some(key);
        if let Err(e) = config::set_user(key, &value) {
            self.notices = vec![Notice::Bad(format!("{} {e}", tr("Could not save the setting.")))];
            return;
        }
        self.snap.user.set(key, &value);
        self.snap.cfg = UserConfig::from(&self.snap.user);
    }

    pub fn toggle_user(&mut self, key: &'static str) {
        let on = face_unlock::keyvalue::parse_bool(self.snap.user.get(key, ""), self.user_default(key));
        self.set_user(key, if on { "no" } else { "yes" }.into());
    }

    fn user_default(&self, key: &str) -> bool {
        !matches!(key, "Enabled" | "ScanOnLock" | "LockBlur" | "QuietWhenCameraBusy")
    }

    /// A system setting: through root, which asks for the password.
    pub fn set_sys(&mut self, key: &'static str, value: String) {
        self.focus = Some(key);
        if self.snap.sys.get(key, "") == value {
            self.open = None;
            return;
        }
        self.run(key, move || root::run(Ask::Dialog, &["set", key, &value]).map(|_| vec![]));
    }

    pub fn toggle_sys(&mut self, key: &'static str, on: bool) {
        self.set_sys(key, if on { "no" } else { "yes" }.into());
    }

    /// sudo, polkit-1 or lockscreens, through root.
    pub fn set_pam(&mut self, service: &'static str, on: bool) {
        self.focus = Some(service);
        self.run(service, move || actions::set_pam(Ask::Dialog, service, on).map(|_| vec![]));
    }

    /// The big switch.
    pub fn toggle_enabled(&mut self) {
        if self.snap.cfg.enabled {
            self.run("Enabled", || Ok(actions::disable(Ask::Dialog)));
            return;
        }
        if self.snap.faces.is_empty() {
            // Nothing to unlock with yet: the first start's questions.
            self.mode = Mode::Onboarding;
            self.step = if self.snap.reachable() { Step::Face } else { Step::Service };
            return;
        }
        self.run("Enabled", || {
            actions::ensure_service(Ask::Dialog)?;
            Ok(actions::enable(Ask::Dialog))
        });
    }

    pub fn start_service(&mut self) {
        self.run("service", || actions::ensure_service(Ask::Dialog).map(|_| vec![]));
    }

    pub fn start_agent(&mut self) {
        self.run("agent", || {
            if system::agent_enable() { Ok(vec![]) } else { Err(tr("Could not start the lock screen agent.").into()) }
        });
    }

    pub fn start_polkit_agent(&mut self) {
        let Some(unit) = self.snap.polkit_unit else { return };
        self.run("polkit-agent", move || {
            system::polkit_agent_start(unit).map(|hint| {
                let mut n = vec![Notice::Ok(tr("The polkit agent is running. Password windows open now.").into())];
                if let Some(h) = hint {
                    n.push(Notice::Note(h.note));
                    n.extend(h.code.into_iter().map(Notice::Code));
                }
                n
            })
        });
    }

    /// A change to a face, which the daemon asks polkit about.
    pub fn face(&mut self, cmd: &'static str, id: String, name: Option<String>) {
        self.rename = None;
        self.confirm = None;
        self.run(&format!("face-{id}"), move || {
            let mut req = serde_json::json!({"cmd": cmd, "id": id});
            if let Some(n) = name {
                req["name"] = n.into();
            }
            let r = client::request(req);
            if client::ok(&r) { Ok(vec![]) } else { Err(face_unlock::text::reason(client::reason(&r))) }
        });
    }

    pub fn set_lock_style(&mut self, style: String) {
        self.focus = Some("LockScreenStyle");
        self.lock_pick = style.clone();
        let notices = actions::lock_style_set(&style);
        self.notices = notices;
        self.snap.user.set("LockScreenStyle", &style);
        self.snap.cfg = UserConfig::from(&self.snap.user);
    }

    pub fn set_screen(&mut self, screen: String) {
        self.screen_pick = screen.clone();
        self.set_user("BubbleScreen", screen);
    }

    /// The questions of the first start, in order, skipping what is done.
    pub fn next_step(&mut self) {
        self.notices.clear();
        let own_locker = self.snap.desktop.own_locker();
        self.step = match self.step {
            Step::Welcome if !self.snap.reachable() => Step::Service,
            Step::Welcome | Step::Service if self.snap.faces.is_empty() => Step::Face,
            Step::Welcome | Step::Service | Step::Face if own_locker && self.snap.cfg.lock_screen_style.is_empty() => Step::LockScreen,
            Step::Done => {
                self.mode = Mode::Main;
                self.pane = Pane::Overview;
                self.refresh();
                return;
            }
            _ => Step::Places,
        };
        if self.step == Step::Face {
            self.enroll = Enroll::default();
        }
    }

    /// Writes down where face unlock is wanted, and turns it on.
    pub fn finish_onboarding(&mut self) {
        let [sudo, polkit, lockers] = self.places;
        for (key, on) in [("Sudo", sudo), ("Polkit", polkit), ("LockScreens", lockers)] {
            self.set_user(key, if on { "yes" } else { "no" }.into());
        }
        if self.snap.desktop.own_locker() && self.snap.cfg.lock_screen_style.is_empty() {
            let style = self.lock_pick.clone();
            let n = actions::lock_style_set(&style);
            self.snap.user.set("LockScreenStyle", &style);
            self.notices.extend(n);
        }
        let Some(host) = self.host.clone() else {
            self.step = Step::Done;
            return;
        };
        self.busy = Some("finish".into());
        let earlier = std::mem::take(&mut self.notices);
        std::thread::spawn(move || {
            let mut n = earlier;
            match actions::ensure_service(Ask::Dialog) {
                Ok(()) => n.extend(actions::enable(Ask::Dialog)),
                Err(e) => n.push(Notice::Bad(e)),
            }
            let snap = Snapshot::read();
            host.app.update(move |s, _| {
                s.busy = None;
                s.notices = n;
                s.apply(snap);
                s.step = Step::Done;
            });
        });
    }

    /// The pictures behind the lock screens, for the question which one.
    pub fn load_wallpapers(&mut self) {
        let Some(cache) = &self.images else { return };
        if !self.wallpapers.is_empty() {
            return;
        }
        let pictures = face_unlock::wallpaper::for_lock(&self.snap.cfg.lock_wallpaper);
        let desktop = face_unlock::wallpaper::for_lock("");
        for (key, set) in [("own", &pictures), ("desktop", &desktop)] {
            if let Some(p) = face_unlock::wallpaper::pick(set, "") {
                self.wallpapers.push((key.into(), cache.file(p.to_string_lossy().as_ref(), 640.0, 2.0)));
            }
        }
    }

    pub fn wallpaper_image(&self, key: &str) -> Option<&rune::Image> {
        self.wallpapers.iter().find(|(k, _)| k == key).map(|(_, i)| i).filter(|i| i.is_ready() && i.error().is_none())
    }
}
