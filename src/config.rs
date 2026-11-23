//! The two settings files.
//!
//! ~/.config/face-unlock/config holds what this user wants from the lock
//! screen and the bubble. /etc/face-unlock/config holds what the daemon does
//! (which camera, how strict), and only root writes it: a setting a user
//! process could change would be one any program running as that user could
//! lower before asking sudo for help.

use crate::keyvalue::{KeyValue, parse_bool};
use crate::paths;

/// What this user wants.
#[derive(Clone, Debug, PartialEq)]
pub struct UserConfig {
    pub enabled: bool,
    pub lock_screen: bool,
    pub scan_on_wake: bool,
    pub scan_on_lock: bool,
    /// "own" or "yours"; empty until picked.
    pub lock_screen_style: String,
    /// Empty for the desktop's picture, "none", a file, or a folder.
    pub lock_wallpaper: String,
    pub lock_blur: bool,
    pub sudo: bool,
    pub polkit: bool,
    pub lock_screens: bool,
    pub bubble: bool,
    /// "full" or "minimal".
    pub bubble_style: String,
    /// "slow", "normal" or "fast".
    pub animation_speed: String,
    /// An output name (DP-1), or empty to follow the mouse.
    pub bubble_screen: String,
    pub bubble_for_prompts: bool,
    pub quiet_when_camera_busy: bool,
}

impl UserConfig {
    pub fn load() -> UserConfig {
        Self::from(&KeyValue::load(&paths::user_config()))
    }

    pub fn from(kv: &KeyValue) -> UserConfig {
        let screen = kv.get("BubbleScreen", "");
        UserConfig {
            enabled: kv.boolean("Enabled", false),
            lock_screen: kv.boolean("LockScreen", true),
            scan_on_wake: kv.boolean("ScanOnWake", true),
            scan_on_lock: kv.boolean("ScanOnLock", false),
            lock_screen_style: match kv.get("LockScreenStyle", "") {
                "own" => "own".into(),
                "" => String::new(),
                _ => "yours".into(),
            },
            lock_wallpaper: kv.get("LockWallpaper", "").into(),
            lock_blur: kv.boolean("LockBlur", false),
            sudo: kv.boolean("Sudo", true),
            polkit: kv.boolean("Polkit", true),
            lock_screens: kv.boolean("LockScreens", true),
            bubble: kv.boolean("Bubble", true),
            bubble_style: if kv.get("BubbleStyle", "full") == "minimal" { "minimal".into() } else { "full".into() },
            animation_speed: match kv.get("AnimationSpeed", "normal") {
                s @ ("slow" | "fast") => s.into(),
                _ => "normal".into(),
            },
            bubble_screen: if screen == "auto" { String::new() } else { screen.into() },
            bubble_for_prompts: kv.boolean("BubbleForPrompts", true),
            quiet_when_camera_busy: kv.boolean("QuietWhenCameraBusy", false),
        }
    }

    /// How long the bubble's animations take, as a multiple of their lengths
    /// in the code.
    pub fn pace(&self) -> f32 {
        match self.animation_speed.as_str() {
            "fast" => 1.0,
            "slow" => 2.0,
            _ => 1.3,
        }
    }
}

/// Writes one of this user's settings.
pub fn set_user(key: &str, value: &str) -> std::io::Result<()> {
    crate::keyvalue::set_in_file(&paths::user_config(), key, value, paths::PRETTY, 0o644)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Liveness {
    Off,
    /// Glare off a screen, the edge of a phone around the face.
    Light,
    /// And a sign of life on top: a blink, or the nose moving like a nose.
    Heavy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strictness {
    Relaxed,
    Normal,
    Strict,
}

/// What the daemon does. Read by everybody, written by root.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// A /dev/video path, "auto", or for testing "file:<video>" and
    /// "images:<directory>".
    pub camera: String,
    pub liveness: Liveness,
    pub strictness: Strictness,
    /// Only a face that looks at the screen with its eyes open counts.
    pub attention: bool,
    pub scan_seconds: u32,
    /// Failed scans in a row before face unlock pauses, and for how long.
    pub max_failures: u32,
    pub lockout_minutes: u32,
    pub skip_lid_closed: bool,
    /// sudo in an SSH session. Off: whoever sits at the camera need not be
    /// whoever types.
    pub ssh_sessions: bool,
    /// Take in a little of each confident unlock, like Face ID.
    pub adapt: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            camera: "auto".into(),
            liveness: Liveness::Light,
            strictness: Strictness::Normal,
            attention: true,
            scan_seconds: 5,
            max_failures: 5,
            lockout_minutes: 15,
            skip_lid_closed: true,
            ssh_sessions: false,
            adapt: true,
        }
    }
}

