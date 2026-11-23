//! Recognition on real pictures, for trying things out without a camera:
//!
//!   pictures MODELDIR REFERENCE.jpg OTHER.jpg...
//!
//! Takes the face in REFERENCE as the enrolled one and prints how every
//! other picture compares, with the pose and quality the daemon would see.
//!
//!   pictures --enroll STATEDIR UID NAME MODELDIR PICTURE...
//!
//! Writes the faces in the pictures straight into a face store, as if they
//! had been set up, so a development daemon has somebody to recognise.

use std::path::Path;
use vision::camera::load_picture;
use vision::face::{Vision, estimate_pose, similarity, yaw_degrees, assess_quality};
use vision::liveness::{detect_device, measure_eyes, measure_glare};
use vision::store::{self, Identity, Sample, Store};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--enroll") {
        if args.len() < 6 {
            eprintln!("usage: pictures --enroll STATEDIR UID NAME MODELDIR PICTURE...");
            std::process::exit(2);
        }
        let mut vision = Vision::load(Path::new(&args[4])).unwrap_or_else(|e| { eprintln!("{e}"); std::process::exit(1) });
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        let mut id = Identity { id: store::new_id(), name: args[3].clone(), created: now, enabled: true, nose_t: 0.55, eyes: 0.0, samples: vec![] };
        let (mut nose, mut eyes) = (vec![], vec![]);
        for p in &args[5..] {
            let Some(img) = load_picture(Path::new(p)) else { eprintln!("cannot read {p}"); continue };
            let faces = vision.detect(&img);
            let Some(f) = faces.first() else { eprintln!("no face in {p}"); continue };
            id.samples.push(Sample { embedding: vision.embed(&img, f), pose: "center".into(), time: now });
            nose.push(estimate_pose(f).nose_t);
            let e = measure_eyes(&img, f);
            if e.valid { eyes.push(e.openness); }
        }
        if id.samples.is_empty() { std::process::exit(1); }
        nose.sort_by(f32::total_cmp);
        eyes.sort_by(f32::total_cmp);
        id.nose_t = nose[nose.len() / 2];
        id.eyes = if eyes.is_empty() { 0.0 } else { eyes[eyes.len() / 2] };
        let store = Store::new(Path::new(&args[1]));
        let uid: u32 = args[2].parse().unwrap_or(0);
        let mut all = store.load(uid).unwrap_or_default();
        let n = id.samples.len();
        all.push(id);
        store.save(uid, &all).unwrap_or_else(|e| { eprintln!("{e}"); std::process::exit(1) });
        println!("enrolled {} with {n} samples", args[3]);
        return;
    }
    if args.len() < 3 {
        eprintln!("usage: pictures MODELDIR REFERENCE OTHER...");
        std::process::exit(2);
    }
    let mut vision = Vision::load(Path::new(&args[0])).unwrap_or_else(|e| { eprintln!("{e}"); std::process::exit(1) });
    let r = load_picture(Path::new(&args[1])).expect("reference");
    let rf = vision.detect(&r);
    let Some(rf) = rf.first() else { eprintln!("no face in {}", args[1]); std::process::exit(1) };
    let reference = vision.embed(&r, rf);
    for p in &args[2..] {
        let Some(img) = load_picture(Path::new(p)) else { println!("{p:<50} unreadable"); continue };
        let t = std::time::Instant::now();
        let faces = vision.detect(&img);
        let Some(f) = faces.first() else { println!("{p:<50} no face"); continue };
        let e = vision.embed(&img, f);
        let ms = t.elapsed().as_millis();
        let pose = estimate_pose(f);
        let q = assess_quality(&img, f);
        let eyes = measure_eyes(&img, f);
        let glare = measure_glare(&img, f);
        println!("{p:<50} similarity {:.3}  yaw {:5.1}  noseT {:.2}  iod {:3.0}  light {:3.0}  sharp {:5.0}  eyes {:.2}/{:.2}  glare {:.3}/{:.2}  device {}  ({ms} ms)",
            similarity(&reference, &e), yaw_degrees(pose.yaw), pose.nose_t, f.interocular(), q.brightness, q.sharpness, eyes.left, eyes.right, glare.fraction, glare.cluster, detect_device(&img, f) as i32);
    }
}
