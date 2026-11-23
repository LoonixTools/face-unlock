//! face-unlock: the command.
//!
//! Without a command it opens the window. The commands are for a terminal:
//! turning face unlock on and off, the faces, a test scan, locking. The
//! camera, the face data and the decision belong to the daemon
//! (face-unlockd, as root), the lock screen and the bubble to the agent
//! (face-unlock-agent, in the session), and sudo and polkit reach the daemon
//! through a PAM module.

use face_unlock::actions::{self, Notice, Notices};
use face_unlock::client;
use face_unlock::config::UserConfig;
use face_unlock::i18n::{tr, trf};
use face_unlock::paths::{self, NAME, PRETTY, VERSION};
use face_unlock::root::{self, Ask};
use face_unlock::system;
use face_unlock::text;
use serde_json::json;
use std::io::{IsTerminal, Write};
use std::os::unix::process::CommandExt;
use std::process::Command;

struct Colors {
    reset: &'static str,
    bold: &'static str,
    dim: &'static str,
    blue: &'static str,
    green: &'static str,
    yellow: &'static str,
    red: &'static str,
}

fn colors() -> &'static Colors {
    static ON: Colors = Colors {
        reset: "\x1b[0m",
        bold: "\x1b[1m",
        dim: "\x1b[2m",
        blue: "\x1b[38;2;52;153;255m",
        green: "\x1b[32m",
        yellow: "\x1b[33m",
        red: "\x1b[31m",
    };
    static OFF: Colors = Colors { reset: "", bold: "", dim: "", blue: "", green: "", yellow: "", red: "" };
    if std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none() { &ON } else { &OFF }
}

fn ok(msg: &str) {
    let c = colors();
    println!("{}✔{} {msg}", c.green, c.reset);
}

fn bad(msg: &str) {
    let c = colors();
    eprintln!("{}✘{} {msg}", c.red, c.reset);
}

fn note(msg: &str) {
    let c = colors();
    println!("{}•{} {msg}", c.dim, c.reset);
}

fn print(notices: &Notices) {
    for n in notices {
        match n {
            Notice::Ok(m) => ok(m),
            Notice::Bad(m) => bad(m),
            Notice::Note(m) => note(m),
            Notice::Code(m) => println!("    {m}"),
        }
    }
}

fn failed(notices: &Notices) -> bool {
    notices.iter().any(|n| matches!(n, Notice::Bad(_)))
}

/// The window. `args` pick what it shows: the setup of a face, one of the
/// questions, or the whole thing.
fn window(args: &[&str]) -> std::io::Result<std::process::ExitStatus> {
    Command::new(paths::gui_exe()).args(args).status()
}

fn ensure_service() -> bool {
    if client::status().is_none() && !system::socket_enabled() {
        println!("  {}", tr("Face unlock's service has to be switched on once for this computer. That needs your password."));
    }
    match actions::ensure_service(Ask::Terminal) {
        Ok(()) => true,
        Err(e) => {
            bad(&e);
            false
        }
    }
}

fn setup(name: Option<&str>) -> bool {
    if !ensure_service() {
        return false;
    }
    if !system::has_display() {
        bad(tr("Setting up a face needs the camera picture on screen. Run this inside your desktop session."));
        return false;
    }
    println!("  {}", tr("The setup window is open. Follow it there."));
    let mut args = vec!["--setup"];
    if let Some(n) = name {
        args.extend(["--name", n]);
    }
    if window(&args).is_ok_and(|s| s.success()) {
        ok(tr("The face is set up."));
        if !UserConfig::load().enabled {
            note(&trf("Face unlock is still off. Turn it on with `%s enable`.", &[&NAME]));
        }
        return true;
    }
    note(tr("No face was added."));
    false
}

fn enable() -> bool {
    if !ensure_service() {
        return false;
    }
    if client::faces().is_some_and(|f| f.is_empty()) {
        println!("  {}", tr("First, set up your face."));
        if !setup(None) {
            return false;
        }
    }
    let notices = actions::enable(Ask::Terminal);
    // Where the lock screen is a program of its own: face-unlock's, with the
    // bubble, or that program with a line of text. Asked once, in a window.
    if system::desktop().own_locker() && UserConfig::load().lock_screen_style.is_empty() {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            println!("  {}", tr("Pick your lock screen in the window."));
            match Command::new(paths::gui_exe()).arg("--choose-lock-screen").output() {
                Ok(o) if o.status.success() => print(&actions::lock_style_set(String::from_utf8_lossy(&o.stdout).trim())),
                _ => note(tr("No lock screen picked. You can do it later under Settings.")),
            }
        } else {
            print(&actions::lock_style_set("yours"));
        }
    }
    print(&notices);
    !failed(&notices)
}

fn disable() -> bool {
    let n = actions::disable(Ask::Terminal);
    print(&n);
    !failed(&n)
}

