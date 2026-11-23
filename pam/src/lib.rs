//! pam_face_unlock: face unlock for sudo, admin prompts and lock screens.
//!
//! All the vision is in the daemon. This asks it to scan for the user and
//! turns the answer into a PAM result. It is meant to sit first in a stack as
//! "auth sufficient": a match lets the person in, anything else falls through
//! to the password as if the module was not there.
//!
//! It refuses to be useful exactly where a camera is the wrong witness:
//! - a remote login (SSH, a remote host in PAM_RHOST). Whoever is in front of
//!   the camera need not be the person typing. The daemon refuses it unless
//!   SshSessions is on in the system settings.
//! - a user who is not sitting at the machine right now, with an active
//!   session on a seat. Then it returns PAM_IGNORE without touching the
//!   camera.
//!
//! Options:
//!
//! ```text
//! purpose=sudo|polkit|other  what the bubble says (default: from the
//!                            service name)
//! lockscreen                 in a lock screen's stack (hyprlock, swaylock,
//!                            gtklock...): see lock_starting()
//! socket=PATH                the daemon's socket (for development)
//! timeout=SECONDS            give up waiting for the daemon after this
//! debug                      log to the auth log what happened
//! ```

use libc::{c_char, c_int, c_void};
use serde_json::Value;
use std::ffi::{CStr, CString};
use std::io::{BufRead, BufReader, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

include!(concat!(env!("OUT_DIR"), "/messages.rs"));

#[allow(non_camel_case_types)]
type pam_handle_t = c_void;

const PAM_SUCCESS: c_int = 0;
const PAM_AUTH_ERR: c_int = 7;
const PAM_AUTHINFO_UNAVAIL: c_int = 9;
const PAM_IGNORE: c_int = 25;
const PAM_SILENT: c_int = 0x8000;
const PAM_SERVICE: c_int = 1;
const PAM_RHOST: c_int = 4;
const PAM_TEXT_INFO: c_int = 4;
const LOG_NOTICE: c_int = 5;
const LOG_WARNING: c_int = 4;
const LOG_DEBUG: c_int = 7;

const SOCKET: &str = "/run/face-unlock/socket";

#[link(name = "pam")]
unsafe extern "C" {
    fn pam_get_user(pamh: *mut pam_handle_t, user: *mut *const c_char, prompt: *const c_char) -> c_int;
    fn pam_get_item(pamh: *const pam_handle_t, item_type: c_int, item: *mut *const c_void) -> c_int;
    fn pam_getenv(pamh: *mut pam_handle_t, name: *const c_char) -> *const c_char;
    fn pam_prompt(pamh: *mut pam_handle_t, style: c_int, response: *mut *mut c_char, fmt: *const c_char, ...) -> c_int;
    fn pam_syslog(pamh: *const pam_handle_t, priority: c_int, fmt: *const c_char, ...);
}

struct Options {
    socket: String,
    purpose: Option<String>,
    timeout: u64,
    lockscreen: bool,
    debug: bool,
}

fn options(args: &[&str]) -> Options {
    let mut o = Options { socket: SOCKET.into(), purpose: None, timeout: 25, lockscreen: false, debug: false };
    for a in args {
        if let Some(v) = a.strip_prefix("socket=") {
            o.socket = v.into();
        } else if let Some(v) = a.strip_prefix("purpose=") {
            o.purpose = Some(v.into());
        } else if let Some(v) = a.strip_prefix("timeout=") {
            o.timeout = v.parse().ok().filter(|t| (3..=120).contains(t)).unwrap_or(25);
        } else if *a == "lockscreen" {
            o.lockscreen = true;
        } else if *a == "debug" {
            o.debug = true;
        }
    }
    o
}

fn log(pamh: *mut pam_handle_t, priority: c_int, msg: &str) {
    let Ok(m) = CString::new(msg) else { return };
    // SAFETY: a valid handle and a "%s" format with one C string.
    unsafe { pam_syslog(pamh, priority, c"%s".as_ptr(), m.as_ptr()) };
}

/// The message in the language of the process, which sudo and polkit's
/// helper keep from the user.
fn translate(msgid: &str) -> &str {
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty())).unwrap_or_default();
    let full = locale.split(['.', '@']).next().unwrap_or("");
    let lang = full.split('_').next().unwrap_or("");
    [full, lang]
        .iter()
        .find_map(|l| CATALOGS.iter().find(|(c, _)| c == l))
        .and_then(|(_, cat)| cat.iter().find(|(id, _)| *id == msgid).map(|(_, t)| *t))
        .unwrap_or(msgid)
}

