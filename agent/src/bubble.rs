//! The bubble: a black island at the top of the screen that slides down,
//! opens up, shows the face while it looks, and closes again.
//!
//! The same shapes and timing as the GNOME Shell extension
//! (res/gnome-shell), which draws it there. Entering, the island slides in
//! first and grows a moment later; leaving, it shrinks first and slides away
//! as it finishes. While it scans the content breathes, so it reads as
//! looking rather than stuck. "full" is the open island with the face in it,
//! "minimal" a small pill with a lock on one side and the face on the other.

use crate::draw::{self, Align, Canvas, Ease, OUT_BACK, Rgb, Tw, Tween, rounded_rect, polyline, sequence, smooth, soft};
use std::f64::consts::PI;
use tiny_skia::PathBuilder;

pub const SUCCESS: Rgb = [0x30, 0xd1, 0x58];
pub const FAILURE: Rgb = [0xff, 0x45, 0x3a];
pub const PANEL: Rgb = [0, 0, 0];
pub const TEXT_PRIMARY: Rgb = [0xff, 0xff, 0xff];
pub const TEXT_SECONDARY: Rgb = [0x94, 0x94, 0x94];
pub const TEXT_DETAIL: Rgb = [0xbd, 0xbd, 0xbd];

const CLOSED_W: f64 = 80.0;
const CLOSED_H: f64 = 24.0;
pub const OPEN_W: f64 = 180.0;
pub const OPEN_H: f64 = 180.0;
const OPEN_RADIUS: f64 = 48.0;
const MINIMAL_W: f64 = 150.0;
const MINIMAL_H: f64 = 40.0;
pub const TOP_GAP: f64 = 6.0;
const SLIDE_IN: f64 = 280.0;
const EXPAND_DELAY: f64 = 150.0;
const GROW: f64 = 460.0;
const SHRINK: f64 = 280.0;
const SLIDE_OUT_DELAY: f64 = 150.0;
const SLIDE_OUT: f64 = 240.0;

/// The surface it is drawn on. Big enough for the open island and its
/// shadow; only the content moves, the surface never changes size.
pub const WIDTH: f64 = 420.0;
pub const HEIGHT: f64 = 300.0;

// How much of each level soft() takes, to look like Qt's MultiEffect: the
// glow round the rings, and the island's shadow.
const GLOW_BLUR: [f32; 4] = [0.0, 0.42, 0.35, 0.23];
const SHADOW_BLUR: [f32; 4] = [0.0, 0.52, 0.16, 0.32];

const SHAKE: [(f64, f64, Ease); 6] = [
    (-9.0, 55.0, Ease::OutQuad),
    (8.0, 70.0, Ease::InOutQuad),
    (-6.0, 65.0, Ease::InOutQuad),
    (4.0, 60.0, Ease::InOutQuad),
    (-2.0, 55.0, Ease::InOutQuad),
    (0.0, 50.0, Ease::OutQuad),
];

pub const PILL_SHAKE: [(f64, f64, Ease); 5] = [
    (-10.0, 55.0, Ease::OutQuad),
    (9.0, 70.0, Ease::Linear),
    (-6.0, 65.0, Ease::Linear),
    (4.0, 60.0, Ease::Linear),
    (0.0, 55.0, Ease::OutQuad),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Hidden,
    Scanning,
    Success,
    Failure,
    Lockout,
    Busy,
}

impl Phase {
    pub fn name(self) -> &'static str {
        match self {
            Phase::Hidden => "hidden",
            Phase::Scanning => "scanning",
            Phase::Success => "success",
            Phase::Failure => "failure",
            Phase::Lockout => "lockout",
            Phase::Busy => "busy",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Idle,
    Scanning,
    Tracking,
    Success,
    Failure,
    Lockout,
    Busy,
}

/// What the bubble is to show.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub phase: Phase,
    pub message: String,
    pub face_seen: bool,
    pub minimal: bool,
    pub pace: f64,
}

fn rgb(c: [f64; 3]) -> Rgb {
    [c[0].round() as u8, c[1].round() as u8, c[2].round() as u8]
}

fn rgbf(c: Rgb) -> [f64; 3] {
    [c[0] as f64, c[1] as f64, c[2] as f64]
}

// ---------------------------------------------------------------------------
// A padlock that opens
// ---------------------------------------------------------------------------

struct LockGlyph {
    openness: Tw,
    open: bool,
    pace: f64,
}

impl LockGlyph {
    fn new() -> Self {
        LockGlyph { openness: Tw::one(0.0), open: false, pace: 1.0 }
    }

