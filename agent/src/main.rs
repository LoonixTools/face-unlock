//! face-unlock-agent: the part in the user's session.
//!
//! ```text
//! (no arguments)   stay in the background: unlock the lock screen by face,
//!                  and show the bubble for every scan
//! --lock           lock the screen: with face-unlock's own lock screen
//!                  where the lock screen is a program of its own (Hyprland,
//!                  Niri), else through logind with the desktop's
//! --demo           play the bubble's animations once, for trying out a
//!                  style (--bubble-style minimal) and for screenshots
//! ```

mod app;
mod bubble;
mod bus;
mod daemon;
mod draw;
mod lockers;
mod lockscreen;
mod pam;
mod socket;
mod wl;

use app::{App, Mode};
use face_unlock::paths::VERSION;
use smithay_client_toolkit::reexports::calloop::EventLoop;
use smithay_client_toolkit::reexports::calloop::channel;

const HELP: &str = "Usage: face-unlock-agent [--lock | --demo [--bubble-style full|minimal]]

The part of face-unlock in your session. It unlocks the lock screen by face
and shows the bubble for every scan.

  --lock           lock the screen
  --demo           play the bubble's animations once
  --bubble-style   full or minimal, for --demo
  --version        print the version
";

/// With no agent running, --lock stays behind as the lock screen until it
/// is unlocked. The command that asked for it returns as soon as the screen
/// is locked, the way swaylock -f does: an idle daemon locking before sleep
/// waits for that, and not a moment longer.
fn fork_for_lock() {
    let mut fds = [0i32; 2];
    // SAFETY: plain pipe and fork, before any thread is started.
    unsafe {
        if libc::pipe(fds.as_mut_ptr()) != 0 {
            return;
        }
        let pid = libc::fork();
        if pid < 0 {
            libc::close(fds[0]);
            libc::close(fds[1]);
            return;
        }
        if pid > 0 {
            libc::close(fds[1]);
            let mut c = 0u8;
            let locked = libc::read(fds[0], (&mut c as *mut u8).cast(), 1) == 1;
            libc::_exit(if locked { 0 } else { 1 });
        }
        libc::close(fds[0]);
        libc::setsid();
    }
    app::READY_FD.store(fds[1], std::sync::atomic::Ordering::SeqCst);
}

fn main() {
    // SAFETY: called once, before any thread.
    unsafe {
        libc::setlocale(libc::LC_ALL, c"".as_ptr());
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |a: &str| args.iter().any(|x| x == a);
    if has("--help") || has("-h") {
        print!("{HELP}");
        return;
    }
    if has("--version") {
        println!("face-unlock-agent {VERSION}");
        return;
    }
    let style = args.windows(2).find(|w| w[0] == "--bubble-style").map(|w| w[1].clone());

    let mode = if has("--lock") {
        if socket::running() {
            std::process::exit(if socket::request_lock() { 0 } else { 1 });
        }
        // Plasma and GNOME lock with their own lock screen.
        if !wl::has_global("ext_session_lock_manager_v1") {
            std::process::exit(if bus::lock_session() { 0 } else { 1 });
        }
        fork_for_lock();
        Mode::LockOnly
    } else if has("--demo") {
        Mode::Demo
    } else {
        if socket::running() {
            eprintln!("an agent is already running in this session");
            return;
        }
        Mode::Agent
    };

    // The fonts take a moment to read: done now, while nothing waits.
    std::thread::spawn(|| {
        draw::measure("Face", 13.0, 500, None);
    });

    let mut event_loop: EventLoop<App> = match EventLoop::try_new() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("cannot start: {e}");
            std::process::exit(2);
        }
    };
    let handle = event_loop.handle();
    let (tx, rx) = channel::channel();
    let _ = handle.insert_source(rx, |e, _, app: &mut App| {
        if let channel::Event::Msg(e) = e {
            app.event(e);
        }
    });
    let wl = wl::Wl::connect(&handle);
    if wl.is_none() {
        eprintln!("no Wayland session: no bubble and no lock screen of its own");
    }
    let mut app = App::new(handle, tx.clone(), mode, wl, style);

    match mode {
        Mode::Demo => {
            if !app.wl.as_ref().is_some_and(|w| w.has_layer()) {
                eprintln!("this desktop has no layer-shell; on GNOME the extension draws the bubble");
                std::process::exit(1);
            }
            app.demo();
        }
        Mode::Agent => {
            if let Err(e) = socket::listen(tx) {
                eprintln!("cannot listen on {}: {e}", face_unlock::paths::agent_socket().display());
            }
            app.start();
            // The last agent went away with the screen locked, and the
            // compositor kept it locked. Take the lock back, so it can be
            // opened again.
            if app.own_lockers && app::marker().exists() {
                app.lock();
            }
        }
        Mode::LockOnly => {
            app.start();
            app.lock();
            if !app.own_locked() {
                app::signal_ready(false);
                std::process::exit(1);
            }
        }
    }

    while !app.exit {
        if let Err(e) = event_loop.dispatch(None, &mut app) {
            eprintln!("{e}");
            break;
        }
    }
    if let Some(wl) = &app.wl {
        let _ = wl.conn.flush();
    }
}
