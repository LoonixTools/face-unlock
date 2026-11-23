//! Telling a face from a picture of one.
//!
//! A webcam sees a flat image either way, so there is no single test that
//! settles it. What is here follows the model Glance (the macOS face unlock)
//! arrived at after trying and dropping a dozen weaker signals: a few cues,
//! each one decisive on its own, split into two kinds.
//!
//! Deny cues are evidence of a fake. Either one fails the scan outright, and
//! a match that already happened does not outvote it.
//!
//! - glare: a phone screen or a glossy print throws back one large, flat,
//!   colourless highlight. Skin shines in small scattered spots.
//! - device: the straight edges of a phone or a tablet around the face.
//!
//! Confirm cues are evidence of a real head. Any one of them is enough, and
//! their absence is never held against anybody on its own: a person can sit
//! still and not blink for a few seconds.
//!
//! - depth: when the head turns, the tip of the nose moves further than a
//!   flat face would let it. The eyes and the corners of the mouth lie close
//!   to one plane, so four of them predict exactly where the nose of a flat
//!   picture has to go. A real nose, about a third of an eye distance in
//!   front of that plane, misses the prediction by about as much as the head
//!   turned.
//! - blink: the dark of the eye shrinks to a line and comes back, within the
//!   fraction of a second a blink takes, while the rest of the face holds
//!   still.
//!
//! "Light" uses the deny cues. "Heavy" also needs one confirm cue before it
//! lets anybody in.

use crate::face::{Face, HeadPose, LEFT_EYE, LEFT_MOUTH, NOSE_TIP, RIGHT_EYE, RIGHT_MOUTH, estimate_pose};
use crate::img::{self, Bgr, Border, Grey, Pt, Rect};
use std::collections::VecDeque;

// The eyes are levelled and scaled onto a fixed canvas before anything is
// measured, so the numbers below are in canvas pixels and mean the same at
// any distance and any head tilt. 64 pixels between the eyes.
const CANVAS: i32 = 128;
const CANVAS_IOD: f32 = 64.0;
const EYE_Y: i32 = CANVAS / 2;
const RIGHT_EYE_X: i32 = CANVAS / 2 - (CANVAS_IOD / 2.0) as i32;
const LEFT_EYE_X: i32 = CANVAS / 2 + (CANVAS_IOD / 2.0) as i32;

// A slice through the middle of the eye, narrow enough to stay on the iris
// and tall enough to hold a fully open one (about 0.3 eye distances).
const BAND_HALF_WIDTH: i32 = 7;
const BAND_HALF_HEIGHT: i32 = 10;
// Skin under the eye, clear of lashes and of the shadow below the lid.
const SKIN_TOP: i32 = EYE_Y + 22;
const SKIN_BOTTOM: i32 = EYE_Y + 32;
const SKIN_HALF_WIDTH: i32 = 12;
// A row of the slice counts as dark below this share of the skin's
// brightness. Iris and pupil are well below it on every skin tone that was
// checked; a closed lid is skin and sits well above it.
const DARK_SHARE: f32 = 0.7;

// Glare: the Y floor and the chroma tolerance for "colourless and nearly
// white", in YCrCb.
const SPECULAR_LUMA: u8 = 235;
const SPECULAR_CHROMA: i32 = 10;
const GLARE_GRID: usize = 8;

