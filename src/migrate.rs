//! Moving over from plasma-face-unlock, the old name.
//!
//! The packages run `face-unlock --root migrate` when they are installed.
//! The command and the window run it too when they find something left over,
//! for anybody who built it from source. This user's own settings need no
//! root and are moved every time.

use crate::pam::{MARK, PamFiles, SERVICES, WRAPPER_MARK};
use crate::paths;
use std::path::PathBuf;

const OLD_NAME: &str = "plasma-face-unlock";
const OLD_MARK: &str = "# plasma-face-unlock: the face first, the password if that does not work";
const OLD_WRAPPER_MARK: &str = "# Written by plasma-face-unlock.";
const OLD_MODULE: &str = "pam_plasma_face_unlock.so";

/// Where the old and the new things are. Moved only for the tests.
pub struct Dirs {
    pub old_sys: PathBuf,
    pub old_state: PathBuf,
    pub state: PathBuf,
    pub sysconfig: PathBuf,
    pub systemd_etc: PathBuf,
    pub pam: PamFiles,
    /// systemctl, or a stand-in that writes down what it was asked.
    pub systemctl: fn(&[&str]) -> bool,
}

impl Dirs {
    pub fn system() -> Dirs {
        Dirs {
            old_sys: format!("/etc/{OLD_NAME}").into(),
            old_state: format!("/var/lib/{OLD_NAME}").into(),
            state: paths::statedir(),
            sysconfig: paths::sysconfig(),
            systemd_etc: "/etc/systemd/system".into(),
            pam: PamFiles::system(),
            systemctl: crate::system::systemctl,
        }
    }

    fn old_socket_link(&self) -> PathBuf {
        self.systemd_etc.join(format!("sockets.target.wants/{OLD_NAME}d.socket"))
    }

    fn pam_has_old(&self, service: &str) -> bool {
        std::fs::read_to_string(self.pam.etc_file(service)).is_ok_and(|t| t.contains(OLD_MODULE))
    }
}

/// Root side: the system settings, the faces, sudo and admin prompts and the
/// daemon's socket, as plasma-face-unlock left them.
pub fn system(d: &Dirs) -> std::io::Result<()> {
    let mut used = false;

    if d.old_sys.is_dir() {
        let old = d.old_sys.join("config");
        if old.is_file() && !d.sysconfig.exists() {
            if let Some(parent) = d.sysconfig.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&old, &d.sysconfig)?;
            std::fs::set_permissions(&d.sysconfig, std::os::unix::fs::PermissionsExt::from_mode(0o644))?;
        }
        std::fs::remove_dir_all(&d.old_sys)?;
    }

    if d.old_state.is_dir() {
        // Faces already set up under the new name win.
        let old_users = d.old_state.join("users");
        let new_users = d.state.join("users");
        let new_has_faces = std::fs::read_dir(&new_users).is_ok_and(|mut r| r.next().is_some());
        if old_users.is_dir() && !new_has_faces {
            std::fs::create_dir_all(&d.state)?;
            std::fs::set_permissions(&d.state, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
            let _ = std::fs::remove_dir_all(&new_users);
            std::fs::rename(&old_users, &new_users)?;
            used = true;
        }
        std::fs::remove_dir_all(&d.old_state)?;
    }

    for service in SERVICES {
        if !d.pam_has_old(service) {
            continue;
        }
        let file = d.pam.etc_file(service);
        let text = std::fs::read_to_string(&file)?;
        if text.lines().take(3).any(|l| l.contains(OLD_WRAPPER_MARK)) {
            std::fs::remove_file(&file)?;
        } else {
            let mode = std::fs::metadata(&file).map(|m| std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o7777).unwrap_or(0o644);
            crate::keyvalue::replace_file(&file, PamFiles::remove_lines(&text, OLD_MARK, OLD_MODULE).as_bytes(), mode)?;
        }
        let _ = d.pam.enable(service);
        used = true;
    }

    let link = d.old_socket_link();
    if link.symlink_metadata().is_ok() {
        (d.systemctl)(&["disable", "--now", &format!("{OLD_NAME}d.socket")]);
        let _ = std::fs::remove_file(&link);
        used = true;
    }

    // The old package switches its socket off when it is removed, mostly
    // before this runs. So its faces and PAM lines count as the sign that it
    // was used.
    if used {
        (d.systemctl)(&["enable", "--now", paths::UNIT_SOCKET]);
    }
    let _ = (MARK, WRAPPER_MARK);
    Ok(())
}

