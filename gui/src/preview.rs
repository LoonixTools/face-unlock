//! The preview above a settings pane: what the setting just changed does,
//! before it happens for real. The bubble and the lock screen are drawn by
//! the agent's own code (face-unlock-paint), frame by frame on a thread, so
//! they move exactly as fast as they will. The rest are small pictures: who
//! gets in, and what a password prompt shows.

use crate::controls::{DESCRIPTION, GUTTER, Kind, Paint, button, prose, raised};
use crate::icons;
use crate::ui::{Host, Mode, Pane, Ui};
use face_unlock::i18n::{tr, trf};
use face_unlock_paint::bubble::{self, Bubble, Look, Phase};
use face_unlock_paint::draw::{self, Align, Canvas, Rgb, rounded_rect};
use face_unlock_paint::lockscreen::{self, LockScreen};
use rune::tok::{motion, radius};
use rune::{Color, El, SurfaceFit, TextStyle, Transition, col, el, icon, row, text};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tiny_skia::{Pixmap, PixmapPaint, Transform};

/// The stage, in points: a whole screen, made small.
const STAGE_W: f32 = 340.0;
const STAGE_H: f32 = 190.0;
/// Pixels per point it is drawn at, so it is sharp on a HiDPI screen too.
const DENSITY: f32 = 2.0;
/// The whole screen in the lock screen stories, in points, shown smaller.
const SCREEN_W: f64 = 1280.0;
const SCREEN_H: f64 = 720.0;

// How long the bubble stays after a scan, as the agent has it.
const SUCCESS_HOLD: f64 = 1100.0;
const FAILURE_HOLD: f64 = 1700.0;
/// Typing a password: a dot every so often.
const KEY_GAP: f64 = 110.0;
const PASSWORD: usize = 8;

/// How the bubble looks, from the user's settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub pace: f64,
    pub minimal: bool,
}

/// What the stage plays, over and over. None for the bubble: it is off.
#[derive(Clone, Debug, PartialEq)]
pub enum Story {
    /// The desktop: the bubble opens, finds the face, closes.
    Unlock(Option<Style>),
    /// Another app has the camera.
    CameraBusy(Option<Style>),
    /// sudo in a terminal, with the bubble over it.
    Sudo(Option<Style>),
    /// Nobody in front of the camera: it looks this many seconds.
    Timeout(Option<Style>, u32),
    /// A lock screen.
    Lock(Lock),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Lock {
    pub bubble: Option<Style>,
    /// The face opens it.
    pub face: bool,
    pub when: When,
    /// The user's own lock screen (hyprlock and the like): a line of text
    /// at the top instead of the bubble.
    pub yours: bool,
    /// face-unlock's own lock screen: its wallpaper setting counts.
    pub own: bool,
    pub wallpaper: String,
    pub blur: bool,
}

/// What starts the scan.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum When {
    /// It is locked and it looks.
    Now,
    /// Somebody comes back: the mouse moves. Whether it looks then.
    Back(bool),
    /// The screen locks. Whether it looks right away, and whether it looks
    /// when somebody comes back.
    Locked(bool, bool),
}

// ---------------------------------------------------------------------------
// The stage's thread
// ---------------------------------------------------------------------------

/// The picture the window shows, and the story it should show.
pub struct Preview {
    pub pixels: rune::Pixels,
    want: Arc<Mutex<Option<Story>>>,
    /// No thread draws (a headless scene): `show` draws one frame itself.
    still: bool,
}

impl Preview {
    pub fn new(host: Option<Host>) -> Preview {
        let preview = Preview { pixels: rune::Pixels::new(), want: Arc::new(Mutex::new(None)), still: host.is_none() };
        if let Some(host) = host {
            let (pixels, want) = (preview.pixels.clone(), preview.want.clone());
            std::thread::spawn(move || run(host, pixels, want));
        }
        preview
    }

    /// What to play now; None stops it.
    pub fn show(&self, story: Option<Story>) {
        let mut want = self.want.lock().unwrap_or_else(|e| e.into_inner());
        if *want == story {
            return;
        }
        if self.still
            && let Some(s) = &story
        {
            // The same picture on every machine: no wallpaper.
            let mut stage = Stage::new(s.clone(), &mut Pictures { plain: true, ..Default::default() });
            let (w, h) = stage.size();
            self.pixels.set(w, h, stage.still());
        }
        *want = story;
    }
}

fn run(host: Host, pixels: rune::Pixels, want: Arc<Mutex<Option<Story>>>) {
    let mut pictures = Pictures::default();
    let mut stage: Option<Stage> = None;
    let mut start = Instant::now();
    loop {
        let wanted = want.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let Some(story) = wanted else {
            stage = None;
            std::thread::sleep(Duration::from_millis(100));
            continue;
        };
        if !host.app.is_visible() {
            std::thread::sleep(Duration::from_millis(250));
            continue;
        }
        if stage.as_ref().is_none_or(|s| s.story != story) {
            stage = Some(Stage::new(story, &mut pictures));
            start = Instant::now();
        }
        let Some(s) = stage.as_mut() else { continue };
        let began = Instant::now();
        let frame = s.frame(start.elapsed().as_secs_f64() * 1000.0);
        let (w, h) = s.size();
        pixels.set(w, h, frame);
        let wait = if s.plan.still { Duration::from_millis(250) } else { Duration::from_millis(16) };
        std::thread::sleep(wait.saturating_sub(began.elapsed()));
    }
}

/// The wallpaper at the stage's size, read once: as on the desktop, and
/// dimmed (and blurred if asked) as behind a lock screen.
#[derive(Default)]
struct Pictures {
    /// No wallpaper, only the gradient.
    plain: bool,
    found: HashMap<(String, bool), Option<PathBuf>>,
    made: HashMap<(Option<PathBuf>, bool, bool), Pixmap>,
}

impl Pictures {
    fn path(&mut self, setting: &str, own: bool) -> Option<PathBuf> {
        if self.plain {
            return None;
        }
        self.found
            .entry((setting.to_string(), own))
            .or_insert_with(|| {
                if own && !setting.is_empty() {
                    let set = face_unlock::wallpaper::for_lock(setting);
                    face_unlock::wallpaper::pick(&set, "").map(|p| p.to_path_buf())
                } else {
                    face_unlock::wallpaper::of_desktop()
                }
            })
            .clone()
    }

    fn get(&mut self, path: Option<PathBuf>, locked: bool, blur: bool) -> Pixmap {
        let (w, h) = px();
        self.made
            .entry((path.clone(), locked, blur))
            .or_insert_with(|| {
                let light = if locked { 0.65 } else { 1.0 };
                path.as_deref().and_then(|p| lockscreen::picture(p, w, h, blur && locked, light)).unwrap_or_else(|| {
                    let mut pm = Pixmap::new(w, h).expect("stage size");
                    let mut cv = Canvas::new(pm.as_mut(), w as f32 / SCREEN_W as f32);
                    lockscreen::gradient(&mut cv, SCREEN_W, SCREEN_H);
                    if !locked {
                        // A desktop without a picture: lighter than the lock.
                        cv.fill(rounded_rect(0.0, 0.0, SCREEN_W, SCREEN_H, 0.0), [0x40, 0x4c, 0x60], 0.35);
                    }
                    pm
                })
            })
            .clone()
    }
}

