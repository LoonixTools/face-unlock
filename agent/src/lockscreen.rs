//! face-unlock's own lock screen, one per screen: the time, the password,
//! and on one screen the bubble at the top, the same one that shows over the
//! desktop. Behind it the desktop's picture, or the one the user set.

use crate::bubble::{FAILURE, OPEN_H, PILL_SHAKE, TEXT_DETAIL, TEXT_PRIMARY, TEXT_SECONDARY, TOP_GAP};
use crate::draw::{self, Align, Canvas, Rgb, rounded_rect, sequence};
use face_unlock::i18n::tr;
use std::path::Path;
use tiny_skia::{Color, GradientStop, LinearGradient, Paint, Pixmap, Point, Rect, SpreadMode, Transform};

const SURFACE_RAISED: Rgb = [0x32, 0x32, 0x32];

/// What all screens of the lock show and share.
pub struct LockScreen {
    pub password: String,
    pub message: String,
    /// The password is being checked.
    pub busy: bool,
    pub face_unlock: bool,
    pub blur: bool,
    pub login: String,
    pub name: String,
    shake_start: Option<f64>,
}

impl LockScreen {
    pub fn new() -> Self {
        let (login, name) = crate::pam::me().unwrap_or_default();
        LockScreen { password: String::new(), message: String::new(), busy: false, face_unlock: false, blur: false, login, name, shake_start: None }
    }

    /// What is still in memory of it, as far as a String lets that happen.
    pub fn clear_password(&mut self) {
        let mut bytes = std::mem::take(&mut self.password).into_bytes();
        bytes.fill(0);
    }

    pub fn reset(&mut self) {
        self.clear_password();
        self.message.clear();
        self.busy = false;
        self.shake_start = None;
    }

    pub fn rejected(&mut self, at: f64) {
        self.message = tr("Wrong password").into();
        self.shake_start = Some(at);
    }

    pub fn busy_at(&self, at: f64) -> bool {
        sequence(&PILL_SHAKE, self.shake_start, at, 1.0).1
    }
}

// ---------------------------------------------------------------------------
// The picture behind it
// ---------------------------------------------------------------------------

/// The picture at `path`, cut to fill w x h pixels, blurred if asked, and
/// dimmed a little so the time and the password read on any picture.
/// Slow for a big picture: it runs on a thread.
pub fn background(path: &Path, w: u32, h: u32, blur: bool) -> Option<Pixmap> {
    use image::imageops::{self, FilterType};
    let img = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()?.to_rgba8();
    let (iw, ih) = img.dimensions();
    if iw == 0 || ih == 0 || w == 0 || h == 0 {
        return None;
    }
    let scale = (w as f64 / iw as f64).max(h as f64 / ih as f64);
    let (sw, sh) = (((iw as f64 * scale).ceil() as u32).max(w), ((ih as f64 * scale).ceil() as u32).max(h));
    let mut out = if blur {
        // Blurred small and grown back: as soft as Qt's strongest blur, and
        // fast.
        let small = imageops::resize(&img, (sw / 16).max(1), (sh / 16).max(1), FilterType::Triangle);
        let small = imageops::blur(&small, 2.5);
        let big = imageops::resize(&small, sw, sh, FilterType::Triangle);
        imageops::crop_imm(&big, (sw - w) / 2, (sh - h) / 2, w, h).to_image()
    } else {
        let big = imageops::resize(&img, sw, sh, FilterType::Triangle);
        imageops::crop_imm(&big, (sw - w) / 2, (sh - h) / 2, w, h).to_image()
    };
    for p in out.pixels_mut() {
        p.0 = [(p.0[0] as f32 * 0.65) as u8, (p.0[1] as f32 * 0.65) as u8, (p.0[2] as f32 * 0.65) as u8, 255];
    }
    Pixmap::from_vec(out.into_raw(), tiny_skia::IntSize::from_wh(w, h)?)
}

fn gradient(cv: &mut Canvas, w: f64, h: f64) {
    let stops = vec![GradientStop::new(0.0, Color::from_rgba8(0x11, 0x16, 0x1d, 255)), GradientStop::new(0.6, Color::from_rgba8(0, 0, 0, 255))];
    let Some(shader) = LinearGradient::new(Point::from_xy(0.0, 0.0), Point::from_xy(0.0, h as f32), stops, SpreadMode::Pad, Transform::identity()) else { return };
    let paint = Paint { shader, ..Default::default() };
    if let Some(rect) = Rect::from_xywh(0.0, 0.0, w as f32, h as f32) {
        cv.pixmap.fill_rect(rect, &paint, cv.t, None);
    }
}

// ---------------------------------------------------------------------------
// The time, in the user's language
// ---------------------------------------------------------------------------

fn strftime(format: &str, tm: &libc::tm) -> String {
    let Ok(f) = std::ffi::CString::new(format) else { return String::new() };
    let mut buf = [0u8; 256];
    // SAFETY: buf is big enough and strftime writes at most its length.
    let n = unsafe { libc::strftime(buf.as_mut_ptr().cast(), buf.len(), f.as_ptr(), tm) };
    String::from_utf8_lossy(&buf[..n]).to_string()
}

