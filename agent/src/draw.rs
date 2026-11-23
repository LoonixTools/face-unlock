//! Drawing on a pixmap with tiny-skia: a canvas that keeps a transform and
//! an opacity, the easing curves the bubble uses, values that ease to a new
//! target like a QML Behavior, the soft blur, and text.

use std::sync::{Mutex, OnceLock};
use tiny_skia::{BlendMode, Color, FillRule, FilterQuality, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PixmapMut, PixmapPaint, Stroke, Transform};

pub type Rgb = [u8; 3];

pub fn ms() -> f64 {
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64() * 1000.0
}

// ---------------------------------------------------------------------------
// Easing
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ease {
    Linear,
    OutQuad,
    InOutQuad,
    OutCubic,
    InCubic,
    InOutSine,
    OutBack(f64),
}

impl Ease {
    pub fn at(self, p: f64) -> f64 {
        match self {
            Ease::Linear => p,
            Ease::OutQuad => 1.0 - (1.0 - p) * (1.0 - p),
            Ease::InOutQuad => {
                if p < 0.5 {
                    2.0 * p * p
                } else {
                    1.0 - (-2.0 * p + 2.0).powi(2) / 2.0
                }
            }
            Ease::OutCubic => 1.0 - (1.0 - p).powi(3),
            Ease::InCubic => p.powi(3),
            Ease::InOutSine => -((std::f64::consts::PI * p).cos() - 1.0) / 2.0,
            Ease::OutBack(s) => {
                let q = p - 1.0;
                q * q * ((s + 1.0) * q + s) + 1.0
            }
        }
    }
}

pub const OUT_BACK: Ease = Ease::OutBack(1.70158);

pub fn smooth(from: f64, to: f64, p: f64) -> f64 {
    let x = ((p - from) / (to - from)).clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// A value that eases to a new target from wherever it is.
#[derive(Clone, Debug)]
pub struct Tween<const N: usize> {
    from: [f64; N],
    to: [f64; N],
    start: f64,
    duration: f64,
    ease: Ease,
}

pub type Tw = Tween<1>;

impl<const N: usize> Tween<N> {
    pub fn new(v: [f64; N]) -> Self {
        Tween { from: v, to: v, start: 0.0, duration: 0.0, ease: Ease::Linear }
    }

    pub fn set(&mut self, to: [f64; N], duration: f64, ease: Ease, at: f64) {
        if to == self.to {
            return;
        }
        self.from = self.get(at);
        self.to = to;
        self.start = at;
        self.duration = duration;
        self.ease = ease;
    }

    pub fn jump(&mut self, v: [f64; N]) {
        self.from = v;
        self.to = v;
        self.duration = 0.0;
    }

    pub fn get(&self, at: f64) -> [f64; N] {
        let p = if self.duration > 0.0 { ((at - self.start) / self.duration).min(1.0) } else { 1.0 };
        let e = self.ease.at(p);
        let mut out = [0.0; N];
        for i in 0..N {
            out[i] = self.from[i] + (self.to[i] - self.from[i]) * e;
        }
        out
    }

    pub fn busy(&self, at: f64) -> bool {
        self.duration > 0.0 && at - self.start < self.duration
    }
}

impl Tween<1> {
    pub fn one(v: f64) -> Self {
        Tween::new([v])
    }
    pub fn to(&mut self, v: f64, duration: f64, ease: Ease, at: f64) {
        self.set([v], duration, ease, at);
    }
    pub fn v(&self, at: f64) -> f64 {
        self.get(at)[0]
    }
    pub fn hold(&mut self, v: f64) {
        self.jump([v]);
    }
}

/// A run of steps from 0, like a QML SequentialAnimation: (target,
/// duration, easing)... Returns the value and whether it still runs.
pub fn sequence(steps: &[(f64, f64, Ease)], start: Option<f64>, at: f64, pace: f64) -> (f64, bool) {
    let Some(start) = start else { return (0.0, false) };
    let mut t = at - start;
    let mut from = 0.0;
    for &(to, duration, ease) in steps {
        let d = duration * pace;
        if t < d {
            return (from + (to - from) * ease.at(t / d), true);
        }
        t -= d;
        from = to;
    }
    (from, false)
}

// ---------------------------------------------------------------------------
// The canvas
// ---------------------------------------------------------------------------

pub struct Canvas<'a> {
    pub pixmap: PixmapMut<'a>,
    pub t: Transform,
    pub alpha: f32,
}

