//! face-unlock-gui: the window, drawn with Rune.
//!
//! ```text
//! (no arguments)          the settings, or the first start's questions
//! --pane NAME             open on a pane: overview, faces, lock, ...
//! --setup [--name NAME]   only set up a face; exits 0 when one was added
//! --choose-lock-screen    ask which lock screen, print own or yours
//! --choose-screen         ask which screen the bubble shows on, print
//!                         auto or an output name
//! ```

mod choice;
mod controls;
mod icons;
mod onboarding;
mod panes;
mod setup;
mod trial;
mod ui;
mod view;

use face_unlock::client::{Face, Status};
use face_unlock::paths::PRETTY;
use rune::{App, AppSpec, Tx};
use ui::{Host, Mode, Pane, Snapshot, Step, Ui};

/// Rune's own headless flags: render, no bus, no threads.
const HEADLESS: &[&str] = &["--snap", "--tree", "--script", "--video", "--list-scenes", "--out", "--trace", "--strip"];

/// The same made-up machine for every scene, so pictures do not depend on
/// this one.
fn scene_base() -> Ui {
    let mut snap = Snapshot::files();
    snap.user = face_unlock::keyvalue::KeyValue::parse("Enabled=yes\n");
    snap.cfg = face_unlock::config::UserConfig::from(&snap.user);
    snap.sys = face_unlock::keyvalue::KeyValue::parse("Liveness=light\n");
    snap.settings = face_unlock::config::Settings::from(&snap.sys);
    snap.status = Some(Status {
        faces: 2,
        identities: 2,
        last_unlock: face_unlock::text::now() - 300,
        last_purpose: "unlock".into(),
        camera: "auto".into(),
        camera_path: "/dev/video0".into(),
        camera_name: "Integrated Camera".into(),
        camera_present: true,
        models: true,
        liveness: "light".into(),
        version: face_unlock::paths::VERSION.into(),
        ..Default::default()
    });
    snap.faces = vec![
        Face { id: "a1".into(), name: "Alex".into(), created: 0, enabled: true, samples: 19, learned: 4 },
        Face { id: "b2".into(), name: "Alex with glasses".into(), created: 0, enabled: true, samples: 19, learned: 0 },
    ];
    snap.cameras = vec![face_unlock::client::Camera { path: "/dev/video0".into(), name: "Integrated Camera".into(), infrared: false }];
    snap.auto_camera = "/dev/video0".into();
    snap.pam = ui::Pam { sudo_available: true, sudo: true, polkit_available: true, polkit: true, lockers: vec![], lockers_on: false };
    snap.socket_enabled = true;
    snap.agent_available = true;
    snap.agent_running = true;
    snap.desktop = face_unlock::system::Desktop::Plasma;
    snap.screens = vec!["eDP-1".into(), "DP-2".into()];
    let mut ui = Ui::new(Mode::Main, None, snap);
    ui.loaded = true;
    ui
}