fn px() -> (u32, u32) {
    ((STAGE_W * DENSITY) as u32, (STAGE_H * DENSITY) as u32)
}

// ---------------------------------------------------------------------------
// The stories
// ---------------------------------------------------------------------------

/// When things happen in one round of a story, in ms from its start.
#[derive(Default)]
struct Plan {
    looks: Vec<(f64, Look)>,
    /// A round's length: then it starts over.
    round: f64,
    /// Nothing moves.
    still: bool,
    /// A good moment for a single picture.
    show: f64,
    lock_in: Option<f64>,
    unlock: Option<f64>,
    pointer: Option<f64>,
    typing: Option<f64>,
    /// hyprlock's line of text, from and to.
    line: Option<(f64, f64)>,
    /// sudo: the line of the PAM module, then the answer.
    sudo: Option<(f64, f64)>,
    /// The scan without a face, from and to.
    bar: Option<(f64, f64)>,
}

fn look(phase: Phase, face_seen: bool, message: &str, s: Style) -> Look {
    Look { phase, message: message.into(), face_seen, minimal: s.minimal, pace: s.pace }
}

/// How long the bubble takes to go once hidden.
fn closing(pace: f64) -> f64 {
    400.0 * pace + 100.0
}

/// A scan that finds the face: when it succeeds, and when the bubble is gone.
fn scan(p: &mut Plan, bubble: Option<Style>, at: f64) -> (f64, f64) {
    let success = at + 1600.0;
    let Some(s) = bubble else { return (success, success) };
    p.looks.push((at, look(Phase::Scanning, false, "", s)));
    p.looks.push((at + 800.0, look(Phase::Scanning, true, "", s)));
    p.looks.push((success, look(Phase::Success, true, "", s)));
    let hide = success + SUCCESS_HOLD * s.pace;
    p.looks.push((hide, look(Phase::Hidden, true, "", s)));
    (success, hide + closing(s.pace))
}

fn typed(p: &mut Plan, at: f64) -> f64 {
    p.typing = Some(at);
    let unlock = at + PASSWORD as f64 * KEY_GAP + 300.0;
    p.unlock = Some(unlock);
    p.round = unlock + 1500.0;
    p.show = at + 4.0 * KEY_GAP;
    unlock
}

