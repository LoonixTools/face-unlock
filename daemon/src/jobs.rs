//! Work that holds the camera: a scan or a setup. Only one runs at a time,
//! on a thread of its own so the socket stays responsive (a cancel has to get
//! through while a frame is being processed).

use face_unlock::config::{Liveness, Settings};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use vision::camera::{self, Camera};
use vision::face::{Embedding, Face, Vision, assess_quality, estimate_pose, yaw_degrees};
use vision::img::{Bgr, Pt};
use vision::liveness::{Analyzer, measure_eyes, measure_frame};
use vision::store::{self, Identity, Sample};

pub struct Ctx {
    pub vision: Arc<Mutex<Vision>>,
    pub cancel: Arc<AtomicBool>,
    pub finish_early: Arc<AtomicBool>,
    /// Progress for whoever asked, and for the bubble.
    pub emit: Box<dyn Fn(Value) + Send>,
}

impl Ctx {
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

pub fn result(ok: bool, reason: &str) -> Value {
    json!({"event": "result", "ok": ok, "reason": reason})
}

fn with(mut v: Value, extra: Value) -> Value {
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        for (k, x) in e {
            o.insert(k.clone(), x.clone());
        }
    }
    v
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn median(mut v: Vec<f32>) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

// ---------------------------------------------------------------------------
// Scanning
// ---------------------------------------------------------------------------

/// Two frames have to match. One could be a fluke of the light.
const MATCHES_NEEDED: u32 = 2;
/// Once somebody has matched, every frame spent on the recognizer is a frame
/// the blink check does not see, and a blink is only a few frames long. So
/// after a match the recognizer runs on every fourth frame only, which is
/// still often enough to notice a different face.
const RECHECK_EVERY: u32 = 4;
/// A face that jumps further than this between two frames is treated as a
/// different face, and everything learned about the previous one goes.
const JUMP_LIMIT: f32 = 0.6;
/// How long the deny cues watch before "light" lets anybody in.
const LIGHT_FRAMES_NEEDED: usize = 5;
/// Heavy: after a match, how long to wait for a sign of life before asking
/// for one, and how much longer to wait once asked.
const ASK_FOR_LIFE_AFTER_MS: u128 = 1200;
const EXTRA_TIME_MS: u128 = 2500;
/// Attention: a head turned further than this is not looking at the screen.
const MAX_YAW_DEGREES: f32 = 25.0;
const MAX_PITCH_T: f32 = 0.16;

pub struct ScanOutcome {
    pub result: Value,
    pub matched_score: f32,
    pub matched_id: String,
    pub matched_embedding: Embedding,
}

pub fn scan(ctx: &Ctx, settings: &Settings, faces: &[Identity], verbose: bool) -> ScanOutcome {
    let mut out = ScanOutcome { result: Value::Null, matched_score: 0.0, matched_id: String::new(), matched_embedding: vec![] };
    let mut cam = match Camera::open(&settings.camera) {
        Ok(c) => c,
        Err(e) => {
            // The daemon checks for a busy camera before a scan, but a video
            // call can take it in between.
            let reason = if camera::in_use(&settings.camera) { "camera-busy" } else { "camera" };
            out.result = with(result(false, reason), json!({"message": e}));
            return out;
        }
    };
    (ctx.emit)(json!({"event": "started", "camera": cam.description}));

    // What the attention check compares against: this person's own
    // level-head nose position and open-eye measurement, from the setup.
    let nose_ts: Vec<f32> = faces.iter().filter(|f| f.enabled).map(|f| f.nose_t).collect();
    let resting_nose_t = median(nose_ts.clone());
    let open_eyes = median(faces.iter().filter(|f| f.enabled && f.eyes > 0.0).map(|f| f.eyes).collect());
    let threshold = settings.threshold();
    let mode = settings.liveness;

    let clock = Instant::now();
    let mut deadline = settings.scan_seconds as u128 * 1000;
    let mut extended = false;
    let mut live = Analyzer::default();
    let (mut matched, mut mismatched, mut attention_misses, mut quality_misses, mut since_check) = (0u32, 0u32, 0u32, 0u32, 0u32);
    let mut last_check_matched = false;
    let mut saw_face = false;
    let mut read_failures = 0;
    let mut first_match_at: Option<u128> = None;
    let mut last_centre: Option<Pt> = None;
    let mut last_face_at: Option<u128> = None;
    let mut best_seen = -1f32;
    let mut matched_identity: Option<usize> = None;
    let mut hinted = HashSet::new();
    let hint = |what: &str, hinted: &mut HashSet<String>| {
        if hinted.insert(what.to_string()) {
            (ctx.emit)(json!({"event": "hint", "hint": what}));
        }
    };

    while !ctx.cancelled() {
        let elapsed = clock.elapsed().as_millis();
        if elapsed > deadline {
            // Heavy has matched and only waits for a sign of life: give the
            // person the time to blink that the hint just asked for.
            if mode == Liveness::Heavy && matched >= MATCHES_NEEDED && !extended {
                deadline += EXTRA_TIME_MS;
                extended = true;
                continue;
            }
            break;
        }
        let Some((frame, t)) = cam.read() else {
            read_failures += 1;
            if read_failures > 10 {
                out.result = with(result(false, "camera"), json!({"message": "the camera stopped delivering frames"}));
                return out;
            }
            continue;
        };
        read_failures = 0;

        let found = ctx.vision.lock().unwrap_or_else(|e| e.into_inner()).detect(&frame);
        let Some(face) = found.first() else { continue };
        let iod = face.interocular();
        let centre = (face.eye_mid() + face.mouth_mid()) * 0.5;

        // A face that appeared somewhere else, or after a gap, may be a
        // different one. Whatever was concluded about the last one goes.
        let jumped = last_centre.is_some_and(|c| (centre - c).norm() > JUMP_LIMIT * iod);
        let gap = last_face_at.is_some_and(|at| elapsed - at > 400);
        if jumped || gap {
            live.reset();
            matched = 0;
            last_check_matched = false;
            first_match_at = None;
            matched_identity = None;
            out.matched_score = 0.0;
            out.matched_embedding.clear();
        }
        last_centre = Some(centre);
        last_face_at = Some(elapsed);
        if !saw_face {
            saw_face = true;
            (ctx.emit)(json!({"event": "face"}));
        }

        let quality = assess_quality(&frame, face);
        let lf = measure_frame(&frame, face, t);
        live.add(lf);
        let reading = live.reading();
        if mode != Liveness::Off && reading.denied {
            out.result = with(result(false, "spoof"), json!({"cue": reading.denied_by}));
            return out;
        }
        if !quality.ok() {
            quality_misses += 1;
            if quality.too_small {
                hint("closer", &mut hinted);
            } else if quality.too_dark {
                hint("light", &mut hinted);
            }
            continue;
        }

        // Whether this face looks at the screen with open eyes. Only asked
        // of a face that matches: a stranger is a stranger whichever way they
        // look, and should be counted as one.
        let attentive = |hinted: &mut HashSet<String>| {
            if !settings.attention {
                return true;
            }
            let facing = yaw_degrees(lf.pose.yaw).abs() <= MAX_YAW_DEGREES && (nose_ts.is_empty() || (lf.pose.nose_t - resting_nose_t).abs() <= MAX_PITCH_T);
            // Eyes that measure as closed, next to how open they measured at
            // setup. Skipped for eyes the measure does not work on.
            let eyes_open = !lf.eyes.valid || open_eyes < 0.15 || lf.eyes.openness >= 0.45 * open_eyes;
            if !facing {
                hint("look", hinted);
            }
            facing && eyes_open
        };

        let mut score = None;
        if matched < MATCHES_NEEDED || {
            since_check += 1;
            since_check >= RECHECK_EVERY
        } {
            since_check = 0;
            let e = ctx.vision.lock().unwrap_or_else(|e| e.into_inner()).embed(&frame, face);
            let m = store::best_match(faces, &e);
            score = Some(m.score);
            best_seen = best_seen.max(m.score);
            if m.identity.is_some() && m.score >= threshold {
                if !attentive(&mut hinted) {
                    attention_misses += 1;
                    continue;
                }
                matched += 1;
                last_check_matched = true;
                first_match_at.get_or_insert(elapsed);
                if m.score > out.matched_score {
                    out.matched_score = m.score;
                    matched_identity = m.identity;
                    out.matched_embedding = e;
                }
            } else {
                mismatched += 1;
                // The face that matched before does not match now. Whatever
                // it did to look alive was done by somebody else's face.
                if last_check_matched {
                    live.reset();
                    matched = 0;
                    first_match_at = None;
                    matched_identity = None;
                    out.matched_score = 0.0;
                    out.matched_embedding.clear();
                }
                last_check_matched = false;
            }
        } else if !attentive(&mut hinted) {
            // Between checks: a matched face that looks away or closes its
            // eyes (a blink does both for a moment) is not counted as
            // looking.
            attention_misses += 1;
            continue;
        }

        if verbose {
            let mut info = json!({
                "event": "frame",
                "t": t as i64,
                "yaw": yaw_degrees(lf.pose.yaw),
                "eyes": lf.eyes.openness,
                "glare": reading.glare,
                "device": reading.device,
                "depth": reading.depth,
                "blink": reading.blink,
                "matched": matched,
            });
            if let Some(s) = score {
                info["score"] = s.into();
            }
            (ctx.emit)(info);
        }

        if matched < MATCHES_NEEDED || !last_check_matched {
            continue;
        }
        let alive = match mode {
            Liveness::Off => true,
            Liveness::Light => live.frame_count() >= LIGHT_FRAMES_NEEDED,
            Liveness::Heavy => {
                if !reading.confirmed && first_match_at.is_some_and(|at| elapsed - at > ASK_FOR_LIFE_AFTER_MS) {
                    hint("blink", &mut hinted);
                }
                reading.confirmed
            }
        };
        if alive && let Some(i) = matched_identity {
            let id = &faces[i];
            out.matched_id = id.id.clone();
            out.result = with(
                result(true, "ok"),
                json!({"name": id.name, "id": id.id, "score": out.matched_score, "liveness": reading.confirmed_by, "ms": clock.elapsed().as_millis() as i64}),
            );
            return out;
        }
    }

    out.result = if ctx.cancelled() {
        result(false, "cancelled")
    } else if matched >= MATCHES_NEEDED {
        result(false, "liveness")
    } else if mismatched >= 3 {
        with(result(false, "mismatch"), json!({"score": best_seen}))
    } else if attention_misses > 0 {
        result(false, "attention")
    } else if quality_misses > 0 {
        result(false, "quality")
    } else {
        result(false, "no-face")
    };
    out
}

// ---------------------------------------------------------------------------
// Setting up a face
// ---------------------------------------------------------------------------
// Like a phone: look straight at the camera first, then move the head around
// in a circle while the ring fills up. The centre gives the samples most
// unlocks will match against and the level-head measurements the attention
// check needs; the eight directions around it are what still matches when
// somebody glances at the screen from the side.

/// A head turned about 15 degrees moves the nose this far (in eye
/// distances), sideways for a turn and up or down for a nod. The ring is
/// drawn in these units, so 1.0 is a comfortable turn.
const TURN_UNIT: f32 = 0.13;
/// Far enough out to count for a direction.
const CAPTURE_AT: f32 = 0.8;
/// Straight enough to count as looking straight ahead.
const STRAIGHT_YAW: f32 = 0.06;
const SAMPLE_SPACING_MS: u128 = 150;
const CENTRE_SPACING_MS: u128 = 250;
const PREVIEW_EVERY_MS: u128 = 66;
const GIVE_UP_AFTER_MS: u128 = 120 * 1000;
const PREVIEW_WIDTH: usize = 480;
pub const SAMPLES_PER_SECTOR: usize = 2;
pub const CENTRE_SAMPLES: usize = 3;
pub const SECTORS_FOR_EARLY_FINISH: usize = 4;

/// The eight directions, clockwise from straight up, as seen in the mirrored
/// preview.
pub fn sector_name(sector: usize) -> &'static str {
    ["up", "up-right", "right", "down-right", "down", "down-left", "left", "up-left"][sector % 8]
}

