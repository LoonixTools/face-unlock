//! The agent's state and what it does: what the bubble shows, whether the
//! session is locked, when to scan, and face-unlock's own lock screen. One
//! main loop; threads report to it as events.

use crate::bubble::{Bubble, Look, Phase};
use crate::bus::{self, BubbleService, BubbleState, Idle};
use crate::daemon::{self, Scan};
use crate::draw::ms;
use crate::lockers;
use crate::lockscreen::LockScreen;
use crate::wl::Wl;
use face_unlock::config::UserConfig;
use face_unlock::i18n::tr;
use face_unlock::paths;
use serde_json::Value;
use smithay_client_toolkit::reexports::calloop::channel::Sender;
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::{LoopHandle, RegistrationToken};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

/// What threads tell the main loop.
pub enum Event {
    /// A scan the daemon runs for somebody else (sudo, an admin prompt).
    Scan(Value),
    /// `face-unlock lock`: answered with true once the screen is locked.
    LockRequest(mpsc::Sender<bool>),
    ScanStep(u64, Value),
    ScanDone(u64, Value),
    /// This session's path at logind.
    Session(String),
    LockedHint(bool),
    Sleep(bool),
    ScreenSaver(bool),
    /// Somebody touched something after a calm.
    Input,
    /// The password on the own lock screen was right, or not.
    Password(bool),
    /// A lock screen's picture, for the output and pixel size it was asked
    /// for.
    Wallpaper(String, (u32, u32), Option<tiny_skia::Pixmap>),
}

// How long a result stays up before the bubble closes: long enough to read,
// short enough not to be in the way. On success the tick is drawn after
// about 730 ms and only gets a short moment on its own.
const SUCCESS_HOLD: f64 = 1100.0;
const FAILURE_HOLD: f64 = 1700.0;
const LOCKOUT_HOLD: f64 = 3000.0;
// A scan that never reports back must not leave the bubble up for good.
const SCAN_WATCHDOG: f64 = 30000.0;
// Locking with the keyboard is itself a key press, and the keys come back up
// a moment later. Input in this window after locking is not somebody coming
// back.
const GRACE_AFTER_LOCK: u64 = 1500;
// After a scan that did not get anybody in, the next one waits until the
// person has kept still for this long and then touched something again.
// That way typing the password does not start a scan with every key.
const CALM_BEFORE_RETRY: u32 = 2000;
// A camera needs a moment after the machine wakes before it delivers frames.
const SCAN_AFTER_WAKE: u64 = 1000;

/// Where `--lock` tells the command that ran it that the screen is locked,
/// when it forked to stay behind as the lock screen. -1: it did not.
pub static READY_FD: AtomicI32 = AtomicI32::new(-1);

pub fn signal_ready(ok: bool) {
    let fd = READY_FD.swap(-1, Ordering::SeqCst);
    if fd >= 0 {
        // SAFETY: fd is the pipe's write end, owned by nobody else.
        unsafe {
            if ok {
                libc::write(fd, b"1".as_ptr().cast(), 1);
            }
            libc::close(fd);
        }
    }
}

/// Left while the own lock screen is up. An agent that starts and finds it
/// takes the lock back: the last one went away with the screen locked.
pub fn marker() -> std::path::PathBuf {
    paths::runtime().join("locked")
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Agent,
    /// Started by `face-unlock lock` with no agent running: only here for
    /// the lock.
    LockOnly,
    Demo,
}

pub struct App {
    pub exit: bool,
    pub handle: LoopHandle<'static, App>,
    pub tx: Sender<Event>,
    pub mode: Mode,
    pub cfg: UserConfig,
    cfg_stamp: Option<SystemTime>,
    pub wl: Option<Wl>,

    // What the bubble shows.
    phase: Phase,
    message: String,
    face_seen: bool,
    /// From opening until it has finished closing.
    pub shown: bool,
    pub preview: bool,
    style_override: Option<String>,
    hide: Option<RegistrationToken>,
    pub bubble: Bubble,
    anim: Option<RegistrationToken>,
    service: Option<BubbleService>,
    lock_text: Option<String>,
    text_tries: u32,