/// Whether something needs `system`.
pub fn pending(d: &Dirs) -> bool {
    d.old_sys.is_dir() || d.old_state.is_dir() || d.old_socket_link().symlink_metadata().is_ok() || SERVICES.iter().any(|s| d.pam_has_old(s))
}

/// This user's settings and the agent's service.
pub fn user() {
    let xdg = paths::xdg_config();
    let old = xdg.join(OLD_NAME);
    let new = xdg.join(paths::NAME);
    if old.is_dir() {
        if new.exists() {
            let _ = std::fs::remove_dir_all(&old);
        } else {
            let _ = std::fs::rename(&old, &new);
        }
    }
    let link = xdg.join(format!("systemd/user/graphical-session.target.wants/{OLD_NAME}-agent.service"));
    if link.symlink_metadata().is_ok() {
        crate::system::systemctl_user(&["disable", "--now", &format!("{OLD_NAME}-agent.service")]);
        let _ = std::fs::remove_file(&link);
        if crate::system::agent_available() {
            crate::system::agent_enable();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static CALLS: Mutex<Vec<String>> = Mutex::new(Vec::new());

    fn fake_systemctl(args: &[&str]) -> bool {
        CALLS.lock().unwrap().push(args.join(" "));
        true
    }

    fn make_old(path: &std::path::Path, module: &std::path::Path) {
        let text = std::fs::read_to_string(path).unwrap();
        let text = text.replace(MARK, OLD_MARK).replace(WRAPPER_MARK, OLD_WRAPPER_MARK).replace(&module.display().to_string(), &format!("/usr/lib/security/{OLD_MODULE}"));
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn moves_everything_over() {
        let (tmp, pam) = crate::pam::tests::sandbox("migrate");
        let d = Dirs {
            old_sys: tmp.join("old-etc"),
            old_state: tmp.join("old-state"),
            state: tmp.join("state"),
            sysconfig: tmp.join("new-etc/config"),
            systemd_etc: tmp.join("systemd"),
            pam,
            systemctl: fake_systemctl,
        };
        let sudo = "#%PAM-1.0\nauth\t\tinclude\t\tsystem-auth\naccount\t\tinclude\t\tsystem-auth\n";
        let file = d.pam.etc_file("sudo");
        std::fs::write(&file, sudo).unwrap();
        d.pam.enable("sudo").unwrap();
        make_old(&file, &d.pam.module);
        assert!(d.pam_has_old("sudo") && !d.pam.enabled("sudo") && pending(&d));
        system(&d).unwrap();
        assert!(d.pam.enabled("sudo") && !d.pam_has_old("sudo"));
        assert!(!std::fs::read_to_string(&file).unwrap().contains(OLD_MARK));
        assert!(!pending(&d));
        d.pam.disable("sudo").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), sudo);

        std::fs::create_dir_all(d.old_sys.clone()).unwrap();
        std::fs::create_dir_all(d.old_state.join("users")).unwrap();
        std::fs::write(d.old_sys.join("config"), "Liveness=heavy\n").unwrap();
        std::fs::write(d.old_state.join("users/1000.json"), "{}").unwrap();
        CALLS.lock().unwrap().clear();
        system(&d).unwrap();
        assert_eq!(std::fs::read_to_string(&d.sysconfig).unwrap(), "Liveness=heavy\n");
        assert!(!d.old_sys.exists() && !d.old_state.exists());
        assert_eq!(std::fs::read_to_string(d.state.join("users/1000.json")).unwrap(), "{}");
        assert!(CALLS.lock().unwrap().contains(&"enable --now face-unlockd.socket".to_string()));

        std::fs::create_dir_all(d.old_state.join("users")).unwrap();
        std::fs::write(d.old_state.join("users/1000.json"), "old").unwrap();
        CALLS.lock().unwrap().clear();
        system(&d).unwrap();
        assert_eq!(std::fs::read_to_string(d.state.join("users/1000.json")).unwrap(), "{}");
        assert!(CALLS.lock().unwrap().is_empty());
        assert!(!pending(&d));
        let _ = std::fs::remove_dir_all(tmp);
    }
}