    fn set_open(&mut self, open: bool, at: f64) {
        if open == self.open {
            return;
        }
        self.open = open;
        self.openness.hold(0.0);
        if open {
            self.openness.to(1.0, 560.0 * self.pace, Ease::Linear, at);
        }
    }

    fn busy(&self, at: f64) -> bool {
        self.openness.busy(at)
    }

    fn draw(&self, cv: &mut Canvas, x: f64, y: f64, size: f64, color: Rgb, at: f64) {
        let u = size / 100.0;
        let o = self.openness.v(at);
        let lift = 12.0 * smooth(0.0, 0.3, o);
        let p = ((o - 0.22) / 0.78).clamp(0.0, 1.0);
        let s = 1.4;
        let swing = 180.0 * (1.0 + (s + 1.0) * (p - 1.0).powi(3) + s * (p - 1.0).powi(2));
        let beat = 1.0 + 0.14 * (PI * smooth(0.55, 1.0, o)).sin();

        let saved = cv.t;
        cv.translate(x + size / 2.0, y + size / 2.0);
        cv.scale(beat);
        cv.translate(-size / 2.0, -size / 2.0);
        cv.fill(rounded_rect(14.0 * u, 46.0 * u, 72.0 * u, 50.0 * u, 12.0 * u), color, 1.0);

        let right = 68.5;
        let radius = 18.5;
        let top = 28.0 - lift;
        let narrow = (swing * PI / 180.0).cos();
        let add = |px: f64, py: f64| [(right + (px - right) * narrow) * u, py * u];
        let mut points = vec![add(31.5, 56.0 - 18.0 * smooth(0.0, 0.3, o)), add(31.5, top)];
        for i in 1..24 {
            let a = PI + PI * i as f64 / 24.0;
            points.push(add(50.0 + radius * a.cos(), top + radius * a.sin()));
        }
        points.push(add(right, top));
        points.push(add(right, 56.0));
        cv.stroke(polyline(&points), color, 1.0, 11.0 * u);
        cv.t = saved;
    }
}

// ---------------------------------------------------------------------------
// A video camera with a line through it, for a camera another program has
// ---------------------------------------------------------------------------

struct CameraGlyph {
    start: Option<f64>,
    pace: f64,
}

impl CameraGlyph {
    fn strike(&self, at: f64) -> f64 {
        match self.start {
            Some(s) => Ease::OutCubic.at(((at - s - 120.0 * self.pace) / (320.0 * self.pace)).clamp(0.0, 1.0)),
            None => 0.0,
        }
    }

    fn busy(&self, at: f64) -> bool {
        self.start.is_some_and(|s| at - s < 440.0 * self.pace)
    }

    fn draw(&self, cv: &mut Canvas, x: f64, y: f64, size: f64, color: Rgb, at: f64) {
        let u = size / 100.0;
        let strike = self.strike(at);
        let saved = cv.t;
        cv.translate(x, y);
        cv.fill(rounded_rect(4.0 * u, 24.0 * u, 62.0 * u, 52.0 * u, 12.0 * u), color, 1.0);
        let lens = || {
            let mut p = PathBuilder::new();
            p.move_to((72.0 * u) as f32, (42.0 * u) as f32);
            p.line_to((94.0 * u) as f32, (29.0 * u) as f32);
            p.line_to((94.0 * u) as f32, (71.0 * u) as f32);
            p.line_to((72.0 * u) as f32, (58.0 * u) as f32);
            p.close();
            p
        };
        cv.fill(lens(), color, 1.0);
        cv.stroke(lens(), color, 1.0, 6.0 * u);
        // The line, with a gap round it so it reads on the filled camera.
        if strike > 0.01 {
            let line = [[12.0 * u, 10.0 * u], [(12.0 + 76.0 * strike) * u, (10.0 + 80.0 * strike) * u]];
            cv.stroke(polyline(&line), PANEL, 1.0, 24.0 * u);
            cv.stroke(polyline(&line), color, 1.0, 9.0 * u);
        }
        cv.t = saved;
    }
}

// ---------------------------------------------------------------------------
// Brackets round a face that looks around, then the rings and a tick, or a
// red shake of the head
// ---------------------------------------------------------------------------

struct FaceGlyph {
    line_width: Option<f64>,
    pace: f64,
    mode: Mode,
    look_start: f64,
    look_frozen: f64,
    look_amount: Tw,
    smile: Tw,
    bracket: Tw,
    bracket_color: Tween<3>,
    shown: Tw,
    lock_opacity: Tw,
    lock_scale: Tw,
    lock: LockGlyph,
    camera_opacity: Tw,
    camera_scale: Tw,
    camera: CameraGlyph,
    success_start: Option<f64>,
    shake_start: Option<f64>,
}