    // Whether the session is locked: by the desktop's lock screen (logind,
    // the screen saver), by one that is a program of its own, or by ours.
    screen_saver: bool,
    locked_hint: bool,
    locker_running: bool,
    locker_ready: bool,
    pub own_lockers: bool,
    session: String,
    system: Option<zbus::blocking::Connection>,
    watch_locked: bool,

    // When to scan.
    locked: bool,
    stopped: bool,
    lock_scan_due: bool,
    scan: Option<Scan>,
    next_scan: u64,
    arm: Option<RegistrationToken>,
    watching: bool,
    calm_ms: u32,
    calm_since: Instant,
    mutter: Option<mpsc::Sender<Idle>>,
    mutter_tried: bool,

    // face-unlock's own lock screen.
    pub screen: LockScreen,
    pub waiting: Vec<mpsc::Sender<bool>>,
}

impl App {
    pub fn new(handle: LoopHandle<'static, App>, tx: Sender<Event>, mode: Mode, wl: Option<Wl>, style: Option<String>) -> App {
        let own_lockers = wl.as_ref().is_some_and(|w| w.has_lock);
        let mut app = App {
            exit: false,
            handle,
            tx,
            mode,
            cfg: UserConfig::load(),
            cfg_stamp: stamp(),
            wl,
            phase: Phase::Hidden,
            message: String::new(),
            face_seen: false,
            shown: false,
            preview: mode == Mode::Demo,
            style_override: style,
            hide: None,
            bubble: Bubble::new(),
            anim: None,
            service: None,
            lock_text: None,
            text_tries: 0,
            screen_saver: false,
            locked_hint: false,
            locker_running: false,
            locker_ready: false,
            own_lockers,
            session: String::new(),
            system: None,
            watch_locked: false,
            locked: false,
            stopped: false,
            lock_scan_due: false,
            scan: None,
            next_scan: 0,
            arm: None,
            watching: false,
            calm_ms: 0,
            calm_since: Instant::now(),
            mutter: None,
            mutter_tried: false,
            screen: LockScreen::new(),
            waiting: Vec::new(),
        };
        app.apply_look();
        app
    }

    /// The parts that watch and listen. Not for the demo.
    pub fn start(&mut self) {
        let has_layer = self.wl.as_ref().is_some_and(|w| w.has_layer());
        if !has_layer && self.mode == Mode::Agent {
            self.service = BubbleService::start();
        }
        self.system = bus::watch_logind(self.tx.clone());
        bus::watch_screensaver(self.tx.clone());
        // hyprlock covers the bubble: it can show it as a line of text.
        if self.own_lockers && self.mode == Mode::Agent {
            self.lock_text = Some(String::new());
            lockers::write_text("");
        }
        if self.own_lockers {
            self.every(Duration::from_secs(1), |app| app.poll_lockers());
            self.after(Duration::ZERO, |app| app.poll_lockers());
        }
        self.every(Duration::from_secs(2), |app| app.reload_config());
        self.apply_look();
    }

    // -----------------------------------------------------------------------
    // Timers
    // -----------------------------------------------------------------------

    pub fn after(&self, d: Duration, f: impl FnOnce(&mut App) + 'static) -> Option<RegistrationToken> {
        let mut f = Some(f);
        self.handle
            .insert_source(Timer::from_duration(d), move |_, _, app| {
                if let Some(f) = f.take() {
                    f(app);
                }
                TimeoutAction::Drop
            })
            .ok()
    }

    fn every(&self, d: Duration, mut f: impl FnMut(&mut App) + 'static) {
        let _ = self.handle.insert_source(Timer::from_duration(d), move |_, _, app| {
            f(app);
            TimeoutAction::ToDuration(d)
        });
    }

    fn cancel(&self, token: Option<RegistrationToken>) {
        if let Some(t) = token {
            self.handle.remove(t);
        }
    }

