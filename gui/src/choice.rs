//! The two questions with pictures: which lock screen (where it is a program
//! of its own, Hyprland and Niri), and which screen the bubble shows on.

use crate::controls::{DESCRIPTION, Kind, Paint, button_tx};
use crate::icons;
use crate::ui::{Mode, Ui};
use face_unlock::i18n::tr;
use rune::{Color, El, TextStyle, Tx, col, el, icon, row, text};

fn rgb(hex: &str) -> Color {
    rune::color::hex(hex).unwrap_or(Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 })
}

fn now_hm() -> String {
    let t = face_unlock::text::now();
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: valid pointers.
    unsafe { libc::localtime_r(&(t as libc::time_t), &mut tm) };
    format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
}

/// A small lock screen: the picture, the time, and what shows on it.
fn mock(ui: &Ui, p: Paint, key: &'static str, picture: Option<&rune::Image>, own: bool) -> El {
    let (w, h) = (300.0, 188.0);
    let mut screen = el().key(key).size(w, h).radius(10.0).clip_soft(0.0).fill(rgb("#21263a"));
    if let Some(img) = picture {
        screen = screen.child(el().key("picture").size(w, h).absolute(0.0, 0.0).pixels_fit(img.pixels(), rune::SurfaceFit::Fill).opacity(if own && ui.snap.cfg.lock_blur { 0.85 } else { 1.0 }));
    } else {
        screen = screen.linear_gradient(2.6, [(0.0, rgb("#2e3857")), (1.0, rgb("#12141f"))]);
    }
    let white = rgb("#fffffff2");
    let clock = text(now_hm()).key("clock").font(TextStyle::new(40.0, 600.0).lh(44.0)).color(white);
    let field = el().key("field").size(120.0, 22.0).radius(11.0).fill(rgb("#ffffff2e"));
    let body = col().key("body").w(w).absolute(0.0, 54.0).gap(10.0).items_center().child(clock).child(field);
    screen = screen.child(body);
    if own {
        // The bubble, at the top.
        let pill = row()
            .key("bubble")
            .h(22.0)
            .px(10.0)
            .gap(6.0)
            .radius(11.0)
            .center_y()
            .fill(rgb("#000000"))
            .child(icon(&icons::SCAN_FACE, 12.0).key("face").color(rgb("#ffffff")))
            .child(el().key("dot").size(6.0, 6.0).radius(3.0).fill(p.accent));
        screen = screen.child(row().key("top").w(w).absolute(0.0, 8.0).justify_center().child(pill));
    } else {
        // hyprlock's line of text.
        screen = screen.child(
            row()
                .key("top")
                .w(w)
                .absolute(0.0, 14.0)
                .justify_center()
                .child(text(tr("Looking for your face…")).key("line").font(TextStyle::new(11.0, 500.0)).color(white)),
        );
    }
    screen
}

fn option(p: Paint, key: &'static str, picked: bool, picture: El, title: &str, description: &str, pick: impl Fn(&mut Ui) + 'static) -> El {
    col()
        .key(key)
        .role("radio")
        .checked(picked)
        .label(title)
        .focusable(true)
        .focus_ring(2.0, 2.0, p.accent)
        .on_press(pick)
        .w(332.0)
        .padding(16.0)
        .gap(12.0)
        .radius(rune::tok::radius::LG)
        .fill(p.t.card)
        .border(if picked { 2.0 } else { p.hair }, if picked { p.accent } else { p.t.card_border })
        .shadow(1.0, 2.0, p.t.shadow)
        .shadow(8.0, 24.0, p.t.shadow_soft)
        .hover(p.t.hover)
        .motion(rune::tok::motion::SNAPPY)
        .child(picture)
        .child(text(title).key("title").font(TextStyle::new(13.0, 600.0)).color(p.text()))
        .child(crate::controls::prose(p, description).key("description").font(DESCRIPTION).color(p.muted()).wrap())
}