impl FaceGlyph {
    fn new(line_width: Option<f64>) -> Self {
        FaceGlyph {
            line_width,
            pace: 1.0,
            mode: Mode::Idle,
            look_start: 0.0,
            look_frozen: 0.0,
            look_amount: Tw::one(0.0),
            smile: Tw::one(1.0),
            bracket: Tw::one(1.0),
            bracket_color: Tween::new(rgbf(TEXT_PRIMARY)),
            shown: Tw::one(1.0),
            lock_opacity: Tw::one(0.0),
            lock_scale: Tw::one(0.7),
            lock: LockGlyph::new(),
            camera_opacity: Tw::one(0.0),
            camera_scale: Tw::one(0.7),
            camera: CameraGlyph { start: None, pace: 1.0 },
            success_start: None,
            shake_start: None,
        }
    }

    fn looking(&self) -> bool {
        self.mode == Mode::Scanning
    }

    fn look_angle(&self, at: f64) -> f64 {
        if !self.looking() {
            return self.look_frozen;
        }
        let period = 2000.0 * self.pace;
        2.0 * PI * (((at - self.look_start) % period) / period)
    }

    fn breathe(&self, at: f64) -> f64 {
        if !self.looking() {
            return 0.0;
        }
        let half = 700.0 * self.pace;
        let p = ((at - self.look_start) % (2.0 * half)) / half;
        if p < 1.0 { Ease::InOutSine.at(p) } else { 1.0 - Ease::InOutSine.at(p - 1.0) }
    }

    /// Returns true when the new mode is a failure: the pill shakes then.
    fn set_mode(&mut self, mode: Mode, at: f64) -> bool {
        if mode == self.mode {
            return false;
        }
        let was_looking = self.looking();
        if was_looking {
            self.look_frozen = self.look_angle(at);
        }
        self.mode = mode;
        let pace = self.pace;
        if self.looking() && !was_looking {
            self.look_start = at;
        }
        let color = match mode {
            Mode::Success => SUCCESS,
            Mode::Failure => FAILURE,
            _ => TEXT_PRIMARY,
        };
        self.look_amount.to(if self.looking() { 1.0 } else { 0.0 }, 350.0 * pace, Ease::InOutQuad, at);
        self.smile.to(if mode == Mode::Failure { 0.0 } else { 1.0 }, 220.0 * pace, Ease::OutQuad, at);
        self.bracket.to(if mode == Mode::Tracking { 0.9 } else { 1.0 }, 260.0 * pace, OUT_BACK, at);
        self.bracket_color.set(rgbf(color), 220.0 * pace, Ease::Linear, at);
        let hidden = matches!(mode, Mode::Lockout | Mode::Busy);
        self.shown.to(if hidden { 0.0 } else { 1.0 }, 180.0 * pace, Ease::Linear, at);
        self.lock_opacity.to(if mode == Mode::Lockout { 1.0 } else { 0.0 }, 200.0 * pace, Ease::Linear, at);
        self.lock_scale.to(if mode == Mode::Lockout { 1.0 } else { 0.7 }, 260.0 * pace, OUT_BACK, at);
        self.camera_opacity.to(if mode == Mode::Busy { 1.0 } else { 0.0 }, 200.0 * pace, Ease::Linear, at);
        self.camera_scale.to(if mode == Mode::Busy { 1.0 } else { 0.7 }, 260.0 * pace, OUT_BACK, at);
        self.camera.pace = pace;
        self.camera.start = (mode == Mode::Busy).then_some(at);
        self.success_start = None;
        match mode {
            Mode::Failure => {
                self.shake_start = Some(at);
                true
            }
            Mode::Success => {
                self.success_start = Some(at);
                false
            }
            _ => false,
        }
    }

    fn busy(&self, at: f64) -> bool {
        self.looking()
            || self.look_amount.busy(at)
            || self.smile.busy(at)
            || self.bracket.busy(at)
            || self.bracket_color.busy(at)
            || self.shown.busy(at)
            || self.lock_opacity.busy(at)
            || self.lock_scale.busy(at)
            || self.camera_opacity.busy(at)
            || self.camera_scale.busy(at)
            || self.camera.busy(at)
            || self.success_start.is_some_and(|s| at - s < 900.0 * self.pace)
            || sequence(&SHAKE, self.shake_start, at, self.pace).1
    }