    /// Keeps frames coming while anything moves.
    pub fn kick(&mut self) {
        if self.anim.is_some() {
            return;
        }
        let token = self.handle.insert_source(Timer::immediate(), |_, _, app| {
            if app.frame() {
                TimeoutAction::ToDuration(Duration::from_millis(16))
            } else {
                app.anim = None;
                TimeoutAction::Drop
            }
        });
        self.anim = token.ok();
    }

    /// One frame. Returns whether another is needed.
    fn frame(&mut self) -> bool {
        let at = ms();
        if self.bubble.advance(at) {
            self.closed();
        }
        let more = self.bubble.busy(at) || self.screen.busy_at(at);
        if let Some(wl) = self.wl.as_mut() {
            wl.draw_bubbles(&self.bubble, at);
            if self.screen.busy_at(at) {
                wl.dirty_locks();
            }
            wl.draw_locks(&self.screen, at);
        }
        more
    }

    // -----------------------------------------------------------------------
    // Events from threads
    // -----------------------------------------------------------------------

    pub fn event(&mut self, e: Event) {
        match e {
            Event::Scan(o) => self.daemon_event(&o),
            Event::LockRequest(answer) => self.lock_request(answer),
            Event::ScanStep(id, e) => {
                if self.scan.as_ref().is_some_and(|s| s.id == id) {
                    match e["event"].as_str() {
                        Some("face") => self.face_found(),
                        Some("hint") => self.hint(e["hint"].as_str().unwrap_or("")),
                        _ => {}
                    }
                }
            }
            Event::ScanDone(id, result) => {
                if self.scan.as_ref().is_some_and(|s| s.id == id) {
                    self.scan = None;
                    self.scan_finished(&result);
                }
            }
            Event::Session(path) => self.session = path,
            Event::LockedHint(v) => {
                self.locked_hint = v;
                self.update_locked();
            }
            Event::ScreenSaver(v) => {
                self.screen_saver = v;
                self.update_locked();
            }
            Event::Sleep(sleeping) => self.prepare_for_sleep(sleeping),
            Event::Input => self.on_resume(),
            Event::Password(ok) => {
                self.screen.busy = false;
                self.screen.clear_password();
                if ok {
                    self.own_unlock();
                } else {
                    self.screen.rejected(ms());
                    self.kick();
                }
                if let Some(wl) = self.wl.as_mut() {
                    wl.dirty_locks();
                    wl.draw_locks(&self.screen, ms());
                }
            }
            Event::Wallpaper(output, size, pixmap) => {
                if let Some(wl) = self.wl.as_mut() {
                    wl.set_wallpaper(&output, size, pixmap);
                    wl.draw_locks(&self.screen, ms());
                }
            }
        }
    }

    fn reload_config(&mut self) {
        let now = stamp();
        if now == self.cfg_stamp {
            return;
        }
        self.cfg_stamp = now;
        let cfg = UserConfig::load();
        if cfg != self.cfg {
            self.cfg = cfg;
            self.apply_look();
        }
    }

    // -----------------------------------------------------------------------
    // The bubble
    // -----------------------------------------------------------------------

    fn enabled(&self) -> bool {
        self.preview || self.cfg.bubble
    }

    fn style(&self) -> &str {
        self.style_override.as_deref().unwrap_or(&self.cfg.bubble_style)
    }

    fn set_phase(&mut self, phase: Phase) {
        if self.phase == phase {
            return;
        }
        self.phase = phase;
        if phase != Phase::Hidden {
            self.shown = true;
        }
    }

    fn hide_in(&mut self, ms: f64) {
        let t = self.hide.take();
        self.cancel(t);
        self.hide = self.after(Duration::from_millis(ms as u64), |app| {
            app.hide = None;
            app.dismiss();
        });
    }

