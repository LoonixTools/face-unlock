//! Finding a face and turning it into numbers.
//!
//! Two small networks from the OpenCV model zoo do the work. YuNet finds
//! faces and five points on each (the eyes, the tip of the nose, the corners
//! of the mouth). SFace turns an aligned crop of one face into 128 numbers,
//! and two crops of the same person give numbers that point the same way.
//! Both run on the CPU through tract, in pure Rust.
//!
//! Everything else here is plain geometry on those five points.

use crate::img::{Bgr, Border, Pt, Rect};
use std::collections::HashMap;
use std::path::Path;
use tract_onnx::prelude::*;
use tract_onnx::tract_hir;

pub const DETECTOR_FILE: &str = "face_detection_yunet_2023mar.onnx";
pub const RECOGNIZER_FILE: &str = "face_recognition_sface_2021dec.onnx";

/// The five points, in the order YuNet reports them. "Right" and "left" are
/// the person's own, so on an unmirrored picture the right eye is on the
/// left.
pub const RIGHT_EYE: usize = 0;
pub const LEFT_EYE: usize = 1;
pub const NOSE_TIP: usize = 2;
pub const RIGHT_MOUTH: usize = 3;
pub const LEFT_MOUTH: usize = 4;

/// A face whose eyes are closer together than this is too far away for the
/// recognizer to see detail in, and the liveness checks drown in noise.
const MIN_INTEROCULAR: f32 = 28.0;

const SCORE_THRESHOLD: f32 = 0.75;
const NMS_THRESHOLD: f32 = 0.3;
const TOP_K: usize = 20;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Face {
    /// x, y, width, height.
    pub bx: f32,
    pub by: f32,
    pub bw: f32,
    pub bh: f32,
    pub points: [Pt; 5],
    pub score: f32,
}

impl Face {
    pub fn interocular(&self) -> f32 {
        (self.points[LEFT_EYE] - self.points[RIGHT_EYE]).norm()
    }
    pub fn eye_mid(&self) -> Pt {
        (self.points[RIGHT_EYE] + self.points[LEFT_EYE]) * 0.5
    }
    pub fn mouth_mid(&self) -> Pt {
        (self.points[RIGHT_MOUTH] + self.points[LEFT_MOUTH]) * 0.5
    }
    pub fn area(&self) -> f32 {
        self.bw * self.bh
    }
    pub fn from_points(points: [Pt; 5]) -> Face {
        Face { points, ..Default::default() }
    }
}

/// Where the head points, read off the five points alone.
///
/// yaw is the nose's sideways offset from the line through the middle of
/// the eyes and the middle of the mouth, in interocular distances. A nose
/// sits about half an interocular distance in front of the face, so this is
/// close to 0.5 * sin(head yaw). Positive when the head turns to the
/// person's left.
///
/// nose_t is how far down the nose tip sits between the eye line (0) and the
/// mouth line (1). It changes with pitch, but where it sits for a level head
/// is different for every face, so it only means something next to the same
/// person's own resting value.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HeadPose {
    pub roll: f32,
    pub yaw: f32,
    pub nose_t: f32,
}

fn rotate_around(p: Pt, centre: Pt, angle: f32) -> Pt {
    let (s, c) = angle.sin_cos();
    let d = p - centre;
    Pt::new(centre.x + d.x * c - d.y * s, centre.y + d.x * s + d.y * c)
}

