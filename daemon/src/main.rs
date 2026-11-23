//! face-unlockd: the part that runs as root.
//!
//! It owns the camera and the face data, and it is the only thing that does.
//! Everything else (the lock screen agent, sudo, the window) asks it over one
//! socket. systemd starts it on the first connection and it exits again after
//! a minute with nothing to do, so most of the time it is not running.
//!
//! The protocol: one JSON object per line, both ways. A client sends one
//! request; the daemon answers with events and ends with one whose "event" is
//! "result". "watch" never ends.
//!
//! ```text
//! hello                       version
//! status                      faces, lockout, camera, settings
//! cameras                     every camera and which one is in use
//! verify user purpose [verbose] [remote]
//!                             scan for that user. Events: started, face,
//!                             hint, frame (verbose only), result. remote:
//!                             the PAM module asks for an SSH session
//! enroll [name]               set up a face for the caller. Asks polkit
//!                             first. Events: authorizing, authorized,
//!                             started, frame, pose, hint, captured, result.
//!                             While it runs the client may send "finish"
//!                             (stop once enough is done) or "cancel".
//! list [user]                 the caller's faces
//! remove id | rename id name | enable id | disable id | clear
//!                             change the caller's faces (polkit)
//! watch                       every scan for the caller, as it happens
//! unlocked                    the caller's session was unlocked some other
//!                             way, which ends a lockout
//! cancel                      stop this connection's scan or setup
//! ```
//!
//! Who may do what:
//! - anybody may ask about themselves and scan for themselves. The answer to
//!   a scan is yes or no, which tells a process nothing it could use.
//! - root may do all of it for anybody. That is sudo and the polkit helper,
//!   through the PAM module.
//! - changing faces needs polkit on top, because a face is a way in.

mod agentlink;
mod jobs;
mod system;

use face_unlock::config::Settings;
use face_unlock::paths;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::FromRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use vision::camera;
use vision::face::Vision;
use vision::store::{self, Store, UserState};

/// A face unlock and the lock screen going away this soon after belong
/// together.
const UNLOCK_PAIR_SECONDS: i64 = 30;
/// A request is a line of JSON a few hundred bytes long. Anything that keeps
/// talking without a newline for this long is not a client.
const MAX_LINE: usize = 64 * 1024;

struct Options {
    socket: PathBuf,
    state: PathBuf,
    config: PathBuf,
    models: PathBuf,
    idle: Option<Duration>,
    systemd_fd: Option<i32>,
    /// Running as somebody other than root is only ever development, and
    /// only touches that person's own test data. polkit is not asked then.
    ask_polkit: bool,
}

type Id = u64;

struct Client {
    stream: UnixStream,
    uid: u32,
    pid: i32,
    /// What this connection is for, once it has said so. A connection makes
    /// one request; "watch", "verify" and "enroll" keep it open.
    role: String,
}

/// What a polkit answer goes on to do.
enum Then {
    Enroll { uid: u32, name: String },
    Change { uid: u32, cmd: String, id: String, name: String },
}

enum Job {
    Scan { uid: u32, purpose: String },
    Enroll { uid: u32 },
}

enum Ev {
    Accept(UnixStream),
    Line(Id, Value),
    Gone(Id),
    JobEvent(Value),
    ScanDone(jobs::ScanOutcome),
    EnrollDone(jobs::EnrollOutcome),
    Authorized(Id, bool, Then),
    Quit,
}

struct Running {
    job: Job,
    owner: Option<Id>,
    cancel: Arc<AtomicBool>,
    finish_early: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<()>,
}

struct Server {
    o: Options,
    tx: Sender<Ev>,
    clients: HashMap<Id, Client>,
    next_id: Id,
    vision: Option<Arc<Mutex<Vision>>>,
    vision_error: String,
    job: Option<Running>,
    /// The session to tell about the running scan, when it is not the lock
    /// screen's own.
    agent: Option<agentlink::AgentLink>,
    /// Asking polkit whether faces may be changed. While that question is
    /// open, a scan for an admin prompt is refused: the answer to "may a face
    /// be added" has to be the password, not a face.
    manage_checks: u32,
}