fn buttons(p: Paint, cancel: impl Fn(&mut Ui, &mut Tx) + 'static, ok_label: &str, ok: impl Fn(&mut Ui, &mut Tx) + 'static) -> El {
    row()
        .key("buttons")
        .gap(10.0)
        .self_center()
        .child(button_tx(p, "cancel", tr("Cancel"), Kind::Plain, true, cancel).h(36.0).px(18.0))
        .child(button_tx(p, "ok", ok_label, Kind::Primary, true, ok).h(36.0).px(22.0))
}

/// The lock screen question.
pub fn lock(ui: &Ui, p: Paint) -> El {
    let own = ui.lock_pick == "own";
    let own_pic = ui.wallpaper_image("own");
    let yours_pic = ui.wallpaper_image("desktop");
    let cards = row()
        .key("cards")
        .gap(20.0)
        .self_center()
        .flex_wrap()
        .child(option(
            p,
            "own",
            own,
            mock(ui, p, "own-pic", own_pic, true),
            tr("face-unlock's lock screen"),
            tr("Shows the bubble while it looks for your face. You lock with `face-unlock lock`."),
            |s: &mut Ui| s.lock_pick = "own".into(),
        ))
        .child(option(
            p,
            "yours",
            !own,
            mock(ui, p, "yours-pic", yours_pic, false),
            tr("Your own lock screen"),
            tr("Stays as it is. hyprlock shows a line of text at the top, and Enter on the empty password field scans."),
            |s: &mut Ui| s.lock_pick = "yours".into(),
        ));
    let ok = |s: &mut Ui, tx: &mut Tx| match s.mode {
        Mode::ChooseLock => {
            crate::ui::answer(0, &s.lock_pick.clone());
            tx.quit();
        }
        Mode::Onboarding => s.next_step(),
        _ => {
            let pick = s.lock_pick.clone();
            s.set_lock_style(pick);
            s.sub = None;
        }
    };
    let cancel = |s: &mut Ui, tx: &mut Tx| match s.mode {
        Mode::ChooseLock => {
            crate::ui::answer(1, "");
            tx.quit();
        }
        Mode::Onboarding => s.next_step(),
        _ => s.sub = None,
    };
    col()
        .key("lock-choice")
        .w_full()
        .pt(12.0)
        .pb(24.0)
        .gap(20.0)
        .child(
            col()
                .key("intro")
                .w(560.0)
                .self_center()
                .gap(8.0)
                .child(text(tr("Which lock screen?")).key("title").font(TextStyle::new(22.0, 650.0).lh(28.0)).color(p.text()).align(rune::Align::Center))
                .child(text(tr("Both take your face. You can change this later under Lock screen.")).key("text").font(crate::controls::TITLE).color(p.muted()).align(rune::Align::Center).wrap()),
        )
        .child(cards)
        .child(buttons(p, cancel, tr("Use this one"), ok))
}