fn say(pamh: *mut pam_handle_t, flags: c_int, msgid: &str) {
    if flags & PAM_SILENT != 0 {
        return;
    }
    let Ok(m) = CString::new(translate(msgid)) else { return };
    let mut response: *mut c_char = std::ptr::null_mut();
    // SAFETY: a valid handle, a "%s" format with one C string.
    unsafe { pam_prompt(pamh, PAM_TEXT_INFO, &mut response, c"%s".as_ptr(), m.as_ptr()) };
    if !response.is_null() {
        // SAFETY: allocated by the conversation function with malloc.
        unsafe { libc::free(response.cast()) };
    }
}

/// hyprlock asks PAM the moment it starts, before anybody pressed a key.
/// Scanning then would open the screen again for whoever just locked it on
/// purpose while still sitting in front of it. So a lock screen that is less
/// than two seconds old gets no scan; pressing Enter asks again, and the
/// agent scans when somebody comes back.
fn lock_starting() -> bool {
    let (Ok(stat), Ok(uptime)) = (std::fs::read_to_string("/proc/self/stat"), std::fs::read_to_string("/proc/uptime")) else { return false };
    let Some(up) = uptime.split_whitespace().next().and_then(|u| u.parse::<f64>().ok()) else { return false };
    let Some(close) = stat.rfind(')') else { return false };
    let Some(started) = stat[close + 1..].split_whitespace().nth(19).and_then(|s| s.parse::<u64>().ok()) else { return false };
    // SAFETY: no preconditions.
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    ticks > 0 && up - (started as f64 / ticks as f64) < 2.0
}

/// The logind session this process belongs to, from its cgroup.
fn own_session() -> Option<String> {
    let cg = std::fs::read_to_string("/proc/self/cgroup").ok()?;
    cg.lines().find_map(|l| {
        l.rsplit('/').find_map(|part| part.strip_prefix("session-").and_then(|s| s.strip_suffix(".scope")).map(String::from))
    })
}

fn systemd_booted() -> bool {
    std::path::Path::new("/run/systemd/system").is_dir()
}

/// Whoever types this is not whoever sits in front of the camera.
fn remote(pamh: *mut pam_handle_t) -> bool {
    let mut rhost: *const c_void = std::ptr::null();
    // SAFETY: a valid handle and out pointer.
    if unsafe { pam_get_item(pamh, PAM_RHOST, &mut rhost) } == PAM_SUCCESS && !rhost.is_null() {
        // SAFETY: PAM keeps the item as a C string.
        let host = unsafe { CStr::from_ptr(rhost.cast()) }.to_string_lossy();
        if !host.is_empty() && host != "localhost" {
            return true;
        }
    }
    for var in ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"] {
        if std::env::var(var).is_ok_and(|v| !v.is_empty()) {
            return true;
        }
        let c = CString::new(var).unwrap_or_default();
        // SAFETY: a valid handle and name.
        let v = unsafe { pam_getenv(pamh, c.as_ptr()) };
        // SAFETY: a C string when not null.
        if !v.is_null() && !unsafe { CStr::from_ptr(v) }.to_bytes().is_empty() {
            return true;
        }
    }
    if let Some(s) = own_session() {
        let file = std::fs::read_to_string(format!("/run/systemd/sessions/{s}")).unwrap_or_default();
        if file.lines().any(|l| l == "REMOTE=1") {
            return true;
        }
    }
    false
}