    /// Tells everything that shows the bubble what it shows now.
    fn apply_look(&mut self) {
        let look = Look {
            phase: self.phase,
            message: self.message.clone(),
            face_seen: self.face_seen,
            minimal: self.style() == "minimal",
            pace: self.cfg.pace() as f64,
        };
        self.bubble.set(&look, ms());
        let shown = self.shown;
        let screen = self.cfg.bubble_screen.clone();
        if let Some(wl) = self.wl.as_mut() {
            wl.show_bubble(shown, &screen);
            wl.place_lock_bubble(&screen, shown);
        }
        if let Some(service) = &self.service {
            service.update(BubbleState {
                phase: self.phase.name().into(),
                message: self.message.clone(),
                face_seen: self.face_seen,
                style: self.style().into(),
                pace: self.cfg.pace() as f64,
                screen,
            });
        }
        self.update_lock_text();
        self.kick();
    }

    pub fn scan_started(&mut self) {
        if !self.enabled() {
            return;
        }
        self.hide_in(SCAN_WATCHDOG);
        self.message.clear();
        self.face_seen = false;
        self.set_phase(Phase::Scanning);
        self.apply_look();
    }

    pub fn face_found(&mut self) {
        if self.phase == Phase::Scanning && !self.face_seen {
            self.face_seen = true;
            self.apply_look();
        }
    }

    pub fn hint(&mut self, hint: &str) {
        if self.phase != Phase::Scanning {
            return;
        }
        let m = match hint {
            "blink" => tr("Blink once"),
            "look" => tr("Look at the screen"),
            "closer" => tr("Move closer"),
            "light" => tr("Too dark"),
            _ => return,
        };
        self.message = m.into();
        self.apply_look();
    }

    pub fn succeeded(&mut self) {
        if self.phase == Phase::Hidden {
            return;
        }
        self.message.clear();
        self.set_phase(Phase::Success);
        self.hide_in(SUCCESS_HOLD * self.cfg.pace() as f64);
        self.apply_look();
    }

    pub fn failed(&mut self, reason: &str, lockout: i64) {
        let pace = self.cfg.pace() as f64;
        if reason == "camera-busy" {
            // A video call, most likely. The daemon starts no scan then, so
            // this may come with nothing on screen yet.
            if !self.enabled() || self.cfg.quiet_when_camera_busy {
                self.dismiss();
                return;
            }
            self.message = tr("Camera in use").into();
            self.set_phase(Phase::Busy);
            self.hide_in(FAILURE_HOLD * pace);
            self.apply_look();
            return;
        }
        if self.phase == Phase::Hidden {
            return;
        }
        if lockout > 0 || reason == "lockout" {
            self.message = tr("Use your password").into();
            self.set_phase(Phase::Lockout);
            self.hide_in(LOCKOUT_HOLD * pace);
            self.apply_look();
            return;
        }
        let m = match reason {
            "mismatch" | "spoof" => tr("Not recognized"),
            "liveness" => tr("Try again and blink"),
            "camera" => tr("Camera unavailable"),
            _ => {
                // Nobody there, a head turned away, the scan cancelled
                // because the password was typed: nothing worth saying.
                self.message.clear();
                self.set_phase(Phase::Hidden);
                let t = self.hide.take();
        self.cancel(t);
                self.apply_look();
                return;
            }
        };
        self.message = m.into();
        self.set_phase(Phase::Failure);
        self.hide_in(FAILURE_HOLD * pace);
        self.apply_look();
    }

    pub fn dismiss(&mut self) {
        let t = self.hide.take();
        self.cancel(t);
        self.set_phase(Phase::Hidden);
        self.apply_look();
    }

    /// The bubble has slid away.
    fn closed(&mut self) {
        if self.phase == Phase::Hidden && self.shown {
            self.shown = false;
            self.apply_look();
            if self.mode == Mode::LockOnly && !self.own_locked() {
                self.exit = true;
            }
        }
    }

    fn daemon_event(&mut self, e: &Value) {
        if !self.cfg.bubble_for_prompts {
            return;
        }
        match e["state"].as_str() {
            Some("start") => self.scan_started(),
            Some("face") => self.face_found(),
            Some("hint") => self.hint(e["hint"].as_str().unwrap_or("")),
            Some("success") => self.succeeded(),
            Some("failure") => self.failed(e["reason"].as_str().unwrap_or(""), e["lockout"].as_f64().unwrap_or(0.0) as i64),
            _ => {}
        }
    }