pub fn color(c: Rgb, alpha: f32) -> Color {
    Color::from_rgba8(c[0], c[1], c[2], (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

impl<'a> Canvas<'a> {
    pub fn new(pixmap: PixmapMut<'a>, scale: f32) -> Self {
        Canvas { pixmap, t: Transform::from_scale(scale, scale), alpha: 1.0 }
    }

    /// Runs `draw` scaled about (cx, cy) and at `opacity`, then puts the
    /// transform and opacity back. A layer, without the cost of one.
    pub fn layer(&mut self, opacity: f64, scale: f64, cx: f64, cy: f64, draw: impl FnOnce(&mut Canvas)) {
        if opacity <= 0.001 {
            return;
        }
        let (t, a) = (self.t, self.alpha);
        self.t = self.t.pre_translate(cx as f32, cy as f32).pre_scale(scale as f32, scale as f32).pre_translate(-cx as f32, -cy as f32);
        self.alpha *= opacity.min(1.0) as f32;
        draw(self);
        self.t = t;
        self.alpha = a;
    }

    pub fn translate(&mut self, x: f64, y: f64) {
        self.t = self.t.pre_translate(x as f32, y as f32);
    }

    pub fn scale(&mut self, s: f64) {
        self.t = self.t.pre_scale(s as f32, s as f32);
    }

    fn paint(&self, c: Rgb, alpha: f64) -> Paint<'static> {
        let mut p = Paint::default();
        p.set_color(color(c, alpha as f32 * self.alpha));
        p.anti_alias = true;
        p
    }

    pub fn fill(&mut self, path: PathBuilder, c: Rgb, alpha: f64) {
        if let Some(path) = path.finish() {
            let paint = self.paint(c, alpha);
            self.pixmap.fill_path(&path, &paint, FillRule::Winding, self.t, None);
        }
    }

    pub fn stroke(&mut self, path: PathBuilder, c: Rgb, alpha: f64, width: f64) {
        if let Some(path) = path.finish() {
            let paint = self.paint(c, alpha);
            let stroke = Stroke { width: width as f32, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
            self.pixmap.stroke_path(&path, &paint, &stroke, self.t, None);
        }
    }
}

pub fn rounded_rect(x: f64, y: f64, w: f64, h: f64, r: f64) -> PathBuilder {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0) as f32;
    let (x, y, w, h) = (x as f32, y as f32, w as f32, h as f32);
    let k = 0.552_284_8 * r;
    let mut p = PathBuilder::new();
    p.move_to(x + r, y);
    p.line_to(x + w - r, y);
    p.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    p.line_to(x + w, y + h - r);
    p.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    p.line_to(x + r, y + h);
    p.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    p.line_to(x, y + r);
    p.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    p.close();
    p
}

pub fn polyline(points: &[[f64; 2]]) -> PathBuilder {
    let mut p = PathBuilder::new();
    for (i, q) in points.iter().enumerate() {
        if i == 0 {
            p.move_to(q[0] as f32, q[1] as f32);
        } else {
            p.line_to(q[0] as f32, q[1] as f32);
        }
    }
    p
}

/// A soft glow or shadow: what `draw` paints into the part x, y, w, h,
/// halved again and again and doubled back smoothly, each halving blurring
/// about twice as wide. `weights` says how much of each level goes into the
/// result, from the first halving on.
#[allow(clippy::too_many_arguments)]
pub fn soft(cv: &mut Canvas, x: f64, y: f64, w: f64, h: f64, weights: &[f32], opacity: f64, draw: impl FnOnce(&mut Canvas)) {
    if opacity <= 0.001 {
        return;
    }
    let s = cv.t.sx;
    let (pw, ph) = (((w as f32) * s).ceil().max(1.0) as u32, ((h as f32) * s).ceil().max(1.0) as u32);
    let Some(mut base) = Pixmap::new(pw, ph) else { return };
    {
        // In the canvas' own units, at its scale, with (x, y) in the corner.
        let mut c = Canvas { pixmap: base.as_mut(), t: Transform::from_scale(s, s).pre_translate(-x as f32, -y as f32), alpha: cv.alpha };
        draw(&mut c);
    }
    let mut levels = vec![base];
    for _ in 0..weights.len() {
        let from = levels.last().expect("one level");
        let (hw, hh) = ((from.width() / 2).max(1), (from.height() / 2).max(1));
        let Some(mut half) = Pixmap::new(hw, hh) else { return };
        half.draw_pixmap(0, 0, from.as_ref(), &PixmapPaint { quality: FilterQuality::Bilinear, ..Default::default() }, Transform::from_scale(0.5, 0.5), None);
        levels.push(half);
    }
    let mut blurred: Option<Pixmap> = None;
    for i in (1..=weights.len()).rev() {
        let level = &levels[i];
        let Some(mut sum) = Pixmap::new(level.width(), level.height()) else { return };
        if let Some(b) = &blurred {
            sum.draw_pixmap(0, 0, b.as_ref(), &PixmapPaint { quality: FilterQuality::Bilinear, ..Default::default() }, Transform::from_scale(2.0, 2.0), None);
        }
        if weights[i - 1] > 0.0 {
            sum.draw_pixmap(0, 0, level.as_ref(), &PixmapPaint { opacity: weights[i - 1], blend_mode: BlendMode::Plus, ..Default::default() }, Transform::identity(), None);
        }
        blurred = Some(sum);
    }
    let Some(b) = blurred else { return };
    // The sum is at the first halving: twice the size of a base pixel.
    let factor = 2.0;
    let paint = PixmapPaint { opacity: opacity as f32, quality: FilterQuality::Bilinear, ..Default::default() };
    let t = cv.t.pre_translate(x as f32, y as f32).pre_scale(factor / s, factor / s);
    cv.pixmap.draw_pixmap(0, 0, b.as_ref(), &paint, t, None);
}

// ---------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------

pub struct Fonts {
    system: cosmic_text::FontSystem,
    cache: cosmic_text::SwashCache,
}

/// The system's fonts, read once. Asked for the first time, this takes a
/// moment, so the agent warms it up when it starts.
pub fn fonts() -> &'static Mutex<Fonts> {
    static F: OnceLock<Mutex<Fonts>> = OnceLock::new();
    F.get_or_init(|| {
        let mut system = cosmic_text::FontSystem::new();
        // The desktop's own sans-serif font, as fontconfig has it.
        if let Ok(out) = std::process::Command::new("fc-match").args(["-f", "%{family[0]}", "sans-serif"]).output()
            && let Ok(family) = String::from_utf8(out.stdout)
            && !family.trim().is_empty()
        {
            system.db_mut().set_sans_serif_family(family.trim());
        }
        Mutex::new(Fonts { system, cache: cosmic_text::SwashCache::new() })
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Align {
    Left,
    Center,
}

/// The size of `text` laid out in `width` points.
pub fn measure(text: &str, size: f32, weight: u16, width: Option<f32>) -> (f32, f32) {
    let mut f = fonts().lock().unwrap_or_else(|e| e.into_inner());
    let buffer = layout(&mut f.system, text, size, weight, width, Align::Left, 1.0);
    let mut w = 0f32;
    let mut h = 0f32;
    for run in buffer.layout_runs() {
        w = w.max(run.line_w);
        h = h.max(run.line_top + run.line_height);
    }
    (w, h)
}

fn layout(system: &mut cosmic_text::FontSystem, text: &str, size: f32, weight: u16, width: Option<f32>, align: Align, scale: f32) -> cosmic_text::Buffer {
    use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping, Weight};
    let mut buffer = Buffer::new(system, Metrics::new(size * scale, size * 1.3 * scale));
    buffer.set_size(width.map(|w| w * scale), None);
    let attrs = Attrs::new().family(Family::SansSerif).weight(Weight(weight));
    let align = (align == Align::Center).then_some(cosmic_text::Align::Center);
    buffer.set_text(text, &attrs, Shaping::Advanced, align);
    buffer.shape_until_scroll(system, false);
    buffer
}

/// Draws `text` with its top left at (x, y) in points, in `width` points
/// when given (wrapped, and centred with Align::Center).
#[allow(clippy::too_many_arguments)]
pub fn text(cv: &mut Canvas, text: &str, x: f64, y: f64, size: f32, weight: u16, width: Option<f32>, align: Align, c: Rgb, alpha: f64) {
    if text.is_empty() || alpha <= 0.001 {
        return;
    }
    let scale = cv.t.sx;
    let ox = (x as f32) * scale + cv.t.tx;
    let oy = (y as f32) * scale + cv.t.ty;
    let a = (alpha as f32 * cv.alpha).clamp(0.0, 1.0);
    let mut guard = fonts().lock().unwrap_or_else(|e| e.into_inner());
    let Fonts { system, cache } = &mut *guard;
    let mut buffer = layout(system, text, size, weight, width, align, scale);
    let (pw, ph) = (cv.pixmap.width() as i32, cv.pixmap.height() as i32);
    let pixels = cv.pixmap.pixels_mut();
    buffer.draw(system, cache, cosmic_text::Color::rgb(c[0], c[1], c[2]), |gx, gy, gw, gh, col| {
        let cov = col.a() as f32 / 255.0 * a;
        if cov <= 0.0 {
            return;
        }
        for yy in 0..gh as i32 {
            for xx in 0..gw as i32 {
                let px = ox as i32 + gx + xx;
                let py = oy as i32 + gy + yy;
                if px < 0 || py < 0 || px >= pw || py >= ph {
                    continue;
                }
                let i = (py * pw + px) as usize;
                let dst = pixels[i];
                let inv = 1.0 - cov;
                let blend = |s: u8, d: u8| ((s as f32 * cov) + d as f32 * inv).round().min(255.0) as u8;
                let out_a = (cov * 255.0 + dst.alpha() as f32 * inv).round().min(255.0) as u8;
                let r = blend(col.r(), dst.red());
                let g = blend(col.g(), dst.green());
                let b = blend(col.b(), dst.blue());
                if let Some(p) = tiny_skia::PremultipliedColorU8::from_rgba(r.min(out_a), g.min(out_a), b.min(out_a), out_a) {
                    pixels[i] = p;
                }
            }
        }
    });
}
