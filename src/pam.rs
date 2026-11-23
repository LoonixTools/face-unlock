//! Putting face unlock in front of sudo, polkit's admin prompts and the lock
//! screens of Hyprland and Niri, and taking it out again.
//!
//! The lock screens of Plasma and GNOME do not go through this at all: the
//! agent opens them. hyprlock, swaylock and the others are programs of their
//! own that only open themselves, so the face goes into their password check,
//! and pressing Enter on the empty field is how a scan starts there. The login
//! screen stays with the password: logging in is what unlocks the wallet, and
//! a face has no password to hand it.
//!
//! The line that goes in:
//!
//! ```text
//! -auth  sufficient  /usr/lib/security/pam_face_unlock.so
//! ```
//!
//! with "lockscreen" after it for a lock screen. "sufficient": a match lets
//! the person in, anything else falls through to the lines below as if this
//! one were not there. The dash makes PAM skip it quietly if the module ever
//! goes missing. sudo keeps working either way.
//!
//! Where the service's file is:
//! - /etc/pam.d/<service> exists: the line goes in before its first auth
//!   line, with a comment saying where it came from.
//! - only the distribution's copy in /usr/lib/pam.d exists (Arch keeps
//!   polkit-1 there): a small /etc/pam.d/<service> is written that puts the
//!   line first and includes the distribution's file for everything else.
//!
//! Undoing takes out exactly those lines, or that file.

use crate::keyvalue::replace_file;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub const MARK: &str = "# face-unlock: the face first, the password if that does not work";
pub const WRAPPER_MARK: &str = "# Written by face-unlock.";
pub const SERVICES: &[&str] = &["sudo", "polkit-1"];
/// Lock screens with a PAM file of their own. One that uses "login" or
/// "system-auth" directly is left alone: those also let people log in.
pub const LOCKERS: &[&str] = &["hyprlock", "swaylock", "gtklock", "waylock"];

pub struct PamFiles {
    pub etc: PathBuf,
    pub vendor: Vec<PathBuf>,
    pub module: PathBuf,
}

impl PamFiles {
    /// The real ones. The folders can only be moved for tests, never as root.
    pub fn system() -> PamFiles {
        let vendor = crate::paths::env_path("FU_PAM_VENDOR_DIRS")
            .map(|v| v.to_string_lossy().split_whitespace().map(PathBuf::from).collect())
            .unwrap_or_else(|| ["/usr/lib/pam.d", "/usr/share/pam.d", "/lib/pam.d"].iter().map(PathBuf::from).collect());
        PamFiles {
            etc: crate::paths::env_path("FU_PAM_ETC_DIR").unwrap_or_else(|| "/etc/pam.d".into()),
            vendor,
            module: crate::paths::pam_module(),
        }
    }

    pub fn etc_file(&self, service: &str) -> PathBuf {
        self.etc.join(service)
    }

    /// The distribution's own copy, when it keeps one outside /etc.
    pub fn vendor_file(&self, service: &str) -> Option<PathBuf> {
        self.vendor.iter().map(|d| d.join(service)).find(|p| p.is_file())
    }

    /// Whether the service has a PAM file here at all.
    pub fn available(&self, service: &str) -> bool {
        self.etc_file(service).is_file() || self.vendor_file(service).is_some()
    }

    pub fn enabled(&self, service: &str) -> bool {
        std::fs::read_to_string(self.etc_file(service)).is_ok_and(|t| t.contains("pam_face_unlock.so"))
    }