fn locale() -> String {
    ["LC_ALL", "LC_TIME", "LANG"].iter().find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty())).unwrap_or_default()
}

/// The time as hours and minutes, and the date written out, as the
/// language has them.
pub fn now_text() -> (String, String) {
    // SAFETY: localtime_r fills the tm it is given.
    let tm = unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        tm
    };
    // SAFETY: nl_langinfo returns a string owned by the C library.
    let twelve = unsafe {
        let p = libc::nl_langinfo(libc::T_FMT);
        !p.is_null() && {
            let f = std::ffi::CStr::from_ptr(p).to_string_lossy();
            f.contains("%p") || f.contains("%r") || f.contains("%I") || f.contains("%l")
        }
    };
    let time = strftime(if twelve { "%-I:%M %p" } else { "%H:%M" }, &tm);
    let loc = locale();
    let lang = loc.split(['_', '.', '@']).next().unwrap_or("");
    let date = match lang {
        "de" => "%A, %-d. %B %Y",
        "en" if loc.starts_with("en_US") || loc == "en" || loc == "C" => "%A, %B %-d, %Y",
        "ja" | "zh" => "%Y年%-m月%-d日 %A",
        "ko" => "%Y년 %-m월 %-d일 %A",
        "" | "C" | "POSIX" => "%A, %B %-d, %Y",
        _ => "%A %-d %B %Y",
    };
    (time, strftime(date, &tm))
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn centered(cv: &mut Canvas, text: &str, w: f64, y: f64, size: f32, weight: u16, color: Rgb, alpha: f64) -> f64 {
    let (tw, th) = draw::measure(text, size, weight, None);
    draw::text(cv, text, (w - tw as f64) / 2.0, y, size, weight, None, Align::Left, color, alpha);
    th as f64
}

/// One screen of it, w x h points. `bg` is the picture at the canvas'
/// pixel size, or None while it loads (or if there is none).
pub fn draw(cv: &mut Canvas, w: f64, h: f64, bg: Option<&Pixmap>, lock: &LockScreen, at: f64) {
    match bg {
        Some(bg) if bg.width() == cv.pixmap.width() && bg.height() == cv.pixmap.height() => {
            cv.pixmap.data_mut().copy_from_slice(bg.data());
        }
        _ => gradient(cv, w, h),
    }

    // Below the open bubble, which hangs from the top edge.
    let (time, date) = now_text();
    let mut y = (TOP_GAP + OPEN_H + 24.0).max((h * 0.2).round());
    let big = ((h * 0.11).round()).clamp(56.0, 120.0) as f32;
    y += centered(cv, &time, w, y, big, 300, TEXT_PRIMARY, 1.0) + 2.0;
    centered(cv, &date, w, y, 18.0, 500, TEXT_DETAIL, 1.0);

    let mut y = (h * 0.62).round();
    y += centered(cv, &lock.name, w, y, 17.0, 600, TEXT_PRIMARY, 1.0) + 12.0;

    // The password, as dots in a pill that shakes when it was wrong.
    let (shake, _) = sequence(&PILL_SHAKE, lock.shake_start, at, 1.0);
    let (pw, ph) = (280.0, 44.0);
    let px = (w - pw) / 2.0 + shake;
    let alpha = if lock.busy { 0.6 } else { 1.0 };
    cv.fill(rounded_rect(px, y, pw, ph, ph / 2.0), SURFACE_RAISED, alpha);
    cv.stroke(rounded_rect(px + 0.5, y + 0.5, pw - 1.0, ph - 1.0, (ph - 1.0) / 2.0), [255, 255, 255], 0.18 * alpha, 1.0);
    let count = lock.password.chars().count().min(20);
    if count == 0 {
        let (tw, th) = draw::measure(tr("Password"), 15.0, 400, None);
        draw::text(cv, tr("Password"), px + (pw - tw as f64) / 2.0, y + (ph - th as f64) / 2.0, 15.0, 400, None, Align::Left, TEXT_SECONDARY, alpha);
    } else {
        let step = 13.0;
        let left = px + pw / 2.0 - (count as f64 - 1.0) * step / 2.0;
        for i in 0..count {
            let mut p = tiny_skia::PathBuilder::new();
            p.push_circle((left + i as f64 * step) as f32, (y + ph / 2.0) as f32, 4.5);
            cv.fill(p, TEXT_PRIMARY, alpha);
        }
    }
    y += ph + 12.0;

    let (text, color) = if !lock.message.is_empty() {
        (lock.message.as_str(), FAILURE)
    } else if lock.face_unlock {
        (tr("Look at the camera or type your password"), TEXT_DETAIL)
    } else {
        (tr("Type your password"), TEXT_DETAIL)
    };
    draw::text(cv, text, (w - 360.0) / 2.0, y, 13.0, 500, Some(360.0), Align::Center, color, 1.0);
}