fn plan(story: &Story) -> Plan {
    let mut p = Plan::default();
    match story {
        Story::Unlock(bubble) => {
            if bubble.is_none() {
                p.still = true;
                p.round = 1000.0;
            }
            let (_, end) = scan(&mut p, *bubble, 600.0);
            p.round = p.round.max(end + 900.0);
            p.show = 1900.0;
        }
        Story::CameraBusy(bubble) => match bubble {
            None => {
                p.still = true;
                p.round = 1000.0;
            }
            Some(s) => {
                p.looks.push((600.0, look(Phase::Busy, false, tr("Camera in use"), *s)));
                let hide = 600.0 + FAILURE_HOLD * s.pace;
                p.looks.push((hide, look(Phase::Hidden, false, tr("Camera in use"), *s)));
                p.round = hide + closing(s.pace) + 900.0;
                p.show = 1400.0;
            }
        },
        Story::Sudo(bubble) => {
            let (success, end) = scan(&mut p, *bubble, 1000.0);
            p.sudo = Some((1000.0, success + 100.0));
            p.round = end.max(success + 200.0) + 1500.0;
            p.show = 1900.0;
        }
        Story::Timeout(bubble, secs) => {
            let end = 600.0 + *secs as f64 * 1000.0;
            p.bar = Some((600.0, end));
            let pace = bubble.map(|s| s.pace).unwrap_or(1.0);
            if let Some(s) = bubble {
                p.looks.push((600.0, look(Phase::Scanning, false, "", *s)));
                p.looks.push((end, look(Phase::Hidden, false, "", *s)));
            }
            p.round = end + closing(pace) + 1200.0;
            p.show = 600.0 + *secs as f64 * 500.0;
        }
        Story::Lock(l) => {
            let up = match l.when {
                When::Locked(..) => {
                    p.lock_in = Some(800.0);
                    1100.0
                }
                _ => {
                    p.lock_in = Some(0.0);
                    300.0
                }
            };
            if !l.face {
                typed(&mut p, up + 600.0);
                return p;
            }
            let scan_at = match l.when {
                When::Now | When::Locked(true, _) => Some(up + 400.0),
                When::Back(scan) => {
                    p.pointer = Some(up + 900.0);
                    scan.then_some(up + 1300.0)
                }
                When::Locked(false, back) => {
                    p.pointer = Some(up + 1700.0);
                    back.then_some(up + 2100.0)
                }
            };
            let Some(at) = scan_at else {
                let pointer = p.pointer.unwrap_or(up);
                typed(&mut p, pointer + 900.0);
                return p;
            };
            let (success, end) = if l.yours {
                p.line = Some((at, at + 1600.0));
                (at + 1600.0, at + 1600.0)
            } else {
                scan(&mut p, l.bubble, at)
            };
            let unlock = success + 150.0;
            p.unlock = Some(unlock);
            p.round = (unlock + 1500.0).max(end + 600.0);
            p.show = at + 1200.0;
        }
    }
    p
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

struct Stage {
    story: Story,
    plan: Plan,
    shell: Shell,
    desktop: Pixmap,
    locked: Pixmap,
    bubble: Bubble,
    lock: LockScreen,
    round: u64,
    fired: usize,
}

impl Stage {
    fn new(story: Story, pictures: &mut Pictures) -> Stage {
        let (setting, own, blur) = match &story {
            Story::Lock(l) => (l.wallpaper.clone(), l.own, l.blur),
            _ => (String::new(), false, false),
        };
        let path = pictures.path(&setting, own);
        let on_desktop = pictures.path("", false).or(path.clone());
        let desktop = pictures.get(on_desktop, false, false);
        let locked = pictures.get(path, true, blur);
        let (login, name) = me();
        let mut lock = LockScreen::new(login, name);
        if let Story::Lock(l) = &story {
            lock.face_unlock = l.face && !l.yours;
        }
        let shell = if pictures.plain { Shell::plain() } else { Shell::here() };
        Stage { plan: plan(&story), story, shell, desktop, locked, bubble: Bubble::new(), lock, round: u64::MAX, fired: 0 }
    }

    fn size(&self) -> (u32, u32) {
        px()
    }

    /// Runs the story up to `t` ms into its round.
    fn step(&mut self, t: f64) -> f64 {
        let round = (t / self.plan.round) as u64;
        let at = t % self.plan.round;
        if round != self.round {
            self.round = round;
            self.bubble = Bubble::new();
            self.fired = 0;
        }
        while let Some((when, look)) = self.plan.looks.get(self.fired).filter(|(when, _)| *when <= at) {
            self.bubble.set(look, *when);
            self.fired += 1;
        }
        self.bubble.advance(at);
        at
    }

    fn frame(&mut self, t: f64) -> Vec<u8> {
        let at = self.step(t);
        self.draw(at)
    }

    /// One picture, at a moment that shows what the story is about.
    fn still(&mut self) -> Vec<u8> {
        let mut t = 0.0;
        while t < self.plan.show {
            self.step(t);
            t += 50.0;
        }
        let at = self.step(self.plan.show);
        self.draw(at)
    }

    fn draw(&mut self, at: f64) -> Vec<u8> {
        let (w, h) = px();
        let mut pm = Pixmap::new(w, h).expect("stage size");
        match self.story.clone() {
            Story::Unlock(_) | Story::CameraBusy(_) | Story::Timeout(..) => self.desktop(&mut pm, at),
            Story::Sudo(_) => self.terminal(&mut pm, at),
            Story::Lock(l) => self.locked(&mut pm, &l, at),
        }
        pm.take()
    }

    /// The whole desktop, with the bubble at the top in the middle.
    fn desktop(&self, pm: &mut Pixmap, at: f64) {
        pm.data_mut().copy_from_slice(self.desktop.data());
        let mut cv = Self::screen(pm);
        self.shell.draw(&mut cv, 1.0);
        if let Some((from, to)) = self.plan.bar {
            // A stopwatch in the corner, for how long it has looked.
            let secs = ((at - from).max(0.0) / 1000.0).min((to - from) / 1000.0);
            let (pw, ph) = (180.0, 66.0);
            let (x, y) = (SCREEN_W - pw - 28.0, SCREEN_H - 52.0 - ph - 24.0);
            cv.fill(rounded_rect(x, y, pw, ph, ph / 2.0), [0, 0, 0], 0.65);
            draw::text(&mut cv, &format!("{secs:.1} s"), x, y + 12.0, 34.0, 600, Some(pw as f32), Align::Center, [0xff, 0xff, 0xff], 0.95);
        }
        self.screen_bubble(&mut cv, at);
    }

    fn screen<'a>(pm: &'a mut Pixmap) -> Canvas<'a> {
        let scale = pm.width() as f32 / SCREEN_W as f32;
        Canvas::new(pm.as_mut(), scale)
    }

    /// The bubble at the top of the whole screen.
    fn screen_bubble(&self, cv: &mut Canvas, at: f64) {
        let t = cv.t;
        cv.translate((SCREEN_W - bubble::WIDTH) / 2.0, 0.0);
        self.bubble.draw(cv, at);
        cv.t = t;
    }

    /// sudo in a terminal on the desktop.
    fn terminal(&self, pm: &mut Pixmap, at: f64) {
        pm.data_mut().copy_from_slice(self.desktop.data());
        let mut cv = Self::screen(pm);
        self.shell.draw(&mut cv, 1.0);
        let (x, y, w, h) = (190.0, 300.0, 900.0, 340.0);
        cv.fill(rounded_rect(x, y, w, h, 12.0), [0x1c, 0x1e, 0x22], 0.97);
        cv.fill(rounded_rect(x, y, w, 36.0, 12.0), [0x2c, 0x2f, 0x35], 1.0);
        cv.fill(rounded_rect(x, y + 24.0, w, 12.0, 0.0), [0x2c, 0x2f, 0x35], 1.0);
        for (i, c) in [[0xff, 0x5f, 0x57], [0xfe, 0xbc, 0x2e], [0x28, 0xc8, 0x40]].iter().enumerate() {
            cv.fill(rounded_rect(x + 16.0 + i as f64 * 22.0, y + 11.0, 14.0, 14.0, 7.0), *c, 1.0);
        }
        let Some((pam, answer)) = self.plan.sudo else { return };
        let command = "sudo whoami";
        let typed = (((at - 200.0) / 70.0).max(0.0) as usize).min(command.len());
        let (lx, size, step) = (x + 32.0, 36.0, 52.0);
        let mut ly = y + 58.0;
        let green: Rgb = [0x7e, 0xe7, 0x87];
        let white: Rgb = [0xe8, 0xe8, 0xe8];
        draw::mono(&mut cv, &format!("$ {}", &command[..typed]), lx, ly, size, white, 1.0);
        ly += step;
        if at >= pam {
            draw::mono(&mut cv, tr("Look at the camera to unlock."), lx, ly, size, [0xb0, 0xb0, 0xb0], 1.0);
            ly += step;
        }
        if at >= answer {
            draw::mono(&mut cv, "root", lx, ly, size, white, 1.0);
            ly += step;
            draw::mono(&mut cv, "$", lx, ly, size, green, 1.0);
        }
        self.screen_bubble(&mut cv, at);
    }

    /// A lock screen, and the desktop once it opens.
    fn locked(&mut self, pm: &mut Pixmap, l: &Lock, at: f64) {
        // How much of the lock screen shows: it fades in when the screen
        // locks and out when it opens.
        let fade = |from: Option<f64>, ms: f64| from.map(|f| ((at - f) / ms).clamp(0.0, 1.0)).unwrap_or(0.0);
        let shown = fade(self.plan.lock_in, 300.0) * (1.0 - fade(self.plan.unlock, 350.0));
        pm.data_mut().copy_from_slice(self.desktop.data());
        if shown > 0.0 {
            pm.draw_pixmap(0, 0, self.locked.as_ref(), &PixmapPaint { opacity: shown as f32, ..Default::default() }, Transform::identity(), None);
        }
        let typed = self.plan.typing.map(|t| (((at - t) / KEY_GAP).floor().max(0.0) as usize).min(PASSWORD)).unwrap_or(0);
        self.lock.password = "•".repeat(typed);
        let mut cv = Self::screen(pm);
        self.shell.draw(&mut cv, 1.0 - shown);
        cv.layer(shown, 1.0, 0.0, 0.0, |cv| lockscreen::content(cv, SCREEN_W, SCREEN_H, &self.lock, at));
        // Before somebody comes back the screen has dimmed; it lights up
        // when the mouse moves.
        if let Some(p) = self.plan.pointer {
            let asleep = 1.0 - ((at - p) / 300.0).clamp(0.0, 1.0);
            let locked_for = self.plan.lock_in.map(|l| ((at - l - 600.0) / 500.0).clamp(0.0, 1.0)).unwrap_or(0.0);
            cv.fill(rounded_rect(0.0, 0.0, SCREEN_W, SCREEN_H, 0.0), [0, 0, 0], 0.55 * asleep * locked_for * shown);
        }
        if let Some((from, to)) = self.plan.line
            && at >= from
            && at < to
            && l.yours
        {
            let line = tr("Looking for your face…");
            let (tw, _) = draw::measure(line, 30.0, 500, None);
            draw::text(&mut cv, line, (SCREEN_W - tw as f64) / 2.0, 32.0, 30.0, 500, None, Align::Left, [0xff, 0xff, 0xff], shown);
        }
        if let Some(p) = self.plan.pointer {
            let moved = ((at - p) / 450.0).clamp(0.0, 1.0);
            let gone = ((at - p - 1600.0) / 300.0).clamp(0.0, 1.0);
            if at >= p && gone < 1.0 {
                let e = 1.0 - (1.0 - moved) * (1.0 - moved);
                pointer(&mut cv, 950.0 - 110.0 * e, 590.0 - 90.0 * e, 1.0 - gone);
            }
        }
        self.screen_bubble(&mut cv, at);
    }
}

