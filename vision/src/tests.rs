//! The liveness cues against synthetic heads and photos, the face store,
//! and the camera's JPEG decoding. No camera, no root, no models.
//!
//! There is no camera in a test, so the five landmarks are made the way a
//! camera would make them: points of a head in millimetres, turned,
//! projected through a pinhole the size of a laptop webcam, and jittered by
//! the noise YuNet adds. A real head keeps its nose in front of its face. A
//! photograph is the same five points printed flat on a card, and the card
//! is turned and tilted instead of the head.

use crate::face::{EMBEDDING_SIZE, similarity};
use crate::img::Pt;
use crate::liveness::{Analyzer, Frame, Reading};
use crate::store::{self, Identity, Sample, Store};
use std::f64::consts::PI;

const FOCAL: f64 = 600.0;
const CX: f64 = 320.0;
const CY: f64 = 240.0;
const DISTANCE: f64 = 550.0;
const FPS: f64 = 20.0;

/// xorshift64*, with Box-Muller for the normal distribution.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn uniform(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn normal(&mut self, sigma: f64) -> f64 {
        let u = self.uniform().max(1e-12);
        let v = self.uniform();
        sigma * (-2.0 * u.ln()).sqrt() * (2.0 * PI * v).cos()
    }
}

type P3 = (f64, f64, f64);

// Person-right eye first, the way YuNet reports it. x to the image's right,
// y down, z towards the camera. 63 mm between the eyes, the nose tip 28 mm in
// front of the eyes, the mouth corners a little in front of them too.
const HEAD: [P3; 5] = [(-31.5, 0.0, 0.0), (31.5, 0.0, 0.0), (0.0, 42.0, 28.0), (-25.0, 72.0, 8.0), (25.0, 72.0, 8.0)];

fn rotate(p: P3, yaw: f64, pitch: f64, roll: f64) -> P3 {
    let a = (p.0 * yaw.cos() + p.2 * yaw.sin(), p.1, -p.0 * yaw.sin() + p.2 * yaw.cos());
    let b = (a.0, a.1 * pitch.cos() - a.2 * pitch.sin(), a.1 * pitch.sin() + a.2 * pitch.cos());
    (b.0 * roll.cos() - b.1 * roll.sin(), b.0 * roll.sin() + b.1 * roll.cos(), b.2)
}

fn project(p: P3, ox: f64, oy: f64, distance: f64) -> Pt {
    let depth = distance - p.2;
    Pt::new((CX + FOCAL * (p.0 + ox) / depth) as f32, (CY + FOCAL * (p.1 + oy) / depth) as f32)
}

fn noisy(mut pts: [Pt; 5], sigma: f64, rng: &mut Rng) -> [Pt; 5] {
    for p in &mut pts {
        p.x += rng.normal(sigma) as f32;
        p.y += rng.normal(sigma) as f32;
    }
    pts
}

fn real_head(amplitude: f64, sigma: f64, rng: &mut Rng) -> Reading {
    let ph = rng.uniform() * 2.0 * PI;
    let mut an = Analyzer::default();
    for i in 0..(3.0 * FPS) as usize {
        let t = i as f64 / FPS;
        let yaw = amplitude.to_radians() * (2.0 * PI * 0.4 * t + ph).sin();
        let pitch = 3f64.to_radians() * (2.0 * PI * 0.3 * t).sin();
        let roll = 2f64.to_radians() * (2.0 * PI * 0.2 * t + 1.0).sin();
        let pts = HEAD.map(|h| project(rotate(h, yaw, pitch, roll), 8.0 * t.sin(), 4.0 * t.cos(), DISTANCE));
        an.add(Frame::from_points(noisy(pts, sigma, rng), t * 1000.0));
    }
    an.reading()
}

fn varied_head(rng: &mut Rng) -> Reading {
    let mut u = || rng.uniform();
    let (eye, nose_z, nose_y) = (29.0 + 5.0 * u(), 18.0 + 17.0 * u(), 38.0 + 10.0 * u());
    let (mouth_z, mouth_y, mouth_x) = (15.0 * u(), 65.0 + 15.0 * u(), 22.0 + 6.0 * u());
    let head: [P3; 5] = [(-eye, 0.0, 0.0), (eye, 0.0, 0.0), (0.0, nose_y, nose_z), (-mouth_x, mouth_y, mouth_z), (mouth_x, mouth_y, mouth_z)];
    let (amplitude, ph, sigma) = (12.0 + 10.0 * u(), 2.0 * PI * u(), 0.5 + 2.0 * u());
    let mut an = Analyzer::default();
    for i in 0..(3.0 * FPS) as usize {
        let t = i as f64 / FPS;
        let yaw = amplitude.to_radians() * (2.0 * PI * 0.4 * t + ph).sin();
        let pitch = 4f64.to_radians() * (2.0 * PI * 0.3 * t).sin();
        let pts = head.map(|h| project(rotate(h, yaw, pitch, 0.0), 0.0, 0.0, DISTANCE));
        an.add(Frame::from_points(noisy(pts, sigma, rng), t * 1000.0));
    }
    an.reading()
}

