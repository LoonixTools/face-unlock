//! The panes of the main window.

use crate::controls::{DESCRIPTION, GUTTER, Kind, Paint, ROW_PX, TITLE, button, card, code, icon_row, page, raised, section, section_note, segment_row, select_row, setting_row, switch, switch_row};
use crate::icons;
use crate::ui::{Mode, Pane, Ui};
use face_unlock::actions::Notice;
use face_unlock::i18n::{tr, trf};
use face_unlock::system::Desktop;
use rune::tok::radius;
use rune::{Color, El, TextStyle, col, el, icon, row, text};
use rune_kit::{Spinner, TextInput};

/// The heading of a pane's actions: a button, or a spinner while it waits.
fn action(ui: &Ui, p: Paint, key: &'static str, busy_key: &str, label: &str, kind: Kind, press: impl Fn(&mut Ui) + 'static) -> El {
    if ui.is_busy(busy_key) {
        return Spinner::new().size(18.0).build(p.k).key(key);
    }
    button(p, key, label, kind, ui.busy.is_none(), press)
}

fn on_off(on: bool) -> &'static str {
    if on { tr("on") } else { tr("off") }
}

/// What the last action said, under the pane's title.
pub fn notices(ui: &Ui, p: Paint) -> Option<El> {
    if ui.notices.is_empty() {
        return None;
    }
    let c = &p.k.c;
    let mut list = col().key("notices").gap(6.0);
    for (i, n) in ui.notices.iter().enumerate() {
        let (glyph, color, line) = match n {
            Notice::Ok(m) => (Some(&icons::CHECK_CIRCLE), c.success.fg, m),
            Notice::Bad(m) => (Some(&icons::WARNING), c.danger.fg, m),
            Notice::Note(m) => (Some(&icons::INFO), c.icon_muted, m),
            Notice::Code(m) => {
                list = list.child(code(p, &format!("code{i}"), m).ml(26.0));
                continue;
            }
        };
        list = list.child(
            row()
                .key(format!("n{i}"))
                .gap(10.0)
                .items_start()
                .children(glyph.map(|g| icon(g, 16.0).key("icon").color(color).mt(1.0)))
                .child(text(line.clone()).key("text").font(TITLE).color(p.text()).wrap().grow(1.0).shrink()),
        );
    }
    Some(
        row()
            .key("notice-bar")
            .mx(GUTTER)
            .mb(20.0)
            .pl(16.0)
            .pr(8.0)
            .py(10.0)
            .gap(12.0)
            .center_y()
            .radius(radius::LG)
            .fill(p.t.card)
            .border(p.hair, p.t.card_border)
            .child(list.grow(1.0).shrink())
            .child(button(p, "dismiss", tr("Dismiss"), Kind::Plain, true, |s: &mut Ui| s.notices.clear())),
    )
}

// ---------------------------------------------------------------------------
// Overview
// ---------------------------------------------------------------------------

pub fn overview_actions(ui: &Ui, p: Paint) -> Vec<El> {
    if !ui.snap.cfg.enabled || ui.snap.faces.is_empty() {
        return vec![];
    }
    vec![button(p, "try", tr("Try it"), Kind::Secondary, true, |s: &mut Ui| s.show_pane(Pane::Try))]
}

fn warning(p: Paint, key: &'static str, glyph: &'static rune::Icon, title: &str, description: Option<&str>, control: Option<El>) -> El {
    let c = &p.k.c;
    let mut r = raised(p, key).child(
        row()
            .key("row")
            .min_h(56.0)
            .px(ROW_PX)
            .py(12.0)
            .gap(12.0)
            .center_y()
            .child(icon(glyph, 18.0).key("icon").color(c.warning.fg))
            .child(
                col()
                    .key("copy")
                    .grow(1.0)
                    .shrink()
                    .gap(2.0)
                    .child(text(title).key("title").font(TextStyle::new(13.0, 600.0)).color(p.text()).wrap())
                    .children(description.map(|d| text(d).key("description").font(DESCRIPTION).color(p.muted()).wrap())),
            )
            .children(control),
    );
    r = r.label(title);
    r
}

