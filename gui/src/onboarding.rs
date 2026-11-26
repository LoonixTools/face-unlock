//! The first start: one question at a time, on a page of its own. Welcome,
//! the service, the face, which lock screen, where the face counts, done.

use crate::controls::{DESCRIPTION, Kind, Paint, button, card, code, switch_row};
use crate::icons;
use crate::ui::{Step, Ui};
use face_unlock::actions::Notice;
use face_unlock::i18n::tr;
use rune::{Color, El, TextStyle, col, el, icon, row, text};
use rune_kit::Spinner;

/// A step's picture: a blue tile with a symbol.
fn badge(p: Paint, glyph: &'static rune::Icon) -> El {
    el().key("badge")
        .size(76.0, 76.0)
        .radius(22.0)
        .center()
        .self_center()
        .fill(p.accent)
        .shadow(6.0, 18.0, p.accent.alpha(0.35))
        .child(icon(glyph, 40.0).key("glyph").color(Color::WHITE))
}

/// The app icon, large, for the welcome.
fn app_icon(ui: &Ui, p: Paint) -> El {
    match ui.logo_big.as_ref().filter(|l| l.is_ready() && l.error().is_none()) {
        Some(logo) => el().key("badge").size(104.0, 104.0).self_center().pixels(logo.pixels()).a11y_hidden(),
        None => badge(p, &icons::SCAN_FACE),
    }
}

fn heading(p: Paint, badge: El, title: &str, body: &str) -> El {
    col()
        .key("heading")
        .w(460.0)
        .self_center()
        .gap(12.0)
        .child(badge)
        .child(text(title).key("title").font(TextStyle::new(26.0, 650.0).lh(32.0)).color(p.text()).align(rune::Align::Center).wrap().mt(8.0))
        .child(text(body).key("body").font(TextStyle::new(14.0, 440.0).lh(20.0)).color(p.muted()).align(rune::Align::Center).wrap())
}

fn primary(p: Paint, key: &'static str, label: &str, busy: bool, press: impl Fn(&mut Ui) + 'static) -> El {
    if busy {
        return Spinner::new().size(22.0).build(p.k).key(key).self_center();
    }
    button(p, key, label, Kind::Primary, true, press).h(36.0).px(22.0).self_center()
}

fn dots(p: Paint, step: Step) -> El {
    let all = [Step::Welcome, Step::Service, Step::Face, Step::LockScreen, Step::Places, Step::Done];
    let at = all.iter().position(|s| *s == step).unwrap_or(0);
    let mut r = row().key("dots").gap(6.0).self_center().a11y_hidden();
    for i in 0..all.len() {
        r = r.child(el().key(format!("d{i}")).size(if i == at { 18.0 } else { 6.0 }, 6.0).radius(3.0).fill(if i <= at { p.accent } else { p.t.control_off }));
    }
    r
}

fn notices(ui: &Ui, p: Paint) -> El {
    let mut list = col().key("notices").w(460.0).self_center().gap(8.0);
    for (i, n) in ui.notices.iter().enumerate() {
        list = list.child(match n {
            Notice::Code(m) => code(p, &format!("c{i}"), m),
            Notice::Ok(m) | Notice::Note(m) | Notice::Bad(m) => {
                let (glyph, color) = match n {
                    Notice::Ok(_) => (&icons::CHECK_CIRCLE, p.k.c.success.fg),
                    Notice::Bad(_) => (&icons::WARNING, p.k.c.danger.fg),
                    _ => (&icons::INFO, p.k.c.icon_muted),
                };
                row()
                    .key(format!("n{i}"))
                    .gap(8.0)
                    .items_start()
                    .child(icon(glyph, 16.0).key("icon").color(color).mt(1.0))
                    .child(text(m.clone()).key("text").font(DESCRIPTION).color(p.text()).wrap().grow(1.0).shrink())
            }
        });
    }
    list
}