/// The person has to be at the machine: an active session on a seat.
fn at_seat(user: &str) -> bool {
    if !systemd_booted() {
        return true;
    }
    let Ok(c) = CString::new(user) else { return false };
    // SAFETY: a valid C string; the result is read at once.
    let pw = unsafe { libc::getpwnam(c.as_ptr()) };
    if pw.is_null() {
        return false;
    }
    // SAFETY: not null.
    let uid = unsafe { (*pw).pw_uid };
    std::fs::read_to_string(format!("/run/systemd/users/{uid}"))
        .unwrap_or_default()
        .lines()
        .find_map(|l| l.strip_prefix("ACTIVE_SEATS="))
        .is_some_and(|s| !s.trim().is_empty())
}

fn connect(o: &Options, pamh: *mut pam_handle_t) -> Option<UnixStream> {
    let s = match UnixStream::connect(&o.socket) {
        Ok(s) => s,
        Err(e) => {
            if o.debug {
                log(pamh, LOG_DEBUG, &format!("cannot reach the daemon at {}: {e}", o.socket));
            }
            return None;
        }
    };
    let _ = s.set_write_timeout(Some(Duration::from_secs(3)));
    // Only root may answer for root. When this runs as somebody else (the
    // tests), a daemon of that same user is no less trusted than the process
    // asking.
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: cred is a ucred of the size passed.
    let r = unsafe { libc::getsockopt(s.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, (&mut cred as *mut libc::ucred).cast(), &mut len) };
    // SAFETY: no preconditions.
    if r != 0 || (cred.uid != 0 && cred.uid != unsafe { libc::geteuid() }) {
        log(pamh, LOG_WARNING, "refusing a face unlock daemon that does not run as root");
        return None;
    }
    Some(s)
}