fn preview(frame: &Bgr, face: Option<&Face>) -> Value {
    let mirrored = frame.flip();
    let scale = PREVIEW_WIDTH as f64 / mirrored.w as f64;
    let small = mirrored.resize_area(PREVIEW_WIDTH, ((mirrored.h as f64 * scale).round() as usize).max(1));
    let rgb: Vec<u8> = small.data.as_chunks::<3>().0.iter().flat_map(|p| [p[2], p[1], p[0]]).collect();
    let mut jpeg = Vec::new();
    let _ = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 72).encode(&rgb, small.w as u32, small.h as u32, image::ExtendedColorType::Rgb8);
    let mut msg = json!({"event": "frame", "w": small.w, "h": small.h, "jpeg": store::base64(&jpeg)});
    if let Some(f) = face {
        // In the mirrored picture, as a share of its size.
        let (w, h) = (frame.w as f32, frame.h as f32);
        msg["face"] = json!({"x": (w - f.bx - f.bw) / w, "y": f.by / h, "w": f.bw / w, "h": f.bh / h});
    }
    msg
}

pub struct EnrollOutcome {
    pub result: Value,
    pub identity: Option<Identity>,
}

pub fn enroll(ctx: &Ctx, settings: &Settings, name: &str) -> EnrollOutcome {
    let fail = |r: Value| EnrollOutcome { result: r, identity: None };
    let mut cam = match Camera::open(&settings.camera) {
        Ok(c) => c,
        Err(e) => return fail(with(result(false, "camera"), json!({"message": e}))),
    };
    (ctx.emit)(json!({"event": "started", "camera": cam.description}));

    let clock = Instant::now();
    let mut centre_nose_t = Vec::new();
    let mut centre_eyes = Vec::new();
    let mut samples: Vec<Sample> = Vec::new();
    let mut centre_count = 0;
    let mut last_centre_sample: Option<u128> = None;
    let mut centre_done = false;
    let mut resting_nose_t = 0.55f32;
    let mut counts = [0usize; 8];
    let mut last_at: [Option<u128>; 8] = [None; 8];
    let mut last_hint = String::new();
    let mut last_preview: Option<u128> = None;
    let mut read_failures = 0;
    let hint = |what: &str, last: &mut String| {
        if what != last {
            *last = what.to_string();
            (ctx.emit)(json!({"event": "hint", "hint": what}));
        }
    };
    let done = |counts: &[usize; 8]| counts.iter().filter(|&&c| c == SAMPLES_PER_SECTOR).count();

    while !ctx.cancelled() && clock.elapsed().as_millis() < GIVE_UP_AFTER_MS {
        let Some((frame, _)) = cam.read() else {
            read_failures += 1;
            if read_failures > 10 {
                return fail(with(result(false, "camera"), json!({"message": "the camera stopped delivering frames"})));
            }
            continue;
        };
        read_failures = 0;
        let now = clock.elapsed().as_millis();
        let found = ctx.vision.lock().unwrap_or_else(|e| e.into_inner()).detect(&frame);
        let face = found.first();
        if last_preview.is_none_or(|at| now - at >= PREVIEW_EVERY_MS) {
            last_preview = Some(now);
            (ctx.emit)(preview(&frame, face));
        }
        let Some(face) = face else {
            (ctx.emit)(json!({"event": "pose", "face": false}));
            hint("no-face", &mut last_hint);
            continue;
        };
        // Somebody else in the picture, close enough to be taken for the
        // person setting up.
        if found.len() > 1 && found[1].area() > 0.5 * face.area() {
            hint("one-face", &mut last_hint);
            continue;
        }
        let q = assess_quality(&frame, face);
        if !q.ok() {
            hint(
                if q.too_small {
                    "closer"
                } else if q.too_dark {
                    "light"
                } else if q.too_bright {
                    "bright"
                } else {
                    "still"
                },
                &mut last_hint,
            );
            continue;
        }
        let pose = estimate_pose(face);

        if !centre_done {
            let straight = pose.yaw.abs() <= STRAIGHT_YAW;
            (ctx.emit)(json!({"event": "pose", "face": true, "phase": "center", "x": -pose.yaw / TURN_UNIT, "y": 0.0}));
            if !straight {
                hint("straight", &mut last_hint);
                continue;
            }
            hint("hold", &mut last_hint);
            centre_nose_t.push(pose.nose_t);
            let eyes = measure_eyes(&frame, face);
            if eyes.valid {
                centre_eyes.push(eyes.openness);
            }
            if centre_count < CENTRE_SAMPLES && last_centre_sample.is_none_or(|at| now - at >= CENTRE_SPACING_MS) {
                let e = ctx.vision.lock().unwrap_or_else(|e| e.into_inner()).embed(&frame, face);
                if !e.is_empty() {
                    samples.push(Sample { embedding: e, pose: "center".into(), time: self::now() });
                    centre_count += 1;
                    last_centre_sample = Some(now);
                }
            }
            if centre_count >= CENTRE_SAMPLES && centre_nose_t.len() >= 6 {
                resting_nose_t = median(centre_nose_t.clone());
                centre_done = true;
                (ctx.emit)(json!({"event": "captured", "pose": "center"}));
                hint("circle", &mut last_hint);
            }
            continue;
        }

        // Screen directions in the mirrored preview: turning to one's own
        // left moves the face to the left of the screen, looking up moves it
        // up.
        let x = -pose.yaw / TURN_UNIT;
        let y = -(pose.nose_t - resting_nose_t) / TURN_UNIT;
        let reach = x.hypot(y);
        let mut angle = (x as f64).atan2(y as f64).to_degrees();
        if angle < 0.0 {
            angle += 360.0;
        }
        (ctx.emit)(json!({"event": "pose", "face": true, "phase": "circle", "x": x, "y": y, "angle": angle, "reach": reach}));
        if reach >= CAPTURE_AT {
            let sector = (angle / 45.0).round() as usize % 8;
            if counts[sector] < SAMPLES_PER_SECTOR && last_at[sector].is_none_or(|at| now - at >= SAMPLE_SPACING_MS) {
                let e = ctx.vision.lock().unwrap_or_else(|e| e.into_inner()).embed(&frame, face);
                if !e.is_empty() {
                    samples.push(Sample { embedding: e, pose: sector_name(sector).into(), time: self::now() });
                    last_at[sector] = Some(now);
                    counts[sector] += 1;
                    if counts[sector] == SAMPLES_PER_SECTOR {
                        (ctx.emit)(json!({"event": "captured", "pose": sector_name(sector), "sector": sector, "done": done(&counts)}));
                    }
                }
            }
        }
        if done(&counts) == 8 || (ctx.finish_early.load(Ordering::Relaxed) && done(&counts) >= SECTORS_FOR_EARLY_FINISH) {
            break;
        }
    }

    if ctx.cancelled() {
        return fail(result(false, "cancelled"));
    }
    if !centre_done || done(&counts) < SECTORS_FOR_EARLY_FINISH {
        return fail(result(false, "timeout"));
    }
    let identity = Identity {
        id: store::new_id(),
        name: name.into(),
        created: now(),
        enabled: true,
        nose_t: resting_nose_t,
        eyes: median(centre_eyes),
        samples,
    };
    EnrollOutcome {
        result: with(result(true, "ok"), json!({"id": identity.id, "name": identity.name, "samples": identity.samples.len()})),
        identity: Some(identity),
    }
}