// ---------------------------------------------------------------------------
// The desktop around it, as this one looks
// ---------------------------------------------------------------------------

/// How the desktop's panels sit, roughly: enough to tell it is yours.
#[derive(Clone, Copy, PartialEq)]
enum Layout {
    /// A panel at the bottom: Plasma, Cinnamon and most others.
    Bottom,
    /// GNOME's bar at the top.
    Gnome,
    /// Ubuntu's: GNOME's bar, and the dock on the left.
    Ubuntu,
    /// A thin bar at the top, as on Hyprland and niri.
    Tiling,
}

struct Shell {
    layout: Layout,
    /// The distribution's colour, from os-release.
    accent: Rgb,
    clock: String,
}

impl Shell {
    fn here() -> Shell {
        use face_unlock::system::{Desktop, os_release};
        let id = os_release("ID").unwrap_or_default();
        let ubuntu = id == "ubuntu";
        // Zorin keeps GNOME's panel at the bottom, as a taskbar.
        let zorin = id == "zorin";
        let layout = match face_unlock::system::desktop() {
            Desktop::Gnome if ubuntu => Layout::Ubuntu,
            Desktop::Gnome if zorin => Layout::Bottom,
            Desktop::Gnome => Layout::Gnome,
            Desktop::Hyprland | Desktop::Niri => Layout::Tiling,
            _ => Layout::Bottom,
        };
        let accent = face_unlock::wallpaper::distro_color().unwrap_or([0x3d, 0xae, 0xe9]);
        Shell { layout, accent, clock: lockscreen::now_text().0 }
    }

    /// The same on every machine, for pictures of the window.
    fn plain() -> Shell {
        Shell { layout: Layout::Bottom, accent: [0x3d, 0xae, 0xe9], clock: "9:41".into() }
    }

    fn draw(&self, cv: &mut Canvas, alpha: f64) {
        if alpha <= 0.001 {
            return;
        }
        let white: Rgb = [0xff, 0xff, 0xff];
        let clock = |cv: &mut Canvas, x: f64, y: f64, w: f64| draw::text(cv, &self.clock, x, y, 16.0, 600, Some(w as f32), Align::Center, white, 0.95 * alpha);
        match self.layout {
            Layout::Bottom => {
                // Floating, as Plasma 6 has it.
                let (y, h) = (SCREEN_H - 52.0, 44.0);
                cv.fill(rounded_rect(8.0, y, SCREEN_W - 16.0, h, 10.0), [0x1d, 0x20, 0x26], 0.92 * alpha);
                let mut c = tiny_skia::PathBuilder::new();
                c.push_circle(34.0, (y + h / 2.0) as f32, 13.0);
                cv.fill(c, self.accent, alpha);
                for i in 0..4 {
                    let x = 66.0 + i as f64 * 40.0;
                    cv.fill(rounded_rect(x, y + 9.0, 26.0, 26.0, 6.0), white, if i == 0 { 0.7 } else { 0.35 } * alpha);
                }
                cv.fill(rounded_rect(68.0, y + h - 5.0, 22.0, 3.0, 1.5), self.accent, alpha);
                for i in 0..3 {
                    cv.fill(rounded_rect(SCREEN_W - 150.0 + i as f64 * 22.0, y + 16.0, 12.0, 12.0, 6.0), white, 0.45 * alpha);
                }
                clock(cv, SCREEN_W - 90.0, y + 11.0, 70.0);
            }
            Layout::Gnome | Layout::Ubuntu => {
                cv.fill(rounded_rect(0.0, 0.0, SCREEN_W, 30.0, 0.0), [0, 0, 0], 0.95 * alpha);
                cv.fill(rounded_rect(14.0, 11.0, 26.0, 8.0, 4.0), white, 0.9 * alpha);
                for i in 0..2 {
                    cv.fill(rounded_rect(46.0 + i as f64 * 14.0, 11.0, 8.0, 8.0, 4.0), white, 0.45 * alpha);
                }
                clock(cv, SCREEN_W / 2.0 - 60.0, 5.0, 120.0);
                for i in 0..3 {
                    cv.fill(rounded_rect(SCREEN_W - 80.0 + i as f64 * 22.0, 9.0, 12.0, 12.0, 6.0), white, 0.8 * alpha);
                }
                if self.layout == Layout::Ubuntu {
                    cv.fill(rounded_rect(0.0, 30.0, 64.0, SCREEN_H - 30.0, 0.0), [0x1d, 0x1d, 0x1d], 0.85 * alpha);
                    for i in 0..5 {
                        let y = 44.0 + i as f64 * 54.0;
                        cv.fill(rounded_rect(12.0, y, 40.0, 40.0, 10.0), white, if i == 0 { 0.75 } else { 0.4 } * alpha);
                    }
                    cv.fill(rounded_rect(4.0, 58.0, 4.0, 12.0, 2.0), self.accent, alpha);
                    for row in 0..3 {
                        for col in 0..3 {
                            cv.fill(rounded_rect(21.0 + col as f64 * 8.0, SCREEN_H - 50.0 + row as f64 * 8.0, 5.0, 5.0, 2.5), white, 0.8 * alpha);
                        }
                    }
                }
            }
            Layout::Tiling => {
                cv.fill(rounded_rect(8.0, 8.0, SCREEN_W - 16.0, 30.0, 8.0), [0x1e, 0x1e, 0x2e], 0.88 * alpha);
                for i in 0..5 {
                    let on = i == 0;
                    cv.fill(rounded_rect(20.0 + i as f64 * 24.0, 17.0, if on { 18.0 } else { 12.0 }, 12.0, 6.0), if on { self.accent } else { white }, if on { 1.0 } else { 0.4 } * alpha);
                }
                clock(cv, SCREEN_W / 2.0 - 60.0, 13.0, 120.0);
                for i in 0..3 {
                    cv.fill(rounded_rect(SCREEN_W - 84.0 + i as f64 * 22.0, 17.0, 12.0, 12.0, 6.0), white, 0.6 * alpha);
                }
            }
        }
    }
}

/// A mouse pointer with its tip at (x, y).
fn pointer(cv: &mut Canvas, x: f64, y: f64, alpha: f64) {
    let shape = [[0.0, 0.0], [0.0, 26.0], [7.0, 20.0], [12.0, 31.0], [17.0, 29.0], [12.0, 18.0], [21.0, 18.0]];
    let pts: Vec<[f64; 2]> = shape.iter().map(|p| [x + p[0] * 1.4, y + p[1] * 1.4]).collect();
    let mut path = draw::polyline(&pts);
    path.close();
    let mut outline = draw::polyline(&pts);
    outline.close();
    cv.fill(path, [0xff, 0xff, 0xff], alpha);
    cv.stroke(outline, [0, 0, 0], alpha, 2.0);
}