fn faces() -> bool {
    let Some(list) = client::faces() else {
        bad(&text::reason("unreachable"));
        return false;
    };
    if list.is_empty() {
        note(tr("No face is set up yet."));
    }
    for f in list {
        let state = if f.enabled { tr("on") } else { tr("off") };
        println!("  {}  {:<24} {:<4} {}", f.id, f.name, state, trf("%s samples, %s learned", &[&f.samples, &f.learned]));
    }
    true
}

fn remove(id: Option<&str>) -> bool {
    let Some(id) = id else {
        bad(&trf("Which face? See `%s faces` for the ids.", &[&NAME]));
        return false;
    };
    let r = client::request(json!({"cmd": "remove", "id": id}));
    if client::ok(&r) {
        ok(tr("Deleted."));
        true
    } else {
        bad(&text::reason(client::reason(&r)));
        false
    }
}

/// One scan, with everything the checks see on one line that keeps updating.
fn test() -> bool {
    if !ensure_service() {
        return false;
    }
    let c = colors();
    println!("  {}\n", tr("Look at the camera. Blink once, or turn your head a little."));
    let tty = std::io::stdout().is_terminal();
    let conn = match client::Conn::open(&json!({"cmd": "verify", "purpose": "test", "verbose": true})) {
        Ok(c) => c,
        Err(_) => {
            bad(&text::reason("unreachable"));
            return false;
        }
    };
    let mut score = "-".to_string();
    let r = conn.result(|e| match e["event"].as_str().unwrap_or("") {
        "face" => println!("\r\x1b[K  {}", tr("Face found.")),
        "hint" => {
            let msg = match e["hint"].as_str().unwrap_or("") {
                "blink" => tr("Recognised. Now blink once, or turn your head a little."),
                "look" => tr("Look at the screen."),
                "closer" => tr("Move closer to the camera."),
                "light" => tr("It is too dark to see your face."),
                _ => return,
            };
            println!("\r\x1b[K  {msg}");
        }
        "frame" if tty => {
            if let Some(s) = e["score"].as_f64() {
                score = format!("{s:.3}");
            }
            let n = |k: &str| e[k].as_f64().unwrap_or(0.0);
            print!(
                "\r\x1b[K{}  {} {:<6} {} {:>5.1}°  {} {:<5.2}  {} {:<4.2} {} {:<4.2}  {} {:<4.2} {} {:<4.2}{}",
                c.dim,
                tr("match"),
                score,
                tr("turn"),
                n("yaw"),
                tr("eyes"),
                n("eyes"),
                tr("depth"),
                n("depth"),
                tr("blink"),
                n("blink"),
                tr("glare"),
                n("glare"),
                tr("edge"),
                n("device"),
                c.reset
            );
            let _ = std::io::stdout().flush();
        }
        _ => {}
    });
    print!("\r\x1b[K");
    if client::ok(&r) {
        ok(&trf(
            "Recognised as %s (match %s) in %s ms.",
            &[&r["name"].as_str().unwrap_or(""), &format!("{:.3}", r["score"].as_f64().unwrap_or(0.0)), &r["ms"].as_i64().unwrap_or(0)],
        ));
        let how = match r["liveness"].as_str().unwrap_or("") {
            "blink" => tr("blink"),
            "depth" => tr("head turn"),
            _ => "",
        };
        if !how.is_empty() {
            println!("  {}", trf("Sign of life: %s", &[&how]));
        }
        true
    } else {
        bad(&text::reason(client::reason(&r)));
        if let Some(m) = r["message"].as_str() {
            println!("  {m}");
        }
        if r.get("lockout").is_some() {
            note(tr("That was too many tries. Face unlock is paused until you unlock with your password."));
        }
        false
    }
}

fn row(label: &str, value: &str) {
    let width: usize = label.chars().map(|c| if (c as u32) >= 0x1100 { 2 } else { 1 }).sum();
    println!("  {label}{} {value}", " ".repeat(30usize.saturating_sub(width)));
}