/// A photograph of the same head, taken from straight ahead (or turned by
/// `baked`), printed flat and turned by up to `amplitude`. `bend` curls the
/// card around a vertical axis, in millimetres of sag at the edges.
fn photo(amplitude: f64, sigma: f64, rng: &mut Rng, baked: f64, bend: f64, seconds: f64) -> Reading {
    let card: Vec<P3> = HEAD
        .iter()
        .map(|&h| {
            let r = rotate(h, baked.to_radians(), 0.0, 0.0);
            let s = DISTANCE / (DISTANCE - r.2);
            let (x, y) = (r.0 * s, r.1 * s);
            (x, y, -bend * (x / 45.0) * (x / 45.0))
        })
        .collect();
    let ph = rng.uniform() * 2.0 * PI;
    let mut an = Analyzer::default();
    for i in 0..(seconds * FPS) as usize {
        let t = i as f64 / FPS;
        let yaw = amplitude.to_radians() * (2.0 * PI * 0.4 * t + ph).sin();
        let pitch = 0.4 * amplitude.to_radians() * (2.0 * PI * 0.3 * t).sin();
        let roll = 4f64.to_radians() * (2.0 * PI * 0.2 * t).sin();
        let mut pts = [Pt::default(); 5];
        for k in 0..5 {
            pts[k] = project(rotate(card[k], yaw, pitch, roll), 10.0 * t.sin(), 5.0 * t.cos(), DISTANCE - 100.0);
        }
        an.add(Frame::from_points(noisy(pts, sigma, rng), t * 1000.0));
    }
    an.reading()
}

const RUNS: usize = 300;

fn rate(mut run: impl FnMut() -> bool) -> f64 {
    (0..RUNS).filter(|_| run()).count() as f64 / RUNS as f64
}

fn expect(what: &str, rate: f64, min: f64, max: f64, failures: &mut Vec<String>) {
    let ok = rate >= min && rate <= max;
    println!("{}  {what:<58} {:5.1}%  (want {:.0}% to {:.0}%)", if ok { "ok  " } else { "FAIL" }, rate * 100.0, min * 100.0, max * 100.0);
    if !ok {
        failures.push(what.into());
    }
}

#[test]
fn depth_cue() {
    let mut rng = Rng(20260922);
    let mut f = Vec::new();
    let depth = |r: Reading| r.confirmed_by == "depth";
    expect("head turning 15 degrees, 1 px noise", rate(|| depth(real_head(15.0, 1.0, &mut rng))), 0.9, 1.0, &mut f);
    expect("head turning 15 degrees, 2 px noise", rate(|| depth(real_head(15.0, 2.0, &mut rng))), 0.6, 1.0, &mut f);
    expect("head turning 25 degrees, 2 px noise", rate(|| depth(real_head(25.0, 2.0, &mut rng))), 0.9, 1.0, &mut f);
    expect("heads of all shapes turning 12 to 22 degrees", rate(|| depth(varied_head(&mut rng))), 0.9, 1.0, &mut f);
    expect("head held still (2 degrees), 1 px noise", rate(|| depth(real_head(2.0, 1.0, &mut rng))), 0.0, 0.05, &mut f);
    expect("photo turned 30 degrees, 0.5 px noise", rate(|| depth(photo(30.0, 0.5, &mut rng, 0.0, 0.0, 3.0))), 0.0, 0.0, &mut f);
    expect("photo turned 30 degrees, 1 px noise", rate(|| depth(photo(30.0, 1.0, &mut rng, 0.0, 0.0, 3.0))), 0.0, 0.01, &mut f);
    expect("photo turned 30 degrees, 2 px noise", rate(|| depth(photo(30.0, 2.0, &mut rng, 0.0, 0.0, 3.0))), 0.0, 0.03, &mut f);
    expect("photo turned 45 degrees, 1 px noise", rate(|| depth(photo(45.0, 1.0, &mut rng, 0.0, 0.0, 3.0))), 0.0, 0.01, &mut f);
    expect("photo of a turned head, turned 30 degrees", rate(|| depth(photo(30.0, 1.0, &mut rng, 20.0, 0.0, 3.0))), 0.0, 0.01, &mut f);
    expect("photo curled by 5 mm, turned 30 degrees", rate(|| depth(photo(30.0, 1.0, &mut rng, 0.0, 5.0, 3.0))), 0.0, 0.05, &mut f);
    // Curled this hard, a card has about a quarter of a real nose's depth.
    // Five points cannot tell that from a very flat face turned a little.
    // Blink, glare and the device edge are what is left against it.
    println!("info  known limit: photo curled by 15 mm, turned 30 degrees {:5.1}%", 100.0 * rate(|| depth(photo(30.0, 1.0, &mut rng, 0.0, 15.0, 3.0))));
    expect("photo turned 60 degrees, 3 px noise", rate(|| depth(photo(60.0, 3.0, &mut rng, 0.0, 0.0, 3.0))), 0.0, 0.05, &mut f);
    expect("photo shaken for 6 seconds, 3 px noise", rate(|| depth(photo(20.0, 3.0, &mut rng, 0.0, 0.0, 6.0))), 0.0, 0.05, &mut f);
    expect("head turning 20 degrees, 3 px noise", rate(|| depth(real_head(20.0, 3.0, &mut rng))), 0.6, 1.0, &mut f);
    assert!(f.is_empty(), "failed: {f:?}");
}