fn authenticate(pamh: *mut pam_handle_t, flags: c_int, args: &[&str]) -> c_int {
    let o = options(args);
    let mut user: *const c_char = std::ptr::null();
    // SAFETY: a valid handle and out pointer.
    if unsafe { pam_get_user(pamh, &mut user, std::ptr::null()) } != PAM_SUCCESS || user.is_null() {
        return PAM_IGNORE;
    }
    // SAFETY: PAM hands back a C string.
    let user = unsafe { CStr::from_ptr(user) }.to_string_lossy().into_owned();
    if user.is_empty() {
        return PAM_IGNORE;
    }
    let mut service: *const c_void = std::ptr::null();
    // SAFETY: a valid handle and out pointer.
    unsafe { pam_get_item(pamh, PAM_SERVICE, &mut service) };
    // SAFETY: a C string when not null.
    let service = if service.is_null() { String::new() } else { unsafe { CStr::from_ptr(service.cast()) }.to_string_lossy().into_owned() };
    let purpose = o.purpose.clone().unwrap_or_else(|| {
        if o.lockscreen {
            "lockscreen".into()
        } else if service.starts_with("sudo") {
            "sudo".into()
        } else if service == "polkit-1" {
            "polkit".into()
        } else {
            "other".into()
        }
    });

    if o.lockscreen && lock_starting() {
        if o.debug {
            log(pamh, LOG_DEBUG, &format!("the lock screen has only just started, not scanning for {user}"));
        }
        return PAM_IGNORE;
    }
    let is_remote = remote(pamh);
    if !at_seat(&user) {
        if o.debug {
            log(pamh, LOG_DEBUG, &format!("{user} is not at the machine, not scanning"));
        }
        return PAM_IGNORE;
    }
    let Some(mut s) = connect(&o, pamh) else { return PAM_IGNORE };
    let request = serde_json::json!({"cmd": "verify", "user": user, "purpose": purpose, "remote": is_remote});
    if s.write_all(format!("{request}\n").as_bytes()).is_err() {
        return PAM_IGNORE;
    }

    let deadline = Instant::now() + Duration::from_secs(o.timeout);
    let Ok(read) = s.try_clone() else { return PAM_IGNORE };
    let mut reader = BufReader::new(read);
    let mut told = false;
    let mut line = String::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        let mut pfd = libc::pollfd { fd: s.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        if reader.buffer().is_empty() {
            // SAFETY: one valid pollfd.
            let ready = unsafe { libc::poll(&mut pfd, 1, left.as_millis().min(i32::MAX as u128) as c_int) };
            // Ctrl+C in sudo: the camera stops, the password follows.
            if ready <= 0 {
                break;
            }
        }
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let Ok(e) = serde_json::from_str::<Value>(line.trim()) else { continue };
        match e["event"].as_str().unwrap_or("") {
            "started" if !told => {
                told = true;
                say(pamh, flags, "Look at the camera to unlock.");
            }
            "hint" => match e["hint"].as_str().unwrap_or("") {
                "blink" => say(pamh, flags, "Blink, or turn your head a little."),
                "look" => say(pamh, flags, "Look at the screen."),
                "closer" => say(pamh, flags, "Move closer to the camera."),
                "light" => say(pamh, flags, "It is too dark to see your face."),
                _ => {}
            },
            "result" => {
                let reason = e["reason"].as_str().unwrap_or("");
                return if e["ok"] == true {
                    log(pamh, LOG_NOTICE, &format!("face recognised for {user} ({purpose})"));
                    PAM_SUCCESS
                } else if reason == "lockout" || e.get("lockout").is_some() {
                    say(pamh, flags, "Face unlock is paused after too many tries. Use your password.");
                    PAM_AUTH_ERR
                } else if matches!(reason, "mismatch" | "spoof" | "liveness") {
                    log(pamh, LOG_NOTICE, &format!("face not recognised for {user} ({reason})"));
                    say(pamh, flags, "Face not recognised.");
                    PAM_AUTH_ERR
                } else if matches!(reason, "no-face" | "attention" | "quality") {
                    PAM_AUTH_ERR
                } else {
                    // Not set up, no camera, the camera in a video call, lid
                    // shut, busy, SSH: face unlock is simply not available.
                    if o.debug {
                        log(pamh, LOG_DEBUG, &format!("face unlock unavailable for {user}: {reason}"));
                    }
                    PAM_AUTHINFO_UNAVAIL
                };
            }
            _ => {}
        }
    }
    PAM_AUTHINFO_UNAVAIL
}

fn args(argc: c_int, argv: *const *const c_char) -> Vec<String> {
    (0..argc.max(0) as usize)
        .filter_map(|i| {
            // SAFETY: PAM passes argc valid C strings.
            let p = unsafe { *argv.add(i) };
            (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
        })
        .collect()
}

/// # Safety
/// Called by libpam with a valid handle and argc arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pam_sm_authenticate(pamh: *mut pam_handle_t, flags: c_int, argc: c_int, argv: *const *const c_char) -> c_int {
    let a = args(argc, argv);
    let refs: Vec<&str> = a.iter().map(String::as_str).collect();
    std::panic::catch_unwind(|| authenticate(pamh, flags, &refs)).unwrap_or(PAM_IGNORE)
}

/// # Safety
/// Called by libpam.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pam_sm_setcred(_pamh: *mut pam_handle_t, _flags: c_int, _argc: c_int, _argv: *const *const c_char) -> c_int {
    PAM_IGNORE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_message_is_translated_somewhere() {
        for m in MESSAGES {
            assert!(CATALOGS.iter().any(|(_, c)| c.iter().any(|(id, _)| id == m)), "{m}");
        }
    }

    #[test]
    fn options_are_read() {
        let o = options(&["purpose=sudo", "timeout=200", "lockscreen", "socket=/x"]);
        assert_eq!(o.purpose.as_deref(), Some("sudo"));
        assert_eq!(o.timeout, 25);
        assert!(o.lockscreen && !o.debug);
        assert_eq!(o.socket, "/x");
    }
}
