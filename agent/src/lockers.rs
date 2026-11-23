//! Lock screens that are programs of their own (hyprlock, swaylock, gtklock),
//! found in /proc, and the signals that open them. Also the line of text
//! hyprlock shows in place of the bubble.

use face_unlock::paths;
use std::path::PathBuf;

fn is_locker(name: &str) -> bool {
    matches!(name, "hyprlock" | "swaylock" | "gtklock")
}

fn read(path: &str) -> Vec<u8> {
    std::fs::read(path).unwrap_or_default()
}

fn comm(pid: i32) -> String {
    String::from_utf8_lossy(&read(&format!("/proc/{pid}/comm"))).trim().to_string()
}

fn parent_of(pid: i32) -> i32 {
    // The fields after the name in brackets, which can hold anything.
    let stat = String::from_utf8_lossy(&read(&format!("/proc/{pid}/stat"))).to_string();
    let Some(close) = stat.rfind(')') else { return 0 };
    stat[close + 1..].split_whitespace().nth(1).and_then(|p| p.parse().ok()).unwrap_or(0)
}

/// The lock screens of this user on this display. Another session of the
/// same user has a display of its own, and its lock screen is left alone. A
/// child of a lock screen (swaylock and gtklock check the password in one)
/// is left out: killed by the signal before its parent unlocks, it would
/// take the parent down with it, and the screen would stay locked.
fn find_all() -> Vec<i32> {
    use std::os::unix::fs::MetadataExt;
    let display = std::env::var("WAYLAND_DISPLAY").ok().filter(|d| !d.is_empty()).unwrap_or_else(|| "wayland-0".into());
    let me = unsafe { libc::getuid() };
    let mut found = Vec::new();
    let Ok(dir) = std::fs::read_dir("/proc") else { return found };
    for entry in dir.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|n| n.parse::<i32>().ok()) else { continue };
        if entry.metadata().ok().map(|m| m.uid()) != Some(me) || !is_locker(&comm(pid)) {
            continue;
        }
        let environ = read(&format!("/proc/{pid}/environ"));
        let same = environ
            .split(|&b| b == 0)
            .find_map(|v| v.strip_prefix(b"WAYLAND_DISPLAY="))
            .is_none_or(|d| d == display.as_bytes());
        if same {
            found.push(pid);
        }
    }
    let all = found.clone();
    found.retain(|&pid| !all.contains(&parent_of(pid)));
    found
}

/// Whether pid catches sig, from the mask in /proc/<pid>/status.
fn catches(pid: i32, sig: i32) -> bool {
    let status = String::from_utf8_lossy(&read(&format!("/proc/{pid}/status"))).to_string();
    status
        .lines()
        .find_map(|l| l.strip_prefix("SigCgt:"))
        .and_then(|m| u64::from_str_radix(m.trim(), 16).ok())
        .is_some_and(|mask| (mask >> (sig - 1)) & 1 == 1)
}

pub fn find(name: Option<&str>) -> Vec<i32> {
    find_all().into_iter().filter(|&pid| name.is_none_or(|n| comm(pid) == n)).collect()
}

/// Whether a lock screen is up that can be opened with `sig`.
pub fn ready(sig: i32) -> bool {
    find(None).into_iter().any(|pid| catches(pid, sig))
}

/// Sends `sig` to the lock screens that catch it. hyprlock sets up its
/// handlers only once it has locked, and an early signal would kill it and
/// leave the screen locked for good. Returns how many are not ready yet.
pub fn signal(sig: i32, name: Option<&str>) -> usize {
    let mut waiting = 0;
    for pid in find(name) {
        if catches(pid, sig) {
            unsafe { libc::kill(pid, sig) };
        } else {
            waiting += 1;
        }
    }
    waiting
}

/// Where hyprlock reads the line of text from (see `face-unlock` hyprlock
/// settings).
pub fn text_path() -> PathBuf {
    paths::runtime().join("lock-text")
}

/// Writes the line of text. hyprlock reads labels as Pango markup.
pub fn write_text(text: &str) {
    let path = text_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let escaped = text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    let tmp = path.with_extension("new");
    if std::fs::write(&tmp, escaped).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}