pub fn estimate_pose(face: &Face) -> HeadPose {
    let mut pose = HeadPose::default();
    let eyes = face.points[LEFT_EYE] - face.points[RIGHT_EYE];
    pose.roll = eyes.y.atan2(eyes.x);

    // Level the face first, so a tilted head does not read as a turned one.
    let centre = face.eye_mid();
    let p: Vec<Pt> = face.points.iter().map(|&q| rotate_around(q, centre, -pose.roll)).collect();
    let eye_mid = (p[RIGHT_EYE] + p[LEFT_EYE]) * 0.5;
    let mouth_mid = (p[RIGHT_MOUTH] + p[LEFT_MOUTH]) * 0.5;
    let iod = (p[LEFT_EYE] - p[RIGHT_EYE]).norm().max(1.0);
    let span = mouth_mid.y - eye_mid.y;
    if span < 1.0 {
        return pose;
    }
    let nose = p[NOSE_TIP];
    let t = (nose.y - eye_mid.y) / span;
    let midline_x = eye_mid.x + (mouth_mid.x - eye_mid.x) * t;
    // The eyes are reported person-right first, which is image-left on an
    // unmirrored frame. A nose moving image-right is a head turning to the
    // person's left.
    pose.yaw = (nose.x - midline_x) / iod;
    pose.nose_t = t;
    pose
}

/// Degrees, for people. The estimate is rough by nature.
pub fn yaw_degrees(yaw: f32) -> f32 {
    (yaw / 0.5).clamp(-1.0, 1.0).asin().to_degrees()
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Quality {
    pub brightness: f32,
    pub sharpness: f32,
    pub too_small: bool,
    pub too_dark: bool,
    pub too_bright: bool,
    pub blurry: bool,
}

impl Quality {
    pub fn ok(&self) -> bool {
        !self.too_small && !self.too_dark && !self.too_bright && !self.blurry
    }
}

pub fn assess_quality(bgr: &Bgr, face: &Face) -> Quality {
    let mut q = Quality { too_small: face.interocular() < MIN_INTEROCULAR, ..Default::default() };
    // The middle of the face, without hair and background at the edges.
    let iod = face.interocular();
    let c = (face.eye_mid() + face.mouth_mid()) * 0.5;
    let roi = Rect { x: (c.x - iod) as i32, y: (c.y - iod) as i32, w: (2.0 * iod) as i32, h: (2.0 * iod) as i32 }.intersect(Rect { x: 0, y: 0, w: bgr.w as i32, h: bgr.h as i32 });
    if roi.w < 8 || roi.h < 8 {
        q.too_small = true;
        return q;
    }
    let grey = bgr.crop(roi).grey();
    q.brightness = grey.mean(Rect { x: 0, y: 0, w: roi.w, h: roi.h });
    // Sharpness at a fixed scale, so a face far away and one up close are
    // judged the same.
    q.sharpness = grey.resize_area(96, 96).laplacian_variance();
    q.too_dark = q.brightness < 40.0;
    q.too_bright = q.brightness > 230.0;
    q.blurry = q.sharpness < 12.0;
    q
}

pub type Embedding = Vec<f32>;
pub const EMBEDDING_SIZE: usize = 128;

/// Unit vectors, so this is their dot product. -1 for a mismatch in size.
pub fn similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return -1.0;
    }
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

type Plan = std::sync::Arc<TypedSimplePlan>;

pub struct Vision {
    detector_raw: InferenceModel,
    /// One plan for each size of picture, made the first time it comes.
    detectors: HashMap<(usize, usize), Plan>,
    recognizer: Plan,
}

impl Vision {
    pub fn load(dir: &Path) -> Result<Vision, String> {
        let det = dir.join(DETECTOR_FILE);
        let rec = dir.join(RECOGNIZER_FILE);
        for f in [&det, &rec] {
            if !f.is_file() {
                return Err(format!("model missing: {}", f.display()));
            }
        }
        let mut detector_raw = tract_onnx::onnx().model_for_path(&det).map_err(|e| e.to_string())?;
        // The file says 640x640, and some shapes inside are worked out for
        // that. Forgotten, so the network takes any size, as OpenCV lets it.
        for id in 0..detector_raw.nodes().len() {
            if detector_raw.node(id).op_is::<tract_hir::ops::source::Source>() {
                continue;
            }
            for o in 0..detector_raw.node(id).outputs.len() {
                detector_raw.set_outlet_fact(OutletId::new(id, o), InferenceFact::default()).map_err(|e| e.to_string())?;
            }
        }
        let recognizer = tract_onnx::onnx()
            .model_for_path(&rec)
            .and_then(|m| m.with_input_fact(0, f32::fact([1, 3, 112, 112]).into()))
            .and_then(|m| m.into_optimized())
            .and_then(|m| m.into_runnable())
            .map_err(|e| e.to_string())?;
        Ok(Vision { detector_raw, detectors: HashMap::new(), recognizer })
    }