struct EyeScript {
    left: fn(usize) -> f32,
    right: fn(usize) -> f32,
    skin: fn(usize) -> f32,
    shift: fn(usize) -> Pt,
}

fn blink_run(s: &EyeScript, rng: &mut Rng) -> bool {
    let mut an = Analyzer::default();
    for i in 0..60 {
        let pts = HEAD.map(|h| project(h, 0.0, 0.0, DISTANCE) + (s.shift)(i));
        let mut f = Frame::from_points(pts, i as f64 * 1000.0 / 30.0);
        f.eyes.valid = true;
        f.eyes.left = ((s.left)(i) + rng.normal(0.03) as f32).max(0.0);
        f.eyes.right = ((s.right)(i) + rng.normal(0.03) as f32).max(0.0);
        f.eyes.openness = (f.eyes.left + f.eyes.right) / 2.0;
        f.eyes.skin = (s.skin)(i);
        an.add(f);
    }
    an.reading().confirmed_by == "blink"
}

#[test]
fn blink_cue() {
    let mut rng = Rng(7);
    let mut f = Vec::new();
    let open = |_: usize| 0.45;
    let blink = |i: usize| if (30..34).contains(&i) { 0.08 } else { 0.45 };
    let slow = |i: usize| if (20..45).contains(&i) { 0.08 } else { 0.45 };
    let skin = |_: usize| 150.0;
    let still = |_: usize| Pt::default();
    let mut r = |s: EyeScript| rate(|| blink_run(&s, &mut rng));
    expect("eyes open the whole time", r(EyeScript { left: open, right: open, skin, shift: still }), 0.0, 0.01, &mut f);
    expect("a normal blink (130 ms)", r(EyeScript { left: blink, right: blink, skin, shift: still }), 0.95, 1.0, &mut f);
    expect("eyes closed for most of a second", r(EyeScript { left: slow, right: slow, skin, shift: still }), 0.0, 0.01, &mut f);
    expect("one eye only", r(EyeScript { left: blink, right: open, skin, shift: still }), 0.0, 0.01, &mut f);
    expect(
        "light dims at the same moment",
        r(EyeScript { left: blink, right: blink, skin: |i| if (30..34).contains(&i) { 110.0 } else { 150.0 }, shift: still }),
        0.0,
        0.01,
        &mut f,
    );
    expect(
        "picture jerked sideways at the same moment",
        r(EyeScript { left: blink, right: blink, skin, shift: |i| if i >= 32 { Pt::new(25.0, 0.0) } else { Pt::default() } }),
        0.0,
        0.01,
        &mut f,
    );
    assert!(f.is_empty(), "failed: {f:?}");
}

fn random_unit(rng: &mut Rng) -> Vec<f32> {
    let v: Vec<f64> = (0..EMBEDDING_SIZE).map(|_| rng.normal(1.0)).collect();
    let n = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    v.iter().map(|x| (x / n) as f32).collect()
}

fn near(base: &[f32], s: f32, rng: &mut Rng) -> Vec<f32> {
    let other = random_unit(rng);
    let w = (1.0 - s * s).sqrt();
    let out: Vec<f32> = base.iter().zip(&other).map(|(b, o)| s * b + w * o).collect();
    let n = out.iter().map(|x| x * x).sum::<f32>().sqrt();
    out.iter().map(|x| x / n).collect()
}

