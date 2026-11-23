//! What turning face unlock on and off does, for the command and the window
//! alike. Each step says how it went as a list of notices.

use crate::client;
use crate::config::{self, UserConfig};
use crate::i18n::{tr, trf};
use crate::pam::PamFiles;
use crate::root::{self, Ask};
use crate::system::{self, Desktop, GnomeExtension, HyprText};

#[derive(Clone, Debug, PartialEq)]
pub enum Notice {
    Ok(String),
    Bad(String),
    Note(String),
    /// A line to copy, under a note.
    Code(String),
}

pub type Notices = Vec<Notice>;

fn hint(out: &mut Notices, h: system::Hint) {
    out.push(Notice::Note(h.note));
    out.extend(h.code.into_iter().map(Notice::Code));
}

/// The daemon is started by its socket. Switching the socket on is the one
/// thing that needs root once per machine.
pub fn ensure_service(how: Ask) -> Result<(), String> {
    if client::status().is_some() {
        return Ok(());
    }
    if !system::socket_enabled() {
        root::run(how, &["socket-enable"])?;
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    if client::status().is_some() {
        return Ok(());
    }
    Err(format!("{} {}", crate::text::reason("unreachable"), trf("Check it with: systemctl status %s", &[&crate::paths::UNIT_SOCKET])))
}

/// The services face unlock should be in front of, from this user's
/// settings, that are not yet.
pub fn pam_wanted(cfg: &UserConfig) -> Vec<&'static str> {
    let files = PamFiles::system();
    let mut out = Vec::new();
    if cfg.sudo && files.available("sudo") && !files.enabled("sudo") {
        out.push("sudo");
    }
    if cfg.polkit && files.available("polkit-1") && !files.enabled("polkit-1") {
        out.push("polkit-1");
    }
    if lockers_here() && cfg.lock_screens && !files.lockers_enabled() {
        out.push("lockscreens");
    }
    out
}

/// Whether this desktop's lock screen is a program of its own and one with a
/// PAM file is installed.
pub fn lockers_here() -> bool {
    system::desktop().own_locker() && !PamFiles::system().lockers().is_empty()
}

/// Turns it on, once a face is set up: the switch, the agent, sudo and the
/// admin prompts, GNOME's extension.
pub fn enable(how: Ask) -> Notices {
    let mut out = Notices::new();
    if config::set_user("Enabled", "yes").is_err() {
        out.push(Notice::Bad(tr("Could not save the setting.").into()));
        return out;
    }
    let cfg = UserConfig::load();

    if system::agent_available() {
        if !system::agent_enable() {
            out.push(Notice::Bad(tr("Could not start the lock screen agent.").into()));
        }
        if !system::agent_autostarts() {
            hint(&mut out, system::hyprland_start_hint(crate::paths::UNIT_AGENT));
        }
    } else {
        out.push(Notice::Note(tr("No systemd user session, so the lock screen agent was not started.").into()));
    }

    let wanted = pam_wanted(&cfg);
    if !wanted.is_empty() {
        let mut args = vec!["pam-enable"];
        args.extend(wanted);
        if let Err(e) = root::run(how, &args) {
            out.push(Notice::Bad(e));
        }
    }

    if system::desktop() == Desktop::Gnome {
        match system::gnome_extension_enable() {
            GnomeExtension::On => {}
            GnomeExtension::NextLogin => out.push(Notice::Note(tr("Log out and back in once: then GNOME shows the bubble too.").into())),
            GnomeExtension::Failed => out.push(Notice::Bad(tr("Could not switch on the GNOME extension that shows the bubble.").into())),
        }
    }

    let files = PamFiles::system();
    if lockers_here() && files.lockers_enabled() && cfg.lock_screen_style != "own" {
        out.push(Notice::Note(trf(
            "The lock screen (%s) takes your face too. If it does not scan when you come back, press Enter on the empty password field.",
            &[&files.lockers().join(", ")],
        )));
    }

    out.push(Notice::Ok(tr("Face unlock is on. Lock the screen and look at it to try.").into()));
    if !cfg.sudo && !cfg.polkit {
        out.push(Notice::Note(tr("It can do sudo and admin prompts too. See Settings.").into()));
    }
    out
}

pub fn disable(how: Ask) -> Notices {
    let mut out = Notices::new();
    if config::set_user("Enabled", "no").is_err() {
        out.push(Notice::Bad(tr("Could not save the setting.").into()));
        return out;
    }
    if system::agent_available() {
        system::agent_disable();
    }
    if system::desktop() == Desktop::Gnome {
        system::gnome_extension_disable();
    }
    let files = PamFiles::system();
    let on: Vec<&str> = crate::pam::SERVICES.iter().chain(crate::pam::LOCKERS).copied().filter(|s| files.enabled(s)).collect();
    if !on.is_empty() {
        let mut args = vec!["pam-disable"];
        args.extend(on);
        if let Err(e) = root::run(how, &args) {
            out.push(Notice::Bad(e));
        }
    }
    out.push(Notice::Ok(tr("Face unlock is off. Your faces are kept; delete them under Faces.").into()));
    out
}

/// Turns one PAM service (or "lockscreens") on or off, and remembers it so
/// that turning face unlock off and on again brings it back.
pub fn set_pam(how: Ask, service: &str, on: bool) -> Result<(), String> {
    root::run(how, &[if on { "pam-enable" } else { "pam-disable" }, service])?;
    let key = match service {
        "sudo" => "Sudo",
        "polkit-1" => "Polkit",
        _ => "LockScreens",
    };
    config::set_user(key, if on { "yes" } else { "no" }).map_err(|e| e.to_string())
}

/// face-unlock's own lock screen, or the user's own with a line of text.
pub fn lock_style_set(style: &str) -> Notices {
    let mut out = Notices::new();
    match style {
        "own" => {
            if config::set_user("LockScreenStyle", "own").is_err() {
                out.push(Notice::Bad(tr("Could not save the setting.").into()));
                return out;
            }
            system::hyprlock_text_off();
            out.push(Notice::Ok(tr("You lock with face-unlock's lock screen now.").into()));
            hint(&mut out, system::own_lock_hint());
        }
        _ => {
            if config::set_user("LockScreenStyle", "yours").is_err() {
                out.push(Notice::Bad(tr("Could not save the setting.").into()));
                return out;
            }
            let conf = "~/.config/hypr/hyprlock.conf";
            out.push(match system::hyprlock_text_on() {
                HyprText::Done => Notice::Ok(tr("hyprlock shows what face unlock is doing at the top, from the next time it locks.").into()),
                HyprText::Failed => Notice::Bad(trf("Could not add the line to %s.", &[&conf])),
                HyprText::NoHyprlock => Notice::Ok(tr("Your lock screen stays as it is. Press Enter on the empty password field to scan.").into()),
                HyprText::NoConfig => Notice::Note(trf("hyprlock has no %s, so it cannot show the text.", &[&conf])),
            });
        }
    }
    out
}
