//! The window, laid out like Waypointer's: a sidebar of panes, and the pane
//! with a big title next to it. The first start and the single questions
//! fill the window instead.

use crate::controls::{GUTTER, Paint};
use crate::ui::{Mode, Pane, Ui};
use face_unlock::i18n::{tr, trf};
use face_unlock::paths::VERSION;
use rune::tok::motion;
use rune::{Color, Cx, Drag, El, Phase, ScrollSpec, Spring, SpringPair, TextStyle, Transition, Tx, col, el, icon, row, text};

/// Sidebar rows, as in Waypointer: 30 pt tall with a 4 pt gap.
const NAV_ROW: f32 = 30.0;
const NAV_GAP: f32 = 4.0;
const NAV_WIDTH: f32 = 220.0;

pub fn view(ui: &Ui, cx: &mut Cx<Ui>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let mut layer = Vec::new();
    let full = |content: El| row().key("window").role("none").w_full().h_full().fill(p.t.bg).child(content);
    let root = match ui.mode {
        Mode::Onboarding => full(crate::onboarding::view(ui, p)),
        Mode::Setup => full(scroll(crate::setup::view(ui, p))),
        Mode::ChooseLock => full(scroll(crate::choice::lock(ui, p))),
        Mode::ChooseScreen => full(scroll(crate::choice::screens(ui, p))),
        Mode::Main => {
            let (actions, body) = match ui.pane {
                Pane::Overview => (crate::panes::overview_actions(ui, p), crate::panes::overview(ui, p)),
                Pane::Faces => (crate::panes::faces_actions(ui, p), crate::panes::faces(ui, p)),
                Pane::LockScreen => (vec![], crate::panes::lock_screen(ui, p, &mut layer)),
                Pane::Prompts => (vec![], crate::panes::prompts(ui, p)),
                Pane::Recognition => (vec![], crate::panes::recognition(ui, p, &mut layer)),
                Pane::Bubble => (vec![], crate::panes::bubble(ui, p, &mut layer)),
                Pane::Try => (vec![], crate::trial::pane(ui, p)),
            };
            let pane = col()
                .key(format!("pane-{}", ui.pane.key()))
                .role("main")
                .w_full()
                .child(header(p, ui.pane.title(), actions))
                .children(crate::panes::notices(ui, p))
                .child(body)
                // The old pane goes at once and the new one comes up
                // quickly, so the two never show over each other.
                .transition(
                    Transition::asymmetric(Transition::offset(0.0, 6.0).combined(Transition::opacity()), Transition::none())
                        .springs(SpringPair::new(Spring::new(0.28, 0.0), Spring::NONE)),
                );
            row()
                .key("window")
                .role("none")
                .w_full()
                .h_full()
                .fill(p.t.bg)
                .child(sidebar(ui, p))
                .child(el().key("divider").w(p.hair).h_full().fill(p.t.separator).a11y_hidden())
                .child(scroll(pane))
        }
    };
    rune_kit::root(root, layer)
}

/// The pane's big title, and its actions on the right.
fn header(p: Paint, title: &str, actions: Vec<El>) -> El {
    row()
        .key("pane-header")
        .min_h(36.0)
        .px(GUTTER)
        .pt(24.0)
        .pb(20.0)
        .gap(8.0)
        .center_y()
        .child(text(title).key("pane-title").role("heading").level(1).font(TextStyle::new(22.0, 700.0)).color(p.text()).shrink().truncate())
        .child(el().key("header-spacer").grow(1.0))
        .children(actions)
}

/// A scrolling body whose edges fade while content hides under them.
fn scroll(content: El) -> El {
    col()
        .key("scroll")
        .grow(1.0)
        .h_full()
        .min_h(0.0)
        .min_w(0.0)
        .scroll(ScrollSpec { stick_bottom: false })
        .scroll_edge_fade(16.0)
        .child(content)
}

