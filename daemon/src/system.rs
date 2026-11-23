//! Small questions about the machine and its users, and polkit.

use std::ffi::{CStr, CString};

pub fn uid_of(user: &str) -> Option<u32> {
    if user.is_empty() {
        return None;
    }
    let c = CString::new(user).ok()?;
    let mut buf = vec![0 as libc::c_char; 16384];
    let mut pw: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: all pointers are valid for the call.
    let r = unsafe { libc::getpwnam_r(c.as_ptr(), &mut pw, buf.as_mut_ptr(), buf.len(), &mut result) };
    (r == 0 && !result.is_null()).then_some(pw.pw_uid)
}

pub fn name_of(uid: u32) -> String {
    let mut buf = vec![0 as libc::c_char; 16384];
    let mut pw: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: all pointers are valid for the call.
    let r = unsafe { libc::getpwuid_r(uid, &mut pw, buf.as_mut_ptr(), buf.len(), &mut result) };
    if r != 0 || result.is_null() {
        return uid.to_string();
    }
    // SAFETY: getpwuid_r filled pw_name with a C string in buf.
    unsafe { CStr::from_ptr(pw.pw_name) }.to_string_lossy().into_owned()
}

/// Whether a laptop lid is shut. Asks logind, and falls back to ACPI.
pub fn lid_closed() -> bool {
    let logind = zbus::blocking::Connection::system().ok().and_then(|c| {
        let p = zbus::blocking::Proxy::new(&c, "org.freedesktop.login1", "/org/freedesktop/login1", "org.freedesktop.login1.Manager").ok()?;
        p.get_property::<bool>("LidClosed").ok()
    });
    if let Some(closed) = logind {
        return closed;
    }
    std::fs::read_dir("/proc/acpi/button/lid")
        .map(|d| d.filter_map(|e| e.ok()).any(|e| std::fs::read_to_string(e.path().join("state")).is_ok_and(|s| s.contains("closed"))))
        .unwrap_or(false)
}

/// When a process started, in clock ticks since boot, as polkit wants it to
/// tell a process from a later one that got the same pid.
pub fn process_start_time(pid: i32) -> u64 {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else { return 0 };
    // The second field is the command name in brackets and can hold spaces
    // and brackets of its own. After the last ')' are plain numbers; the
    // start time is the 20th of them.
    let Some(close) = stat.rfind(')') else { return 0 };
    stat[close + 1..].split_whitespace().nth(19).and_then(|s| s.parse().ok()).unwrap_or(0)
}

/// Forget the failed logins pam_faillock counted for somebody, as a correct
/// password does. False without faillock.
pub fn reset_failed_logins(uid: u32) -> bool {
    // Not from PATH: this runs as root.
    let Some(faillock) = ["/usr/sbin/faillock", "/usr/bin/faillock", "/sbin/faillock"].into_iter().find(|p| std::path::Path::new(p).exists()) else {
        return false;
    };
    std::process::Command::new(faillock)
        .args(["--user", &name_of(uid), "--reset"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Asks polkit whether the process may change face data. Interactive: this
/// can take as long as it takes somebody to type a password into the polkit
/// agent of their own session.
pub fn polkit_allows(pid: i32, uid: u32, action: &str) -> bool {
    use std::collections::HashMap;
    use zbus::zvariant::Value;
    let run = || -> zbus::Result<bool> {
        let conn = zbus::blocking::Connection::system()?;
        let mut details: HashMap<&str, Value> = HashMap::new();
        details.insert("pid", Value::U32(pid as u32));
        details.insert("start-time", Value::U64(process_start_time(pid)));
        details.insert("uid", Value::I32(uid as i32));
        let subject = ("unix-process", details);
        let empty: HashMap<&str, &str> = HashMap::new();
        // AllowUserInteraction, and long enough to find the dialog and type.
        let reply = conn.call_method(
            Some("org.freedesktop.PolicyKit1"),
            "/org/freedesktop/PolicyKit1/Authority",
            Some("org.freedesktop.PolicyKit1.Authority"),
            "CheckAuthorization",
            &(subject, action, empty, 1u32, ""),
        )?;
        let (authorized, _challenge, _details): (bool, bool, HashMap<String, String>) = reply.body().deserialize()?;
        Ok(authorized)
    };
    match run() {
        Ok(ok) => ok,
        Err(e) => {
            eprintln!("warning: polkit: {e}");
            false
        }
    }
}