pub fn overview(ui: &Ui, p: Paint) -> El {
    let s = &ui.snap;
    let on = s.cfg.enabled;
    let hero_text = if !s.reachable() && ui.loaded {
        tr("The face unlock service is not running.")
    } else if on {
        tr("Face unlock is on. Lock the screen and look at it to try.")
    } else if s.faces.is_empty() {
        tr("Set up your face, then lock the screen and look at it.")
    } else {
        tr("Face unlock is off. Your faces are kept; delete them under Faces.")
    };
    let switch = if ui.is_busy("Enabled") {
        Spinner::new().size(20.0).build(p.k).key("switch")
    } else {
        switch(p, on, ui.busy.is_none()).a11y_hidden()
    };
    let hero = raised(p, "hero").child(
        row()
            .key("row")
            .label(tr("Face unlock"))
            .role("switch")
            .checked(on)
            .focusable(true)
            .focus_ring(2.0, -2.0, p.accent)
            .on_press(|s: &mut Ui| s.toggle_enabled())
            .radius(radius::MD)
            .hover(p.t.hover)
            .press(p.t.press)
            .min_h(84.0)
            .px(ROW_PX + 2.0)
            .py(14.0)
            .gap(16.0)
            .center_y()
            .child(
                el().key("badge")
                    .size(48.0, 48.0)
                    .radius(14.0)
                    .center()
                    .fill(if on { p.accent } else { p.t.control_off })
                    .child(icon(&icons::SCAN_FACE, 26.0).key("glyph").color(if on { Color::WHITE } else { p.muted() })),
            )
            .child(
                col()
                    .key("copy")
                    .grow(1.0)
                    .shrink()
                    .gap(3.0)
                    .child(text(tr("Face unlock")).key("title").font(TextStyle::new(16.0, 600.0).lh(22.0)).color(p.text()))
                    .child(text(hero_text).key("description").font(TITLE).color(p.muted()).wrap()),
            )
            .child(switch),
    );

    let mut warnings = Vec::new();
    if ui.loaded && !s.reachable() {
        warnings.push(warning(
            p,
            "w-service",
            &icons::POWER,
            tr("The face unlock service is not running."),
            Some(tr("Face unlock's service has to be switched on once for this computer. That needs your password.")),
            Some(action(ui, p, "start-service", "service", tr("Switch on"), Kind::Primary, |s: &mut Ui| s.start_service())),
        ));
    }
    if let Some(st) = &s.status {
        if !st.models {
            warnings.push(warning(p, "w-models", &icons::WARNING, tr("The recognition models are missing. Reinstall the package."), None, None));
        }
        if st.lockout > 0 {
            let line = trf("for %d more minutes, or until the password is used", &[&st.lockout.div_ceil(60)]);
            warnings.push(warning(p, "w-lockout", &icons::LOCK, tr("Paused"), Some(&line), None));
        }
        if !st.camera_present {
            warnings.push(warning(p, "w-camera", &icons::CAMERA, tr("No camera found."), Some(tr("Connect a camera, or pick another one under Recognition.")), None));
        }
    }
    if on && s.cfg.lock_screen && s.agent_available && !s.agent_running && ui.loaded {
        let hint = face_unlock::system::hyprland_start_hint(face_unlock::paths::UNIT_AGENT);
        let desc = if s.desktop == Desktop::Hyprland { Some(hint.note.as_str()) } else { None };
        warnings.push(warning(
            p,
            "w-agent",
            &icons::LOCK,
            tr("The lock screen agent is not running."),
            desc,
            Some(action(ui, p, "start-agent", "agent", tr("Start"), Kind::Secondary, |s: &mut Ui| s.start_agent())),
        ));
    }
    if s.polkit_missing {
        let control = if s.polkit_unit.is_some() {
            Some(action(ui, p, "start-polkit", "polkit-agent", tr("Start"), Kind::Secondary, |s: &mut Ui| s.start_polkit_agent()))
        } else {
            None
        };
        let mut w = warning(
            p,
            "w-polkit",
            &icons::KEY,
            tr("No polkit agent is running, so no password window can open. Adding a face and the admin prompts need one."),
            if s.polkit_unit.is_none() { Some(tr("Install one in a terminal:")) } else { None },
            control,
        );
        if s.polkit_unit.is_none() {
            w = w.child(code(p, "cmd", "face-unlock polkit-agent").mx(ROW_PX).mb(12.0));
        }
        warnings.push(w);
    }

    let names: Vec<&str> = s.faces.iter().filter(|f| f.enabled).map(|f| f.name.as_str()).collect();
    let faces_line = if s.faces.is_empty() {
        tr("none set up yet").to_string()
    } else if names.is_empty() {
        tr("all turned off").to_string()
    } else {
        names.join(", ")
    };
    let value = |t: &str| text(t.to_string()).key("value").font(TITLE).color(p.muted()).truncate();
    let camera = match &s.status {
        Some(st) if st.camera_present => if st.camera_name.is_empty() { st.camera_path.clone() } else { st.camera_name.clone() },
        Some(_) => tr("none found").into(),
        None => "–".into(),
    };
    let mut rows = vec![
        icon_row(p, "faces", &icons::USERS, tr("Faces"), None, value(&faces_line)).on_press(|s: &mut Ui| s.show_pane(Pane::Faces)).hover(p.t.hover).press(p.t.press),
        icon_row(p, "camera", &icons::CAMERA, tr("Camera"), None, value(&camera)),
        icon_row(p, "lock", &icons::LOCK, tr("Lock screen"), None, value(on_off(on && s.cfg.lock_screen))),
        icon_row(p, "sudo", &icons::SUDO, tr("sudo"), None, value(on_off(s.pam.sudo))),
        icon_row(p, "polkit", &icons::SHIELD, tr("Admin prompts"), None, value(on_off(s.pam.polkit))),
    ];
    if !s.pam.lockers.is_empty() {
        let line = format!("{} ({})", on_off(s.pam.lockers_on), s.pam.lockers.join(", "));
        rows.push(icon_row(p, "lockers", &icons::LOCK, tr("Lock screens"), None, value(&line)));
    }
    rows.push(icon_row(p, "liveness", &icons::EYE, tr("Anti-spoofing"), None, value(liveness_label(s.sys.get("Liveness", "light")))));
    rows.push(icon_row(p, "last", &icons::CHECK_CIRCLE, tr("Last unlock"), None, value(&face_unlock::text::time_ago(s.status.as_ref().map(|st| st.last_unlock).unwrap_or(0)))));

    page("overview").child(hero).children(warnings).child(section(p, "glance", tr("At a glance"), rows))
}

