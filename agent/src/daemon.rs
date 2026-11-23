//! Requests to the daemon, each on a thread of its own. What comes back goes
//! to the main loop as events.

use crate::app::Event;
use face_unlock::client::{self, Conn};
use serde_json::{Value, json};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;

type Tx = smithay_client_toolkit::reexports::calloop::channel::Sender<Event>;

/// A scan that runs. Dropping it does not stop it; abort() does.
pub struct Scan {
    pub id: u64,
    closer: Option<UnixStream>,
}

impl Scan {
    pub fn abort(self) {
        if let Some(c) = self.closer {
            let _ = c.shutdown(Shutdown::Both);
        }
    }
}

/// Asks the daemon to look for this user's face, to unlock.
pub fn scan(id: u64, tx: Tx) -> Scan {
    let conn = match Conn::open(&json!({"cmd": "verify", "purpose": "unlock"})) {
        Ok(c) => c,
        Err(e) => {
            let mut r = client::failure("unreachable");
            r["message"] = Value::from(e.to_string());
            let _ = tx.send(Event::ScanDone(id, r));
            return Scan { id, closer: None };
        }
    };
    let closer = conn.closer();
    std::thread::spawn(move || {
        let ev = tx.clone();
        let result = conn.result(|e| {
            let _ = ev.send(Event::ScanStep(id, e.clone()));
        });
        let _ = tx.send(Event::ScanDone(id, result));
    });
    Scan { id, closer }
}

/// A request whose answer does not matter: "hello" to have the daemon load
/// its networks before anybody waits, "unlocked" to end a lockout.
pub fn fire(cmd: &'static str) {
    std::thread::spawn(move || {
        if let Ok(conn) = Conn::open(&json!({"cmd": cmd})) {
            conn.set_read_timeout(Some(std::time::Duration::from_secs(30)));
            let _ = conn.result(|_| {});
        }
    });
}