    /// hyprlock's line of text, for those who keep it as their lock screen.
    fn update_lock_text(&mut self) {
        let Some(current) = &self.lock_text else { return };
        let now = if self.cfg.lock_screen_style == "yours" {
            match self.phase {
                Phase::Scanning if self.message.is_empty() => tr("Looking for your face…").to_string(),
                Phase::Success => tr("Face recognized").to_string(),
                Phase::Hidden => String::new(),
                _ => self.message.clone(),
            }
        } else {
            String::new()
        };
        if &now != current {
            lockers::write_text(&now);
            self.lock_text = Some(now);
            self.text_tries = 0;
            self.refresh_lock_text();
        }
    }

    /// A hyprlock that is only starting reads the file once it is up, but
    /// may have read it just before this changed: asked again then.
    fn refresh_lock_text(&mut self) {
        if lockers::signal(libc::SIGUSR2, Some("hyprlock")) > 0 {
            self.text_tries += 1;
            if self.text_tries < 20 {
                self.after(Duration::from_millis(250), |app| app.refresh_lock_text());
            }
        }
    }

    // -----------------------------------------------------------------------
    // Is the session locked?
    // -----------------------------------------------------------------------

    pub fn own_locked(&self) -> bool {
        self.wl.as_ref().is_some_and(|w| w.lock.is_some())
    }

    fn poll_lockers(&mut self) {
        self.locker_running = !lockers::find(None).is_empty();
        let ready = lockers::ready(libc::SIGUSR1);
        let became = ready && !self.locker_ready;
        self.locker_ready = ready;
        self.update_locked();
        // A lock screen that was only starting can be opened now: the scan
        // that was due when it locked follows.
        if became && self.lock_scan_due {
            self.lock_scan_due = false;
            self.start_scan("lock");
        }
    }

    fn can_unlock(&self) -> bool {
        self.own_locked() || !self.own_lockers || self.locker_ready
    }

    pub fn update_locked(&mut self) {
        let locked = self.screen_saver || self.locked_hint || self.locker_running || self.own_locked();
        if locked != self.watch_locked {
            self.watch_locked = locked;
            self.locked_changed(locked);
        }
    }

    fn unlock(&mut self) {
        if self.own_locked() {
            self.own_unlock();
            return;
        }
        // Looked up again rather than taken from the last poll: a second is
        // long enough for a pid to belong to something else.
        lockers::signal(libc::SIGUSR1, None);
        bus::unlock(self.system.clone(), self.session.clone());
    }

    // -----------------------------------------------------------------------
    // When to scan
    // -----------------------------------------------------------------------

    fn locked_changed(&mut self, locked: bool) {
        if locked == self.locked {
            return;
        }
        if !locked {
            self.locked = false;
            self.lock_scan_due = false;
            let t = self.arm.take();
            self.cancel(t);
            self.stop_input();
            if let Some(scan) = self.scan.take() {
                scan.abort();
            }
            // The tick of our own unlock is still playing; anything else
            // goes.
            if self.phase == Phase::Scanning {
                self.dismiss();
            }
            // However it was unlocked, it was the right person: a lockout
            // after failed scans ends here, the way a phone takes its code.
            daemon::fire("unlocked");
            return;
        }
        let face = self.cfg.enabled && self.cfg.lock_screen;
        self.screen.face_unlock = face;
        if let Some(wl) = self.wl.as_mut() {
            wl.dirty_locks();
        }
        if !face {
            return;
        }
        self.locked = true;
        self.stopped = false;
        // The daemon is started by its socket and loads the networks when
        // it starts. Doing that now, while nobody waits, takes it off the
        // first scan.
        daemon::fire("hello");
        if self.cfg.scan_on_lock {
            self.after(Duration::from_millis(400), |app| {
                app.lock_scan_due = app.locked && !app.can_unlock();
                app.start_scan("lock");
            });
        } else {
            let t = self.arm.take();
            self.cancel(t);
            self.arm = self.after(Duration::from_millis(GRACE_AFTER_LOCK), |app| {
                app.arm = None;
                app.arm_input(0);
            });
        }
    }