    /// The lock screens installed here.
    pub fn lockers(&self) -> Vec<&'static str> {
        LOCKERS.iter().copied().filter(|s| self.available(s)).collect()
    }

    /// Whether every lock screen installed here takes the face, and there is
    /// one.
    pub fn lockers_enabled(&self) -> bool {
        let l = self.lockers();
        !l.is_empty() && l.iter().all(|s| self.enabled(s))
    }

    pub fn line(&self, service: &str) -> String {
        if LOCKERS.contains(&service) {
            format!("-auth       sufficient   {} lockscreen", self.module.display())
        } else {
            format!("-auth       sufficient   {}", self.module.display())
        }
    }

    /// The file with the line added before its first auth line. Debian and
    /// Ubuntu have none in sudo's file, only "@include common-auth", and that
    /// counts as one. A file with neither gets it at the end.
    pub fn insert(&self, text: &str, service: &str) -> String {
        let mut out = String::new();
        let mut done = false;
        for line in text.lines() {
            let t = line.trim_start();
            let is_auth = {
                let t2 = t.strip_prefix('-').unwrap_or(t);
                t2.strip_prefix("auth").is_some_and(|r| r.starts_with([' ', '\t']))
            };
            let is_include = t.strip_prefix("@include").is_some_and(|r| {
                r.starts_with([' ', '\t']) && r.split_whitespace().next() == Some("common-auth")
            });
            if !done && (is_auth || is_include) {
                out.push_str(MARK);
                out.push('\n');
                out.push_str(&self.line(service));
                out.push('\n');
                done = true;
            }
            out.push_str(line);
            out.push('\n');
        }
        if !done {
            out.push_str(MARK);
            out.push('\n');
            out.push_str(&self.line(service));
            out.push('\n');
        }
        out
    }

    /// The file without our comment and our line.
    pub fn remove_lines(text: &str, mark: &str, module: &str) -> String {
        let mut out = String::new();
        for line in text.lines() {
            if line == mark || line.contains(module) {
                continue;
            }
            out.push_str(line);
            out.push('\n');
        }
        out
    }

    pub fn wrapper(&self, vendor: &Path, service: &str) -> String {
        let v = vendor.display();
        format!(
            "#%PAM-1.0\n{WRAPPER_MARK} The face first, then everything\n# {v} does for this service. Removed again by\n# \"face-unlock disable\" or from its settings.\n\n{}\nauth       include      {v}\naccount    include      {v}\npassword   include      {v}\nsession    include      {v}\n",
            self.line(service)
        )
    }

    /// Writes the new file next to the old one and swaps it in, with the old
    /// one's owner and mode. A half-written PAM file is a locked-out machine.
    fn replace(&self, file: &Path, content: &str) -> std::io::Result<()> {
        let old = std::fs::metadata(file).ok();
        let mode = old.as_ref().map(|m| m.permissions().mode() & 0o7777).unwrap_or(0o644);
        replace_file(file, content.as_bytes(), mode)?;
        if let Some(m) = old {
            let _ = std::os::unix::fs::chown(file, Some(m.uid()), Some(m.gid()));
        }
        Ok(())
    }

    /// Root side.
    pub fn enable(&self, service: &str) -> Result<(), String> {
        if !self.module.exists() {
            return Err(crate::i18n::trf("The PAM module is not installed at %s.", &[&self.module.display()]));
        }
        if self.enabled(service) {
            return Ok(());
        }
        let file = self.etc_file(service);
        let content = if file.is_file() {
            let text = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
            self.insert(&text, service)
        } else if let Some(vendor) = self.vendor_file(service) {
            self.wrapper(&vendor, service)
        } else {
            return Err(crate::i18n::trf("There is no PAM configuration for %s on this system.", &[&service]));
        };
        self.replace(&file, &content).map_err(|e| e.to_string())
    }

    /// Root side.
    pub fn disable(&self, service: &str) -> Result<(), String> {
        if !self.enabled(service) {
            return Ok(());
        }
        let file = self.etc_file(service);
        let text = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
        if text.lines().take(3).any(|l| l.contains(WRAPPER_MARK)) {
            // Ours from the first line to the last. Without it the
            // distribution's own copy is in charge again.
            return std::fs::remove_file(&file).map_err(|e| e.to_string());
        }
        self.replace(&file, &Self::remove_lines(&text, MARK, "pam_face_unlock.so")).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const ARCH_SUDO: &str = "#%PAM-1.0\nauth\t\tinclude\t\tsystem-auth\naccount\t\tinclude\t\tsystem-auth\nsession\t\tinclude\t\tsystem-auth\nsession\t\toptional\tpam_systemd.so class=none\n";
    const DEBIAN_SUDO: &str = "#%PAM-1.0\n\n# Set up user limits from /etc/security/limits.conf.\nsession    required   pam_limits.so\n\nsession    required   pam_env.so readenv=1 user_readenv=0\nsession    required   pam_env.so readenv=1 envfile=/etc/default/locale user_readenv=0\n@include common-auth\n@include common-account\n@include common-session-noninteractive\n";
    const FEDORA_SUDO: &str = "#%PAM-1.0\nauth       include      system-auth\naccount    include      system-auth\npassword   include      system-auth\nsession    optional     pam_keyinit.so revoke\nsession    required     pam_limits.so\nsession    include      system-auth\n";
    const ARCH_POLKIT: &str = "#%PAM-1.0\n\nauth       include      system-auth\naccount    include      system-auth\npassword   include      system-auth\nsession    include      system-auth\n";

    pub(crate) fn sandbox(name: &str) -> (PathBuf, PamFiles) {
        let tmp = std::env::temp_dir().join(format!("fu-pam-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        for d in ["etc", "vendor", "lib"] {
            std::fs::create_dir_all(tmp.join(d)).unwrap();
        }
        let module = tmp.join("lib/pam_face_unlock.so");
        std::fs::write(&module, "").unwrap();
        let files = PamFiles { etc: tmp.join("etc"), vendor: vec![tmp.join("vendor")], module };
        (tmp, files)
    }

    fn first_auth(path: &Path) -> String {
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .find(|l| {
                let t = l.trim_start();
                let t2 = t.strip_prefix('-').unwrap_or(t);
                t2.starts_with("auth ") || t2.starts_with("auth\t") || t.starts_with("@include common-auth")
            })
            .unwrap_or("")
            .to_string()
    }

    #[test]
    fn sudo_layouts() {
        for (name, layout) in [("arch", ARCH_SUDO), ("debian", DEBIAN_SUDO), ("fedora", FEDORA_SUDO)] {
            let (tmp, p) = sandbox(name);
            let file = p.etc_file("sudo");
            std::fs::write(&file, layout).unwrap();
            p.enable("sudo").unwrap();
            assert!(p.enabled("sudo"), "{name}");
            assert!(first_auth(&file).contains("pam_face_unlock.so"), "{name}: face first");
            let text = std::fs::read_to_string(&file).unwrap();
            assert!(text.lines().any(|l| l == format!("-auth       sufficient   {}", p.module.display())), "{name}");
            assert_eq!(text.lines().next(), Some("#%PAM-1.0"));
            p.enable("sudo").unwrap();
            assert_eq!(std::fs::read_to_string(&file).unwrap().matches("pam_face_unlock").count(), 1);
            p.disable("sudo").unwrap();
            assert_eq!(std::fs::read_to_string(&file).unwrap(), layout, "{name}: the same file back");
            assert!(!p.enabled("sudo"));
            let _ = std::fs::remove_dir_all(tmp);
        }
    }

    #[test]
    fn vendor_polkit() {
        let (tmp, p) = sandbox("polkit");
        let vendor = tmp.join("vendor/polkit-1");
        std::fs::write(&vendor, ARCH_POLKIT).unwrap();
        assert!(p.available("polkit-1"));
        p.enable("polkit-1").unwrap();
        let ours = std::fs::read_to_string(p.etc_file("polkit-1")).unwrap();
        assert!(ours.contains(WRAPPER_MARK));
        assert!(ours.lines().any(|l| l == format!("auth       include      {}", vendor.display())));
        assert!(first_auth(&p.etc_file("polkit-1")).contains("pam_face_unlock.so"));
        assert_eq!(std::fs::read_to_string(&vendor).unwrap(), ARCH_POLKIT);
        p.disable("polkit-1").unwrap();
        assert!(!p.etc_file("polkit-1").exists());
        assert!(!p.available("nosuchservice"));
        assert!(p.enable("nosuchservice").is_err());
        let _ = std::fs::remove_dir_all(tmp);
    }

    #[test]
    fn lock_screens() {
        let (tmp, p) = sandbox("lockers");
        let hyprlock = "#%PAM-1.0\n\nauth        include     login\n";
        std::fs::write(p.etc_file("hyprlock"), hyprlock).unwrap();
        std::fs::write(tmp.join("vendor/swaylock"), "auth include login\n").unwrap();
        assert_eq!(p.lockers(), vec!["hyprlock", "swaylock"]);
        assert!(!p.lockers_enabled());
        for l in p.lockers() {
            p.enable(l).unwrap();
        }
        assert!(p.lockers_enabled());
        assert!(first_auth(&p.etc_file("hyprlock")).ends_with("pam_face_unlock.so lockscreen"));
        let sway = std::fs::read_to_string(p.etc_file("swaylock")).unwrap();
        assert!(sway.contains(WRAPPER_MARK) && first_auth(&p.etc_file("swaylock")).ends_with("lockscreen"));
        assert!(!p.line("sudo").contains("lockscreen"));
        p.disable("hyprlock").unwrap();
        p.disable("swaylock").unwrap();
        assert_eq!(std::fs::read_to_string(p.etc_file("hyprlock")).unwrap(), hyprlock);
        assert!(!p.etc_file("swaylock").exists());
        assert!(!p.lockers_enabled());
        let _ = std::fs::remove_dir_all(tmp);
    }
}