    fn draw(&self, cv: &mut Canvas, x: f64, y: f64, size: f64, at: f64) {
        let u = size / 100.0;
        let lw = self.line_width.unwrap_or(size * 0.055);
        let pace = self.pace;
        let color = rgb(self.bracket_color.get(at));

        let look_amount = self.look_amount.v(at);
        let angle = self.look_angle(at);
        let yaw = 0.34 * angle.cos() * look_amount;
        let pitch = 0.2 * angle.sin() * look_amount;
        let fp = |px: f64, py: f64, pz: f64| -> (f32, f32) {
            let depth = pz * yaw.cos() - px * yaw.sin();
            (((50.0 + px * yaw.cos() + pz * yaw.sin()) * u) as f32, ((50.0 + py * pitch.cos() - depth * pitch.sin()) * u) as f32)
        };

        let t = self.success_start.map_or(0.0, |s| ((at - s) / (850.0 * pace)).min(1.0));
        let tick = match self.success_start.map(|s| s + 470.0 * pace) {
            Some(ts) if at >= ts => Ease::OutQuad.at(((at - ts) / (260.0 * pace)).min(1.0)),
            _ => 0.0,
        };
        let (shake, _) = sequence(&SHAKE, self.shake_start, at, pace);

        let face_gone = smooth(0.0, 0.22, t);
        let rings_in = smooth(0.04, 0.26, t);
        let turn = turned((t / 0.7).min(1.0));
        let ring_size = 40.0 * (0.7 + 0.3 * rings_in);
        let ring_a = ring(0.0, -360.0 * turn, 90.0 * turn, ring_size);
        let ring_b = ring(70.0 + 360.0 * turn, 30.0 + 360.0 * turn, -90.0 * turn, ring_size);
        let ring_b_shown = 1.0 - smooth(0.36, 0.58, t);
        let ring_a_back = 1.0 - 0.6 * tilt(&ring_a);
        let ring_b_back = 1.0 - 0.6 * tilt(&ring_b);
        let glow = 1.0 - smooth(0.42, 0.75, t);
        let on_screen = |p: &[f64; 3]| [(50.0 + p[0]) * u, (50.0 + p[1]) * u];
        let closed = |points: &[[f64; 3]]| {
            let mut v: Vec<[f64; 2]> = points.iter().map(on_screen).collect();
            v.push(on_screen(&points[0]));
            v
        };

        let bracket_scale = self.bracket.v(at) - 0.04 * self.breathe(at);
        let c = 50.0 * u;

        let saved = cv.t;
        cv.translate(x + shake * u, y);

        // Brackets, giving way to the rings
        cv.layer(1.0 - face_gone, 1.0 - 0.15 * face_gone, c, c, |cv| {
            let s = cv.t;
            cv.translate(c, c);
            cv.scale(bracket_scale);
            cv.translate(-c, -c);
            let mut p = PathBuilder::new();
            let mut corner = |s: [f64; 2], l: [f64; 2], q: [f64; 2], k: [f64; 2], e: [f64; 2]| {
                p.move_to((s[0] * u) as f32, (s[1] * u) as f32);
                p.line_to((l[0] * u) as f32, (l[1] * u) as f32);
                p.quad_to((k[0] * u) as f32, (k[1] * u) as f32, (q[0] * u) as f32, (q[1] * u) as f32);
                p.line_to((e[0] * u) as f32, (e[1] * u) as f32);
            };
            corner([6.0, 30.0], [6.0, 19.0], [19.0, 6.0], [6.0, 6.0], [30.0, 6.0]);
            corner([70.0, 6.0], [81.0, 6.0], [94.0, 19.0], [94.0, 6.0], [94.0, 30.0]);
            corner([94.0, 70.0], [94.0, 81.0], [81.0, 94.0], [94.0, 94.0], [70.0, 94.0]);
            corner([30.0, 94.0], [19.0, 94.0], [6.0, 81.0], [6.0, 94.0], [6.0, 70.0]);
            cv.stroke(p, color, 1.0, lw);
            cv.t = s;
        });

        // The rings: a soft glow under them, their far halves dimmed, the
        // near halves on top.
        if t > 0.0 {
            cv.layer(rings_in, 1.0, c, c, |cv| {
                soft(cv, -32.0, -32.0, size + 64.0, size + 64.0, &GLOW_BLUR, 0.8 * glow, |c2| {
                    c2.stroke(polyline(&closed(&ring_a)), color, 1.0, lw * 1.6);
                    c2.stroke(polyline(&closed(&ring_b)), color, ring_b_shown, lw * 1.6);
                });
                let half = |points: &[[f64; 3]], front: bool| -> Vec<[f64; 2]> { half(points, front).iter().map(on_screen).collect() };
                cv.stroke(polyline(&half(&ring_a, false)), color, ring_a_back, lw);
                cv.stroke(polyline(&half(&ring_b, false)), color, ring_b_back * ring_b_shown, lw);
                cv.stroke(polyline(&half(&ring_b, true)), color, ring_b_shown, lw);
                cv.stroke(polyline(&half(&ring_a, true)), color, 1.0, lw);
            });
        }

        // The face itself
        cv.layer(self.shown.v(at) * (1.0 - face_gone), 1.0 - 0.2 * face_gone, c, c, |cv| {
            let smile = self.smile.v(at);
            let mut p = PathBuilder::new();
            let m = |p: &mut PathBuilder, q: (f32, f32)| p.move_to(q.0, q.1);
            let l = |p: &mut PathBuilder, q: (f32, f32)| p.line_to(q.0, q.1);
            let qd = |p: &mut PathBuilder, k: (f32, f32), q: (f32, f32)| p.quad_to(k.0, k.1, q.0, q.1);
            m(&mut p, fp(-16.0, -14.0, 26.0));
            l(&mut p, fp(-16.0, -5.0, 26.0));
            m(&mut p, fp(16.0, -14.0, 26.0));
            l(&mut p, fp(16.0, -5.0, 26.0));
            m(&mut p, fp(0.0, -13.0, 29.0));
            l(&mut p, fp(0.0, 6.0, 37.0));
            qd(&mut p, fp(0.0, 11.0, 35.0), fp(-5.0, 11.0, 31.0));
            m(&mut p, fp(-15.0, 20.0, 21.0));
            qd(&mut p, fp(0.0, 20.0 + 11.0 * smile, 24.0), fp(15.0, 20.0, 21.0));
            cv.stroke(p, color, 1.0, lw);
        });

        // The tick, drawn inside the ring as it comes to rest
        if tick > 0.001 {
            cv.layer((tick * 8.0).min(1.0), 0.75, c, c, |cv| {
                cv.translate(1.5 * u, 0.0);
                let a = [28.0 * u, 52.0 * u];
                let b = [43.0 * u, 67.0 * u];
                let e = [73.0 * u, 35.0 * u];
                let lerp = |p: [f64; 2], q: [f64; 2], k: f64| [p[0] + (q[0] - p[0]) * k, p[1] + (q[1] - p[1]) * k];
                let split = 0.33;
                let points = if tick <= split { vec![a, lerp(a, b, tick / split)] } else { vec![a, b, lerp(b, e, (tick - split) / (1.0 - split))] };
                cv.stroke(polyline(&points), SUCCESS, 1.0, lw / 0.75);
            });
        }

        // A lock instead of the face, after too many tries
        let lock_size = size * 0.42;
        cv.layer(self.lock_opacity.v(at), self.lock_scale.v(at), c, c, |cv| {
            self.lock.draw(cv, c - lock_size / 2.0, c - lock_size / 2.0, lock_size, TEXT_PRIMARY, at);
        });

        // A camera with a line through it, while another program has it
        let camera_size = size * 0.5;
        cv.layer(self.camera_opacity.v(at), self.camera_scale.v(at), c, c, |cv| {
            self.camera.draw(cv, c - camera_size / 2.0, c - camera_size / 2.0, camera_size, TEXT_PRIMARY, at);
        });

        cv.t = saved;
    }
}

