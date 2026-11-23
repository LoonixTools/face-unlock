//! The desktop, the services, and the few things around them: the polkit
//! agent, GNOME's extension, hyprlock's line of text, the screens.

use crate::i18n::{tr, trf};
use crate::paths::{self, NAME, UNIT_AGENT, UNIT_SOCKET};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Desktop {
    Plasma,
    Gnome,
    Hyprland,
    Niri,
    Other,
}

impl Desktop {
    /// Where the lock screen is a program of its own (Hyprland, Niri and the
    /// like), and face-unlock's own one is on offer.
    pub fn own_locker(self) -> bool {
        !matches!(self, Desktop::Plasma | Desktop::Gnome)
    }
}

pub fn desktop() -> Desktop {
    let d = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let parts: Vec<&str> = d.split(':').collect();
    if parts.contains(&"KDE") {
        Desktop::Plasma
    } else if parts.contains(&"GNOME") {
        Desktop::Gnome
    } else if parts.contains(&"Hyprland") {
        Desktop::Hyprland
    } else if parts.contains(&"niri") {
        Desktop::Niri
    } else {
        Desktop::Other
    }
}

pub fn have(cmd: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else { return false };
    std::env::split_paths(&path).any(|d| {
        let p = d.join(cmd);
        std::fs::metadata(&p).is_ok_and(|m| {
            use std::os::unix::fs::PermissionsExt;
            m.is_file() && m.permissions().mode() & 0o111 != 0
        })
    })
}