pub fn view(ui: &Ui, p: Paint) -> El {
    let s = &ui.snap;
    let body = match ui.step {
        Step::Welcome => col()
            .key("welcome")
            .gap(28.0)
            .child(heading(
                p,
                app_icon(ui, p),
                tr("Face ID for Linux"),
                tr("Unlock the lock screen, sudo and admin prompts with your face. It takes a minute to set up."),
            ))
            .child(primary(p, "next", tr("Get started"), false, |s: &mut Ui| s.next_step())),
        Step::Service => {
            let failed = ui.notices.iter().any(|n| matches!(n, Notice::Bad(_)));
            col()
                .key("service")
                .gap(24.0)
                .child(heading(
                    p,
                    badge(p, &icons::POWER),
                    tr("Switch on the service"),
                    tr("Face unlock's service has to be switched on once for this computer. That needs your password."),
                ))
                .children(failed.then(|| notices(ui, p)))
                .children((failed && s.polkit_missing).then(|| {
                    col()
                        .key("manual")
                        .w(460.0)
                        .self_center()
                        .gap(6.0)
                        .child(text(tr("No password window? Run this in a terminal instead:")).key("t").font(DESCRIPTION).color(p.muted()))
                        .child(code(p, "cmd", "sudo systemctl enable --now face-unlockd.socket"))
                }))
                .child(
                    row()
                        .key("buttons")
                        .gap(10.0)
                        .self_center()
                        .children((!ui.is_busy("service") && s.reachable()).then(|| button(p, "next", tr("Continue"), Kind::Primary, true, |s: &mut Ui| s.next_step()).h(36.0).px(22.0)))
                        .children((!s.reachable()).then(|| primary(p, "switch-on", tr("Switch on"), ui.is_busy("service"), |s: &mut Ui| s.start_service()))),
                )
        }
        Step::Face => crate::setup::view(ui, p),
        Step::LockScreen => crate::choice::lock(ui, p),
        Step::Places => {
            let [sudo, polkit, lockers] = ui.places;
            let mut rows = vec![];
            if s.pam.sudo_available {
                rows.push(switch_row(p, "sudo", tr("Terminal (sudo)"), Some(tr("Your face instead of the password for sudo.")), sudo, true, |s: &mut Ui| s.places[0] = !s.places[0]));
            }
            if s.pam.polkit_available {
                rows.push(switch_row(
                    p,
                    "polkit-1",
                    tr("Admin prompts"),
                    Some(tr("Password windows, for example when installing apps.")),
                    polkit,
                    true,
                    |s: &mut Ui| s.places[1] = !s.places[1],
                ));
            }
            if !s.pam.lockers.is_empty() && s.cfg.lock_screen_style != "own" && ui.lock_pick != "own" {
                rows.push(switch_row(
                    p,
                    "lockscreens",
                    tr("Lock screens"),
                    Some(tr("Enter on the empty field scans.")),
                    lockers,
                    true,
                    |s: &mut Ui| s.places[2] = !s.places[2],
                ));
            }
            col()
                .key("places")
                .gap(24.0)
                .child(heading(
                    p,
                    badge(p, &icons::SHIELD),
                    tr("Where should your face count?"),
                    tr("The lock screen always takes it. The password always works too. You can change all of this later."),
                ))
                .child(card(p, "list", rows).w(480.0).self_center())
                .child(primary(p, "finish", tr("Turn on face unlock"), ui.is_busy("finish"), |s: &mut Ui| s.finish_onboarding()))
        }
        Step::Done => {
            let failed = ui.notices.iter().any(|n| matches!(n, Notice::Bad(_)));
            col()
                .key("done")
                .gap(24.0)
                .child(heading(
                    p,
                    badge(p, if failed { &icons::WARNING } else { &icons::CHECK_CIRCLE }),
                    if failed { tr("Almost done") } else { tr("You are all set") },
                    tr("Lock the screen and look at it to try it out. Everything else is in the settings."),
                ))
                .child(notices(ui, p))
                .child(primary(p, "open", tr("Open the settings"), false, |s: &mut Ui| s.next_step()))
        }
    };
    col()
        .key("onboarding")
        .w_full()
        .h_full()
        .min_h(0.0)
        .scroll(rune::ScrollSpec { stick_bottom: false })
        .child(col().key("inner").w_full().min_h(600.0).pt(40.0).pb(32.0).gap(28.0).center_y().child(body).child(dots(p, ui.step)))
}