    fn prepare_for_sleep(&mut self, sleeping: bool) {
        if sleeping {
            if let Some(scan) = self.scan.take() {
                scan.abort();
                self.dismiss();
            }
            return;
        }
        // Waking up is somebody coming back, lid or no lid.
        if self.locked && !self.stopped && self.cfg.scan_on_wake {
            self.after(Duration::from_millis(SCAN_AFTER_WAKE), |app| app.start_scan("resume"));
        }
    }

    fn arm_input(&mut self, calm: u32) {
        if !self.locked || self.stopped || !self.cfg.scan_on_wake {
            return;
        }
        self.watch_input(calm);
    }

    fn on_resume(&mut self) {
        if self.locked && self.scan.is_none() {
            self.start_scan("wake");
        }
    }

    fn start_scan(&mut self, why: &str) {
        // A lock screen this cannot open (gtklock, waylock, a shell's own)
        // asks for the face itself, through PAM, when Enter is pressed.
        if !self.locked || self.scan.is_some() || self.stopped || !self.can_unlock() {
            return;
        }
        eprintln!("scanning ({why})");
        self.scan_started();
        self.next_scan += 1;
        self.scan = Some(daemon::scan(self.next_scan, self.tx.clone()));
    }

    fn scan_finished(&mut self, result: &Value) {
        if !self.locked {
            return;
        }
        let reason = result["reason"].as_str().unwrap_or("");
        if result["ok"].as_bool() == Some(true) {
            // Straight away, the way a phone does it: the lock screen takes a
            // moment to go, and the bubble stays above the desktop, so the
            // rings and the tick play on over it.
            self.succeeded();
            self.unlock();
            return;
        }
        let lockout = result["lockout"].as_f64().unwrap_or(0.0) as i64;
        self.failed(reason, lockout);
        if reason == "lockout" || result.get("lockout").is_some() || matches!(reason, "not-enrolled" | "models" | "denied" | "unreachable") {
            // Nothing another scan could change before the next lock.
            self.stopped = true;
            return;
        }
        if reason == "busy" {
            let t = self.arm.take();
            self.cancel(t);
            self.arm = self.after(Duration::from_millis(GRACE_AFTER_LOCK), |app| {
                app.arm = None;
                app.arm_input(0);
            });
            return;
        }
        self.arm_input(CALM_BEFORE_RETRY);
    }

    // -----------------------------------------------------------------------
    // Telling when somebody comes back
    // -----------------------------------------------------------------------

    fn watch_input(&mut self, calm: u32) {
        self.stop_input();
        self.watching = true;
        self.calm_ms = calm;
        self.calm_since = Instant::now();
        // Hyprland never says resumed for a timeout of 0. A tenth of a
        // second of calm first changes nothing anywhere.
        if let Some(wl) = self.wl.as_mut()
            && wl.watch_idle(calm.max(100))
        {
            return;
        }
        if !self.mutter_tried {
            self.mutter_tried = true;
            self.mutter = bus::mutter_idle(self.tx.clone());
        }
        match &self.mutter {
            Some(m) => {
                let _ = m.send(Idle::Watch(calm));
            }
            None => eprintln!("no ext_idle_notifier_v1 and no Mutter IdleMonitor, so no telling when somebody comes back"),
        }
    }

    fn stop_input(&mut self) {
        self.watching = false;
        if let Some(wl) = self.wl.as_mut() {
            wl.stop_idle();
        }
        if let Some(m) = &self.mutter {
            let _ = m.send(Idle::Stop);
        }
    }

    /// A key or the pointer on the own lock screen.
    pub fn note_input(&mut self) {
        if self.watching && self.calm_since.elapsed() >= Duration::from_millis(self.calm_ms as u64) {
            self.watching = false;
            let _ = self.handle.insert_idle(|app| app.on_resume());
        }
        self.calm_since = Instant::now();
    }