    fn detector(&mut self, h: usize, w: usize) -> Option<&Plan> {
        if !self.detectors.contains_key(&(h, w)) {
            let plan = self
                .detector_raw
                .clone()
                .with_input_fact(0, f32::fact([1, 3, h, w]).into())
                .and_then(|m| m.into_optimized())
                .and_then(|m| m.into_runnable())
                .map_err(|e| eprintln!("warning: cannot prepare the face finder for {w}x{h}: {e}"))
                .ok()?;
            self.detectors.insert((h, w), plan);
        }
        self.detectors.get(&(h, w))
    }

    /// Faces sorted by size, largest first.
    pub fn detect(&mut self, bgr: &Bgr) -> Vec<Face> {
        if bgr.w == 0 || bgr.h == 0 {
            return vec![];
        }
        // Padded with black to a multiple of 32, as OpenCV's FaceDetectorYN.
        let pad = |n: usize| (n - 1) / 32 * 32 + 32;
        let (pw, ph) = (pad(bgr.w), pad(bgr.h));
        let mut input = tract_ndarray::Array4::<f32>::zeros((1, 3, ph, pw));
        for y in 0..bgr.h {
            for x in 0..bgr.w {
                let i = (y * bgr.w + x) * 3;
                for c in 0..3 {
                    input[[0, c, y, x]] = bgr.data[i + c] as f32;
                }
            }
        }
        let Some(plan) = self.detector(ph, pw) else { return vec![] };
        let Ok(out) = plan.run(tvec!(Tensor::from(input).into())) else { return vec![] };
        if out.len() != 12 {
            return vec![];
        }
        let mut faces = Vec::new();
        for (i, stride) in [8usize, 16, 32].into_iter().enumerate() {
            let (cols, rows) = (pw / stride, ph / stride);
            let get = |k: usize| floats(&out[k]);
            let (cls, obj, bbox, kps) = (get(i), get(i + 3), get(i + 6), get(i + 9));
            let n = cols * rows;
            if cls.len() < n || obj.len() < n || bbox.len() < n * 4 || kps.len() < n * 10 {
                continue;
            }
            let s = stride as f32;
            for r in 0..rows {
                for c in 0..cols {
                    let idx = r * cols + c;
                    let score = (cls[idx].clamp(0.0, 1.0) * obj[idx].clamp(0.0, 1.0)).sqrt();
                    if score < SCORE_THRESHOLD {
                        continue;
                    }
                    let cx = (c as f32 + bbox[idx * 4]) * s;
                    let cy = (r as f32 + bbox[idx * 4 + 1]) * s;
                    let w = bbox[idx * 4 + 2].exp() * s;
                    let h = bbox[idx * 4 + 3].exp() * s;
                    let mut points = [Pt::default(); 5];
                    for (k, p) in points.iter_mut().enumerate() {
                        *p = Pt::new((kps[idx * 10 + 2 * k] + c as f32) * s, (kps[idx * 10 + 2 * k + 1] + r as f32) * s);
                    }
                    faces.push(Face { bx: cx - w / 2.0, by: cy - h / 2.0, bw: w, bh: h, points, score });
                }
            }
        }
        let mut faces = nms(faces);
        faces.sort_by(|a, b| b.area().total_cmp(&a.area()));
        faces
    }