/// This user's login and full name, as the lock screen greets them.
fn me() -> (String, String) {
    // SAFETY: getpwuid returns a pointer into the C library's own buffer,
    // read here before anything else can call it on this thread.
    unsafe {
        let pw = libc::getpwuid(libc::getuid());
        if pw.is_null() {
            return (String::new(), String::new());
        }
        let s = |p: *const libc::c_char| if p.is_null() { String::new() } else { std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned() };
        let login = s((*pw).pw_name);
        let gecos = s((*pw).pw_gecos);
        let name = gecos.split(',').next().filter(|n| !n.is_empty()).unwrap_or(&login).to_string();
        (login, name)
    }
}

// ---------------------------------------------------------------------------
// The card above the pane
// ---------------------------------------------------------------------------

/// What the card shows: a heading, a line on what is seen, and the stage.
struct Shown {
    title: String,
    caption: String,
    stage: Content,
}

enum Content {
    Story(Story),
    Picture(El),
}

fn rgb(hex: &str) -> Color {
    rune::color::hex(hex).unwrap_or(Color::BLACK)
}

fn on_off(on: bool) -> &'static str {
    if on { tr("on") } else { tr("off") }
}

fn heading(title: &str, value: &str) -> String {
    format!("{title}: {value}")
}

fn user_on(ui: &Ui, key: &str, default: bool) -> bool {
    face_unlock::keyvalue::parse_bool(ui.snap.user.get(key, ""), default)
}

fn sys_on(ui: &Ui, key: &str, default: bool) -> bool {
    face_unlock::keyvalue::parse_bool(ui.snap.sys.get(key, ""), default)
}

/// The bubble as this user has it, or None when it is off.
fn style(ui: &Ui) -> Option<Style> {
    user_on(ui, "Bubble", true).then(|| Style { pace: ui.snap.cfg.pace() as f64, minimal: ui.snap.cfg.bubble_style == "minimal" })
}