fn now() -> i64 {
    face_unlock::text::now()
}

fn result(ok: bool, reason: &str) -> Value {
    jobs::result(ok, reason)
}

fn peer(stream: &UnixStream) -> Option<(u32, i32)> {
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    use std::os::fd::AsRawFd;
    // SAFETY: cred is a ucred of the size passed.
    let r = unsafe { libc::getsockopt(stream.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, (&mut cred as *mut libc::ucred).cast(), &mut len) };
    (r == 0).then_some((cred.uid, cred.pid))
}

impl Server {
    fn settings(&self) -> Settings {
        Settings::load(&self.o.config)
    }

    fn ensure_vision(&mut self) -> bool {
        if self.vision.is_none() {
            match Vision::load(&self.o.models) {
                Ok(v) => self.vision = Some(Arc::new(Mutex::new(v))),
                Err(e) => self.vision_error = e,
            }
        }
        self.vision.is_some()
    }

    fn send(&mut self, id: Id, message: &Value) {
        if let Some(c) = self.clients.get_mut(&id) {
            let mut line = message.to_string();
            line.push('\n');
            if c.stream.write_all(line.as_bytes()).is_err() {
                let _ = c.stream.shutdown(std::net::Shutdown::Both);
            }
        }
    }

    fn reply(&mut self, id: Id, message: &Value) {
        self.send(id, message);
        if let Some(c) = self.clients.get(&id) {
            let _ = c.stream.shutdown(std::net::Shutdown::Both);
        }
    }