fn status() -> bool {
    let c = colors();
    println!("\n{}{}  {PRETTY}{}\n", c.bold, c.blue, c.reset);
    let cfg = UserConfig::load();
    let on_off = |on: bool| if on { format!("{}{}{}", c.green, tr("on"), c.reset) } else { format!("{}{}{}", c.dim, tr("off"), c.reset) };
    row(tr("Face unlock"), &on_off(cfg.enabled));
    println!();
    let Some(st) = client::status() else {
        row(tr("Service"), &format!("{}{}{}", c.yellow, tr("not running"), c.reset));
        if cfg.enabled {
            println!("\n  {}{}{}", c.dim, tr("Turning face unlock on again starts it."), c.reset);
        }
        return true;
    };
    let faces = client::faces().unwrap_or_default();
    if faces.is_empty() {
        row(tr("Faces"), &format!("{}{}{}", c.yellow, tr("none set up yet"), c.reset));
    } else {
        let names: Vec<&str> = faces.iter().filter(|f| f.enabled).map(|f| f.name.as_str()).collect();
        let names = if names.is_empty() { tr("all turned off").to_string() } else { names.join(", ") };
        row(tr("Faces"), &format!("{} {}({names}){}", st.faces, c.dim, c.reset));
    }
    let camera = if st.camera_present {
        if st.camera_name.is_empty() { st.camera_path.clone() } else { st.camera_name.clone() }
    } else {
        format!("{}{}{}", c.yellow, tr("none found"), c.reset)
    };
    row(tr("Camera"), &camera);
    let files = face_unlock::pam::PamFiles::system();
    row(tr("Lock screen"), &on_off(cfg.enabled && cfg.lock_screen));
    row(tr("sudo"), &on_off(files.enabled("sudo")));
    row(tr("Admin prompts"), &on_off(files.enabled("polkit-1")));
    if actions::lockers_here() {
        row(tr("Lock screens"), &format!("{} {}({}){}", on_off(files.lockers_enabled()), c.dim, files.lockers().join(", "), c.reset));
    }
    if st.lockout > 0 {
        row(tr("Paused"), &format!("{}{}{}", c.yellow, trf("for %d more minutes, or until the password is used", &[&st.lockout.div_ceil(60)]), c.reset));
    }
    row(tr("Last unlock"), &text::time_ago(st.last_unlock));
    if !st.models {
        println!("\n  {}{}{}", c.red, tr("The recognition models are missing. Reinstall the package."), c.reset);
    }
    if cfg.enabled && cfg.lock_screen && system::agent_available() && !system::agent_running() {
        println!("\n  {}{}{}", c.yellow, tr("The lock screen agent is not running."), c.reset);
    }
    println!();
    true
}

fn help() {
    println!(
        "{PRETTY} {VERSION}\n\n{}\n\n{}\n  enable              {}\n  disable             {}\n  setup [NAME]        {}\n  faces               {}\n  remove ID           {}\n  test                {}\n  lock                {}\n  status              {}\n  -h, --help          {}\n  -V, --version       {}\n\n{}",
        tr("Usage: face-unlock [command]"),
        tr("Commands:"),
        tr("Turn face unlock on"),
        tr("Turn it off (faces are kept)"),
        tr("Add a face"),
        tr("List the faces"),
        tr("Delete a face"),
        tr("Look at the camera and see what it sees"),
        tr("Lock the screen"),
        tr("Show what is on"),
        tr("Show this help"),
        tr("Show the version"),
        tr("Without a command the window opens."),
    );
}

/// The user's settings, moved over from plasma-face-unlock, and the system's
/// through root when there is something left.
fn migrate() {
    face_unlock::migrate::user();
    if face_unlock::migrate::pending(&face_unlock::migrate::Dirs::system()) {
        println!("  {}", tr("Moving your faces and settings over from plasma-face-unlock. That needs your password."));
        if let Err(e) = root::run(root::ask(), &["migrate"]) {
            bad(&e);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("");
    let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();

    if cmd == "--root" {
        let verb_args: Vec<String> = args[1..].to_vec();
        match root::verb(&verb_args) {
            Ok(()) => return,
            Err(e) => {
                bad(&e);
                std::process::exit(1);
            }
        }
    }

    // Face data and settings are per user; root has neither a lock screen
    // nor a face of its own to set up.
    if paths::is_root() && std::env::var_os("FU_ALLOW_ROOT").is_none() && !matches!(cmd, "-h" | "--help" | "help" | "-V" | "--version") {
        bad(tr("Run this as your own user, not as root. It asks for your password when it needs to."));
        std::process::exit(2);
    }

    if matches!(cmd, "" | "enable" | "disable" | "setup" | "add" | "enroll" | "faces" | "list" | "remove" | "delete" | "test" | "try") {
        migrate();
    }

    let good = match cmd {
        "" => {
            if system::has_display() {
                let err = Command::new(paths::gui_exe()).exec();
                bad(&err.to_string());
                false
            } else {
                status();
                help();
                true
            }
        }
        "enable" => enable(),
        "disable" => disable(),
        "setup" | "add" | "enroll" => setup(rest.first().copied()),
        "faces" | "list" => faces(),
        "remove" | "delete" => remove(rest.first().copied()),
        "test" | "try" => test(),
        "status" => status(),
        "lock" => {
            let err = Command::new(paths::agent_exe()).arg("--lock").exec();
            bad(&err.to_string());
            false
        }
        "-h" | "--help" | "help" => {
            help();
            true
        }
        "-V" | "--version" => {
            println!("{NAME} {VERSION}");
            true
        }
        other => {
            bad(&trf("Unknown command: %s", &[&other]));
            println!();
            help();
            std::process::exit(1);
        }
    };
    std::process::exit(if good { 0 } else { 1 });
}
