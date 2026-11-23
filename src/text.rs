//! Words the command and the window share: what a daemon answer means, how
//! long ago something was.

use crate::i18n::{tr, trf};

/// What a daemon answer means, for people.
pub fn reason(reason: &str) -> String {
    match reason {
        "mismatch" => tr("The face did not match.").into(),
        "spoof" => tr("That looked like a photo or a screen.").into(),
        "liveness" => tr("The face matched, but did not blink or move.").into(),
        "attention" => tr("The face was not looking at the screen.").into(),
        "quality" => tr("The picture was too dark, too blurry or too far away.").into(),
        "no-face" => tr("No face in view.").into(),
        "lockout" => tr("Face unlock is paused after too many tries. Unlock once with your password.").into(),
        "not-enrolled" => tr("No face is set up yet.").into(),
        "camera" => tr("The camera could not be used.").into(),
        "camera-busy" => tr("Another app is using the camera.").into(),
        "models" => tr("The recognition models are missing. Reinstall the package.").into(),
        "lid-closed" => tr("The lid is closed.").into(),
        "busy" => tr("The camera is busy with another scan.").into(),
        "unreachable" => tr("The face unlock service is not running.").into(),
        "timeout" => tr("The setup took too long. Try again.").into(),
        "cancelled" => tr("Stopped.").into(),
        "denied" => {
            let mut s = tr("Not allowed.").to_string();
            // Hyprland, Niri and the like bring no polkit agent, so no
            // password window shows.
            if crate::system::desktop().own_locker() {
                s.push(' ');
                s.push_str(tr("No password window? Start a polkit agent, for example hyprpolkitagent."));
            }
            s
        }
        other => trf("Something went wrong (%s).", &[&other]),
    }
}

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// "5 minutes ago" for a unix time. 0 is "never".
pub fn time_ago(ts: i64) -> String {
    if ts <= 0 {
        return tr("never").into();
    }
    let delta = (now() - ts).max(0);
    match delta {
        d if d < 60 => tr("just now").into(),
        d if d < 120 => tr("1 minute ago").into(),
        d if d < 3600 => trf("%d minutes ago", &[&(d / 60)]),
        d if d < 7200 => tr("1 hour ago").into(),
        d if d < 86400 => trf("%d hours ago", &[&(d / 3600)]),
        d if d < 172800 => tr("1 day ago").into(),
        d => trf("%d days ago", &[&(d / 86400)]),
    }
}