    fn fail(&mut self, id: Id, reason: &str, extra: Value) {
        let mut r = result(false, reason);
        if let (Some(o), Some(e)) = (r.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        self.reply(id, &r);
    }

    fn add_client(&mut self, stream: UnixStream) {
        let Some((uid, pid)) = peer(&stream) else { return };
        let Ok(read) = stream.try_clone() else { return };
        let id = self.next_id;
        self.next_id += 1;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(read);
            let mut line = Vec::new();
            loop {
                line.clear();
                match reader.by_ref().take(MAX_LINE as u64).read_until(b'\n', &mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) if !line.ends_with(b"\n") && line.len() >= MAX_LINE => break,
                    Ok(_) => {}
                }
                let text = String::from_utf8_lossy(&line);
                let text = text.trim();
                if text.is_empty() {
                    continue;
                }
                let v = serde_json::from_str::<Value>(text).ok().filter(Value::is_object).unwrap_or(Value::Null);
                if tx.send(Ev::Line(id, v)).is_err() {
                    break;
                }
            }
            let _ = tx.send(Ev::Gone(id));
        });
        self.clients.insert(id, Client { stream, uid, pid, role: String::new() });
    }

    fn gone(&mut self, id: Id) {
        // Whoever asked for a scan has given up on it (the PAM module timed
        // out, the password was typed, the window was closed). The camera
        // goes off now rather than when the scan would have ended.
        if let Some(j) = &mut self.job
            && j.owner == Some(id)
        {
            j.cancel.store(true, Ordering::Relaxed);
            j.owner = None;
        }
        self.clients.remove(&id);
    }

    /// The user a request is about: the caller, or for root whoever it
    /// names. None (and answered) when the caller may not act for them.
    fn target(&mut self, id: Id, request: &Value) -> Option<u32> {
        let caller = self.clients.get(&id)?.uid;
        let name = request["user"].as_str().unwrap_or("");
        if name.is_empty() {
            return Some(caller);
        }
        let Some(target) = system::uid_of(name) else {
            self.fail(id, "unknown-user", json!({}));
            return None;
        };
        if caller != 0 && caller != target {
            self.fail(id, "denied", json!({}));
            return None;
        }
        Some(target)
    }

    fn authorize(&mut self, id: Id, then: Then) {
        let Some(c) = self.clients.get(&id) else { return };
        if c.uid == 0 || !self.o.ask_polkit {
            self.authorized(id, true, then);
            return;
        }
        let (pid, uid) = (c.pid, c.uid);
        self.send(id, &json!({"event": "authorizing"}));
        self.manage_checks += 1;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let ok = system::polkit_allows(pid, uid, paths::ACTION_MANAGE);
            let _ = tx.send(Ev::Authorized(id, ok, then));
        });
    }

    fn authorized(&mut self, id: Id, ok: bool, then: Then) {
        if !self.clients.contains_key(&id) {
            return;
        }
        if !ok {
            self.fail(id, "denied", json!({}));
            return;
        }
        if self.o.ask_polkit && self.clients.get(&id).is_some_and(|c| c.uid != 0) {
            self.send(id, &json!({"event": "authorized"}));
        }
        match then {
            Then::Enroll { uid, name } => {
                if self.job.is_some() {
                    self.fail(id, "busy", json!({}));
                    return;
                }
                if !self.ensure_vision() {
                    let m = self.vision_error.clone();
                    self.fail(id, "models", json!({"message": m}));
                    return;
                }
                let name = if name.is_empty() { system::name_of(uid) } else { name };
                let settings = self.settings();
                self.start(Job::Enroll { uid }, Some(id), move |ctx| Ev::EnrollDone(jobs::enroll(ctx, &settings, &name)));
            }
            Then::Change { uid, cmd, id: face, name } => {
                let st = Store::new(&self.o.state);
                let mut faces = st.load(uid).unwrap_or_default();
                let mut found = cmd == "clear";
                if cmd == "clear" {
                    faces.clear();
                }
                if let Some(i) = faces.iter().position(|f| f.id == face) {
                    found = true;
                    match cmd.as_str() {
                        "remove" => {
                            faces.remove(i);
                        }
                        "rename" if !name.is_empty() => faces[i].name = name,
                        "enable" => faces[i].enabled = true,
                        "disable" => faces[i].enabled = false,
                        _ => {}
                    }
                }
                if !found {
                    self.fail(id, "unknown-face", json!({}));
                    return;
                }
                match st.save(uid, &faces) {
                    Ok(()) => self.reply(id, &result(true, "")),
                    Err(e) => self.fail(id, "store", json!({"message": e})),
                }
            }
        }
    }

    fn start(&mut self, job: Job, owner: Option<Id>, run: impl FnOnce(&jobs::Ctx) -> Ev + Send + 'static) {
        let cancel = Arc::new(AtomicBool::new(false));
        let finish_early = Arc::new(AtomicBool::new(false));
        let tx = self.tx.clone();
        let events = self.tx.clone();
        let ctx = jobs::Ctx {
            vision: self.vision.clone().expect("loaded before a job starts"),
            cancel: cancel.clone(),
            finish_early: finish_early.clone(),
            emit: Box::new(move |v| {
                let _ = events.send(Ev::JobEvent(v));
            }),
        };
        let thread = std::thread::spawn(move || {
            let done = run(&ctx);
            let _ = tx.send(done);
        });
        self.job = Some(Running { job, owner, cancel, finish_early, thread });
    }

    fn request(&mut self, id: Id, request: Value) {
        let cmd = request["cmd"].as_str().unwrap_or("").to_string();
        // Messages for a scan or setup that already runs on this connection.
        if cmd == "cancel" || cmd == "finish" {
            if let Some(j) = &self.job
                && j.owner == Some(id)
            {
                if cmd == "cancel" {
                    j.cancel.store(true, Ordering::Relaxed);
                } else {
                    j.finish_early.store(true, Ordering::Relaxed);
                }
            }
            return;
        }
        let Some(c) = self.clients.get_mut(&id) else { return };
        if !c.role.is_empty() {
            // One request per connection.
            return;
        }
        c.role = if cmd.is_empty() { "?".into() } else { cmd.clone() };
        if request.is_null() {
            self.fail(id, "bad-request", json!({}));
            return;
        }
        match cmd.as_str() {
            "hello" => self.reply(id, &json!({"event": "result", "ok": true, "version": paths::VERSION})),
            "status" => self.status(id),
            "cameras" => {
                let list: Vec<Value> = camera::list().into_iter().map(|c| json!({"path": c.path, "name": c.name, "infrared": c.infrared})).collect();
                let mut r = result(true, "");
                r["cameras"] = list.into();
                r["selected"] = self.settings().camera.into();
                r["auto"] = camera::auto_path().into();
                self.reply(id, &r);
            }
            "verify" => self.verify(id, &request),
            "enroll" => {
                let Some(uid) = self.target(id, &request) else { return };
                if self.job.is_some() {
                    self.fail(id, "busy", json!({}));
                    return;
                }
                let name: String = request["name"].as_str().unwrap_or("").trim().chars().take(64).collect();
                self.authorize(id, Then::Enroll { uid, name });
            }
            "list" => {
                let Some(uid) = self.target(id, &request) else { return };
                let faces = Store::new(&self.o.state).load(uid).unwrap_or_default();
                let list: Vec<Value> = faces
                    .iter()
                    .map(|f| {
                        json!({"id": f.id, "name": f.name, "created": f.created as f64, "enabled": f.enabled,
                               "samples": f.samples.len() - f.adaptive_count(), "adaptive": f.adaptive_count()})
                    })
                    .collect();
                let mut r = result(true, "");
                r["faces"] = list.into();
                self.reply(id, &r);
            }
            "remove" | "rename" | "enable" | "disable" | "clear" => {
                let Some(uid) = self.target(id, &request) else { return };
                let face = request["id"].as_str().unwrap_or("").to_string();
                let name: String = request["name"].as_str().unwrap_or("").trim().chars().take(64).collect();
                self.authorize(id, Then::Change { uid, cmd, id: face, name });
            }
            "watch" => self.send(id, &json!({"event": "watching"})),
            "unlocked" => {
                let uid = self.clients[&id].uid;
                let mut state = UserState::load(&self.o.state, uid);
                // Our unlock goes through logind. The lock screen's password
                // prompt is cut off mid-question by it and counts that as a
                // wrong password, so a few face unlocks would lock the account
                // (pam_faillock). Taken back here.
                if paths::is_root() && state.last_purpose == "unlock" && now() - state.last_unlock <= UNLOCK_PAIR_SECONDS {
                    system::reset_failed_logins(uid);
                }
                if state.failures > 0 || state.locked_until > 0 {
                    state.failures = 0;
                    state.locked_until = 0;
                    state.save(&self.o.state, uid);
                }
                self.reply(id, &result(true, ""));
            }
            _ => self.fail(id, "bad-request", json!({})),
        }
    }

    fn status(&mut self, id: Id) {
        let uid = self.clients[&id].uid;
        let s = self.settings();
        let faces = Store::new(&self.o.state).load(uid).unwrap_or_default();
        let state = UserState::load(&self.o.state, uid);
        let mut path = s.camera.clone();
        if path.is_empty() || path == "auto" {
            path = camera::auto_path();
        }
        let (name, present) = if path.starts_with("file:") || path.starts_with("images:") {
            (path.clone(), true)
        } else {
            match camera::list().into_iter().find(|c| c.path == path) {
                Some(c) => (c.name, true),
                None => (String::new(), false),
            }
        };
        let models = self.ensure_vision();
        let r = json!({
            "event": "result", "ok": true, "version": paths::VERSION,
            "user": system::name_of(uid),
            "faces": faces.iter().filter(|f| f.enabled).count(),
            "identities": faces.len(),
            "lockout": state.lockout_left(now()),
            "lastUnlock": state.last_unlock,
            "lastPurpose": state.last_purpose,
            "camera": s.camera, "cameraPath": path, "cameraName": name, "cameraPresent": present,
            "models": models,
            "liveness": s.liveness_name(), "strictness": s.strictness_name(),
            "attention": s.attention, "scanSeconds": s.scan_seconds,
            "busy": self.job.is_some(),
        });
        self.reply(id, &r);
    }

    fn verify(&mut self, id: Id, request: &Value) {
        let Some(uid) = self.target(id, request) else { return };
        let s = self.settings();
        // Whoever sits in front of the camera need not be whoever types.
        if request["remote"] == true && !s.ssh_sessions {
            self.fail(id, "remote", json!({}));
            return;
        }
        if self.job.is_some() {
            self.fail(id, "busy", json!({}));
            return;
        }
        let state = UserState::load(&self.o.state, uid);
        let left = state.lockout_left(now());
        if left > 0 {
            self.fail(id, "lockout", json!({"seconds": left}));
            return;
        }
        if s.skip_lid_closed && system::lid_closed() {
            self.fail(id, "lid-closed", json!({}));
            return;
        }
        let faces: Vec<_> = Store::new(&self.o.state).load(uid).unwrap_or_default().into_iter().filter(|f| f.enabled).collect();
        if faces.is_empty() {
            self.fail(id, "not-enrolled", json!({}));
            return;
        }
        if !self.ensure_vision() {
            let m = self.vision_error.clone();
            self.fail(id, "models", json!({"message": m}));
            return;
        }
        let mut purpose = request["purpose"].as_str().unwrap_or("").to_string();
        if !["unlock", "sudo", "polkit", "lockscreen", "test"].contains(&purpose.as_str()) {
            purpose = "other".into();
        }
        // The password dialog open right now may be the one asking whether
        // faces may be changed.
        if self.manage_checks > 0 && purpose != "unlock" && purpose != "test" {
            self.fail(id, "busy", json!({}));
            return;
        }
        // The lock screen's agent draws its own scans. Everybody else's it
        // has to be told about.
        if purpose != "unlock" {
            self.agent = agentlink::AgentLink::open(uid);
        }
        // Another program has the camera, a video call most likely. No scan
        // starts: the bubble shows a camera with a line through it, or
        // nothing if the user wants it quiet, and the password is asked for
        // at once.
        if camera::in_use(&s.camera) {
            eprintln!("scan for {} ({purpose}): the camera is in use", system::name_of(uid));
            self.broadcast(uid, json!({"event": "scan", "state": "failure", "reason": "camera-busy", "purpose": purpose}));
            self.fail(id, "camera-busy", json!({}));
            return;
        }
        let verbose = request["verbose"] == true;
        self.broadcast(uid, json!({"event": "scan", "state": "start", "purpose": purpose}));
        self.start(Job::Scan { uid, purpose }, Some(id), move |ctx| Ev::ScanDone(jobs::scan(ctx, &s, &faces, verbose)));
    }

    fn job_event(&mut self, event: Value) {
        let Some(j) = &self.job else { return };
        let owner = j.owner;
        let scan = match &j.job {
            Job::Scan { uid, purpose } => Some((*uid, purpose.clone())),
            Job::Enroll { .. } => None,
        };
        if let Some(owner) = owner {
            self.send(owner, &event);
        }
        // The bubble hears about scans, not about what the setup sees.
        if let Some((uid, purpose)) = scan {
            let what = event["event"].as_str().unwrap_or("");
            if what == "face" || what == "hint" {
                let mut e = json!({"event": "scan", "state": what, "purpose": purpose});
                if what == "hint" {
                    e["hint"] = event["hint"].clone();
                }
                self.broadcast(uid, e);
            }
        }
    }

    fn take_job(&mut self) -> Option<Running> {
        let j = self.job.take()?;
        Some(j)
    }

    fn scan_done(&mut self, outcome: jobs::ScanOutcome) {
        let Some(j) = self.take_job() else { return };
        let _ = j.thread.join();
        let Job::Scan { uid, purpose } = j.job else { return };
        let s = self.settings();
        let mut res = outcome.result;
        let mut state = UserState::load(&self.o.state, uid);
        let reason = res["reason"].as_str().unwrap_or("").to_string();
        let t = now();
        if res["ok"] == true {
            state.failures = 0;
            state.locked_until = 0;
            state.last_unlock = t;
            state.last_purpose = purpose.clone();
            // A lock screen that asks for the face through PAM only asks
            // again after a password, so Enter on the empty field is how a
            // scan starts there, and pam_faillock counts it as a wrong
            // password. The face was right: taken back, as a correct password
            // would.
            if paths::is_root() && purpose == "lockscreen" {
                system::reset_failed_logins(uid);
            }
            // Learn from a confident match, the way Face ID keeps up with a
            // beard growing in. Only well clear of the threshold, so the
            // samples cannot creep towards somebody else.
            if s.adapt && outcome.matched_score >= s.threshold() + 0.08 && !outcome.matched_embedding.is_empty() {
                let st = Store::new(&self.o.state);
                let mut faces = st.load(uid).unwrap_or_default();
                if let Some(f) = faces.iter_mut().find(|f| f.id == outcome.matched_id)
                    && store::adapt(f, &outcome.matched_embedding, t)
                    && let Err(e) = st.save(uid, &faces)
                {
                    eprintln!("warning: could not save what was learned: {e}");
                }
            }
        } else if matches!(reason.as_str(), "mismatch" | "spoof" | "liveness") {
            // A face that did not match, a fake, or a match that never showed
            // a sign of life. An empty chair or somebody looking away does not
            // count: none of them is anybody trying to get in.
            state.failures += 1;
            if state.failures >= s.max_failures {
                state.failures = 0;
                state.locked_until = t + s.lockout_minutes as i64 * 60;
                res["lockout"] = state.lockout_left(t).into();
            }
        }
        state.save(&self.o.state, uid);
        let mut e = json!({"event": "scan", "state": if res["ok"] == true { "success" } else { "failure" }, "reason": reason, "purpose": purpose});
        if let Some(l) = res.get("lockout") {
            e["lockout"] = l.clone();
        }
        self.broadcast(uid, e);
        eprintln!("scan for {} ({purpose}): {}", system::name_of(uid), if res["ok"] == true { "recognised".to_string() } else { format!("not recognised, {reason}") });
        if let Some(owner) = j.owner {
            self.reply(owner, &res);
        }
    }

    fn enroll_done(&mut self, outcome: jobs::EnrollOutcome) {
        let Some(j) = self.take_job() else { return };
        let _ = j.thread.join();
        let Job::Enroll { uid } = j.job else { return };
        let mut res = outcome.result;
        if let Some(identity) = outcome.identity {
            let st = Store::new(&self.o.state);
            let mut faces = st.load(uid).unwrap_or_default();
            let name = identity.name.clone();
            faces.push(identity);
            match st.save(uid, &faces) {
                Ok(()) => eprintln!("new face \"{name}\" for {}", system::name_of(uid)),
                Err(e) => {
                    res = result(false, "store");
                    res["message"] = e.into();
                }
            }
        }
        if let Some(owner) = j.owner {
            self.reply(owner, &res);
        }
    }

    fn broadcast(&mut self, uid: u32, event: Value) {
        if let Some(link) = &self.agent {
            link.send(&event);
            let state = event["state"].as_str().unwrap_or("");
            if state == "success" || state == "failure" {
                self.agent = None;
            }
        }
        let watchers: Vec<Id> = self.clients.iter().filter(|(_, c)| c.role == "watch" && c.uid == uid).map(|(id, _)| *id).collect();
        for id in watchers {
            self.send(id, &event);
        }
    }

    fn idle(&self) -> bool {
        self.clients.is_empty() && self.job.is_none() && self.manage_checks == 0
    }

    fn run(mut self, rx: Receiver<Ev>) {
        loop {
            let ev = match self.o.idle {
                Some(d) if self.idle() => match rx.recv_timeout(d) {
                    Ok(ev) => ev,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        eprintln!("idle, exiting");
                        break;
                    }
                    Err(_) => break,
                },
                _ => match rx.recv() {
                    Ok(ev) => ev,
                    Err(_) => break,
                },
            };
            match ev {
                Ev::Accept(s) => self.add_client(s),
                Ev::Line(id, v) => self.request(id, v),
                Ev::Gone(id) => self.gone(id),
                Ev::JobEvent(v) => self.job_event(v),
                Ev::ScanDone(o) => self.scan_done(o),
                Ev::EnrollDone(o) => self.enroll_done(o),
                Ev::Authorized(id, ok, then) => {
                    self.manage_checks = self.manage_checks.saturating_sub(1);
                    self.authorized(id, ok, then);
                }
                Ev::Quit => break,
            }
        }
        // SIGTERM from systemd is the normal way this ends. The camera thread
        // is stopped and waited for rather than being cut off mid-frame.
        if let Some(j) = self.job.take() {
            j.cancel.store(true, Ordering::Relaxed);
            let _ = j.thread.join();
        }
    }
}