/// How far the rings have turned at p: speeding up from rest, fastest a
/// quarter of the way in, then settling softly, facing forward.
fn turned(p: f64) -> f64 {
    p * p * (10.0 - p * (20.0 - p * (15.0 - 4.0 * p)))
}

/// A ring turned in 3D (degrees about x, then y, then z).
fn ring(ax: f64, ay: f64, az: f64, radius: f64) -> Vec<[f64; 3]> {
    let d = PI / 180.0;
    let (ca, sa) = ((ax * d).cos(), (ax * d).sin());
    let (cb, sb) = ((ay * d).cos(), (ay * d).sin());
    let (cg, sg) = ((az * d).cos(), (az * d).sin());
    (0..64)
        .map(|i| {
            let phi = 2.0 * PI * i as f64 / 64.0;
            let mut px = radius * phi.cos();
            let mut py = radius * phi.sin() * ca;
            let mut pz = radius * phi.sin() * sa;
            let x1 = px * cb + pz * sb;
            pz = pz * cb - px * sb;
            px = x1;
            let x2 = px * cg - py * sg;
            py = px * sg + py * cg;
            px = x2;
            [px, py, pz]
        })
        .collect()
}

fn tilt(points: &[[f64; 3]]) -> f64 {
    let most = points.iter().fold(0f64, |m, p| m.max(p[2].abs()));
    let p = points[0];
    most / (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt().max(1.0)
}

/// The half of a ring in front of the middle, or behind it, in one piece
/// and reaching one point into the other half so the two meet.
fn half(points: &[[f64; 3]], front: bool) -> Vec<[f64; 3]> {
    let n = points.len() as i64;
    let in_front = |i: i64| points[i.rem_euclid(n) as usize][2] >= -1e-6;
    let start = (0..n).find(|&i| in_front(i) == front && in_front(i - 1) != front);
    let mut out = Vec::new();
    let Some(start) = start else {
        if in_front(0) == front {
            for i in 0..=n {
                out.push(points[(i % n) as usize]);
            }
        }
        return out;
    };
    out.push(points[((start + n - 1) % n) as usize]);
    let mut i = start;
    while in_front(i) == front && i < start + n {
        out.push(points[(i % n) as usize]);
        i += 1;
    }
    out.push(points[(i % n) as usize]);
    out
}

// ---------------------------------------------------------------------------
// The island
// ---------------------------------------------------------------------------

pub struct Bubble {
    phase: Phase,
    message: String,
    face_seen: bool,
    minimal: bool,
    pace: f64,
    shown_phase: Phase,
    expanded: bool,
    opening: bool,
    expand_at: Option<f64>,
    slide_out_at: Option<f64>,
    close_done_at: Option<f64>,
    /// Whether there is anything on screen: from opening until it has slid
    /// away again.
    pub visible: bool,
    width: Tw,
    height: Tw,
    y: Tw,
    shadow: Tw,
    full_opacity: Tw,
    small_opacity: Tw,
    glyph_y: Tw,
    message_opacity: Tw,
    pulse_start: Option<f64>,
    settle: Tw,
    pill_shake_start: Option<f64>,
    face: FaceGlyph,
    small_face: FaceGlyph,
    lock: LockGlyph,
}

impl Bubble {
    pub fn new() -> Self {
        Bubble {
            phase: Phase::Hidden,
            message: String::new(),
            face_seen: false,
            minimal: false,
            pace: 1.0,
            shown_phase: Phase::Hidden,
            expanded: false,
            opening: false,
            expand_at: None,
            slide_out_at: None,
            close_done_at: None,
            visible: false,
            width: Tw::one(CLOSED_W),
            height: Tw::one(CLOSED_H),
            y: Tw::one(-CLOSED_H - 20.0),
            shadow: Tw::one(0.0),
            full_opacity: Tw::one(0.0),
            small_opacity: Tw::one(0.0),
            glyph_y: Tw::one(40.0),
            message_opacity: Tw::one(0.0),
            pulse_start: None,
            settle: Tw::one(0.0),
            pill_shake_start: None,
            face: FaceGlyph::new(None),
            small_face: FaceGlyph::new(Some(2.4)),
            lock: LockGlyph::new(),
        }
    }

    pub fn set(&mut self, look: &Look, at: f64) {
        let was_open = self.phase != Phase::Hidden;
        self.phase = look.phase;
        self.message = look.message.clone();
        self.face_seen = look.face_seen;
        self.minimal = look.minimal;
        if look.pace != self.pace {
            self.pace = look.pace;
            self.face.pace = look.pace;
            self.small_face.pace = look.pace;
            self.lock.pace = look.pace;
        }
        if self.phase != Phase::Hidden {
            self.shown_phase = self.phase;
        }
        let mode = match self.shown_phase {
            Phase::Hidden => Mode::Idle,
            Phase::Scanning if self.face_seen => Mode::Tracking,
            Phase::Scanning => Mode::Scanning,
            Phase::Success => Mode::Success,
            Phase::Failure => Mode::Failure,
            Phase::Lockout => Mode::Lockout,
            Phase::Busy => Mode::Busy,
        };
        self.face.set_mode(mode, at);
        if self.small_face.set_mode(if mode == Mode::Lockout { Mode::Failure } else { mode }, at) {
            self.pill_shake_start = Some(at);
        }
        self.lock.set_open(self.shown_phase == Phase::Success, at);

        let has_message = !self.message.is_empty();
        self.glyph_y.to(if has_message { 28.0 } else { 40.0 }, 220.0 * self.pace, Ease::OutCubic, at);
        self.message_opacity.to(if has_message { 1.0 } else { 0.0 }, 200.0 * self.pace, Ease::Linear, at);
        self.update_pulse(at);

        let want_open = self.phase != Phase::Hidden;
        if want_open != was_open {
            self.choreograph(want_open, at);
        }
    }

    fn choreograph(&mut self, want_open: bool, at: f64) {
        if want_open {
            self.opening = true;
            self.slide_out_at = None;
            self.close_done_at = None;
            self.set_positioned(true, at);
            if !self.expanded {
                self.expand_at = Some(at + EXPAND_DELAY * self.pace);
            }
            self.visible = true;
        } else {
            self.opening = false;
            self.expand_at = None;
            self.set_expanded(false, at);
            self.slide_out_at = Some(at + SLIDE_OUT_DELAY * self.pace);
        }
    }

    /// Runs what is due. Returns true when it has just finished closing.
    pub fn advance(&mut self, at: f64) -> bool {
        if let Some(t) = self.expand_at.filter(|&t| at >= t) {
            self.expand_at = None;
            self.set_expanded(true, t);
        }
        if let Some(t) = self.slide_out_at.filter(|&t| at >= t) {
            self.slide_out_at = None;
            self.set_positioned(false, t);
            self.close_done_at = Some(t + SLIDE_OUT * self.pace + 60.0);
        }
        if self.close_done_at.is_some_and(|t| at >= t) {
            self.close_done_at = None;
            self.visible = false;
            return true;
        }
        false
    }

    fn set_positioned(&mut self, positioned: bool, at: f64) {
        let to = if positioned { TOP_GAP } else { -CLOSED_H - 20.0 };
        let (duration, ease) = if self.opening { (SLIDE_IN, Ease::OutCubic) } else { (SLIDE_OUT, Ease::InCubic) };
        self.y.to(to, duration * self.pace, ease, at);
    }

    fn set_expanded(&mut self, expanded: bool, at: f64) {
        self.expanded = expanded;
        let (w, h) = match (expanded, self.minimal) {
            (false, _) => (CLOSED_W, CLOSED_H),
            (true, true) => (MINIMAL_W, MINIMAL_H),
            (true, false) => (OPEN_W, OPEN_H),
        };
        let pace = self.pace;
        let duration = if self.opening { GROW } else { SHRINK } * pace;
        // Growing overshoots sideways more than down, so the bottom edge does
        // not sag.
        self.width.to(w, duration, if self.opening { Ease::OutBack(1.6) } else { Ease::OutCubic }, at);
        self.height.to(h, duration, if self.opening { Ease::OutBack(0.9) } else { Ease::OutCubic }, at);
        let on = if expanded { 1.0 } else { 0.0 };
        self.shadow.to(0.35 * on, if self.opening { 200.0 } else { 120.0 } * pace, Ease::Linear, at);
        self.full_opacity.to(on, if self.opening { 260.0 } else { 120.0 } * pace, Ease::Linear, at);
        self.small_opacity.to(on, if self.opening { 200.0 } else { 120.0 } * pace, Ease::Linear, at);
        self.update_pulse(at);
    }

    /// The breathing while it scans: 1500 ms a breath, settling when it
    /// stops.
    fn update_pulse(&mut self, at: f64) {
        let running = self.phase == Phase::Scanning && self.expanded;
        if running && self.pulse_start.is_none() {
            self.pulse_start = Some(at);
        } else if !running && self.pulse_start.is_some() {
            let now = self.pulse(at);
            self.settle.hold(now);
            self.settle.to(0.0, 200.0 * self.pace, Ease::OutQuad, at);
            self.pulse_start = None;
        }
    }

    fn pulse(&self, at: f64) -> f64 {
        let Some(start) = self.pulse_start else { return self.settle.v(at) };
        let p = self.pace;
        let mut t = (at - start) % (1500.0 * p);
        if t < 600.0 * p {
            return 0.0;
        }
        t -= 600.0 * p;
        if t < 400.0 * p {
            return Ease::InOutQuad.at(t / (400.0 * p));
        }
        t -= 400.0 * p;
        if t < 50.0 * p {
            return 1.0;
        }
        t -= 50.0 * p;
        if t < 400.0 * p {
            return 1.0 - Ease::InOutQuad.at(t / (400.0 * p));
        }
        0.0
    }

    /// Whether the next frame looks any different from this one.
    pub fn busy(&self, at: f64) -> bool {
        self.phase != Phase::Hidden
            || self.expand_at.is_some()
            || self.slide_out_at.is_some()
            || self.close_done_at.is_some()
            || self.width.busy(at)
            || self.y.busy(at)
            || self.face.busy(at)
            || self.small_face.busy(at)
            || self.lock.busy(at)
    }

    /// Draws it at the top middle of a WIDTH x HEIGHT surface.
    pub fn draw(&self, cv: &mut Canvas, at: f64) {
        let w = self.width.v(at);
        let h = self.height.v(at);
        let (pill_shake, _) = sequence(&PILL_SHAKE, self.pill_shake_start, at, self.pace);
        let x = (WIDTH - w) / 2.0 + pill_shake;
        let y = self.y.v(at);
        let radius = if self.minimal { h / 2.0 } else { OPEN_RADIUS.min(h / 2.0) };

        // The shadow: a soft one, 3 px down.
        soft(cv, x - 32.0, y - 32.0, w + 64.0, h + 64.0, &SHADOW_BLUR, self.shadow.v(at), |c| {
            c.fill(rounded_rect(x, y + 3.0, w, h, radius), [0, 0, 0], 1.0);
        });
        cv.fill(rounded_rect(x, y, w, h, radius), PANEL, 1.0);

        let pulse = self.pulse(at);
        if !self.minimal {
            // The face and a line of text, laid out at the open size and
            // scaled with the island.
            let fit = (w / OPEN_W).min(h / OPEN_H);
            let ox = x + w / 2.0 - OPEN_W / 2.0;
            let oy = y + h / 2.0 - OPEN_H / 2.0;
            let (cx, cy) = (ox + OPEN_W / 2.0, oy + OPEN_H / 2.0);
            cv.layer(self.full_opacity.v(at), fit, cx, cy, |cv| {
                cv.layer(1.0 - 0.35 * pulse, 1.0 - 0.03 * pulse, cx, cy, |cv| {
                    self.face.draw(cv, ox + 40.0, oy + self.glyph_y.v(at), 100.0, at);
                    self.draw_message(cv, ox, oy, at);
                });
            });
        } else {
            cv.layer(self.small_opacity.v(at), 1.0, x + w / 2.0, y + h / 2.0, |cv| {
                let failed = matches!(self.shown_phase, Phase::Failure | Phase::Lockout);
                self.lock.draw(cv, x + 16.0, y + (h - 18.0) / 2.0, 18.0, if failed { FAILURE } else { TEXT_PRIMARY }, at);
                let fx = x + w - 14.0 - 24.0;
                let fy = y + (h - 24.0) / 2.0;
                cv.layer(1.0 - 0.35 * pulse, 1.0 - 0.03 * pulse, fx + 12.0, fy + 12.0, |cv| {
                    self.small_face.draw(cv, fx, fy, 24.0, at);
                });
            });
        }
    }

    fn draw_message(&self, cv: &mut Canvas, ox: f64, oy: f64, at: f64) {
        let opacity = self.message_opacity.v(at);
        if opacity <= 0.001 || self.message.is_empty() {
            return;
        }
        let width = (OPEN_W - 32.0) as f32;
        let (_, th) = draw::measure(&self.message, 13.0, 500, Some(width));
        let detail = matches!(self.shown_phase, Phase::Failure | Phase::Lockout | Phase::Busy);
        let color = if detail { TEXT_DETAIL } else { TEXT_SECONDARY };
        draw::text(cv, &self.message, ox + 16.0, oy + OPEN_H - 20.0 - th as f64, 13.0, 500, Some(width), Align::Center, color, opacity);
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// Draws the life of a bubble onto grey, into $FU_SHOTS/bubble-*.png,
    /// for looking at. Does nothing without FU_SHOTS.
    #[test]
    fn frames() {
        let Some(dir) = std::env::var_os("FU_SHOTS") else { return };
        let dir = std::path::PathBuf::from(dir);
        for minimal in [false, true] {
            let mut b = Bubble::new();
            let look = |phase, message: &str, face_seen| Look { phase, message: message.into(), face_seen, minimal, pace: 1.0 };
            let steps: &[(f64, Look)] = &[
                (0.0, look(Phase::Scanning, "", false)),
                (900.0, look(Phase::Scanning, "Blink once", true)),
                (2000.0, look(Phase::Success, "", true)),
                (4000.0, look(Phase::Failure, "Not recognized", true)),
                (6000.0, look(Phase::Hidden, "", false)),
            ];
            let shots = [200.0, 700.0, 1200.0, 2300.0, 2700.0, 3100.0, 4100.0, 4400.0, 6150.0, 6400.0];
            let mut next = 0;
            let mut t = 0.0;
            while t <= 6600.0 {
                while next < steps.len() && steps[next].0 <= t {
                    b.set(&steps[next].1, t);
                    next += 1;
                }
                b.advance(t);
                if shots.contains(&t) {
                    let mut pm = tiny_skia::Pixmap::new(WIDTH as u32 * 2, HEIGHT as u32 * 2).unwrap();
                    pm.fill(tiny_skia::Color::from_rgba8(110, 120, 135, 255));
                    b.draw(&mut Canvas::new(pm.as_mut(), 2.0), t);
                    pm.save_png(dir.join(format!("bubble-{}-{:05}.png", if minimal { "minimal" } else { "full" }, t as u32))).unwrap();
                }
                t += 50.0;
            }
        }
    }
}