#[test]
fn face_store() {
    use std::os::unix::fs::PermissionsExt;
    let mut rng = Rng(7);
    let dir = std::env::temp_dir().join(format!("fu-store-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::new(&dir);
    let face_a = random_unit(&mut rng);
    let a = Identity {
        id: store::new_id(),
        name: "Felix mit Brille".into(),
        created: 1758550000,
        enabled: true,
        nose_t: 0.57,
        eyes: 0.41,
        samples: (0..5).map(|i| Sample { embedding: near(&face_a, 0.8, &mut rng), pose: "center".into(), time: 1758550000 + i }).collect(),
    };
    let b = Identity { id: store::new_id(), name: "Other".into(), created: 1, enabled: true, nose_t: 0.55, eyes: 0.0, samples: vec![Sample { embedding: random_unit(&mut rng), pose: "center".into(), time: 1 }] };
    store.save(1000, &[a.clone(), b]).unwrap();
    let mode = std::fs::metadata(dir.join("users/1000.json")).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "readable by its owner only");
    let loaded = store.load(1000).unwrap();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].name, a.name);
    assert_eq!(loaded[0].samples.len(), 5);
    assert!((loaded[0].nose_t - 0.57).abs() < 1e-6 && (loaded[0].eyes - 0.41).abs() < 1e-6);
    assert!(similarity(&loaded[0].samples[0].embedding, &a.samples[0].embedding) > 0.9999);
    assert!(store.load(1001).unwrap().is_empty());

    let m = store::best_match(&loaded, &near(&face_a, 0.9, &mut rng));
    assert!(m.identity == Some(0) && m.score > 0.6, "{m:?}");
    assert!(store::best_match(&loaded, &random_unit(&mut rng)).score < 0.4);
    let mut off = loaded.clone();
    off[0].enabled = false;
    assert_ne!(store::best_match(&off, &near(&face_a, 0.9, &mut rng)).identity, Some(0));

    let mut learning = loaded[0].clone();
    let same = learning.samples[0].embedding.clone();
    assert!(!store::adapt(&mut learning, &same, 5), "nothing new: nothing learned");
    for i in 0..40 {
        store::adapt(&mut learning, &near(&face_a, 0.7, &mut rng), 10 + i);
    }
    assert!(learning.adaptive_count() <= store::MAX_ADAPTIVE);
    assert_eq!(learning.samples.iter().filter(|s| s.pose == "center").count(), 5, "the setup's samples stay");

    // A file the C++ daemon wrote reads the same.
    let old = r#"{"faces":[{"created":1758550000,"enabled":true,"eyes":0.41,"id":"abc","name":"Old","noseT":0.5,"samples":[{"e":"EMB","pose":"center","t":1758550000}]}],"version":1}"#;
    let e: Vec<u8> = a.samples[0].embedding.iter().flat_map(|v| v.to_le_bytes()).collect();
    std::fs::write(dir.join("users/1002.json"), old.replace("EMB", &store::base64(&e))).unwrap();
    let old = store.load(1002).unwrap();
    assert_eq!(old[0].name, "Old");
    assert!(similarity(&old[0].samples[0].embedding, &a.samples[0].embedding) > 0.9999);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn base64_round_trip() {
    for n in 0..10 {
        let data: Vec<u8> = (0..n).map(|i| (i * 37 + 5) as u8).collect();
        assert_eq!(store::unbase64(&store::base64(&data)).unwrap(), data);
    }
    assert_eq!(store::base64(b"Man"), "TWFu");
    assert_eq!(store::base64(b"Ma"), "TWE=");
}

#[test]
fn mjpeg_without_huffman_tables() {
    use image::ImageEncoder;
    let (w, h) = (64u32, 48u32);
    let rgb: Vec<u8> = (0..w * h).flat_map(|i| [(i % 251) as u8, (i * 3 % 256) as u8, 128]).collect();
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 90).write_image(&rgb, w, h, image::ExtendedColorType::Rgb8).unwrap();
    let full = crate::camera::decode_jpeg(&jpeg).unwrap();
    // Every DHT segment out.
    let mut stripped = Vec::new();
    let mut i = 0;
    while i < jpeg.len() {
        if jpeg[i] == 0xff && jpeg.get(i + 1) == Some(&0xc4) {
            let len = (jpeg[i + 2] as usize) << 8 | jpeg[i + 3] as usize;
            i += 2 + len;
            continue;
        }
        if jpeg[i] == 0xff && jpeg.get(i + 1) == Some(&0xda) {
            stripped.extend_from_slice(&jpeg[i..]);
            break;
        }
        stripped.push(jpeg[i]);
        i += 1;
    }
    assert!(!stripped.windows(2).any(|w| w == [0xff, 0xc4]));
    let fixed = crate::camera::decode_jpeg(&stripped).expect("decodes with the standard tables");
    assert_eq!(fixed.data, full.data);
}