impl Settings {
    pub fn load(path: &std::path::Path) -> Settings {
        Self::from(&KeyValue::load(path))
    }

    pub fn from(kv: &KeyValue) -> Settings {
        let d = Settings::default();
        Settings {
            camera: kv.get("Camera", "auto").into(),
            liveness: match kv.get("Liveness", "").to_lowercase().as_str() {
                "off" => Liveness::Off,
                "heavy" => Liveness::Heavy,
                _ => Liveness::Light,
            },
            strictness: match kv.get("Strictness", "").to_lowercase().as_str() {
                "relaxed" => Strictness::Relaxed,
                "strict" => Strictness::Strict,
                _ => Strictness::Normal,
            },
            attention: kv.boolean("Attention", d.attention),
            scan_seconds: kv.integer("ScanSeconds", d.scan_seconds.into(), 2, 15) as u32,
            max_failures: kv.integer("MaxFailures", d.max_failures.into(), 1, 20) as u32,
            lockout_minutes: kv.integer("LockoutMinutes", d.lockout_minutes.into(), 1, 24 * 60) as u32,
            skip_lid_closed: kv.boolean("SkipLidClosed", d.skip_lid_closed),
            ssh_sessions: kv.boolean("SshSessions", d.ssh_sessions),
            adapt: kv.boolean("Adapt", d.adapt),
        }
    }

    /// Cosine similarity between two SFace embeddings. Photos of the same
    /// person years apart land around 0.75, different people below 0.3.
    /// OpenCV's own recommendation (0.363) is tuned for telling apart photos
    /// in a data set; this guards a login, so the bar is higher.
    pub fn threshold(&self) -> f32 {
        match self.strictness {
            Strictness::Relaxed => 0.42,
            Strictness::Normal => 0.50,
            Strictness::Strict => 0.58,
        }
    }

    pub fn liveness_name(&self) -> &'static str {
        match self.liveness {
            Liveness::Off => "off",
            Liveness::Light => "light",
            Liveness::Heavy => "heavy",
        }
    }

    pub fn strictness_name(&self) -> &'static str {
        match self.strictness {
            Strictness::Relaxed => "relaxed",
            Strictness::Normal => "normal",
            Strictness::Strict => "strict",
        }
    }
}

/// Whether root may write `value` for the system setting `key`. Anything
/// else is refused on the root side, whatever asked for it.
pub fn system_value_ok(key: &str, value: &str) -> bool {
    let num = |min: u32, max: u32| value.parse::<u32>().is_ok_and(|n| (min..=max).contains(&n)) && !value.starts_with('0');
    let yes_no = || matches!(value, "yes" | "no");
    match key {
        "Camera" => value == "auto" || value.strip_prefix("/dev/video").is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())),
        "Liveness" => matches!(value, "off" | "light" | "heavy"),
        "Strictness" => matches!(value, "relaxed" | "normal" | "strict"),
        "Attention" | "Adapt" | "SkipLidClosed" | "SshSessions" => yes_no(),
        "ScanSeconds" => num(2, 15),
        "MaxFailures" => num(1, 20),
        "LockoutMinutes" => num(1, 9999),
        _ => false,
    }
}

pub fn is_true(value: &str) -> bool {
    parse_bool(value, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_values_are_checked() {
        assert!(system_value_ok("Camera", "/dev/video2"));
        assert!(!system_value_ok("Camera", "/dev/video"));
        assert!(!system_value_ok("Camera", "images:/tmp"));
        assert!(system_value_ok("ScanSeconds", "15"));
        assert!(!system_value_ok("ScanSeconds", "16"));
        assert!(!system_value_ok("ScanSeconds", "05"));
        assert!(!system_value_ok("Nope", "yes"));
    }

    #[test]
    fn defaults() {
        let u = UserConfig::from(&KeyValue::default());
        assert!(!u.enabled && u.lock_screen && u.bubble && u.sudo);
        assert_eq!(u.pace(), 1.3);
        let s = Settings::from(&KeyValue::parse("ScanSeconds=99\nLiveness=HEAVY"));
        assert_eq!(s.scan_seconds, 15);
        assert_eq!(s.liveness, Liveness::Heavy);
    }
}
