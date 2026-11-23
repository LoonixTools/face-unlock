//! Telling a user's session about a scan it did not start itself (sudo, an
//! admin prompt, a test from the window), so the bubble can show it.
//!
//! The agent listens on /run/user/UID/face-unlock/agent.socket. That is a
//! place the user controls, so nothing is taken for granted: the socket has
//! to belong to the user, and so does the process that answers on it,
//! checked by the kernel before a single byte is written. What is written is
//! only where a scan is ("start", "success", ...), never anything about the
//! face.

use serde_json::Value;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

pub struct AgentLink {
    tx: Sender<Value>,
}

fn owned_by(path: &str, uid: u32, socket: bool) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.uid() == uid && if socket { m.file_type().is_socket() } else { m.is_dir() })
}

impl AgentLink {
    pub fn open(uid: u32) -> Option<AgentLink> {
        let dir = format!("/run/user/{uid}/face-unlock");
        let path = format!("{dir}/agent.socket");
        if !owned_by(&dir, uid, false) || !owned_by(&path, uid, true) {
            // No agent in that session, or not one of this user's making.
            return None;
        }
        let (tx, rx) = channel::<Value>();
        std::thread::spawn(move || {
            let Ok(mut s) = UnixStream::connect(&path) else { return };
            let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
            let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
            // SAFETY: cred is a ucred of the size passed.
            let r = unsafe { libc::getsockopt(s.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, (&mut cred as *mut libc::ucred).cast(), &mut len) };
            if r != 0 || cred.uid != uid {
                return;
            }
            let _ = s.set_write_timeout(Some(Duration::from_secs(2)));
            // However the scan ends, this does not outlive it by much.
            while let Ok(event) = rx.recv_timeout(Duration::from_secs(60)) {
                let mut line = event.to_string();
                line.push('\n');
                if s.write_all(line.as_bytes()).is_err() {
                    return;
                }
                let state = event["state"].as_str().unwrap_or("");
                if state == "success" || state == "failure" {
                    return;
                }
            }
        });
        Some(AgentLink { tx })
    }

    pub fn send(&self, event: &Value) {
        let _ = self.tx.send(event.clone());
    }
}
