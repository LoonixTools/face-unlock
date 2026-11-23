//! Where everything lives.
//!
//! The install folders come from the Makefile at build time. For trying things
//! out without installing, most can be moved with an environment variable,
//! but only for a process that does not run as root: run as root, only what
//! was installed counts.

use std::path::PathBuf;

pub const NAME: &str = "face-unlock";
pub const PRETTY: &str = "Face Unlock";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The daemon's socket, as its systemd unit makes it.
pub const SOCKET: &str = "/run/face-unlock/socket";
/// Face data and what the daemon remembers about each user.
pub const STATEDIR: &str = "/var/lib/face-unlock";
/// The system settings. Only root writes them.
pub const SYSCONFIG: &str = "/etc/face-unlock/config";

pub const UNIT_SOCKET: &str = "face-unlockd.socket";
pub const UNIT_AGENT: &str = "face-unlock-agent.service";

/// The polkit action the daemon asks for before faces change.
pub const ACTION_MANAGE: &str = "io.github.loonixtools.face-unlock.manage";

const LIBEXECDIR: &str = match option_env!("FU_LIBEXECDIR") {
    Some(d) => d,
    None => "/usr/lib/face-unlock",
};
const MODELDIR: &str = match option_env!("FU_MODELDIR") {
    Some(d) => d,
    None => "/usr/share/face-unlock/models",
};
const PAMDIR: &str = match option_env!("FU_PAMDIR") {
    Some(d) => d,
    None => "/usr/lib/security",
};
const BINDIR: &str = match option_env!("FU_BINDIR") {
    Some(d) => d,
    None => "/usr/bin",
};

pub fn is_root() -> bool {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() == 0 }
}

/// An environment variable that may move a path: set, not empty, and not
/// in a process that runs as root.
pub fn env_path(var: &str) -> Option<PathBuf> {
    if is_root() {
        return None;
    }
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from)
}

pub fn socket() -> PathBuf {
    env_path("FU_SOCKET").unwrap_or_else(|| SOCKET.into())
}

pub fn libexec() -> PathBuf {
    env_path("FU_LIBEXECDIR").unwrap_or_else(|| LIBEXECDIR.into())
}

pub fn models() -> PathBuf {
    env_path("FU_MODELDIR").unwrap_or_else(|| MODELDIR.into())
}

pub fn agent_exe() -> PathBuf {
    env_path("FU_AGENT").unwrap_or_else(|| libexec().join("face-unlock-agent"))
}

pub fn daemon_exe() -> PathBuf {
    libexec().join("face-unlockd")
}

/// The window, next to this program when it runs from a build folder.
pub fn gui_exe() -> PathBuf {
    if let Some(p) = env_path("FU_GUI") {
        return p;
    }
    let beside = self_exe().with_file_name("face-unlock-gui");
    if beside.exists() { beside } else { PathBuf::from(BINDIR).join("face-unlock-gui") }
}

/// The installed command.
pub fn installed_cli() -> PathBuf {
    PathBuf::from(BINDIR).join(NAME)
}

/// The command, which carries out the root tasks: this program when it is the
/// command, else the one next to it in a build folder, else the installed one.
/// Never the window: it does not know `--root`.
pub fn cli_exe() -> PathBuf {
    let me = self_exe();
    if me.file_name().is_some_and(|n| n == NAME) {
        return me;
    }
    let beside = me.with_file_name(NAME);
    if beside.exists() { beside } else { installed_cli() }
}

pub fn pam_module() -> PathBuf {
    env_path("FU_PAM_MODULE").unwrap_or_else(|| PathBuf::from(PAMDIR).join("pam_face_unlock.so"))
}

pub fn sysconfig() -> PathBuf {
    env_path("FU_SYSCONFIG").unwrap_or_else(|| SYSCONFIG.into())
}

pub fn statedir() -> PathBuf {
    env_path("FU_STATEDIR").unwrap_or_else(|| STATEDIR.into())
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from).unwrap_or_else(|| "/".into())
}

/// ~/.config, or wherever XDG_CONFIG_HOME points.
pub fn xdg_config() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| home().join(".config"))
}

/// This user's settings.
pub fn user_config() -> PathBuf {
    if let Some(p) = env_path("FU_CONFIG") {
        return p;
    }
    xdg_config().join(NAME).join("config")
}

/// $XDG_RUNTIME_DIR/face-unlock: the agent's socket and what it leaves for
/// lock screens to read.
pub fn runtime() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| format!("/run/user/{}", unsafe { libc::getuid() }).into());
    base.join(NAME)
}

pub fn agent_socket() -> PathBuf {
    runtime().join("agent.socket")
}

/// This program. An update can replace the binary while it runs; then the
/// new one at the same path is meant.
pub fn self_exe() -> PathBuf {
    match std::env::current_exe() {
        Ok(p) => {
            let s = p.to_string_lossy();
            match s.strip_suffix(" (deleted)") {
                Some(rest) => PathBuf::from(rest),
                None => p,
            }
        }
        Err(_) => installed_cli(),
    }
}

/// "~/Pictures/x.png" for a path under the home folder.
pub fn tilde(path: &std::path::Path) -> String {
    match path.strip_prefix(home()) {
        Ok(rest) if home() != std::path::Path::new("/") => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}

/// The other way round, for a path somebody typed.
pub fn untilde(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None if path == "~" => home(),
        None => PathBuf::from(path),
    }
}