    pub fn idle_resumed(&mut self) {
        if self.watching {
            self.watching = false;
            let _ = self.handle.insert_idle(|app| app.on_resume());
        }
    }

    // -----------------------------------------------------------------------
    // The own lock screen
    // -----------------------------------------------------------------------

    fn lock_request(&mut self, answer: mpsc::Sender<bool>) {
        if self.wl.as_ref().is_none_or(|w| !w.has_lock) {
            let _ = answer.send(false);
            return;
        }
        self.lock();
        if self.wl.as_ref().is_some_and(|w| w.confirmed) {
            let _ = answer.send(true);
        } else if self.own_locked() {
            self.waiting.push(answer);
        } else {
            let _ = answer.send(false);
        }
    }

    pub fn lock(&mut self) {
        if self.own_locked() {
            return;
        }
        self.screen.reset();
        self.screen.blur = self.cfg.lock_blur;
        let wallpaper = self.cfg.lock_wallpaper.clone();
        let screen = self.cfg.bubble_screen.clone();
        let Some(wl) = self.wl.as_mut() else { return };
        if !wl.lock(&wallpaper, self.screen.blur, self.tx.clone()) {
            return;
        }
        wl.place_lock_bubble(&screen, self.shown);
        // The time on it.
        let _ = self.handle.insert_source(Timer::from_duration(Duration::from_secs(1)), |_, _, app| {
            if !app.own_locked() {
                return TimeoutAction::Drop;
            }
            if let Some(wl) = app.wl.as_mut() {
                wl.draw_locks(&app.screen, ms());
            }
            TimeoutAction::ToDuration(Duration::from_secs(1))
        });
        if let Some(dir) = marker().parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if std::fs::write(marker(), b"").is_err() {
            eprintln!("cannot write {}", marker().display());
        }
        self.update_locked();
    }

    /// The compositor has the lock.
    pub fn lock_confirmed(&mut self) {
        for w in self.waiting.drain(..) {
            let _ = w.send(true);
        }
        signal_ready(true);
    }

    pub fn own_unlock(&mut self) {
        let Some(wl) = self.wl.as_mut() else { return };
        if wl.unlock() {
            self.released();
        }
    }

    /// The lock is gone: unlocked, or ended by the compositor.
    pub fn released(&mut self) {
        let _ = std::fs::remove_file(marker());
        for w in self.waiting.drain(..) {
            let _ = w.send(false);
        }
        signal_ready(false);
        self.screen.reset();
        self.update_locked();
        if self.mode == Mode::LockOnly {
            // Only here for the lock. It goes once the bubble has finished.
            if !self.shown {
                self.exit = true;
            } else {
                self.after(Duration::from_secs(5), |app| app.exit = true);
            }
        }
    }

    pub fn submit(&mut self) {
        if self.screen.busy || self.screen.password.is_empty() {
            return;
        }
        self.screen.busy = true;
        let user = self.screen.login.clone();
        let password = self.screen.password.as_bytes().to_vec();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let ok = crate::pam::check(&user, password);
            let _ = tx.send(Event::Password(ok));
        });
    }

    // -----------------------------------------------------------------------
    // The demo: the whole life of a bubble, twice. A face that is recognised
    // after a blink, then one that is not. Last, a camera another program
    // has.
    // -----------------------------------------------------------------------

    pub fn demo(&mut self) {
        type Step = (u64, fn(&mut App));
        let steps: [Step; 9] = [
            (300, |a| a.scan_started()),
            (900, |a| a.face_found()),
            (1900, |a| a.hint("blink")),
            (3000, |a| a.succeeded()),
            (5600, |a| a.scan_started()),
            (6200, |a| a.face_found()),
            (7800, |a| a.failed("mismatch", 0)),
            (11000, |a| a.failed("camera-busy", 0)),
            (14000, |a| a.exit = true),
        ];
        for (at, f) in steps {
            self.after(Duration::from_millis(at), f);
        }
    }
}

fn stamp() -> Option<SystemTime> {
    std::fs::metadata(paths::user_config()).and_then(|m| m.modified()).ok()
}
