//! The few things that need root, and how the rest gets them done.
//!
//! The command and the window run as the user. When they need root they run
//! the command with `--root` and one verb: through sudo (or run0,
//! or doas) in a terminal, or through pkexec from the window, so the password
//! goes into the desktop's usual dialog. The verbs are all there is: there is
//! no way to hand the root side a command of one's own.
//!
//! ```text
//! --root set <Key> <Value>           one line of /etc/face-unlock/config
//! --root pam-enable <service>...     sudo, polkit-1, a lock screen, or
//! --root pam-disable <service>...    lockscreens for all lock screens here
//! --root socket-enable               start the daemon's socket, and at boot
//! --root socket-disable
//! --root migrate                     move over from plasma-face-unlock
//! --root polkit-agent-install        install a polkit agent
//! ```

use crate::i18n::{tr, trf};
use crate::pam::{LOCKERS, PamFiles, SERVICES};
use crate::paths;
use crate::system;
use std::io::IsTerminal;
use std::process::Command;

/// Carries out a verb. Only ever called as root.
pub fn verb(args: &[String]) -> Result<(), String> {
    if !paths::is_root() {
        return Err(tr("This command has to run as root.").into());
    }
    let verb = args.first().map(String::as_str).unwrap_or("");
    let rest = &args[args.len().min(1)..];
    match verb {
        "set" => {
            let (key, value) = match rest {
                [k, v] => (k.as_str(), v.as_str()),
                _ => ("", ""),
            };
            if !crate::config::system_value_ok(key, value) {
                return Err(trf("Not a valid setting: %s=%s", &[&key, &value]));
            }
            let file = paths::sysconfig();
            crate::keyvalue::set_in_file(&file, key, value, &format!("{} (system settings)", paths::PRETTY), 0o644).map_err(|e| e.to_string())?;
            let _ = std::fs::set_permissions(&file, std::os::unix::fs::PermissionsExt::from_mode(0o644));
            Ok(())
        }
        "pam-enable" | "pam-disable" => {
            let files = PamFiles::system();
            let mut services = Vec::new();
            for s in rest {
                if s == "lockscreens" {
                    services.extend(files.lockers().into_iter().map(String::from));
                } else if SERVICES.contains(&s.as_str()) || LOCKERS.contains(&s.as_str()) {
                    services.push(s.clone());
                } else {
                    return Err(trf("Not a service this can be used for: %s", &[s]));
                }
            }
            let mut errors = Vec::new();
            for s in &services {
                let r = if verb == "pam-enable" { files.enable(s) } else { files.disable(s) };
                if let Err(e) = r {
                    errors.push(e);
                }
            }
            if errors.is_empty() { Ok(()) } else { Err(errors.join("\n")) }
        }
        "socket-enable" => ok_or(system::systemctl(&["enable", "--now", paths::UNIT_SOCKET])),
        "socket-disable" => ok_or(system::systemctl(&["disable", "--now", paths::UNIT_SOCKET])),
        "migrate" => crate::migrate::system(&crate::migrate::Dirs::system()).map_err(|e| e.to_string()),
        "polkit-agent-install" => system::polkit_agent_install(),
        _ => Err(trf("Unknown command: %s", &[&verb])),
    }
}

fn ok_or(ok: bool) -> Result<(), String> {
    if ok { Ok(()) } else { Err(tr("Something went wrong.").into()) }
}

/// How root is asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    /// sudo, run0 or doas, in the terminal this runs in.
    Terminal,
    /// pkexec: the desktop's password dialog.
    Dialog,
}

/// The way that fits: the terminal when there is one, else the dialog.
pub fn ask() -> Ask {
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() { Ask::Terminal } else { Ask::Dialog }
}

/// Runs a verb as root: right here when this already is root, else through
/// `how`.
pub fn run(how: Ask, args: &[&str]) -> Result<(), String> {
    if paths::is_root() {
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        return verb(&owned);
    }
    let exe = paths::cli_exe();
    let mut cmd = match how {
        Ask::Terminal => {
            let Some(elevate) = ["sudo", "run0", "doas"].into_iter().find(|c| system::have(c)) else {
                return Err(tr("This needs root, and neither sudo, run0 nor doas is installed.").into());
            };
            Command::new(elevate)
        }
        Ask::Dialog => {
            if !system::have("pkexec") {
                return Err(tr("This needs root, and pkexec is not installed.").into());
            }
            Command::new("pkexec")
        }
    };
    cmd.arg(exe).arg("--root").args(args);
    if how == Ask::Dialog {
        // pkexec's answers, so a refused dialog can be told from a failure.
        let out = cmd.stdin(std::process::Stdio::null()).output().map_err(|e| e.to_string())?;
        return match out.status.code() {
            Some(0) => Ok(()),
            // Dismissed, or not allowed.
            Some(126) | Some(127) => {
                if system::polkit_agent_missing() {
                    Err(tr("No polkit agent is running, so no password window can open. Adding a face and the admin prompts need one.").into())
                } else {
                    Err(tr("Not allowed.").into())
                }
            }
            _ => {
                let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
                Err(if err.is_empty() { tr("Something went wrong.").into() } else { err })
            }
        };
    }
    match cmd.status() {
        Ok(s) if s.success() => Ok(()),
        Ok(_) => Err(tr("Could not save the setting.").into()),
        Err(e) => Err(e.to_string()),
    }
}
