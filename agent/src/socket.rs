//! The agent's socket, $XDG_RUNTIME_DIR/face-unlock/agent.socket. The daemon
//! tells it about scans it did not start (sudo, admin prompts), so the bubble
//! shows them, and `face-unlock lock` asks it to lock the screen.

use crate::app::Event;
use face_unlock::paths;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::mpsc;
use std::time::Duration;

type Tx = smithay_client_toolkit::reexports::calloop::channel::Sender<Event>;

/// Whether an agent answers on the socket.
pub fn running() -> bool {
    UnixStream::connect(paths::agent_socket()).is_ok()
}

/// Asks the running agent to lock. The answer comes once the compositor has
/// the lock, so that an idle daemon that locks before sleep does not suspend
/// an unlocked screen.
pub fn request_lock() -> bool {
    let Ok(mut s) = UnixStream::connect(paths::agent_socket()) else { return false };
    let _ = s.set_write_timeout(Some(Duration::from_millis(500)));
    if s.write_all(b"{\"event\":\"lock\"}\n").is_err() {
        return false;
    }
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let mut line = String::new();
    let _ = BufReader::new(s).read_line(&mut line);
    serde_json::from_str::<Value>(line.trim()).is_ok_and(|v| v["event"] == "locked")
}

fn peer_uid(s: &UnixStream) -> Option<u32> {
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: cred is a ucred of the size passed.
    let r = unsafe { libc::getsockopt(s.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, (&mut cred as *mut libc::ucred).cast(), &mut len) };
    (r == 0).then_some(cred.uid)
}

fn serve(s: UnixStream, uid: u32, tx: Tx) {
    let me = unsafe { libc::getuid() };
    let Ok(writer) = s.try_clone() else { return };
    let mut writer = writer;
    let _ = writer.set_write_timeout(Some(Duration::from_secs(2)));
    let mut reader = BufReader::new(s.take(64 * 1024));
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let Ok(o) = serde_json::from_str::<Value>(line.trim()) else { continue };
        match o["event"].as_str() {
            Some("scan") => {
                let _ = tx.send(Event::Scan(o));
            }
            Some("lock") if uid == me => {
                let (answer, wait) = mpsc::channel();
                let _ = tx.send(Event::LockRequest(answer));
                if wait.recv_timeout(Duration::from_secs(10)) == Ok(true) {
                    let _ = writer.write_all(format!("{}\n", json!({"event": "locked"})).as_bytes());
                }
            }
            _ => {}
        }
    }
}

/// Listens, each connection on a thread of its own. Anybody may write to the
/// socket itself, because the directory around it lets nobody but this user
/// in. The daemon gets through that directory with CAP_DAC_READ_SEARCH,
/// which allows passing through but not writing to something not open to
/// others. Who is on the other end is checked on every connection anyway:
/// root (the daemon) or this user.
pub fn listen(tx: Tx) -> std::io::Result<()> {
    let path = paths::agent_socket();
    let dir = path.parent().expect("a directory");
    std::fs::create_dir_all(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666))?;
    let me = unsafe { libc::getuid() };
    std::thread::spawn(move || {
        for s in listener.incoming().flatten() {
            match peer_uid(&s) {
                Some(uid) if uid == 0 || uid == me => {
                    let tx = tx.clone();
                    std::thread::spawn(move || serve(s, uid, tx));
                }
                _ => {}
            }
        }
    });
    Ok(())
}