pub fn liveness_label(v: &str) -> &'static str {
    match v {
        "heavy" => tr("strict"),
        "off" => tr("off"),
        _ => tr("basic"),
    }
}

// ---------------------------------------------------------------------------
// Faces
// ---------------------------------------------------------------------------

pub fn faces_actions(ui: &Ui, p: Paint) -> Vec<El> {
    if ui.sub.is_some() {
        return vec![];
    }
    vec![button(p, "add", tr("Add a face"), Kind::Primary, ui.snap.reachable(), |s: &mut Ui| {
        s.enroll = crate::setup::Enroll::default();
        s.sub = Some(Mode::Setup);
    })]
}

fn icon_button(p: Paint, key: &'static str, glyph: &'static rune::Icon, label: &str, press: impl Fn(&mut Ui) + 'static) -> El {
    el().key(key)
        .size(30.0, 30.0)
        .radius(radius::SM)
        .center()
        .role("button")
        .label(label)
        .focusable(true)
        .focus_ring(2.0, -2.0, p.accent)
        .hover(p.t.control_track)
        .press(p.t.control_off)
        .on_press(press)
        .child(icon(glyph, 16.0).key("icon").color(p.muted()).a11y_hidden())
}

pub fn faces(ui: &Ui, p: Paint) -> El {
    if ui.sub == Some(Mode::Setup) {
        return crate::setup::view(ui, p);
    }
    let s = &ui.snap;
    if !s.reachable() {
        return page("faces").child(card(p, "empty", vec![setting_row(p, "none", &face_unlock::text::reason("unreachable"), None, el())]));
    }
    if s.faces.is_empty() {
        return page("faces").child(card(
            p,
            "empty",
            vec![setting_row(p, "none", tr("No face is set up yet."), Some(tr("Add one with the button at the top.")), el())],
        ));
    }
    let mut rows = Vec::new();
    for f in &s.faces {
        let id = f.id.clone();
        let busy = ui.is_busy(&format!("face-{id}"));
        let detail = trf("%s samples, %s learned", &[&f.samples, &f.learned]);
        if let Some((rid, edit)) = &ui.rename
            && *rid == f.id
        {
            let save = {
                let id = id.clone();
                move |s: &mut Ui| {
                    let name = s.rename.as_ref().map(|(_, e)| e.text.trim().to_string()).unwrap_or_default();
                    if !name.is_empty() {
                        s.face("rename", id.clone(), Some(name));
                    }
                }
            };
            let field = TextInput::new(edit)
                .label(tr("New name:"))
                .width(Some(240.0))
                .on_edit(|s: &mut Ui| &mut s.rename.as_mut().expect("renaming").1)
                .on_submit({
                    let save = save.clone();
                    move |s: &mut Ui, _| save(s)
                })
                .build(p.k)
                .key("field");
            rows.push(
                row()
                    .key(format!("face-{}", f.id))
                    .min_h(56.0)
                    .px(ROW_PX)
                    .py(10.0)
                    .gap(10.0)
                    .center_y()
                    .child(field.grow(1.0))
                    .child(button(p, "cancel", tr("Cancel"), Kind::Plain, true, |s: &mut Ui| s.rename = None))
                    .child(button(p, "save", tr("Save"), Kind::Primary, true, save)),
            );
            continue;
        }
        if ui.confirm.as_deref() == Some(&f.id) {
            let id2 = id.clone();
            rows.push(
                row()
                    .key(format!("face-{}", f.id))
                    .min_h(56.0)
                    .px(ROW_PX)
                    .py(10.0)
                    .gap(10.0)
                    .center_y()
                    .child(text(trf("Delete \"%s\"?", &[&f.name])).key("ask").font(TextStyle::new(13.0, 600.0)).color(p.text()).grow(1.0).wrap())
                    .child(button(p, "cancel", tr("Cancel"), Kind::Plain, true, |s: &mut Ui| s.confirm = None))
                    .child(button(p, "delete", tr("Delete"), Kind::Danger, true, move |s: &mut Ui| s.face("remove", id2.clone(), None))),
            );
            continue;
        }
        let (id_t, id_r, id_d) = (id.clone(), id.clone(), id.clone());
        let enabled = f.enabled;
        let name = f.name.clone();
        let controls = row()
            .key("controls")
            .gap(4.0)
            .center_y()
            .child(icon_button(p, "rename", &icons::PENCIL, tr("Rename"), move |s: &mut Ui| s.rename = Some((id_r.clone(), rune::TextEdit::new(name.clone())))))
            .child(icon_button(p, "delete", &icons::TRASH, tr("Delete"), move |s: &mut Ui| s.confirm = Some(id_d.clone())))
            .child(if busy {
                Spinner::new().size(18.0).build(p.k).key("switch").ml(8.0)
            } else {
                let free = ui.busy.is_none();
                let mut toggle = el()
                    .key("toggle")
                    .role("switch")
                    .label(tr("On"))
                    .checked(enabled)
                    .ml(8.0)
                    .radius(12.0)
                    .child(switch(p, enabled, free).a11y_hidden());
                if free {
                    toggle = toggle
                        .focusable(true)
                        .focus_ring(2.0, 2.0, p.accent)
                        .on_press(move |s: &mut Ui| s.face(if enabled { "disable" } else { "enable" }, id_t.clone(), None));
                } else {
                    toggle = toggle.disabled(true);
                }
                toggle
            });
        rows.push(icon_row(p, &format!("face-{}", f.id), &icons::SCAN_FACE, &f.name, Some(&detail), controls));
    }
    page("faces").child(section_note(p, "list", tr("Faces"), tr("Changes ask for your password"), rows)).child(
        text(tr("A face is kept as numbers, not as a picture, and only root can read them."))
            .key("note")
            .font(DESCRIPTION)
            .color(p.faint())
            .mx(ROW_PX)
            .mt(-8.0),
    )
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

fn user_on(ui: &Ui, key: &str, default: bool) -> bool {
    face_unlock::keyvalue::parse_bool(ui.snap.user.get(key, ""), default)
}

fn set_user(s: &mut Ui, key: &'static str, v: String) {
    s.set_user(key, v);
}

fn set_sys(s: &mut Ui, key: &'static str, v: String) {
    s.set_sys(key, v);
}

/// A switch for a system setting: a spinner while the password is asked
/// for.
fn sys_switch(ui: &Ui, p: Paint, key: &'static str, title: &str, description: &str, default: bool) -> El {
    let on = face_unlock::keyvalue::parse_bool(ui.snap.sys.get(key, ""), default);
    if ui.is_busy(key) {
        return setting_row(p, key, title, Some(description), Spinner::new().size(18.0).build(p.k).key("spinner"));
    }
    switch_row(p, key, title, Some(description), on, ui.busy.is_none(), move |s: &mut Ui| s.toggle_sys(key, on))
}

fn pam_switch(ui: &Ui, p: Paint, key: &'static str, title: &str, description: &str, on: bool, available: bool) -> El {
    if ui.is_busy(key) {
        return setting_row(p, key, title, Some(description), Spinner::new().size(18.0).build(p.k).key("spinner"));
    }
    switch_row(p, key, title, Some(description), on, available && ui.busy.is_none(), move |s: &mut Ui| s.set_pam(key, !on))
}

pub fn lock_screen(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    if ui.sub == Some(Mode::ChooseLock) {
        return crate::choice::lock(ui, p);
    }
    let s = &ui.snap;
    let lock = user_on(ui, "LockScreen", true);
    let help_lock = tr("Opens the lock screen when it sees you.");
    let mut out = page("lock").child(section(
        p,
        "unlock",
        tr("Lock screen"),
        vec![
            switch_row(p, "LockScreen", tr("Unlock with face"), Some(help_lock), lock, true, |s: &mut Ui| s.toggle_user("LockScreen")),
            switch_row(
                p,
                "ScanOnWake",
                tr("Scan on wake"),
                Some(tr("When you press a key or move the mouse.")),
                user_on(ui, "ScanOnWake", true),
                lock,
                |s: &mut Ui| s.toggle_user("ScanOnWake"),
            ),
            switch_row(
                p,
                "ScanOnLock",
                tr("Scan right after locking"),
                Some(tr("Unlocks again at once if you are still there.")),
                user_on(ui, "ScanOnLock", false),
                lock,
                |s: &mut Ui| s.toggle_user("ScanOnLock"),
            ),
        ],
    ));
    if s.desktop.own_locker() {
        let own = s.cfg.lock_screen_style == "own";
        let style = match s.cfg.lock_screen_style.as_str() {
            "own" => tr("face-unlock's"),
            "" => tr("not picked yet"),
            _ => tr("yours"),
        };
        let choose = button(p, "choose", tr("Choose…"), Kind::Secondary, true, |s: &mut Ui| {
            s.load_wallpapers();
            s.sub = Some(Mode::ChooseLock);
        });
        let save = button(p, "save", tr("Save"), Kind::Secondary, ui.wallpaper.text.trim() != s.cfg.lock_wallpaper, |s: &mut Ui| {
            let v = s.wallpaper.text.trim().to_string();
            s.set_user("LockWallpaper", v);
        });
        let field = TextInput::new(&ui.wallpaper)
            .placeholder(tr("same as desktop"))
            .label(tr("Wallpaper"))
            .width(Some(220.0))
            .on_edit(|s: &mut Ui| &mut s.wallpaper)
            .on_submit(|s: &mut Ui, _| {
                let v = s.wallpaper.text.trim().to_string();
                s.set_user("LockWallpaper", v);
            })
            .build(p.k)
            .key("field");
        let wall = row().key("wall").gap(8.0).center_y().child(field).child(save);
        let mut rows = vec![
            setting_row(
                p,
                "LockScreenStyle",
                &format!("{}: {style}", tr("Which lock screen")),
                Some(tr("face-unlock's own one shows the bubble.")),
                choose,
            ),
            setting_row(
                p,
                "LockWallpaper",
                tr("Wallpaper"),
                Some(tr("Empty: same as the desktop. Or a file, a folder or `none`.")),
                wall,
            ),
            switch_row(p, "LockBlur", tr("Blur wallpaper"), Some(tr("Softens the picture behind the clock.")), user_on(ui, "LockBlur", false), own, |s: &mut Ui| s.toggle_user("LockBlur")),
        ];
        if !own {
            rows.remove(2);
            rows.remove(1);
        }
        out = out.child(section(p, "which", tr("Which lock screen"), rows));
        if own {
            let hint = face_unlock::system::own_lock_hint();
            let mut lines = col().key("hint").gap(6.0).mx(ROW_PX).child(text(hint.note).key("note").font(DESCRIPTION).color(p.muted()).wrap());
            for (i, c) in hint.code.iter().enumerate() {
                lines = lines.child(code(p, &format!("c{i}"), c));
            }
            out = out.child(lines);
        }
    }
    let _ = layer;
    out
}

pub fn prompts(ui: &Ui, p: Paint) -> El {
    let s = &ui.snap;
    let mut rows = vec![
        pam_switch(ui, p, "sudo", tr("Terminal (sudo)"), tr("Your face instead of the password for sudo."), s.pam.sudo, s.pam.sudo_available),
        pam_switch(
            ui,
            p,
            "polkit-1",
            tr("Admin prompts"),
            tr("Password windows, for example when installing apps."),
            s.pam.polkit,
            s.pam.polkit_available,
        ),
    ];
    if !s.pam.lockers.is_empty() {
        let help = trf("Enter on the empty field scans. Found: %s.", &[&s.pam.lockers.join(", ")]);
        rows.push(pam_switch(ui, p, "lockscreens", tr("Lock screens"), &help, s.pam.lockers_on, true));
    }
    rows.push(sys_switch(
        ui,
        p,
        "SshSessions",
        tr("Over SSH"),
        tr("sudo over SSH, when you sit at this computer."),
        false,
    ));
    page("prompts").child(section_note(p, "pam", tr("Password prompts"), tr("for all users, asks for your password"), rows))
}

pub fn recognition(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let s = &ui.snap;
    let free = ui.busy.is_none();
    let liveness = s.sys.get("Liveness", "light");
    let strict = s.sys.get("Strictness", "normal");
    let liveness_help = match liveness {
        "heavy" => tr("Also blocks printed photos. Needs a blink."),
        "off" => tr("No check. Only for testing a camera."),
        _ => tr("Blocks photos on screens. No blinking needed."),
    };
    let strict_help = match strict {
        "strict" => tr("Fewer mix-ups, but harder with glasses."),
        "relaxed" => tr("Knows you faster, but also look-alikes."),
        _ => tr("Recommended for most people."),
    };
    let pairs = |v: &[(&str, &str)]| v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
    let mut cameras = vec![(
        "auto".to_string(),
        match s.cameras.iter().find(|c| c.path == s.auto_camera) {
            Some(c) => face_unlock::i18n::fill(tr("automatic (%s)"), &[&c.name]),
            None => tr("automatic").into(),
        },
    )];
    for c in &s.cameras {
        let mut l = c.name.clone();
        if c.infrared {
            l.push(' ');
            l.push_str(tr("(infrared)"));
        }
        cameras.push((c.path.clone(), l));
    }
    let camera = s.sys.get("Camera", "auto");
    if !cameras.iter().any(|(v, _)| v == camera) {
        cameras.push((camera.to_string(), format!("{camera} {}", tr("(not connected)"))));
    }
    let secs: Vec<(String, String)> = ["3", "4", "5", "6", "8", "10"].iter().map(|v| (v.to_string(), trf("%s seconds", &[v]))).collect();
    let busy_row = |key: &'static str, title: &str| setting_row(p, key, title, None, Spinner::new().size(18.0).build(p.k).key("spinner"));
    let mut rows = vec![];
    rows.push(if ui.is_busy("Liveness") {
        busy_row("Liveness", tr("Anti-spoofing"))
    } else {
        segment_row(p, "Liveness", tr("Anti-spoofing"), Some(liveness_help), liveness, pairs(&[("off", tr("off")), ("light", tr("basic")), ("heavy", tr("strict"))]), free, set_sys)
    });
    rows.push(if ui.is_busy("Strictness") {
        busy_row("Strictness", tr("Strictness"))
    } else {
        segment_row(
            p,
            "Strictness",
            tr("Strictness"),
            Some(strict_help),
            strict,
            pairs(&[("relaxed", tr("relaxed")), ("normal", tr("normal")), ("strict", tr("strict"))]),
            free,
            set_sys,
        )
    });
    rows.push(sys_switch(ui, p, "Attention", tr("Require attention"), tr("Only unlocks when you look at the screen."), true));
    rows.push(if ui.is_busy("Camera") {
        busy_row("Camera", tr("Camera"))
    } else {
        select_row(
            ui,
            p,
            layer,
            "Camera",
            tr("Camera"),
            Some(tr("After a change, set up your face again.")),
            camera,
            cameras,
            free,
            set_sys,
        )
    });
    rows.push(if ui.is_busy("ScanSeconds") {
        busy_row("ScanSeconds", tr("Scan time"))
    } else {
        select_row(ui, p, layer, "ScanSeconds", tr("Scan time"), Some(tr("How long it looks for your face.")), s.sys.get("ScanSeconds", "5"), secs, free, set_sys)
    });
    rows.push(sys_switch(ui, p, "Adapt", tr("Adapt to changes"), tr("Keeps up with new glasses, beards or haircuts."), true));
    rows.push(sys_switch(ui, p, "SkipLidClosed", tr("Pause when lid is closed"), tr("No scan while the laptop is shut."), true));
    page("recognition").child(section_note(p, "rec", tr("Recognition"), tr("for all users, asks for your password"), rows))
}

pub fn bubble(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    if ui.sub == Some(Mode::ChooseScreen) {
        return crate::choice::screens(ui, p);
    }
    let s = &ui.snap;
    let on = user_on(ui, "Bubble", true);
    let pairs = |v: &[(&str, &str)]| v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
    let gnome = s.desktop == Desktop::Gnome;
    let show_help = if gnome { tr("Uses a small GNOME extension.") } else { tr("Shows at the top what the camera is doing.") };
    let style = s.user.get("BubbleStyle", "full");
    let screen_label = if s.cfg.bubble_screen.is_empty() {
        tr("follows the mouse").to_string()
    } else if s.screens.contains(&s.cfg.bubble_screen) || s.screens.is_empty() {
        s.cfg.bubble_screen.clone()
    } else {
        format!("{} {}", s.cfg.bubble_screen, tr("(not connected)"))
    };
    let choose = button(p, "choose", tr("Choose…"), Kind::Secondary, on, |s: &mut Ui| s.sub = Some(Mode::ChooseScreen));
    page("bubble").child(section(
        p,
        "bubble",
        tr("Bubble"),
        vec![
            switch_row(p, "Bubble", tr("Show bubble"), Some(show_help), on, true, |s: &mut Ui| s.toggle_user("Bubble")),
            select_row(
                ui,
                p,
                layer,
                "BubbleStyle",
                tr("Style"),
                Some(tr("How the bubble looks.")),
                style,
                pairs(&[("full", tr("island with the face")), ("minimal", tr("small pill with a lock"))]),
                on,
                set_user,
            ),
            segment_row(
                p,
                "AnimationSpeed",
                tr("Animation speed"),
                Some(tr("How fast the bubble moves.")),
                s.user.get("AnimationSpeed", "normal"),
                pairs(&[("slow", tr("slow")), ("normal", tr("normal")), ("fast", tr("fast"))]),
                on,
                set_user,
            ),
            setting_row(p, "BubbleScreen", &format!("{}: {screen_label}", tr("Screen")), Some(tr("Where the bubble shows up.")), choose),
            switch_row(
                p,
                "BubbleForPrompts",
                tr("Show for sudo and admin prompts"),
                Some(tr("Not only on the lock screen.")),
                user_on(ui, "BubbleForPrompts", true),
                on,
                |s: &mut Ui| s.toggle_user("BubbleForPrompts"),
            ),
            switch_row(
                p,
                "QuietWhenCameraBusy",
                tr("Hide when camera is in use"),
                Some(tr("For example during a video call.")),
                user_on(ui, "QuietWhenCameraBusy", false),
                on,
                |s: &mut Ui| s.toggle_user("QuietWhenCameraBusy"),
            ),
        ],
    ))
}