/// The screen question: the screens drawn to scale, and "the one with the
/// mouse".
pub fn screens(ui: &Ui, p: Paint) -> El {
    let pick = ui.screen_pick.clone();
    // The screens as the compositor has them, else the names the kernel
    // knows, side by side.
    let mut boxes: Vec<(String, f64, f64, f64, f64)> = ui
        .displays
        .iter()
        .filter_map(|d| {
            let b = d.native_bounds();
            Some((d.name.clone()?, b[0], b[1], b[2], b[3]))
        })
        .collect();
    if boxes.is_empty() {
        boxes = ui.snap.screens.iter().enumerate().map(|(i, n)| (n.clone(), i as f64 * 1700.0, 0.0, 1600.0, 900.0)).collect();
    }
    let (minx, miny) = boxes.iter().fold((f64::MAX, f64::MAX), |a, b| (a.0.min(b.1), a.1.min(b.2)));
    let (maxx, maxy) = boxes.iter().fold((f64::MIN, f64::MIN), |a, b| (a.0.max(b.1 + b.3), a.1.max(b.2 + b.4)));
    let (area_w, area_h) = (560.0f64, 240.0f64);
    let scale = if boxes.is_empty() { 1.0 } else { (area_w / (maxx - minx)).min(area_h / (maxy - miny)) * 0.92 };
    let off_x = (area_w - (maxx - minx) * scale) / 2.0;
    let off_y = (area_h - (maxy - miny) * scale) / 2.0;
    let mut stage = el().key("stage").size(area_w as f32, area_h as f32).self_center();
    for (name, x, y, w, h) in &boxes {
        let picked = pick == *name;
        let n = name.clone();
        let laptop = name.starts_with("eDP") || name.starts_with("LVDS") || name.starts_with("DSI");
        stage = stage.child(
            col()
                .key(format!("screen-{name}"))
                .role("radio")
                .checked(picked)
                .label(name.clone())
                .focusable(true)
                .focus_ring(2.0, 2.0, p.accent)
                .on_press(move |s: &mut Ui| s.screen_pick = n.clone())
                .absolute((off_x + (x - minx) * scale) as f32 + 3.0, (off_y + (y - miny) * scale) as f32 + 3.0)
                .size((w * scale) as f32 - 6.0, (h * scale) as f32 - 6.0)
                .radius(8.0)
                .center()
                .gap(6.0)
                .fill(if picked { p.accent } else { p.t.card })
                .border(p.hair, p.t.card_border)
                .shadow(1.0, 2.0, p.t.shadow)
                .hover(p.t.hover)
                .motion(rune::tok::motion::SNAPPY)
                .child(icon(if laptop { &icons::LAPTOP } else { &icons::MONITOR }, 20.0).key("icon").color(if picked { rune::Color::WHITE } else { p.muted() }))
                .child(text(name.clone()).key("name").font(TextStyle::new(13.0, 600.0)).color(if picked { rune::Color::WHITE } else { p.text() })),
        );
    }
    let auto = pick == "auto";
    let follow = row()
        .key("auto")
        .role("radio")
        .checked(auto)
        .label(tr("follows the mouse"))
        .focusable(true)
        .focus_ring(2.0, 2.0, p.accent)
        .on_press(|s: &mut Ui| s.screen_pick = "auto".into())
        .self_center()
        .h(40.0)
        .px(16.0)
        .gap(10.0)
        .center_y()
        .radius(20.0)
        .fill(if auto { p.accent } else { p.t.control_off })
        .hover(p.t.hover)
        .motion(rune::tok::motion::SNAPPY)
        .child(icon(&icons::SPARKLE, 16.0).key("icon").color(if auto { rune::Color::WHITE } else { p.muted() }))
        .child(text(tr("The screen with the mouse pointer")).key("label").font(TextStyle::new(13.0, 600.0)).color(if auto { rune::Color::WHITE } else { p.text() }));
    let ok = |s: &mut Ui, tx: &mut Tx| {
        let pick = s.screen_pick.clone();
        match s.mode {
            Mode::ChooseScreen => {
                crate::ui::answer(0, &pick);
                tx.quit();
            }
            _ => {
                s.set_screen(pick);
                s.sub = None;
            }
        }
    };
    let cancel = |s: &mut Ui, tx: &mut Tx| match s.mode {
        Mode::ChooseScreen => {
            crate::ui::answer(1, "");
            tx.quit();
        }
        _ => s.sub = None,
    };
    col()
        .key("screen-choice")
        .w_full()
        .pt(12.0)
        .pb(24.0)
        .gap(20.0)
        .child(
            col()
                .key("intro")
                .w(560.0)
                .self_center()
                .gap(8.0)
                .child(text(tr("Where should the bubble show?")).key("title").font(TextStyle::new(22.0, 650.0).lh(28.0)).color(p.text()).align(rune::Align::Center))
                .child(text(tr("On the screen with the mouse pointer, or always on one screen.")).key("text").font(crate::controls::TITLE).color(p.muted()).align(rune::Align::Center).wrap()),
        )
        .child(stage)
        .child(follow)
        .child(buttons(p, cancel, tr("Use this one"), ok))
}