/// sd_listen_fds(), without libsystemd for one function.
fn systemd_socket() -> Option<i32> {
    let pid: u32 = std::env::var("LISTEN_PID").ok()?.parse().ok()?;
    let fds: u32 = std::env::var("LISTEN_FDS").ok()?.parse().ok()?;
    if pid != std::process::id() || fds < 1 {
        return None;
    }
    // SAFETY: nothing else reads the environment at this point.
    unsafe {
        std::env::remove_var("LISTEN_PID");
        std::env::remove_var("LISTEN_FDS");
        std::env::remove_var("LISTEN_FDNAMES");
    }
    Some(3)
}

fn usage() -> ! {
    eprintln!("usage: face-unlockd [--socket PATH] [--state-dir DIR] [--config FILE] [--models DIR] [--idle-exit SECONDS]");
    std::process::exit(2);
}

fn main() {
    // Face data and state are for root's eyes only, whatever creates them.
    // SAFETY: plain libc calls.
    unsafe { libc::umask(0o077) };

    let mut o = Options {
        socket: paths::SOCKET.into(),
        state: paths::STATEDIR.into(),
        config: paths::SYSCONFIG.into(),
        models: paths::models(),
        idle: None,
        systemd_fd: systemd_socket(),
        ask_polkit: paths::is_root(),
    };
    let mut idle: Option<u64> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| usage());
        match a.as_str() {
            "--socket" => o.socket = value().into(),
            "--state-dir" => o.state = value().into(),
            "--config" => o.config = value().into(),
            "--models" => o.models = value().into(),
            "--idle-exit" => idle = Some(value().parse().unwrap_or_else(|_| usage())),
            "--version" | "-V" => {
                println!("face-unlockd {}", paths::VERSION);
                return;
            }
            _ => usage(),
        }
    }
    // Started by the socket: go away again when there is nothing to do.
    // Started by hand (development): stay.
    o.idle = match idle {
        Some(0) => None,
        Some(s) => Some(Duration::from_secs(s)),
        None => o.systemd_fd.map(|_| Duration::from_secs(60)),
    };
    if !o.ask_polkit {
        eprintln!("not running as root: face changes are not checked with polkit (development only)");
    }

    // Blocked everywhere, and waited for on a thread of their own.
    // SAFETY: plain signal set handling before any thread exists.
    let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
    unsafe {
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGTERM);
        libc::sigaddset(&mut set, libc::SIGINT);
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
    }

    let _ = std::fs::create_dir_all(&o.state);
    let _ = std::fs::set_permissions(&o.state, std::os::unix::fs::PermissionsExt::from_mode(0o700));

    let listener = match o.systemd_fd {
        // Accepted here, never removed: the socket file is systemd's. Gone,
        // nothing could start the daemon again after it exits idle.
        // SAFETY: systemd hands us fd 3 as a listening socket.
        Some(fd) => unsafe { UnixListener::from_raw_fd(fd) },
        None => {
            if let Some(dir) = o.socket.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::remove_file(&o.socket);
            match UnixListener::bind(&o.socket) {
                Ok(l) => {
                    let _ = std::fs::set_permissions(&o.socket, std::os::unix::fs::PermissionsExt::from_mode(0o666));
                    l
                }
                Err(e) => {
                    eprintln!("error: cannot listen: {e}");
                    std::process::exit(1);
                }
            }
        }
    };

    let (tx, rx) = channel();
    {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut sig = 0;
            // SAFETY: set holds the blocked signals.
            unsafe { libc::sigwait(&set, &mut sig) };
            let _ = tx.send(Ev::Quit);
        });
    }
    {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for s in listener.incoming() {
                match s {
                    Ok(s) => {
                        if tx.send(Ev::Accept(s)).is_err() {
                            break;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => std::thread::sleep(Duration::from_millis(100)),
                }
            }
        });
    }

    let mut server = Server { o, tx, clients: HashMap::new(), next_id: 1, vision: None, vision_error: String::new(), job: None, agent: None, manage_checks: 0 };
    // Loaded up front: whoever started the daemon is about to ask for a scan.
    if !server.ensure_vision() {
        eprintln!("warning: {}", server.vision_error);
    }
    server.run(rx);
}
