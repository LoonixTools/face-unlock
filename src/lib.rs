//! face-unlock: Face ID for Linux.
//!
//! The library holds what the command, the window, the agent and the daemon
//! share: paths, the settings files, translations, the way to the daemon,
//! and the PAM files.

pub mod actions;
pub mod client;
pub mod config;
pub mod i18n;
pub mod keyvalue;
pub mod migrate;
pub mod pam;
pub mod paths;
pub mod root;
pub mod system;
pub mod text;
pub mod wallpaper;