/// Runs a command quietly and says whether it worked.
pub fn quiet(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// A command's output, when it worked.
pub fn output(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn systemctl(args: &[&str]) -> bool {
    quiet("systemctl", args)
}

pub fn systemctl_user(args: &[&str]) -> bool {
    let mut all = vec!["--user"];
    all.extend_from_slice(args);
    quiet("systemctl", &all)
}

// ---------------------------------------------------------------------------
// The two services
// ---------------------------------------------------------------------------

pub fn socket_enabled() -> bool {
    systemctl(&["is-enabled", "--quiet", UNIT_SOCKET])
}

pub fn agent_available() -> bool {
    have("systemctl") && std::env::var_os("XDG_RUNTIME_DIR").is_some()
}

pub fn agent_enabled() -> bool {
    systemctl_user(&["is-enabled", "--quiet", UNIT_AGENT])
}

pub fn agent_running() -> bool {
    systemctl_user(&["is-active", "--quiet", UNIT_AGENT])
}

pub fn agent_enable() -> bool {
    systemctl_user(&["daemon-reload"]);
    systemctl_user(&["enable", "--now", UNIT_AGENT])
}

pub fn agent_disable() {
    systemctl_user(&["disable", "--now", UNIT_AGENT]);
}

/// Whether the agent's service starts with the session. It is wanted by
/// graphical-session.target, which Hyprland only reaches when it runs under
/// uwsm. Plasma, GNOME and niri-session always do.
pub fn agent_autostarts() -> bool {
    desktop() != Desktop::Hyprland || systemctl_user(&["is-active", "--quiet", "graphical-session.target"])
}

/// A note and the lines to copy under it.
#[derive(Clone, Debug, PartialEq)]
pub struct Hint {
    pub note: String,
    pub code: Vec<String>,
}

/// What to add to Hyprland's config so it starts a user service: the lock
/// screen agent, or a polkit agent. In the Lua it has since 0.56, or in the
/// older format.
pub fn hyprland_start_hint(unit: &str) -> Hint {
    let lua = paths::xdg_config().join("hypr/hyprland.lua").is_file();
    let conf = if lua { "~/.config/hypr/hyprland.lua" } else { "~/.config/hypr/hyprland.conf" };
    let note = if unit == UNIT_AGENT {
        trf("Hyprland does not start the lock screen agent by itself. Add this to %s:", &[&conf])
    } else {
        trf("Hyprland does not start the polkit agent by itself. Add this to %s:", &[&conf])
    };
    let code = if lua {
        vec![
            "hl.on(\"hyprland.start\", function ()".into(),
            format!("  hl.exec_cmd(\"systemctl --user start {unit}\")"),
            "end)".into(),
        ]
    } else {
        vec![format!("exec-once = systemctl --user start {unit}")]
    };
    Hint { note, code }
}

// ---------------------------------------------------------------------------
// The polkit agent
// ---------------------------------------------------------------------------
// polkit asks for the password in a window of the session's polkit agent: to
// add a face, in the admin prompts, and for this program's own settings.
// Plasma and GNOME bring one, Hyprland and Niri do not.

/// Whether no polkit agent runs in this session. polkit has no call to ask
/// that, so this tries to register an agent of its own: polkit refuses it
/// when there is one. Let in, busctl ends at once and polkit drops it again.
/// Any other answer counts as not missing, so nobody is warned wrongly.
/// Asked once per run.
pub fn polkit_agent_missing() -> bool {
    static MISSING: OnceLock<bool> = OnceLock::new();
    *MISSING.get_or_init(|| {
        if !desktop().own_locker() || std::env::var_os("WAYLAND_DISPLAY").is_none() || !have("busctl") {
            return false;
        }
        let session = std::env::var("XDG_SESSION_ID").ok().filter(|s| !s.is_empty()).or_else(|| {
            let uid = unsafe { libc::getuid() }.to_string();
            output("loginctl", &["show-user", &uid, "-p", "Display", "--value"]).map(|s| s.trim().to_string())
        });
        let Some(session) = session.filter(|s| !s.is_empty()) else { return false };
        quiet(
            "busctl",
            &[
                "call",
                "--system",
                "org.freedesktop.PolicyKit1",
                "/org/freedesktop/PolicyKit1/Authority",
                "org.freedesktop.PolicyKit1.Authority",
                "RegisterAuthenticationAgent",
                "(sa{sv})ss",
                "unix-session",
                "1",
                "session-id",
                "s",
                &session,
                "C",
                "/io/github/loonixtools/FaceUnlock/Probe",
            ],
        )
    })
}

/// Root side. hyprpolkitagent where the distribution has it, else KDE's
/// agent, which runs anywhere and which niri suggests. The package manager
/// shows what it installs.
pub fn polkit_agent_install() -> Result<(), String> {
    let run = |cmd: &str, args: &[&str]| Command::new(cmd).args(args).status().is_ok_and(|s| s.success());
    let ok = if have("pacman") {
        if quiet("pacman", &["-Si", "hyprpolkitagent"]) {
            run("pacman", &["-S", "--needed", "--noconfirm", "hyprpolkitagent"])
        } else {
            run("pacman", &["-S", "--needed", "--noconfirm", "polkit-kde-agent"])
        }
    } else if have("dnf") {
        if quiet("dnf", &["-q", "info", "hyprpolkitagent"]) {
            run("dnf", &["install", "-y", "hyprpolkitagent"])
        } else {
            run("dnf", &["install", "-y", "polkit-kde"])
        }
    } else if have("apt-get") {
        if quiet("apt-cache", &["show", "hyprpolkitagent"]) {
            run("apt-get", &["install", "-y", "hyprpolkitagent"])
        } else {
            run("apt-get", &["install", "-y", "polkit-kde-agent-1"])
        }
    } else {
        return Err(tr("No package manager found to install a polkit agent with.").into());
    };
    if ok { Ok(()) } else { Err(tr("Could not install a polkit agent.").into()) }
}

/// The user service of the polkit agent that is installed, if any.
pub fn polkit_agent_unit() -> Option<&'static str> {
    ["hyprpolkitagent.service", "plasma-polkit-agent.service"]
        .into_iter()
        .find(|u| systemctl_user(&["list-unit-files", "--quiet", u]) && output("systemctl", &["--user", "list-unit-files", "--quiet", u]).is_some_and(|o| !o.trim().is_empty()))
}

/// Starts the installed polkit agent now and with the session. Returns a
/// hint when Hyprland will not start it by itself.
pub fn polkit_agent_start(unit: &str) -> Result<Option<Hint>, String> {
    if unit == "plasma-polkit-agent.service" {
        // KDE's has no [Install] section, and outside Plasma nothing makes it
        // wait for the desktop: it would start before there is one.
        let dropin = paths::xdg_config().join(format!("systemd/user/{unit}.d"));
        let _ = std::fs::create_dir_all(&dropin);
        let _ = std::fs::write(dropin.join(format!("{NAME}.conf")), format!("# Written by {NAME}.\n[Unit]\nAfter=graphical-session.target\n"));
        systemctl_user(&["daemon-reload"]);
        systemctl_user(&["add-wants", "graphical-session.target", unit]);
        systemctl_user(&["start", unit]);
    } else {
        systemctl_user(&["enable", "--now", unit]);
    }
    if !systemctl_user(&["is-active", "--quiet", unit]) {
        return Err(trf("The polkit agent did not start. See: systemctl --user status %s", &[&unit]));
    }
    Ok((!agent_autostarts()).then(|| hyprland_start_hint(unit)))
}

// ---------------------------------------------------------------------------
// The bubble's GNOME Shell extension
// ---------------------------------------------------------------------------
// GNOME lets no program draw above its windows; its extensions may. The
// package brings one, and turning face unlock on switches it on.

pub const GNOME_EXTENSION: &str = "face-unlock@loonixtools.github.io";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GnomeExtension {
    On,
    /// GNOME only knows extensions it found when the session started. One
    /// that came later goes straight into the setting and loads at the next
    /// login.
    NextLogin,
    Failed,
}

pub fn gnome_extension_enable() -> GnomeExtension {
    if !have("gsettings") {
        return GnomeExtension::Failed;
    }
    if quiet("gnome-extensions", &["info", GNOME_EXTENSION]) && quiet("gnome-extensions", &["enable", GNOME_EXTENSION]) {
        return GnomeExtension::On;
    }
    let Some(list) = output("gsettings", &["get", "org.gnome.shell", "enabled-extensions"]) else {
        return GnomeExtension::Failed;
    };
    let list = list.trim();
    let quoted = format!("'{GNOME_EXTENSION}'");
    if list.contains(&quoted) {
        return GnomeExtension::NextLogin;
    }
    let new = match list {
        "@as []" | "[]" => format!("[{quoted}]"),
        l => format!("{}, {quoted}]", l.trim_end_matches(']')),
    };
    if quiet("gsettings", &["set", "org.gnome.shell", "enabled-extensions", &new]) {
        GnomeExtension::NextLogin
    } else {
        GnomeExtension::Failed
    }
}

pub fn gnome_extension_disable() {
    if !have("gsettings") || quiet("gnome-extensions", &["disable", GNOME_EXTENSION]) {
        return;
    }
    let Some(list) = output("gsettings", &["get", "org.gnome.shell", "enabled-extensions"]) else { return };
    let quoted = format!("'{GNOME_EXTENSION}'");
    let mut list = list.trim().replace(&format!(", {quoted}"), "").replace(&format!("{quoted}, "), "").replace(&quoted, "");
    if list == "[]" {
        list = "@as []".into();
    }
    quiet("gsettings", &["set", "org.gnome.shell", "enabled-extensions", &list]);
}

// ---------------------------------------------------------------------------
// The lock screen where it is a program of its own (Hyprland, Niri)
// ---------------------------------------------------------------------------
// Either face-unlock's (`face-unlock lock`, with the bubble), or the user's
// own program. hyprlock then shows what the bubble would as a line of text:
// a label in its config reads a file the agent writes.

fn hyprlock_snippet() -> PathBuf {
    paths::xdg_config().join(NAME).join("hyprlock.conf")
}

fn hyprlock_mark() -> String {
    format!("# {NAME}: what face unlock is doing, at the top")
}

fn hyprlock_conf() -> PathBuf {
    paths::xdg_config().join("hypr/hyprlock.conf")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HyprText {
    Done,
    Failed,
    NoHyprlock,
    NoConfig,
}

pub fn hyprlock_text_on() -> HyprText {
    if !have("hyprlock") {
        return HyprText::NoHyprlock;
    }
    let conf = hyprlock_conf();
    if !conf.is_file() {
        return HyprText::NoConfig;
    }
    let snippet = hyprlock_snippet();
    let body = format!(
        "# Written by {NAME}: what face unlock is doing, as a line at the top of\n# hyprlock. Picking {NAME}'s lock screen in its settings takes it out again.\nlabel {{\n    monitor =\n    text = cmd[update:0:1] cat \"$XDG_RUNTIME_DIR/{NAME}/lock-text\" 2>/dev/null\n    color = rgba(255, 255, 255, 0.95)\n    font_size = 20\n    shadow_passes = 2\n    shadow_size = 3\n    halign = center\n    valign = top\n    position = 0, -48\n    zindex = 10\n}}\n"
    );
    if snippet.parent().is_some_and(|d| std::fs::create_dir_all(d).is_err()) || std::fs::write(&snippet, body).is_err() {
        return HyprText::Failed;
    }
    let Ok(text) = std::fs::read_to_string(&conf) else { return HyprText::Failed };
    let mark = hyprlock_mark();
    if text.lines().any(|l| l == mark) {
        return HyprText::Done;
    }
    // With ~ where it can, like a hand-written config. hyprlock expands it.
    let add = format!("\n{mark}\nsource = {}\n", paths::tilde(&snippet));
    match std::fs::OpenOptions::new().append(true).open(&conf).and_then(|mut f| std::io::Write::write_all(&mut f, add.as_bytes())) {
        Ok(()) => HyprText::Done,
        Err(_) => HyprText::Failed,
    }
}

/// The mark, the source line under it, and the empty lines before it.
/// Written back in place, so a hyprlock.conf that is a link to a dotfile
/// stays one.
pub fn hyprlock_text_off() {
    let _ = std::fs::remove_file(hyprlock_snippet());
    let conf = hyprlock_conf();
    let Ok(text) = std::fs::read_to_string(&conf) else { return };
    let mark = hyprlock_mark();
    if !text.lines().any(|l| l == mark) {
        return;
    }
    let mut out = String::new();
    let mut held = String::new();
    let mut skip = false;
    for line in text.lines() {
        if line == mark {
            held.clear();
            skip = true;
            continue;
        }
        if skip && line.trim_start().strip_prefix("source").is_some_and(|r| r.trim_start().starts_with('=')) {
            skip = false;
            continue;
        }
        skip = false;
        if line.trim().is_empty() {
            held.push_str(line);
            held.push('\n');
            continue;
        }
        out.push_str(&held);
        held.clear();
        out.push_str(line);
        out.push('\n');
    }
    let _ = std::fs::write(&conf, out);
}

/// Where face-unlock's lock screen has to be started from: the user's key and
/// idle lock, which are theirs to change.
pub fn own_lock_hint() -> Hint {
    match desktop() {
        Desktop::Hyprland => {
            let lua = paths::xdg_config().join("hypr/hyprland.lua").is_file();
            let conf = if lua { "~/.config/hypr/hyprland.lua" } else { "~/.config/hypr/hyprland.conf" };
            Hint {
                note: trf("To lock with it, add this to %s, and set %s in %s:", &[&conf, &format!("lock_cmd = {NAME} lock"), &"hypridle.conf"]),
                code: vec![if lua {
                    format!("hl.bind(\"SUPER + L\", hl.dsp.exec_cmd(\"{NAME} lock\"))")
                } else {
                    format!("bind = SUPER, L, exec, {NAME} lock")
                }],
            }
        }
        Desktop::Niri => Hint {
            note: trf("To lock with it, add this to the binds in %s, and use %s in swayidle:", &[&"~/.config/niri/config.kdl", &format!("{NAME} lock")]),
            code: vec![format!("Mod+Alt+L {{ spawn \"{NAME}\" \"lock\"; }}")],
        },
        _ => Hint { note: trf("To lock with it, run %s where your lock screen is started now.", &[&format!("{NAME} lock")]), code: vec![] },
    }
}

/// The connected screens by output name (DP-1), from the kernel, which names
/// them as the desktops do.
pub fn screens() -> Vec<String> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir("/sys/class/drm") else { return out };
    let mut names: Vec<String> = dir.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    for name in names {
        let Some(rest) = name.strip_prefix("card") else { continue };
        let Some((num, output)) = rest.split_once('-') else { continue };
        if num.is_empty() || !num.bytes().all(|b| b.is_ascii_digit()) || output.starts_with("Writeback-") {
            continue;
        }
        let status = std::fs::read_to_string(format!("/sys/class/drm/{name}/status")).unwrap_or_default();
        if status.trim() == "connected" && !out.contains(&output.to_string()) {
            out.push(output.to_string());
        }
    }
    out
}

/// Whether a graphical session is around to open a window in.
pub fn has_display() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some()
}