// Tuning, in one place.
pub const WINDOW_MS: f64 = 4000.0;
pub const GLARE_FRACTION: f32 = 0.012;
pub const GLARE_CLUSTER: f32 = 0.55;
pub const DEPTH_MIN_TURN: f32 = 0.05;
pub const DEPTH_MIN_RANGE: f32 = 0.10;
pub const DEPTH_MIN_RATIO: f32 = 0.65;
pub const DEPTH_FLATTEST_NOSE: f32 = 0.30;
pub const DEPTH_SHAPE_SLACK: f32 = 0.02;
pub const BLINK_DIP: f32 = 0.55;
pub const BLINK_OPEN: f32 = 0.8;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GlareSample {
    pub valid: bool,
    /// Share of the face that is a colourless highlight.
    pub fraction: f32,
    /// Share of all highlight pixels that fall in the fullest of 8x8 cells.
    /// One slab of glass reflects into one place; skin sparkles everywhere.
    pub cluster: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EyeSample {
    pub valid: bool,
    /// How much of the eye's height is dark (iris, pupil). Falls towards zero
    /// when the lid comes down.
    pub left: f32,
    pub right: f32,
    pub openness: f32,
    /// Brightness of the skin under the eyes, to tell a blink from a change
    /// in the light.
    pub skin: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frame {
    pub t: f64,
    pub points: [Pt; 5],
    pub interocular: f32,
    pub pose: HeadPose,
    pub glare: GlareSample,
    pub device: bool,
    pub eyes: EyeSample,
}

impl Frame {
    /// A frame of nothing but the five points, for the tests.
    pub fn from_points(points: [Pt; 5], t: f64) -> Frame {
        let face = Face::from_points(points);
        Frame { t, points, interocular: face.interocular(), pose: estimate_pose(&face), ..Default::default() }
    }
}

fn levelled_face(bgr: &Bgr, face: &Face) -> Grey {
    let mid = face.eye_mid();
    let d = face.points[LEFT_EYE] - face.points[RIGHT_EYE];
    let roll = (d.y as f64).atan2(d.x as f64).to_degrees();
    let scale = CANVAS_IOD as f64 / face.interocular().max(1.0) as f64;
    let mut m = img::rotation(mid, roll, scale);
    m[0][2] += CANVAS as f64 / 2.0 - mid.x as f64;
    m[1][2] += CANVAS as f64 / 2.0 - mid.y as f64;
    bgr.grey().warp(&m, CANVAS as usize, CANVAS as usize, Border::Replicate)
}

fn eye_openness(canvas: &Grey, eye_x: i32) -> (f32, f32) {
    let skin_rect = Rect { x: eye_x - SKIN_HALF_WIDTH, y: SKIN_TOP, w: 2 * SKIN_HALF_WIDTH, h: SKIN_BOTTOM - SKIN_TOP };
    let skin = canvas.mean(skin_rect);
    if skin < 20.0 {
        return (-1.0, skin);
    }
    let band = Rect { x: eye_x - BAND_HALF_WIDTH, y: EYE_Y - BAND_HALF_HEIGHT, w: 2 * BAND_HALF_WIDTH + 1, h: 2 * BAND_HALF_HEIGHT + 1 };
    let rows = canvas.row_means(band);
    let dark = rows.iter().filter(|&&v| v < DARK_SHARE * skin).count();
    (dark as f32 / rows.len() as f32, skin)
}

pub fn measure_glare(bgr: &Bgr, face: &Face) -> GlareSample {
    let mut g = GlareSample::default();
    let iod = face.interocular();
    let centre = (face.eye_mid() + face.mouth_mid()) * 0.5;
    let roi = Rect { x: (centre.x - 1.1 * iod) as i32, y: (centre.y - 1.3 * iod) as i32, w: (2.2 * iod) as i32, h: (2.4 * iod) as i32 }
        .intersect(Rect { x: 0, y: 0, w: bgr.w as i32, h: bgr.h as i32 });
    if roi.w < 16 || roi.h < 16 {
        return g;
    }
    let ycc = bgr.crop(roi).ycrcb();
    let (w, h) = (roi.w as usize, roi.h as usize);

    // Glasses throw the screen back from right in front of the eyes, and that
    // is a big flat highlight too. It is not what is being looked for, so the
    // eyes are left out.
    let radius = (0.38 * iod) as i32;
    let eyes: Vec<(i32, i32)> = [RIGHT_EYE, LEFT_EYE]
        .iter()
        .map(|&e| {
            let p = face.points[e] - Pt::new(roi.x as f32, roi.y as f32);
            (p.x.round() as i32, p.y.round() as i32)
        })
        .collect();

    let mut cells = [0usize; GLARE_GRID * GLARE_GRID];
    let (mut total, mut considered) = (0usize, 0usize);
    for y in 0..h {
        let gy = (GLARE_GRID - 1).min(y * GLARE_GRID / h);
        for x in 0..w {
            if eyes.iter().any(|&(ex, ey)| {
                let (dx, dy) = (x as i32 - ex, y as i32 - ey);
                dx * dx + dy * dy <= radius * radius
            }) {
                continue;
            }
            considered += 1;
            let i = (y * w + x) * 3;
            let (luma, cr, cb) = (ycc.data[i], ycc.data[i + 1] as i32, ycc.data[i + 2] as i32);
            if luma < SPECULAR_LUMA || (cr - 128).abs() > SPECULAR_CHROMA || (cb - 128).abs() > SPECULAR_CHROMA {
                continue;
            }
            total += 1;
            let gx = (GLARE_GRID - 1).min(x * GLARE_GRID / w);
            cells[gy * GLARE_GRID + gx] += 1;
        }
    }
    g.valid = considered > 0;
    g.fraction = if considered > 0 { total as f32 / considered as f32 } else { 0.0 };
    // Too few pixels to say anything about their shape.
    g.cluster = if total >= 24 { *cells.iter().max().unwrap_or(&0) as f32 / total as f32 } else { 0.0 };
    g
}

pub fn measure_eyes(bgr: &Bgr, face: &Face) -> EyeSample {
    let mut s = EyeSample::default();
    if face.interocular() < 20.0 {
        return s;
    }
    let canvas = levelled_face(bgr, face);
    // "Right" is the person's right eye, which is on the canvas' left.
    let (right, skin_r) = eye_openness(&canvas, RIGHT_EYE_X);
    let (left, skin_l) = eye_openness(&canvas, LEFT_EYE_X);
    s.right = right;
    s.left = left;
    if right < 0.0 || left < 0.0 {
        return s;
    }
    s.openness = (left + right) / 2.0;
    s.skin = (skin_l + skin_r) / 2.0;
    s.valid = true;
    s
}

pub fn detect_device(bgr: &Bgr, face: &Face) -> bool {
    // Edges are looked for at half size. The bezel of a phone held up to the
    // camera is big, and small detail would only add rectangles that are not
    // one.
    let scale = 320.0 / bgr.w.max(1) as f64;
    let (sw, sh) = (((bgr.w as f64 * scale).round() as usize).max(1), ((bgr.h as f64 * scale).round() as usize).max(1));
    let grey = bgr.resize_area(sw, sh).grey().gaussian5();
    let edges = grey.canny(40, 120).dilate3();
    let contours = img::find_contours(&edges);

    let frame_area = (sw * sh) as f64;
    let face_area = face.area() as f64 * scale * scale;
    let points: Vec<Pt> = face.points.iter().map(|&p| p * scale as f32).collect();

    for c in &contours {
        let area = img::contour_area(c).abs();
        // A device held up to take somebody's place fills a good part of the
        // picture, and the face fills a good part of the device. A door or a
        // monitor far behind somebody's head does neither.
        if area < 0.10 * frame_area || area > 0.95 * frame_area || area < 1.6 * face_area || face_area / area < 0.08 {
            continue;
        }
        let quad = img::approx_poly(c, 0.03 * img::arc_length(c));
        if quad.len() != 4 || !img::is_convex(&quad) {
            continue;
        }
        let (a, b) = img::min_area_rect(&quad);
        let (short, long) = (a.min(b), a.max(b));
        if long <= 0.0 || short / long < 0.3 {
            continue;
        }
        // It has to frame the face: every one of the five points inside.
        if points.iter().all(|&p| img::inside_polygon(&quad, p)) {
            return true;
        }
    }
    false
}

pub fn measure_frame(bgr: &Bgr, face: &Face, t: f64) -> Frame {
    Frame {
        t,
        points: face.points,
        interocular: face.interocular(),
        pose: estimate_pose(face),
        glare: measure_glare(bgr, face),
        device: detect_device(bgr, face),
        eyes: measure_eyes(bgr, face),
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reading {
    pub frames: usize,
    pub denied: bool,
    pub denied_by: &'static str,
    pub confirmed: bool,
    pub confirmed_by: &'static str,
    /// For the test screen: how close each cue is to firing, 0 to 1 and
    /// more.
    pub glare: f32,
    pub device: f32,
    pub depth: f32,
    pub blink: f32,
}

#[derive(Clone, Copy, Debug)]
struct DepthPoint {
    points: [Pt; 5],
    yaw: f32,
    smooth_yaw: f32,
    nose_t: f32,
    /// Eye distance over eye to mouth distance: shrinks as the face turns
    /// away from the camera, whatever the face is made of.
    shape: f32,
    smooth_shape: f32,
}

#[derive(Default)]
pub struct Analyzer {
    frames: VecDeque<Frame>,
    total: usize,
    // The depth cue looks at the whole scan rather than the window, and is
    // worked out a frame at a time so it stays cheap.
    depth: Vec<DepthPoint>,
    depth_num: f64,
    depth_den: f64,
    depth_pairs: usize,
    glare_hits: usize,
    device_hits: usize,
    depth_fired: bool,
    blink_fired: bool,
}

impl Analyzer {
    pub fn reset(&mut self) {
        *self = Analyzer::default();
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub fn add(&mut self, frame: Frame) {
        self.frames.push_back(frame);
        while self.frames.front().is_some_and(|f| frame.t - f.t > WINDOW_MS) {
            self.frames.pop_front();
        }
        // Deny cues count over the whole scan, not just the window: a phone
        // that was seen once does not stop having been a phone.
        self.total += 1;
        if frame.glare.valid && frame.glare.fraction >= GLARE_FRACTION && frame.glare.cluster >= GLARE_CLUSTER {
            self.glare_hits += 1;
        }
        if frame.device {
            self.device_hits += 1;
        }
        self.add_depth(&frame);
        // Confirm cues latch once seen, for the rest of this scan.
        if !self.depth_fired {
            self.depth_fired = self.depth_range() >= DEPTH_MIN_RANGE && self.depth_pairs >= 6 && self.depth_ratio() >= DEPTH_MIN_RATIO && self.depth_consistent();
        }
        if !self.blink_fired {
            self.blink_fired = self.blink_seen().0;
        }
    }

    // How far the nose misses the spot a flat face would put it, measured
    // against how far the head turned.
    //
    // For a pair of frames far enough apart in yaw, the four points that lie
    // close to one plane (eyes, mouth corners) give the homography between
    // the two frames. A point on a flat picture lands exactly where it
    // predicts. The real nose tip is about a third of an eye distance in
    // front of that plane, so it lands off the prediction, sideways, by about
    // as much as the yaw estimate changed: both are the same parallax, seen
    // two ways. The slope of miss against yaw change is therefore near 1 for
    // anything with a nose sticking out of it, and near 0 for paper or a
    // screen.
    //
    // The yaw a frame is compared at comes from its neighbours, never from
    // the frame itself: yaw and miss are both read off the same nose, so the
    // nose's own jitter would push both the same way, and the slope of noise
    // against itself is 1.
    //
    // The slope alone says nothing about depth, only that nose and plane
    // disagree consistently. A card curled around a vertical axis has a
    // little depth too. What it cannot do is move the nose off the midline
    // by much, so the cue also needs the smoothed yaw to have covered
    // DEPTH_MIN_RANGE: about 12 degrees of a real head turning. And a card
    // curled hard and turned far enough is caught by depth_consistent: a face
    // that narrowed a lot more than its nose says it turned has less nose
    // than any head.
    fn add_depth(&mut self, frame: &Frame) {
        const MAX_FRAMES: usize = 400;
        if self.depth.len() >= MAX_FRAMES {
            return;
        }
        let face = Face::from_points(frame.points);
        let emd = (face.mouth_mid() - face.eye_mid()).norm();
        let shape = if emd > 1.0 { face.interocular() / emd } else { 0.0 };
        self.depth.push(DepthPoint { points: frame.points, yaw: frame.pose.yaw, smooth_yaw: 0.0, nose_t: frame.pose.nose_t, shape, smooth_shape: 0.0 });

        // The frame two back now has two neighbours on each side.
        let n = self.depth.len();
        if n < 5 {
            return;
        }
        let j = n - 3;
        let d = &self.depth;
        let smooth_yaw = (d[j - 2].yaw + d[j - 1].yaw + d[j + 1].yaw + d[j + 2].yaw) / 4.0;
        let smooth_shape = (d[j - 2].shape + d[j - 1].shape + d[j].shape + d[j + 1].shape + d[j + 2].shape) / 5.0;
        self.depth[j].smooth_yaw = smooth_yaw;
        self.depth[j].smooth_shape = smooth_shape;
        let b = self.depth[j];

        let axis = b.points[LEFT_EYE] - b.points[RIGHT_EYE];
        let axis_len = axis.norm().max(1e-3);
        let unit = axis * (1.0 / axis_len);

        for i in 2..j {
            let a = self.depth[i];
            let dy = b.smooth_yaw - a.smooth_yaw;
            if dy.abs() < DEPTH_MIN_TURN {
                continue;
            }
            let src = [a.points[RIGHT_EYE], a.points[LEFT_EYE], a.points[LEFT_MOUTH], a.points[RIGHT_MOUTH]];
            let dst = [b.points[RIGHT_EYE], b.points[LEFT_EYE], b.points[LEFT_MOUTH], b.points[RIGHT_MOUTH]];
            let Some(h) = img::perspective(&src, &dst) else { continue };
            let out = img::apply_perspective(&h, a.points[NOSE_TIP]);
            let miss = b.points[NOSE_TIP] - out;
            let rx = (miss.x * unit.x + miss.y * unit.y) / axis_len;
            self.depth_num += rx as f64 * dy as f64;
            self.depth_den += dy as f64 * dy as f64;
            self.depth_pairs += 1;
        }
    }

    pub fn depth_ratio(&self) -> f32 {
        if self.depth_den > 0.0 { (self.depth_num / self.depth_den) as f32 } else { 0.0 }
    }

    /// The spread of the smoothed yaw, from the 10th to the 90th percentile,
    /// so a single bad frame cannot stretch it.
    pub fn depth_range(&self) -> f32 {
        if self.depth.len() < 5 {
            return 0.0;
        }
        let mut ys: Vec<f32> = self.depth[2..self.depth.len() - 2].iter().map(|d| d.smooth_yaw).collect();
        if ys.len() < 3 {
            return 0.0;
        }
        ys.sort_by(f32::total_cmp);
        let at = |q: f64| ys[(q * (ys.len() - 1) as f64) as usize];
        at(0.9) - at(0.1)
    }

    pub fn depth_consistent(&self) -> bool {
        // Nodding also changes the eye to mouth distance, so only frames at
        // the head's usual pitch are compared. A card has no pitch of its own
        // to read (its nose is printed on), so none of its frames are left
        // out.
        if self.depth.len() < 5 {
            return false;
        }
        let inner = &self.depth[2..self.depth.len() - 2];
        if inner.len() < 3 {
            return false;
        }
        let mut ts: Vec<f32> = inner.iter().map(|d| d.nose_t).collect();
        ts.sort_by(f32::total_cmp);
        let median_t = ts[ts.len() / 2];
        let mut shapes: Vec<f32> = inner.iter().filter(|d| (d.nose_t - median_t).abs() < 0.05).map(|d| d.smooth_shape).collect();
        if shapes.len() < 3 {
            return false;
        }
        shapes.sort_by(f32::total_cmp);
        let widest = shapes[(0.95 * (shapes.len() - 1) as f64) as usize];
        let narrowest = shapes[(0.10 * (shapes.len() - 1) as f64) as usize];
        if widest <= 0.0 {
            return false;
        }
        let narrowed = 1.0 - narrowest / widest;
        let sin_turn = (self.depth_range() / DEPTH_FLATTEST_NOSE).min(1.0);
        let allowed = 1.0 - (1.0 - sin_turn * sin_turn).sqrt();
        narrowed <= allowed + DEPTH_SHAPE_SLACK
    }

    /// Whether a blink was seen, and how close the deepest dip came.
    fn blink_seen(&self) -> (bool, f32) {
        let s: Vec<&Frame> = self.frames.iter().filter(|f| f.eyes.valid).collect();
        if s.len() < 6 {
            return (false, 0.0);
        }
        let values: Vec<f32> = s.iter().map(|f| f.eyes.openness).collect();
        let mut sorted = values.clone();
        sorted.sort_by(f32::total_cmp);
        let baseline = sorted[(0.7 * (sorted.len() - 1) as f64) as usize];
        // An eye this narrow is not one the measure works on (heavy lids,
        // very dark eyes on dark skin, a camera that is too soft). Better to
        // abstain than to guess.
        if baseline < 0.15 {
            return (false, 0.0);
        }
        let strength = (1.0 - sorted[0] / baseline).clamp(0.0, 1.0) / (1.0 - BLINK_DIP);

        let n = s.len();
        for i in 1..n.saturating_sub(1) {
            if values[i] >= BLINK_DIP * baseline {
                continue;
            }
            // The run of closed frames around this one.
            let (mut a, mut b) = (i, i);
            while a > 0 && values[a - 1] < BLINK_OPEN * baseline {
                a -= 1;
            }
            while b + 1 < n && values[b + 1] < BLINK_OPEN * baseline {
                b += 1;
            }
            if a == 0 || b + 1 >= n {
                continue;
            }
            // Open right before and right after, within the time a blink
            // takes (a real one is 100 to 400 ms; slower is somebody closing
            // their eyes, or a picture being tilted away and back).
            let (before, after) = (s[a - 1], s[b + 1]);
            if after.t - before.t > 600.0 {
                continue;
            }
            // The rest of the face held still. A picture being moved blurs
            // and shifts everything at once; a blink only moves the lids.
            let moved = (after.points[NOSE_TIP] - before.points[NOSE_TIP]).norm();
            let iod = before.interocular.max(1.0);
            if moved > 0.15 * iod || (after.interocular - before.interocular).abs() > 0.08 * iod {
                continue;
            }
            let steady_light = (a..=b).all(|k| {
                let ratio = s[k].eyes.skin / before.eyes.skin.max(1.0);
                ratio > 0.9 && ratio < 1.1
            });
            if !steady_light {
                continue;
            }
            // Both eyes together. One eye going dark on its own is a shadow
            // or a hand, not a blink.
            if (a..=b).any(|k| s[k].eyes.left < 0.7 * baseline && s[k].eyes.right < 0.7 * baseline) {
                return (true, strength);
            }
        }
        (false, strength)
    }

    pub fn reading(&self) -> Reading {
        let mut r = Reading { frames: self.frames.len(), ..Default::default() };
        let seen = self.total.max(1) as f32;
        let glare_share = self.glare_hits as f32 / seen;
        let device_share = self.device_hits as f32 / seen;
        r.glare = (glare_share / 0.3).min(self.glare_hits as f32 / 3.0);
        r.device = (device_share / 0.3).min(self.device_hits as f32 / 3.0);
        if self.glare_hits >= 3 && glare_share >= 0.3 {
            r.denied = true;
            r.denied_by = "glare";
        } else if self.device_hits >= 3 && device_share >= 0.3 {
            r.denied = true;
            r.denied_by = "device";
        }
        r.depth = if self.depth_fired {
            1.0
        } else {
            (self.depth_ratio() / DEPTH_MIN_RATIO).clamp(0.0, 1.0) * (self.depth_range() / DEPTH_MIN_RANGE).clamp(0.0, 1.0)
        };
        let (_, strength) = self.blink_seen();
        r.blink = if self.blink_fired { 1.0 } else { strength.min(0.99) };
        if self.depth_fired {
            r.confirmed = true;
            r.confirmed_by = "depth";
        } else if self.blink_fired {
            r.confirmed = true;
            r.confirmed_by = "blink";
        }
        r
    }
}
