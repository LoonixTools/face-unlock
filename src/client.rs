//! The way to the daemon.
//!
//! One JSON object per line, both ways. A client sends one request; the
//! daemon answers with events and ends with one whose "event" is "result".
//! "watch" never ends. See daemon/src/server.rs for every request.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

pub struct Conn {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
}

impl Conn {
    /// Connects and sends `request`.
    pub fn open(request: &Value) -> std::io::Result<Conn> {
        let stream = UnixStream::connect(crate::paths::socket())?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        let reader = BufReader::new(stream.try_clone()?);
        let mut conn = Conn { stream, reader };
        conn.send(request)?;
        Ok(conn)
    }

    pub fn send(&mut self, message: &Value) -> std::io::Result<()> {
        let mut line = message.to_string();
        line.push('\n');
        self.stream.write_all(line.as_bytes())
    }

    /// The next message, or None once the daemon hung up.
    pub fn next_event(&mut self) -> Option<Value> {
        let mut line = String::new();
        loop {
            line.clear();
            match self.reader.read_line(&mut line) {
                Ok(0) | Err(_) => return None,
                Ok(_) => {
                    if let Ok(v) = serde_json::from_str::<Value>(line.trim()) {
                        return Some(v);
                    }
                }
            }
        }
    }

    /// Every event until the result, which is returned.
    pub fn result(mut self, mut each: impl FnMut(&Value)) -> Value {
        while let Some(e) = self.next_event() {
            if e["event"] == "result" {
                return e;
            }
            each(&e);
        }
        failure("disconnected")
    }

    /// A handle that ends the connection from another thread.
    pub fn closer(&self) -> Option<UnixStream> {
        self.stream.try_clone().ok()
    }

    pub fn set_read_timeout(&self, t: Option<Duration>) {
        let _ = self.stream.set_read_timeout(t);
    }
}

/// A result that says why there was none.
pub fn failure(reason: &str) -> Value {
    json!({"event": "result", "ok": false, "reason": reason})
}

/// Sends `request` and waits for its result.
pub fn request(request: Value) -> Value {
    match Conn::open(&request) {
        Ok(c) => c.result(|_| {}),
        Err(e) => {
            let mut f = failure("unreachable");
            f["message"] = e.to_string().into();
            f
        }
    }
}

pub fn ok(result: &Value) -> bool {
    result["ok"] == true
}

pub fn reason(result: &Value) -> &str {
    result["reason"].as_str().unwrap_or("")
}

/// What the daemon says about this user and itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub faces: u32,
    pub identities: u32,
    pub lockout: u64,
    pub last_unlock: i64,
    pub last_purpose: String,
    pub camera: String,
    pub camera_path: String,
    pub camera_name: String,
    pub camera_present: bool,
    pub models: bool,
    pub liveness: String,
    pub busy: bool,
    pub version: String,
}

pub fn status() -> Option<Status> {
    let r = request(json!({"cmd": "status"}));
    if !ok(&r) {
        return None;
    }
    let s = |k: &str| r[k].as_str().unwrap_or("").to_string();
    Some(Status {
        faces: r["faces"].as_u64().unwrap_or(0) as u32,
        identities: r["identities"].as_u64().unwrap_or(0) as u32,
        lockout: r["lockout"].as_u64().unwrap_or(0),
        last_unlock: r["lastUnlock"].as_i64().unwrap_or(0),
        last_purpose: s("lastPurpose"),
        camera: s("camera"),
        camera_path: s("cameraPath"),
        camera_name: s("cameraName"),
        camera_present: r["cameraPresent"] == true,
        models: r["models"] == true,
        liveness: s("liveness"),
        busy: r["busy"] == true,
        version: s("version"),
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub id: String,
    pub name: String,
    pub created: i64,
    pub enabled: bool,
    pub samples: u32,
    pub learned: u32,
}

pub fn faces() -> Option<Vec<Face>> {
    let r = request(json!({"cmd": "list"}));
    if !ok(&r) {
        return None;
    }
    Some(
        r["faces"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|f| Face {
                        id: f["id"].as_str().unwrap_or("").into(),
                        name: f["name"].as_str().unwrap_or("").into(),
                        created: f["created"].as_f64().unwrap_or(0.0) as i64,
                        enabled: f["enabled"] != false,
                        samples: f["samples"].as_u64().unwrap_or(0) as u32,
                        learned: f["adaptive"].as_u64().unwrap_or(0) as u32,
                    })
                    .collect()
            })
            .unwrap_or_default(),
    )
}

#[derive(Clone, Debug, PartialEq)]
pub struct Camera {
    pub path: String,
    pub name: String,
    pub infrared: bool,
}

/// Every camera, and the one "auto" means.
pub fn cameras() -> Option<(Vec<Camera>, String)> {
    let r = request(json!({"cmd": "cameras"}));
    if !ok(&r) {
        return None;
    }
    let list = r["cameras"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|c| Camera {
                    path: c["path"].as_str().unwrap_or("").into(),
                    name: c["name"].as_str().unwrap_or("").into(),
                    infrared: c["infrared"] == true,
                })
                .collect()
        })
        .unwrap_or_default();
    Some((list, r["auto"].as_str().unwrap_or("").into()))
}