type Scene = (&'static str, fn(&mut Ui));

const SCENES: &[Scene] = &[
    ("overview", |_| {}),
    ("faces", |s| s.pane = Pane::Faces),
    ("lock", |s| s.pane = Pane::LockScreen),
    ("lock-hyprland", |s| {
        s.pane = Pane::LockScreen;
        s.snap.desktop = face_unlock::system::Desktop::Hyprland;
        s.snap.user.set("LockScreenStyle", "own");
        s.snap.cfg = face_unlock::config::UserConfig::from(&s.snap.user);
    }),
    ("prompts", |s| s.pane = Pane::Prompts),
    ("recognition", |s| s.pane = Pane::Recognition),
    ("bubble", |s| s.pane = Pane::Bubble),
    ("try", |s| {
        s.pane = Pane::Try;
        s.trial.running = true;
        s.trial.face = true;
        s.trial.score = Some(0.71);
        s.trial.frame = Some(serde_json::json!({"yaw": 4.0, "eyes": 0.42, "depth": 0.6, "blink": 0.2, "glare": 0.0, "device": 0.0}));
    }),
    ("welcome", |s| {
        s.mode = Mode::Onboarding;
        s.step = Step::Welcome;
    }),
    ("setup", |s| {
        s.mode = Mode::Onboarding;
        s.step = Step::Face;
    }),
    ("setup-circle", |s| {
        s.mode = Mode::Onboarding;
        s.step = Step::Face;
        s.enroll.state = setup::State::Circle;
        s.enroll.sectors = [true, true, false, false, true, false, false, true];
        s.enroll.face_visible = true;
        s.enroll.pose = (0.6, -0.3);
        s.enroll.instruction = face_unlock::i18n::tr("Move your head slowly to complete the circle.").into();
    }),
    ("setup-done", |s| {
        s.mode = Mode::Onboarding;
        s.step = Step::Face;
        s.enroll.state = setup::State::Done;
        s.enroll.sectors = [true; 8];
    }),
    ("lock-choice", |s| {
        s.mode = Mode::Onboarding;
        s.step = Step::LockScreen;
    }),
    ("places", |s| {
        s.mode = Mode::Onboarding;
        s.step = Step::Places;
    }),
    ("done", |s| {
        s.mode = Mode::Onboarding;
        s.step = Step::Done;
        s.notices = vec![face_unlock::actions::Notice::Ok(face_unlock::i18n::tr("Face unlock is on. Lock the screen and look at it to try.").into())];
    }),
    ("screens", |s| {
        s.mode = Mode::ChooseScreen;
    }),
];

const LOGO: &[u8] = include_bytes!("../../res/face-unlock.svg");

fn setup(s: &mut Ui, waker: rune::Waker) {
    let cache = rune::ImageCache::new(waker);
    let logo = cache.svg(LOGO, 28.0, 2.0);
    let big = cache.svg(LOGO, 104.0, 2.0);
    let _ = logo.wait();
    let _ = big.wait();
    s.logo = Some(logo);
    s.logo_big = Some(big);
    s.images = Some(cache);
    if matches!(s.mode, Mode::ChooseLock) || (s.mode == Mode::Onboarding && s.snap.desktop.own_locker()) {
        s.load_wallpapers();
    }
}

fn init_default() -> Ui {
    scene_base()
}

/// Closing the window: a setup that runs stops, and a question counts as
/// not answered.
fn on_close(s: &mut Ui, tx: &mut Tx) {
    if s.mode == Mode::Setup || s.sub == Some(Mode::Setup) || (s.mode == Mode::Onboarding && s.step == Step::Face) {
        let done = s.enroll.state == setup::State::Done;
        if !done {
            s.enroll_cancel(tx);
        }
        if s.mode == Mode::Setup {
            ui::answer(if done { 0 } else { 1 }, "");
        }
    }
    s.trial.stop();
    tx.quit();
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |a: &str| args.iter().any(|x| x == a);
    let value = |a: &str| args.windows(2).find(|w| w[0] == a).map(|w| w[1].clone());
    let mode = if has("--setup") {
        Mode::Setup
    } else if has("--choose-lock-screen") {
        Mode::ChooseLock
    } else if has("--choose-screen") {
        Mode::ChooseScreen
    } else {
        Mode::Main
    };
    let name = value("--name");
    let pane = value("--pane").map(|p| Pane::from_name(&p)).unwrap_or_default();
    let mut rune_args = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--setup" | "--choose-lock-screen" | "--choose-screen" => {}
            "--name" | "--pane" => {
                it.next();
            }
            _ => rune_args.push(a.clone()),
        }
    }
    let headless = rune_args.iter().any(|a| HEADLESS.contains(&a.as_str()));
    // Wayland's app_id: the desktop file of the same name gives the window
    // its icon and name in the task bar.
    rune::set_app_id("face-unlock");

    let size = match mode {
        Mode::Main => (900.0, 680.0),
        Mode::Setup => (560.0, 720.0),
        Mode::ChooseLock => (760.0, 560.0),
        Mode::ChooseScreen => (680.0, 560.0),
        Mode::Onboarding => (760.0, 680.0),
    };
    let spec = AppSpec::<Ui> {
        fonts: rune_theme_opencode::FONTS,
        families: rune_theme_opencode::FAMILIES,
        min_size: Some((560.0, 480.0)),
        scenes: SCENES,
        setup: Some(setup),
        ..AppSpec::new(PRETTY, size, init_default, view::view)
    };

    if headless {
        App::new(spec).args(rune_args).init(scene_base).run();
        return;
    }

    let make = move |app: App<Ui>, displays: Vec<rune::Display>| {
        let host = Host { app: app.handle() };
        let h = host.clone();
        let app = app.on_close(on_close).init(move || {
            let mut ui = Ui::new(mode, Some(h), Snapshot::files());
            ui.pane = pane;
            ui.displays = displays;
            if let Some(n) = name {
                ui.enroll.name = rune::TextEdit::new(n);
            }
            ui.refresh();
            ui
        });
        // Kept current while it is open: the agent, the daemon and the
        // files can change behind its back.
        let refresh = host.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(5));
                if refresh.app.is_visible() {
                    refresh.app.update_if(|s, _| {
                        if s.busy.is_none() && !s.trial.running && s.mode == Mode::Main {
                            s.refresh();
                        }
                        false
                    });
                }
            }
        });
        app
    };

    if mode == Mode::ChooseScreen {
        rune::run_windows_with_args(rune::RunOptions::default(), rune_args, move |displays| {
            let app = make(App::new(spec), displays.to_vec());
            vec![rune::WindowSpec::from_app("main", app)]
        });
    } else {
        let app = make(App::new(spec).args(rune_args), vec![]);
        app.run();
    }

    let answer = ui::ANSWER.lock().unwrap_or_else(|e| e.into_inner()).take();
    match answer {
        Some((code, text)) => {
            if !text.is_empty() {
                println!("{text}");
            }
            std::process::exit(code);
        }
        None => std::process::exit(if mode == Mode::Main { 0 } else { 1 }),
    }
}