fn sidebar(ui: &Ui, p: Paint) -> El {
    let index = Pane::ALL.iter().position(|x| *x == ui.pane).unwrap_or(0);
    let inner = NAV_WIDTH - 20.0;
    // One highlight under the rows that springs to the chosen one. The rows
    // themselves never change shape, so nothing in them jumps.
    let mut nav = col().key("nav").role("list").gap(NAV_GAP).focus_group().child(
        el().key("nav-highlight")
            .size(inner, NAV_ROW)
            .absolute(0.0, index as f32 * (NAV_ROW + NAV_GAP))
            .radius(7.0)
            .fill(if p.dark { Color::WHITE.alpha(0.075) } else { Color::BLACK.alpha(0.06) })
            .motion(motion::SNAPPY)
            .a11y_hidden(),
    );
    for pane in Pane::ALL {
        nav = nav.child(nav_item(p, inner, pane, ui.pane == pane, attention(ui, pane)));
    }
    col()
        .key("sidebar")
        .role("navigation")
        .w(NAV_WIDTH)
        .h_full()
        .pt(12.0)
        .pb(12.0)
        .px(10.0)
        .fill(if p.dark { Color::BLACK.alpha(0.12) } else { Color::BLACK.alpha(0.025) })
        .child(
            row()
                .key("brand")
                .h(44.0)
                .px(4.0)
                .mb(10.0)
                .gap(10.0)
                .center_y()
                .child(brand(ui, p))
                .child(text(tr("Face Unlock")).key("brand-name").font(TextStyle::new(14.0, 650.0)).color(p.text()).shrink().truncate()),
        )
        .child(nav)
        .child(el().key("spacer").grow(1.0))
        .child(text(trf("Version %s", &[&VERSION])).key("version").font(TextStyle::new(11.5, 400.0)).color(p.faint()).px(8.0))
}

fn brand(ui: &Ui, p: Paint) -> El {
    match ui.logo.as_ref().filter(|l| l.is_ready() && l.error().is_none()) {
        Some(logo) => el().key("logo").size(28.0, 28.0).pixels(logo.pixels()).a11y_hidden(),
        None => el()
            .key("logo")
            .size(28.0, 28.0)
            .radius(7.0)
            .center()
            .fill(p.accent)
            .a11y_hidden()
            .child(icon(&crate::icons::SCAN_FACE, 16.0).key("glyph").color(Color::WHITE)),
    }
}

/// A dot on a pane that wants a look.
fn attention(ui: &Ui, pane: Pane) -> bool {
    let s = &ui.snap;
    match pane {
        Pane::Overview => {
            ui.loaded
                && (!s.reachable()
                    || s.polkit_missing
                    || s.status.as_ref().is_some_and(|st| !st.models || st.lockout > 0)
                    || (s.cfg.enabled && s.cfg.lock_screen && s.agent_available && !s.agent_running))
        }
        Pane::Faces => ui.loaded && s.reachable() && s.faces.is_empty(),
        _ => false,
    }
}

fn nav_item(p: Paint, width: f32, pane: Pane, on: bool, dot: bool) -> El {
    row()
        .key(pane.key())
        .role("tab")
        .label(pane.title())
        .checked(on)
        .focusable(true)
        .focus_ring(2.0, -2.0, p.accent)
        .w(width)
        .h(NAV_ROW)
        .px(8.0)
        .gap(10.0)
        .center_y()
        .radius(7.0)
        .hover(if on { Color::TRANSPARENT } else { p.t.hover })
        .press(p.t.press)
        // Like a native sidebar: the pane changes on the press, not on the
        // release.
        .on_drag(move |s: &mut Ui, d: &mut Drag, _: &mut Tx| {
            if d.phase == Phase::Down && s.pane != pane {
                s.show_pane(pane);
            }
        })
        .on_press(move |s: &mut Ui| {
            if s.pane != pane {
                s.show_pane(pane);
            }
        })
        .child(icon(pane.icon(), 16.0).key("icon").color(if on { p.accent } else { p.muted() }).a11y_hidden())
        .child(
            text(pane.title())
                .key("label")
                .font(TextStyle::new(13.0, if on { 600.0 } else { 500.0 }))
                .color(if on { p.text() } else { p.muted() })
                .grow(1.0)
                .truncate()
                .a11y_hidden(),
        )
        .children(dot.then(|| crate::controls::dot(p.k.c.warning.icon)))
}
