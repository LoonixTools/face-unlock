//! The face data.
//!
//! One file per user under /var/lib/face-unlock/users, named after the
//! numeric user id so a rename cannot hand one person's faces to another.
//! Only root can read or write the folder. There are no pictures in it: every
//! sample is the 128 numbers the recognizer made of one frame, and the frame
//! itself was never written anywhere.
//!
//! The same file the C++ daemon of version 2 wrote, so faces survive the
//! update.

use crate::face::{EMBEDDING_SIZE, Embedding, similarity};
use serde_json::{Value, json};
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const FORMAT_VERSION: i64 = 1;
pub const MAX_ADAPTIVE: usize = 12;

#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub embedding: Embedding,
    /// Which way the head pointed: "center", one of the eight directions
    /// ("up", "up-right", ...), or "adaptive" for one taken in from a
    /// successful unlock.
    pub pose: String,
    pub time: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Identity {
    pub id: String,
    pub name: String,
    pub created: i64,
    pub enabled: bool,
    /// Where this person's nose sits for a level head, and how open their
    /// eyes measure when they look at the camera. Both are what the
    /// attention check compares against.
    pub nose_t: f32,
    pub eyes: f32,
    pub samples: Vec<Sample>,
}

impl Identity {
    pub fn adaptive_count(&self) -> usize {
        self.samples.iter().filter(|s| s.pose == "adaptive").count()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Match {
    pub score: f32,
    pub identity: Option<usize>,
}

pub struct Store {
    dir: PathBuf,
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn unbase64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;
    for c in text.bytes() {
        if c == b'=' {
            break;
        }
        let v = B64.iter().position(|&b| b == c)? as u32;
        buf = buf << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits & 0xff) as u8);
        }
    }
    Some(out)
}

fn pack(e: &[f32]) -> String {
    let bytes: Vec<u8> = e.iter().flat_map(|v| v.to_le_bytes()).collect();
    base64(&bytes)
}

fn unpack(text: &str) -> Embedding {
    match unbase64(text) {
        Some(raw) if raw.len() == EMBEDDING_SIZE * 4 => raw.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect(),
        _ => vec![],
    }
}

impl Store {
    pub fn new(state_dir: &Path) -> Store {
        Store { dir: state_dir.join("users") }
    }

    fn file(&self, uid: u32) -> PathBuf {
        self.dir.join(format!("{uid}.json"))
    }

    pub fn load(&self, uid: u32) -> Result<Vec<Identity>, String> {
        let text = match std::fs::read_to_string(self.file(uid)) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e.to_string()),
        };
        let doc: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let mut faces = Vec::new();
        for o in doc["faces"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
            let samples: Vec<Sample> = o["samples"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or(&[])
                .iter()
                .map(|s| Sample { embedding: unpack(s["e"].as_str().unwrap_or("")), pose: s["pose"].as_str().unwrap_or("").into(), time: s["t"].as_f64().unwrap_or(0.0) as i64 })
                .filter(|s| !s.embedding.is_empty())
                .collect();
            let id = Identity {
                id: o["id"].as_str().unwrap_or("").into(),
                name: o["name"].as_str().unwrap_or("").into(),
                created: o["created"].as_f64().unwrap_or(0.0) as i64,
                enabled: o["enabled"].as_bool().unwrap_or(true),
                nose_t: o["noseT"].as_f64().unwrap_or(0.55) as f32,
                eyes: o["eyes"].as_f64().unwrap_or(0.0) as f32,
                samples,
            };
            if !id.id.is_empty() && !id.samples.is_empty() {
                faces.push(id);
            }
        }
        Ok(faces)
    }

    pub fn save(&self, uid: u32, faces: &[Identity]) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("cannot create {}: {e}", self.dir.display()))?;
        let _ = std::fs::set_permissions(&self.dir, std::fs::Permissions::from_mode(0o700));
        if faces.is_empty() {
            self.remove(uid);
            return Ok(());
        }
        let list: Vec<Value> = faces
            .iter()
            .map(|id| {
                json!({
                    "id": id.id,
                    "name": id.name,
                    "created": id.created as f64,
                    "enabled": id.enabled,
                    "noseT": id.nose_t as f64,
                    "eyes": id.eyes as f64,
                    "samples": id.samples.iter().map(|s| json!({"e": pack(&s.embedding), "pose": s.pose, "t": s.time as f64})).collect::<Vec<_>>(),
                })
            })
            .collect();
        let doc = json!({"version": FORMAT_VERSION, "faces": list});
        write_private(&self.file(uid), doc.to_string().as_bytes())
    }

    pub fn remove(&self, uid: u32) -> bool {
        std::fs::remove_file(self.file(uid)).is_ok()
    }
}