/// The setting the preview is about: the one changed last on this pane, or
/// the pane's first.
fn focus(ui: &Ui, keys: &[&'static str]) -> &'static str {
    ui.focus.filter(|k| keys.contains(k)).unwrap_or(keys[0])
}

/// The card, pinned above the pane's settings. None on panes without one.
pub fn card(ui: &Ui, p: Paint) -> Option<El> {
    let shown = if ui.mode != Mode::Main || ui.sub.is_some() {
        None
    } else {
        match ui.pane {
            Pane::Bubble => Some(bubble_pane(ui)),
            Pane::LockScreen => Some(lock_pane(ui)),
            Pane::Recognition => Some(recognition_pane(ui, p)),
            Pane::Prompts => Some(prompts_pane(ui, p)),
            _ => None,
        }
    };
    let Some(shown) = shown else {
        ui.preview.show(None);
        return None;
    };
    let stage = match shown.stage {
        Content::Story(story) => {
            ui.preview.show(Some(story));
            el().key("stage").size(STAGE_W, STAGE_H).radius(radius::MD).fill(Color::BLACK).pixels_fit(&ui.preview.pixels, SurfaceFit::Fill).a11y_hidden()
        }
        Content::Picture(picture) => {
            ui.preview.show(None);
            picture
        }
    };
    let copy = col()
        .key("copy")
        .grow(1.0)
        .shrink()
        .min_w(120.0)
        .gap(4.0)
        .child(text(tr("Preview")).key("label").font(TextStyle::new(11.0, 600.0)).color(p.faint()))
        .child(text(shown.title).key("title").font(TextStyle::new(13.0, 600.0)).color(p.text()).wrap())
        .child(prose(p, &shown.caption).key("caption").font(DESCRIPTION).color(p.muted()).wrap());
    Some(raised(p, "preview").mx(GUTTER).mb(16.0).child(row().key("row").padding(10.0).gap(16.0).center_y().child(stage.shrink()).child(copy)))
}

fn bubble_pane(ui: &Ui) -> Shown {
    let s = &ui.snap;
    let bubble = style(ui);
    let key = focus(ui, &["AnimationSpeed", "Bubble", "BubbleStyle", "BubbleScreen", "BubbleForPrompts", "QuietWhenCameraBusy"]);
    let unlock_caption = if bubble.is_some() {
        tr("What you see when your face unlocks the screen.")
    } else {
        tr("No bubble. Your face still unlocks the screen, you just do not see it.")
    };
    match key {
        "QuietWhenCameraBusy" => {
            let quiet = user_on(ui, key, false);
            let caption = match (bubble, quiet) {
                (None, _) => unlock_caption,
                (Some(_), true) => tr("Another app uses the camera: no bubble, and you type your password."),
                (Some(_), false) => tr("Another app uses the camera: the bubble shows it, and you type your password."),
            };
            Shown { title: heading(tr("Quiet while the camera is in use"), on_off(quiet)), caption: caption.into(), stage: Content::Story(Story::CameraBusy(bubble.filter(|_| !quiet))) }
        }
        "BubbleForPrompts" => {
            let on = user_on(ui, key, true);
            let caption = if bubble.is_some() && on { tr("sudo looks for your face, and the bubble shows it.") } else { tr("sudo looks for your face without the bubble. The terminal says so.") };
            Shown { title: heading(tr("Also for sudo and admin prompts"), on_off(on)), caption: caption.into(), stage: Content::Story(Story::Sudo(bubble.filter(|_| on))) }
        }
        _ => {
            let title = match key {
                "Bubble" => heading(tr("Show the bubble"), on_off(bubble.is_some())),
                "BubbleStyle" => heading(tr("Style"), if s.cfg.bubble_style == "minimal" { tr("small pill with a lock") } else { tr("island with the face") }),
                "BubbleScreen" => heading(tr("Screen"), &if s.cfg.bubble_screen.is_empty() { tr("follows the mouse").to_string() } else { s.cfg.bubble_screen.clone() }),
                _ => heading(
                    tr("Animation speed"),
                    match s.cfg.animation_speed.as_str() {
                        "slow" => tr("slow"),
                        "fast" => tr("fast"),
                        _ => tr("normal"),
                    },
                ),
            };
            Shown { title, caption: unlock_caption.into(), stage: Content::Story(Story::Unlock(bubble)) }
        }
    }
}

fn lock_pane(ui: &Ui) -> Shown {
    let s = &ui.snap;
    let own_locker = s.desktop.own_locker();
    let own = own_locker && s.cfg.lock_screen_style == "own";
    let yours = own_locker && s.cfg.lock_screen_style == "yours";
    let face = user_on(ui, "LockScreen", true);
    let wake = user_on(ui, "ScanOnWake", true);
    let at_lock = user_on(ui, "ScanOnLock", false);
    let key = focus(ui, &["LockScreen", "ScanOnWake", "ScanOnLock", "LockScreenStyle", "LockWallpaper", "LockBlur"]);
    let when = match key {
        "ScanOnWake" => When::Back(wake),
        "ScanOnLock" => When::Locked(at_lock, wake),
        _ => When::Now,
    };
    let lock = Lock { bubble: style(ui), face, when, yours, own, wallpaper: s.cfg.lock_wallpaper.clone(), blur: s.cfg.lock_blur };
    let off = tr("Only your password opens the lock screen.");
    let (title, caption) = match key {
        "ScanOnWake" => (
            heading(tr("Scan when you come back"), on_off(wake)),
            if !face {
                off
            } else if wake {
                tr("When you press a key or move the mouse, it looks for your face.")
            } else {
                tr("When you come back, it does not look. You type your password.")
            },
        ),
        "ScanOnLock" => (
            heading(tr("Scan right after locking"), on_off(at_lock)),
            if !face {
                off
            } else if at_lock {
                tr("Right after locking it looks for your face, so the screen opens again while you sit there.")
            } else {
                tr("Right after locking it waits until you come back.")
            },
        ),
        "LockScreenStyle" => (
            heading(tr("Which lock screen"), if own { tr("face-unlock's") } else { tr("yours") }),
            if own {
                tr("Shows the bubble while it looks for your face. You lock with `face-unlock lock`.")
            } else {
                tr("Stays as it is. hyprlock shows a line of text at the top, and Enter on the empty password field scans.")
            },
        ),
        "LockWallpaper" => (
            heading(tr("Wallpaper"), if s.cfg.lock_wallpaper.is_empty() { tr("same as desktop") } else { &s.cfg.lock_wallpaper }),
            tr("The picture behind face-unlock's own lock screen. Empty: the one on your desktop. Or a file, a folder to take one from at random, or `none`."),
        ),
        "LockBlur" => (heading(tr("Blur the wallpaper"), on_off(s.cfg.lock_blur)), tr("Blurs the picture behind face-unlock's own lock screen.")),
        _ => (heading(tr("Unlock with your face"), on_off(face)), if face { tr("Your face opens the lock screen as soon as it is found.") } else { off }),
    };
    Shown { title, caption: caption.into(), stage: Content::Story(Story::Lock(lock)) }
}

// ---------------------------------------------------------------------------
// Pictures of who gets in
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Verdict {
    In,
    Out,
    Maybe,
    /// Nothing happens: neither good nor bad.
    Skip,
}

impl Verdict {
    fn key(self) -> &'static str {
        match self {
            Verdict::In => "in",
            Verdict::Out => "out",
            Verdict::Maybe => "maybe",
            Verdict::Skip => "skip",
        }
    }

    /// Its colour and its mark. Softer than the kit's states, which are
    /// made for small icons and glare at this size.
    fn tone(self, p: Paint) -> (Color, &'static rune::Icon) {
        let ok = |l: f32, c: f32, h: f32| rune::color::oklch(l, c, h);
        match (self, p.dark) {
            (Verdict::In, true) => (ok(0.80, 0.15, 152.0), &icons::CHECK),
            (Verdict::In, false) => (ok(0.55, 0.15, 150.0), &icons::CHECK),
            (Verdict::Out, true) => (ok(0.72, 0.15, 25.0), &icons::CROSS),
            (Verdict::Out, false) => (ok(0.55, 0.19, 27.0), &icons::CROSS),
            (Verdict::Maybe, true) => (ok(0.84, 0.13, 80.0), &icons::QUESTION),
            (Verdict::Maybe, false) => (ok(0.62, 0.13, 65.0), &icons::QUESTION),
            (Verdict::Skip, _) => (p.muted(), &icons::MINUS),
        }
    }
}

/// Something held up to the camera, in a viewfinder, with what face-unlock
/// does with it. A change pops the badge, so the eye goes to what changed.
fn tile(p: Paint, key: &str, glyph: &'static rune::Icon, label: &str, verdict: Verdict, said: &str) -> El {
    const SHOT: f32 = 68.0;
    const BADGE: f32 = 26.0;
    let (tone, mark) = verdict.tone(p);
    let shot = el()
        .key("shot")
        .size(SHOT, SHOT)
        .radius(20.0)
        .center()
        .fill(p.t.card)
        .child(icon(&icons::VIEWFINDER, SHOT).key("corners").color(tone).absolute(0.0, 0.0).motion(motion::SNAPPY))
        .child(icon(glyph, 28.0).key("glyph").color(p.text()))
        // Over the viewfinder's bottom right corner, which it stands in for.
        .child(
            el().key(format!("badge-{}", verdict.key()))
                .size(BADGE, BADGE)
                .radius(BADGE / 2.0)
                .center()
                .fill(tone)
                .border(2.5, p.t.control_track)
                .absolute(SHOT * 21.0 / 24.0 - BADGE / 2.0, SHOT * 21.0 / 24.0 - BADGE / 2.0)
                .transition(Transition::scale(0.6).allow_small_scale())
                .child(icon(mark, 14.0).key("mark").color(p.t.card)),
        );
    // The tiles are as tall as the tallest, and each says what happens at
    // its bottom, so the answers stand in one line however long the words.
    col()
        .key(key)
        .w(100.0)
        .gap(10.0)
        .items_center()
        .child(shot)
        .child(text(label).key("label").font(TextStyle::new(11.5, 500.0).lh(15.0)).color(p.text()).w_full().wrap().align(rune::Align::Center))
        .child(el().key("push").grow(1.0))
        .child(text(said).key(format!("said-{}", verdict.key())).font(TextStyle::new(11.5, 650.0).lh(15.0)).color(tone).w_full().wrap().align(rune::Align::Center))
}

fn tiles(p: Paint, list: Vec<El>) -> El {
    let mut r = row().key("tiles").gap(6.0).items_stretch();
    for t in list {
        r = r.child(t);
    }
    el().key("stage").size(STAGE_W, STAGE_H).radius(radius::MD).fill(p.t.control_track).center().child(r).a11y_hidden()
}

fn recognition_pane(ui: &Ui, p: Paint) -> Shown {
    let s = &ui.snap;
    let key = focus(ui, &["Liveness", "Strictness", "Attention", "Camera", "ScanSeconds", "Adapt", "SkipLidClosed"]);
    let (yes, no, maybe) = (tr("unlocks"), tr("blocked"), tr("sometimes"));
    match key {
        "ScanSeconds" => {
            let secs: u32 = s.sys.get("ScanSeconds", "5").parse().unwrap_or(5);
            Shown {
                title: heading(tr("How long a scan lasts"), &trf("%s seconds", &[&secs])),
                caption: trf("It looks for %s seconds. If it does not find you, you type your password.", &[&secs]),
                stage: Content::Story(Story::Timeout(style(ui), secs)),
            }
        }
        "Strictness" => {
            let strict = s.sys.get("Strictness", "normal");
            let (glasses, alike) = match strict {
                "strict" => ((Verdict::Maybe, maybe), (Verdict::Out, no)),
                "relaxed" => ((Verdict::In, yes), (Verdict::Maybe, maybe)),
                _ => ((Verdict::In, yes), (Verdict::Out, no)),
            };
            let label = match strict {
                "strict" => tr("strict"),
                "relaxed" => tr("relaxed"),
                _ => tr("normal"),
            };
            let caption = match strict {
                "strict" => tr("Fewer wrong matches, but it may not know you with glasses or in bad light."),
                "relaxed" => tr("Knows you more easily, but also somebody who looks a lot like you."),
                _ => tr("The default. Right for most people."),
            };
            Shown {
                title: heading(tr("How closely the face has to match"), label),
                caption: caption.into(),
                stage: Content::Picture(tiles(
                    p,
                    vec![
                        tile(p, "you", &icons::FACE, tr("Your face"), Verdict::In, yes),
                        tile(p, "glasses", &icons::GLASSES, tr("With glasses or in bad light"), glasses.0, glasses.1),
                        tile(p, "alike", &icons::USERS, tr("Somebody who looks like you"), alike.0, alike.1),
                    ],
                )),
            }
        }
        "Attention" => {
            let on = sys_on(ui, key, true);
            let away = if on { (Verdict::Out, no) } else { (Verdict::In, yes) };
            Shown {
                title: heading(tr("Only when you look at the screen"), on_off(on)),
                caption: tr("Your eyes have to be open and on the screen. So it does not unlock while you look away or sleep.").into(),
                stage: Content::Picture(tiles(
                    p,
                    vec![
                        tile(p, "looking", &icons::EYE, tr("Looking at the screen"), Verdict::In, yes),
                        tile(p, "away", &icons::EYE_OFF, tr("Eyes closed or looking away"), away.0, away.1),
                    ],
                )),
            }
        }
        "Camera" => {
            let name = match &s.status {
                Some(st) if st.camera_present => if st.camera_name.is_empty() { st.camera_path.clone() } else { st.camera_name.clone() },
                Some(_) => tr("none found").into(),
                None => "–".into(),
            };
            let picture = el().key("stage").size(STAGE_W, STAGE_H).radius(radius::MD).fill(p.t.control_track).center().child(
                col()
                    .key("camera")
                    .gap(10.0)
                    .items_center()
                    .px(16.0)
                    .child(el().key("badge").size(52.0, 52.0).radius(26.0).center().fill(p.t.card).child(icon(&icons::CAMERA, 26.0).key("glyph").color(p.text())))
                    .child(text(name.clone()).key("name").font(TextStyle::new(13.0, 600.0)).color(p.text()).wrap().align(rune::Align::Center))
                    .child(button(p, "try", tr("Try it"), Kind::Secondary, true, |s: &mut Ui| s.show_pane(Pane::Try))),
            );
            Shown {
                title: heading(tr("Camera"), &name),
                caption: tr("Automatic takes the first normal camera. After a change, set up your face again. An infrared camera needs its light on (linux-enable-ir-emitter).").into(),
                stage: Content::Picture(picture),
            }
        }
        "Adapt" => {
            let on = sys_on(ui, key, true);
            let new_look = if on { (Verdict::In, yes) } else { (Verdict::Maybe, maybe) };
            Shown {
                title: heading(tr("Learn from every unlock"), on_off(on)),
                caption: tr("After a sure match it keeps how you look now. So a new haircut or glasses need no new setup.").into(),
                stage: Content::Picture(tiles(
                    p,
                    vec![
                        tile(p, "you", &icons::FACE, tr("Your face"), Verdict::In, yes),
                        tile(p, "new", &icons::GLASSES, tr("New glasses or haircut"), new_look.0, new_look.1),
                    ],
                )),
            }
        }
        "SkipLidClosed" => {
            let on = sys_on(ui, key, true);
            let closed = if on { (Verdict::Skip, tr("no scan")) } else { (Verdict::Maybe, tr("scans anyway")) };
            Shown {
                title: heading(tr("Not when the lid is closed"), on_off(on)),
                caption: tr("No scan while the laptop is closed, for example at a desk with an external screen.").into(),
                stage: Content::Picture(tiles(
                    p,
                    vec![
                        tile(p, "open", &icons::LAPTOP, tr("Lid open"), Verdict::In, tr("scans")),
                        tile(p, "closed", &icons::MONITOR, tr("Lid closed"), closed.0, closed.1),
                    ],
                )),
            }
        }
        _ => {
            let level = s.sys.get("Liveness", "light");
            let (phone, print, you) = match level {
                "heavy" => ((Verdict::Out, no), (Verdict::Out, no), (Verdict::In, tr("after a blink"))),
                "off" => ((Verdict::In, yes), (Verdict::In, yes), (Verdict::In, yes)),
                _ => ((Verdict::Out, no), (Verdict::Maybe, maybe), (Verdict::In, yes)),
            };
            let caption = match level {
                "heavy" => tr("You have to blink or turn your head a little. This also stops printed photos."),
                "off" => tr("No check at all. Only for trying out a camera."),
                _ => tr("Stops photos on a phone, a tablet or glossy paper. You do not have to blink. A matte printed photo can get through."),
            };
            Shown {
                title: heading(tr("Photo check"), crate::panes::liveness_label(level)),
                caption: caption.into(),
                stage: Content::Picture(tiles(
                    p,
                    vec![
                        tile(p, "phone", &icons::PHONE, tr("Your photo on a phone"), phone.0, phone.1),
                        tile(p, "print", &icons::PHOTO, tr("Your photo on paper"), print.0, print.1),
                        tile(p, "you", &icons::FACE, tr("Your face"), you.0, you.1),
                    ],
                )),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// What a password prompt shows
// ---------------------------------------------------------------------------

/// A terminal with `lines`: (text, colour).
fn terminal(p: Paint, lines: &[(String, Color)]) -> El {
    let mono = TextStyle { size: 12.0, line_height: 18.0, ..p.k.ty.mono };
    let mut body = col().key("lines").px(14.0).py(10.0).gap(2.0);
    for (i, (line, color)) in lines.iter().enumerate() {
        body = body.child(text(line.clone()).key(format!("l{i}")).font(mono).color(*color).wrap());
    }
    let dots = row()
        .key("bar")
        .h(24.0)
        .px(10.0)
        .gap(6.0)
        .center_y()
        .fill(rgb("#2c2f35"))
        .children(["#ff5f57", "#febc2e", "#28c840"].iter().enumerate().map(|(i, c)| el().key(format!("d{i}")).size(9.0, 9.0).radius(4.5).fill(rgb(c))));
    el().key("stage")
        .size(STAGE_W, STAGE_H)
        .radius(radius::MD)
        .fill(rgb("#1c1e22"))
        .clip_rounded(radius::MD)
        .child(col().key("term").w_full().child(dots).child(body))
        .a11y_hidden()
}

fn prompts_pane(ui: &Ui, p: Paint) -> Shown {
    let s = &ui.snap;
    let key = focus(ui, &["sudo", "polkit-1", "lockscreens", "SshSessions"]);
    let (login, _) = me();
    let white = rgb("#e8e8e8");
    let grey = rgb("#b0b0b0");
    let green = rgb("#7ee787");
    let off = tr("Off: you type your password.");
    let sudo_lines = |face: bool, mut lines: Vec<(String, Color)>| {
        lines.push(("$ sudo whoami".into(), white));
        if face {
            lines.push((tr("Look at the camera to unlock.").into(), grey));
            lines.push(("root".into(), white));
            lines.push(("$".into(), green));
        } else {
            // sudo's own words, not face-unlock's.
            lines.push((format!("[sudo] password for {login}:"), white));
        }
        lines
    };
    match key {
        "polkit-1" => {
            let on = s.pam.polkit;
            let answer = if on {
                row()
                    .key("face")
                    .gap(8.0)
                    .center_y()
                    .child(icon(&icons::SCAN_FACE, 18.0).key("icon").color(p.accent))
                    .child(text(tr("Look at the camera to unlock.")).key("line").font(TextStyle::new(11.5, 500.0)).color(p.text()).wrap().shrink())
            } else {
                el().key("field").w_full().h(28.0).px(10.0).center_y().radius(radius::SM).fill(p.t.control_off).child(text(tr("Password")).key("ph").font(TextStyle::new(11.5, 400.0)).color(p.muted()))
            };
            let dialog = col()
                .key("dialog")
                .w(250.0)
                .padding(14.0)
                .gap(10.0)
                .radius(radius::LG)
                .fill(p.t.card)
                .border(p.hair, p.t.card_border)
                .shadow(8.0, 24.0, p.t.shadow_soft)
                .child(
                    row()
                        .key("head")
                        .gap(8.0)
                        .center_y()
                        .child(icon(&icons::SHIELD, 18.0).key("icon").color(p.text()))
                        .child(text(tr("An app wants to make changes")).key("title").font(TextStyle::new(12.0, 600.0)).color(p.text()).wrap().shrink()),
                )
                .child(answer);
            Shown {
                title: heading(tr("Admin prompts"), on_off(on)),
                caption: if on { tr("The password windows of your desktop and apps, for example when you install software. No match: you type the password.") } else { off }.into(),
                stage: Content::Picture(el().key("stage").size(STAGE_W, STAGE_H).radius(radius::MD).fill(p.t.control_track).center().child(dialog).a11y_hidden()),
            }
        }
        "lockscreens" => {
            let on = s.pam.lockers_on;
            let (time, _) = lockscreen::now_text();
            let white = Color::WHITE;
            let screen = el()
                .key("stage")
                .size(STAGE_W, STAGE_H)
                .radius(radius::MD)
                .fill(rgb("#11161d"))
                .center()
                .child(
                    col()
                        .key("lock")
                        .gap(10.0)
                        .items_center()
                        .child(text(time).key("clock").font(TextStyle::new(34.0, 300.0)).color(white))
                        .child(el().key("field").size(150.0, 26.0).radius(13.0).fill(Color::WHITE.alpha(0.18)))
                        .child(
                            text(if on { tr("Press Enter on the empty field to scan.") } else { tr("Type your password") })
                                .key("line")
                                .font(TextStyle::new(11.0, 500.0))
                                .color(white.alpha(0.8)),
                        ),
                )
                .a11y_hidden();
            let help = trf("The lock screen takes your face in its password check. Press Enter on the empty field to scan. Found here: %s.", &[&s.pam.lockers.join(", ")]);
            Shown { title: heading(tr("Lock screens"), on_off(on)), caption: if on { help } else { off.into() }, stage: Content::Picture(screen) }
        }
        "SshSessions" => {
            let on = sys_on(ui, key, false);
            let host = std::fs::read_to_string("/etc/hostname").map(|h| h.trim().to_string()).unwrap_or_else(|_| "pc".into());
            let lines = sudo_lines(on && s.pam.sudo, vec![(format!("$ ssh {login}@{host}"), white)]);
            Shown {
                title: heading(tr("In SSH sessions"), on_off(on)),
                caption: if on {
                    tr("sudo in an SSH session takes your face too, if you sit at this computer. Off by default: the person at the camera may not be the one typing.")
                } else {
                    off
                }
                .into(),
                stage: Content::Picture(terminal(p, &lines)),
            }
        }
        _ => {
            let on = s.pam.sudo;
            Shown {
                title: heading(tr("sudo in a terminal"), on_off(on)),
                caption: if on { tr("sudo takes your face instead of the password. No match: you type the password as usual.") } else { off }.into(),
                stage: Content::Picture(terminal(p, &sudo_lines(on, vec![]))),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Draws every story frame by frame into $FU_SHOTS/preview-*.png, for
    /// looking at. Does nothing without FU_SHOTS.
    #[test]
    fn frames() {
        let Some(dir) = std::env::var_os("FU_SHOTS") else { return };
        let dir = std::path::PathBuf::from(dir);
        let s = Some(Style { pace: 1.3, minimal: false });
        let lock = |face, when, yours| Story::Lock(Lock { bubble: s, face, when, yours, own: false, wallpaper: String::new(), blur: false });
        let stories = [
            ("unlock", Story::Unlock(s)),
            ("busy", Story::CameraBusy(s)),
            ("sudo", Story::Sudo(s)),
            ("timeout", Story::Timeout(s, 3)),
            ("lock", lock(true, When::Now, false)),
            ("lock-off", lock(false, When::Now, false)),
            ("back", lock(true, When::Back(true), false)),
            ("back-off", lock(true, When::Back(false), false)),
            ("locked", lock(true, When::Locked(true, true), false)),
            ("locked-off", lock(true, When::Locked(false, true), false)),
            ("yours", lock(true, When::Now, true)),
        ];
        let mut pictures = Pictures::default();
        for (name, layout) in [("bottom", Layout::Bottom), ("gnome", Layout::Gnome), ("ubuntu", Layout::Ubuntu), ("tiling", Layout::Tiling)] {
            let mut stage = Stage::new(Story::Sudo(s), &mut pictures);
            stage.shell.layout = layout;
            let (w, h) = stage.size();
            let pm = Pixmap::from_vec(stage.still(), tiny_skia::IntSize::from_wh(w, h).unwrap()).unwrap();
            pm.save_png(dir.join(format!("shell-{name}.png"))).unwrap();
        }
        for (name, story) in stories {
            let mut stage = Stage::new(story, &mut pictures);
            let (w, h) = stage.size();
            let round = stage.plan.round;
            let mut t = 0.0;
            while t < round {
                let frame = stage.frame(t);
                if (t as u64).is_multiple_of(400) {
                    let pm = Pixmap::from_vec(frame, tiny_skia::IntSize::from_wh(w, h).unwrap()).unwrap();
                    pm.save_png(dir.join(format!("preview-{name}-{:05}.png", t as u64))).unwrap();
                }
                t += 50.0;
            }
        }
    }
}