    /// 128 numbers of unit length, or empty when the network failed.
    pub fn embed(&self, bgr: &Bgr, face: &Face) -> Embedding {
        let aligned = align(bgr, face);
        let mut input = tract_ndarray::Array4::<f32>::zeros((1, 3, 112, 112));
        for y in 0..112 {
            for x in 0..112 {
                let i = (y * 112 + x) * 3;
                // RGB, as OpenCV feeds it with swapRB.
                input[[0, 0, y, x]] = aligned.data[i + 2] as f32;
                input[[0, 1, y, x]] = aligned.data[i + 1] as f32;
                input[[0, 2, y, x]] = aligned.data[i] as f32;
            }
        }
        let Ok(out) = self.recognizer.run(tvec!(Tensor::from(input).into())) else { return vec![] };
        let v = floats(&out[0]);
        if v.len() != EMBEDDING_SIZE {
            return vec![];
        }
        let norm = v.iter().map(|x| (*x as f64) * (*x as f64)).sum::<f64>().sqrt();
        if norm <= 0.0 {
            return vec![];
        }
        v.iter().map(|x| (*x as f64 / norm) as f32).collect()
    }
}

fn floats(t: &Tensor) -> Vec<f32> {
    t.try_as_plain_ram().and_then(|v| v.as_slice::<f32>().map(<[f32]>::to_vec)).unwrap_or_default()
}

/// Where the five points go on SFace's 112x112 crop.
const ARCFACE: [Pt; 5] = [Pt::new(38.2946, 51.6963), Pt::new(73.5318, 51.5014), Pt::new(56.0252, 71.7366), Pt::new(41.5493, 92.3655), Pt::new(70.7299, 92.2041)];

/// The face turned, scaled and moved onto SFace's crop: the least-squares
/// similarity from its five points onto the template's.
pub fn align(bgr: &Bgr, face: &Face) -> Bgr {
    let src = face.points;
    let n = 5.0f64;
    let (smx, smy) = (src.iter().map(|p| p.x as f64).sum::<f64>() / n, src.iter().map(|p| p.y as f64).sum::<f64>() / n);
    // OpenCV's own mean of the template, which differs a hair from the
    // exact one.
    let (dmx, dmy) = (56.0262f64, 71.9008f64);
    let (mut a, mut b, mut var) = (0f64, 0f64, 0f64);
    for i in 0..5 {
        let (sx, sy) = (src[i].x as f64 - smx, src[i].y as f64 - smy);
        let (dx, dy) = (ARCFACE[i].x as f64 - dmx, ARCFACE[i].y as f64 - dmy);
        a += sx * dx + sy * dy;
        b += sx * dy - sy * dx;
        var += sx * sx + sy * sy;
    }
    let m = if var > 0.0 {
        let (a, b) = (a / var, b / var);
        [[a, -b, dmx - (a * smx - b * smy)], [b, a, dmy - (b * smx + a * smy)]]
    } else {
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
    };
    bgr.warp(&m, 112, 112, Border::Constant)
}

/// Non-maximum suppression on the boxes, as cv::dnn::NMSBoxes does with
/// integer boxes.
fn nms(mut faces: Vec<Face>) -> Vec<Face> {
    faces.sort_by(|a, b| b.score.total_cmp(&a.score));
    faces.truncate(TOP_K);
    let ibox = |f: &Face| (f.bx as i32 as f32, f.by as i32 as f32, f.bw as i32 as f32, f.bh as i32 as f32);
    let iou = |a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)| {
        let x0 = a.0.max(b.0);
        let y0 = a.1.max(b.1);
        let x1 = (a.0 + a.2).min(b.0 + b.2);
        let y1 = (a.1 + a.3).min(b.1 + b.3);
        let inter = (x1 - x0).max(0.0) * (y1 - y0).max(0.0);
        let union = a.2 * a.3 + b.2 * b.3 - inter;
        if union > 0.0 { inter / union } else { 0.0 }
    };
    let mut kept: Vec<Face> = Vec::new();
    for f in faces {
        if kept.iter().all(|k| iou(ibox(k), ibox(&f)) <= NMS_THRESHOLD) {
            kept.push(f);
        }
    }
    kept
}