/// Written next to the file and moved over it, readable by its owner only.
pub fn write_private(path: &Path, content: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    let r = (|| {
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
        f.write_all(content)?;
        f.sync_all()?;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        std::fs::rename(&tmp, path)
    })();
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r.map_err(|e| e.to_string())
}

pub fn new_id() -> String {
    let mut b = [0u8; 8];
    let _ = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b));
    format!("{:x}", u64::from_le_bytes(b) & 0xffff_ffff_ffff)
}

/// The best sample over every enabled identity.
pub fn best_match(faces: &[Identity], e: &[f32]) -> Match {
    let mut best = Match { score: -1.0, identity: None };
    for (i, id) in faces.iter().enumerate() {
        if !id.enabled {
            continue;
        }
        for s in &id.samples {
            let score = similarity(&s.embedding, e);
            if score > best.score {
                best = Match { score, identity: Some(i) };
            }
        }
    }
    best
}

/// Adds what a confident unlock saw, if it adds anything, and keeps at most
/// MAX_ADAPTIVE of those per identity (the oldest go first). The samples
/// from the setup are never touched.
pub fn adapt(identity: &mut Identity, e: &[f32], now: i64) -> bool {
    // Something the samples already cover adds nothing but weight.
    let closest = identity.samples.iter().map(|s| similarity(&s.embedding, e)).fold(-1.0f32, f32::max);
    if closest >= 0.9 {
        return false;
    }
    if identity.adaptive_count() >= MAX_ADAPTIVE
        && let Some(i) = identity.samples.iter().position(|s| s.pose == "adaptive")
    {
        identity.samples.remove(i);
    }
    identity.samples.push(Sample { embedding: e.to_vec(), pose: "adaptive".into(), time: now });
    true
}

/// What the daemon remembers about each user between scans: failures in a
/// row, the lockout they lead to, and the last successful unlock. Kept on
/// disk: the daemon exits when it has been idle for a minute, and a lockout
/// that ended with the process would be one that waiting a minute gets
/// around.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UserState {
    pub failures: u32,
    pub locked_until: i64,
    pub last_unlock: i64,
    pub last_purpose: String,
}

impl UserState {
    fn file(state_dir: &Path, uid: u32) -> PathBuf {
        state_dir.join(format!("users/{uid}.state"))
    }

    pub fn load(state_dir: &Path, uid: u32) -> UserState {
        let Ok(text) = std::fs::read_to_string(Self::file(state_dir, uid)) else { return UserState::default() };
        let o: Value = serde_json::from_str(&text).unwrap_or_default();
        UserState {
            failures: o["failures"].as_u64().unwrap_or(0) as u32,
            locked_until: o["lockedUntil"].as_f64().unwrap_or(0.0) as i64,
            last_unlock: o["lastUnlock"].as_f64().unwrap_or(0.0) as i64,
            last_purpose: o["lastPurpose"].as_str().unwrap_or("").into(),
        }
    }

    pub fn save(&self, state_dir: &Path, uid: u32) -> bool {
        let _ = std::fs::create_dir_all(state_dir.join("users"));
        let o = json!({"failures": self.failures, "lockedUntil": self.locked_until as f64, "lastUnlock": self.last_unlock as f64, "lastPurpose": self.last_purpose});
        write_private(&Self::file(state_dir, uid), o.to_string().as_bytes()).is_ok()
    }

    /// Seconds left, 0 when not locked out.
    pub fn lockout_left(&self, now: i64) -> i64 {
        (self.locked_until - now).max(0)
    }
}
