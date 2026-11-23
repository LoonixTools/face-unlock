//! The settings look, as in Waypointer: cards of rows under quiet section
//! labels, a blue switch, flat buttons and segmented controls. Colours come
//! from Rune's own tokens. rune-kit's widgets (lists, text fields, spinners)
//! get the same colours through a recoloured kit.

use crate::ui::Ui;
use rune::tok::{self, Theme, motion, radius, space, type_};
use rune::{Color, El, Spring, TextStyle, Tx, col, el, icon, row, text};
use rune_kit::{Kit, Select};
use std::sync::OnceLock;

/// Row titles, and the quiet copy under them.
pub const TITLE: TextStyle = type_::BODY;
pub const DESCRIPTION: TextStyle = TextStyle::new(11.5, 400.0).lh(15.0);
const BUTTON: TextStyle = TextStyle::new(12.5, 600.0);

/// The page's edge, the gap between sections, and a row's inset in its card.
pub const GUTTER: f32 = 28.0;
pub const SECTION_GAP: f32 = 24.0;
pub const ROW_PX: f32 = space::S3;
const ROW_H: f32 = 46.0;
const SELECT_W: f32 = 220.0;
const MENU_CHAR_W: f32 = 8.0;
const MENU_EXTRA_W: f32 = 56.0;

/// Everything a control needs to paint itself.
#[derive(Clone, Copy)]
pub struct Paint {
    pub k: &'static Kit,
    pub t: Theme,
    pub accent: Color,
    pub dark: bool,
    pub hair: f32,
}

impl Paint {
    pub fn new(dark: bool, scale: f32) -> Self {
        Paint { k: kit(dark), t: tok::theme(dark), accent: tok::accent(0, dark), dark, hair: 1.0 / scale.max(1.0) }
    }
    pub fn text(self) -> Color {
        self.t.text
    }
    pub fn muted(self) -> Color {
        self.t.text_secondary
    }
    pub fn faint(self) -> Color {
        self.t.text_secondary.alpha(0.7)
    }
}

/// The OpenCode kit in Rune's colours, so its widgets match the rest.
fn kit(dark: bool) -> &'static Kit {
    static KITS: [OnceLock<Kit>; 2] = [OnceLock::new(), OnceLock::new()];
    KITS[dark as usize].get_or_init(|| {
        let mut k = rune_theme_opencode::kit(dark).clone();
        let t = tok::theme(dark);
        let a = tok::accent(0, dark);
        let c = &mut k.c;
        c.page = t.bg;
        c.bg_deep = t.bg;
        c.bg_base = t.card;
        c.surface_raised = t.card;
        // Open lists stand out from the card under them.
        c.layer_01 = if dark { rune::color::oklch(0.27, 0.005, 265.0) } else { t.card };
        c.layer_02 = t.control_off;
        c.text_base = t.text;
        c.text_strong = t.text;
        c.text_muted = t.text_secondary;
        c.text_weak = t.text_secondary;
        c.text_faint = t.text_secondary.alpha(0.7);
        c.icon_base = t.text;
        c.icon_muted = t.text_secondary;
        c.text_accent = a;
        c.icon_accent = a;
        c.bg_accent = a;
        c.border_focus = a;
        c.border_muted = t.separator;
        c.border_weak = t.card_border;
        c.border_base = t.card_border;
        c.hover = t.hover;
        c.pressed = t.press;
        k
    })
}

/// A page: a column of sections.
pub fn page(key: &str) -> El {
    col().key(key).px(GUTTER).pb(GUTTER + 8.0).gap(SECTION_GAP)
}

/// A section label over its card; screen readers move between them.
fn label(p: Paint, title: &str) -> El {
    text(title).key("header").role("heading").level(2).font(type_::SECTION).color(p.muted())
}

/// A titled card of rows.
pub fn section(p: Paint, key: &str, title: &str, rows: Vec<El>) -> El {
    col().key(key).gap(space::S2).child(label(p, title).mx(ROW_PX)).child(card(p, "card", rows))
}

/// A titled card with a quiet note at the end of the label's line.
pub fn section_note(p: Paint, key: &str, title: &str, note: &str, rows: Vec<El>) -> El {
    col()
        .key(key)
        .gap(space::S2)
        .child(
            row()
                .key("header-row")
                .mx(ROW_PX)
                .gap(8.0)
                .center_y()
                .child(label(p, title).grow(1.0))
                .child(text(note).key("note").font(DESCRIPTION).color(p.faint())),
        )
        .child(card(p, "card", rows))
}

/// The card's surface with hairlines between its rows.
pub fn card(p: Paint, key: &str, rows: Vec<El>) -> El {
    let mut c = raised(p, key);
    for (i, r) in rows.into_iter().enumerate() {
        if i > 0 {
            c = c.child(el().key(format!("sep{i}")).h(p.hair).mx(ROW_PX).fill(p.t.separator).a11y_hidden());
        }
        c = c.child(r);
    }
    c
}

/// A card's surface, empty.
pub fn raised(p: Paint, key: &str) -> El {
    col()
        .key(key)
        .padding(space::S1)
        .fill(p.t.card)
        .radius(radius::LG)
        .border(p.hair, p.t.card_border)
        .shadow(1.0, 2.0, p.t.shadow)
        .shadow(8.0, 24.0, p.t.shadow_soft)
}

fn copy(p: Paint, title: &str, description: Option<&str>) -> El {
    col()
        .key("copy")
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .gap(2.0)
        .child(text(title).key("title").font(TITLE).color(p.text()).wrap().a11y_hidden())
        .children(description.map(|d| prose(p, d).key("description").font(DESCRIPTION).color(p.muted()).wrap().a11y_hidden()))
}

/// A setting: title and description on the left, its control on the right.
pub fn setting_row(p: Paint, key: &str, title: &str, description: Option<&str>, control: El) -> El {
    let r = row().key(key).label(title).min_h(ROW_H).px(ROW_PX).py(8.0).gap(16.0).center_y().child(copy(p, title, description)).child(control);
    match description {
        Some(d) => r.description(d),
        None => r,
    }
}

/// A row with an icon at its start.
pub fn icon_row(p: Paint, key: &str, glyph: &'static rune::Icon, title: &str, description: Option<&str>, control: El) -> El {
    let lead = icon(glyph, 16.0).key("icon").color(p.muted()).a11y_hidden();
    row().key(key).label(title).min_h(ROW_H).px(ROW_PX).py(8.0).gap(12.0).center_y().radius(radius::MD).child(lead).child(copy(p, title, description)).child(control)
}

/// The switch itself: blue when on, its knob springs across.
pub fn switch(p: Paint, on: bool, enabled: bool) -> El {
    let (w, h, k) = (40.0, 24.0, 20.0);
    let inset = (h - k) / 2.0;
    el().key("switch")
        .size(w, h)
        .radius(h / 2.0)
        .fill(if on && enabled { p.accent } else { p.t.control_off })
        .motion(Spring::BOUNCY)
        .child(
            el().key("knob")
                .size(k, k)
                .absolute(if on { w - k - inset } else { inset }, inset)
                .radius(k / 2.0)
                .fill(p.t.knob)
                .shadow(1.0, 3.0, p.t.knob_shadow)
                .shadow(0.0, 0.5, p.t.edge),
        )
}

/// A switch row: the whole row toggles, the switch shows the state.
pub fn switch_row(p: Paint, key: &'static str, title: &str, description: Option<&str>, on: bool, enabled: bool, flip: impl Fn(&mut Ui) + 'static) -> El {
    let r = setting_row(p, key, title, description, switch(p, on, enabled).a11y_hidden()).role("switch").checked(on).radius(radius::MD);
    if enabled {
        r.hover(p.t.hover).press(p.t.press).focusable(true).focus_ring(2.0, -2.0, p.accent).on_press(flip)
    } else {
        r.opacity(0.5)
    }
}

/// What a button is for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The one thing to do next: blue.
    Primary,
    /// Anything else: grey.
    Secondary,
    /// Cancel and the like: no fill until the pointer is on it.
    Plain,
    /// Deleting: red.
    Danger,
}

fn button_el(p: Paint, key: &str, label: &str, kind: Kind, enabled: bool) -> El {
    let (fill, ink, hover) = match kind {
        Kind::Primary => (p.accent, Color::WHITE, Color::WHITE.alpha(0.08)),
        Kind::Danger => (p.k.c.danger.icon, Color::WHITE, Color::WHITE.alpha(0.08)),
        Kind::Secondary => (p.t.control_off, p.t.text, p.t.hover),
        Kind::Plain => (Color::TRANSPARENT, p.t.text, p.t.control_track),
    };
    let b = el()
        .key(key)
        .role("button")
        .label(label)
        .h(30.0)
        .px(14.0)
        .center()
        .radius(radius::MD)
        .fill(fill)
        .child(text(label).key("label").font(BUTTON).color(ink).a11y_hidden());
    if enabled {
        b.focusable(true).focus_ring(2.0, 2.0, p.accent).hover(hover).press(Color::BLACK.alpha(0.08)).press_scale(0.97)
    } else {
        b.disabled(true).opacity(0.45)
    }
}

/// A button, as in Waypointer: a flat rounded box, blue for the main action.
pub fn button(p: Paint, key: &str, label: &str, kind: Kind, enabled: bool, press: impl Fn(&mut Ui) + 'static) -> El {
    let b = button_el(p, key, label, kind, enabled);
    if enabled { b.on_press(press) } else { b }
}

/// [`button`] whose action needs the transaction.
pub fn button_tx(p: Paint, key: &str, label: &str, kind: Kind, enabled: bool, press: impl Fn(&mut Ui, &mut Tx) + 'static) -> El {
    let b = button_el(p, key, label, kind, enabled);
    if enabled { b.on_press_tx(press) } else { b }
}

/// A segmented control whose indicator springs to the chosen segment.
/// `choices` are (value, label); `set` gets the value.
fn segmented(p: Paint, key: &'static str, current: &str, choices: &[(String, String)], enabled: bool, set: fn(&mut Ui, &'static str, String)) -> El {
    let (h, pad) = (28.0, 2.0);
    let selected = choices.iter().position(|(v, _)| v == current);
    // Wide enough for the longest label, as some languages need.
    let widest = choices.iter().map(|(_, l)| rune::text::measure(l, &[], type_::LABEL, None).0).fold(0.0, f32::max);
    let seg = (widest + 24.0).max(56.0);
    let width = seg * choices.len() as f32 + 2.0 * pad;
    let mut segs = row().key("segments");
    for (i, (value, name)) in choices.iter().enumerate() {
        let on = selected == Some(i);
        let mut s = el()
            .key(format!("{key}-{i}"))
            .role("radio")
            .label(name.as_str())
            .checked(on)
            .focusable(enabled)
            .size(seg, h - 2.0 * pad)
            .center()
            .radius(radius::SM)
            .child(text(name.as_str()).key("label").font(type_::LABEL).color(if on { p.t.text } else { p.t.text_secondary }).a11y_hidden());
        if enabled {
            let value = value.clone();
            s = s.hover(if on { Color::TRANSPARENT } else { p.t.hover }).on_press_tx(move |st: &mut Ui, tx: &mut Tx| {
                set(st, key, value.clone());
                tx.color_motion(motion::CALM);
            });
        }
        segs = segs.child(s);
    }
    let mut c = el().key(key).role("radiogroup").size(width, h).padding(pad).radius(radius::MD).fill(p.t.control_track).motion(motion::SNAPPY);
    if let Some(i) = selected {
        c = c.child(
            el().key("indicator")
                .size(seg, h - 2.0 * pad)
                .absolute(pad + i as f32 * seg, pad)
                .radius(radius::SM)
                .fill(p.t.indicator)
                .shadow(1.0, 3.0, p.t.shadow)
                .shadow(0.0, 0.5, p.t.edge)
                .lead(motion::LEAD, motion::DEFAULT)
                .a11y_hidden(),
        );
    }
    c = c.child(segs);
    if !enabled {
        c = c.opacity(0.5);
    }
    c
}

/// A row that picks one of a few short values with a segmented control.
#[allow(clippy::too_many_arguments)]
pub fn segment_row(
    p: Paint,
    key: &'static str,
    title: &str,
    description: Option<&str>,
    current: &str,
    choices: Vec<(String, String)>,
    enabled: bool,
    set: fn(&mut Ui, &'static str, String),
) -> El {
    setting_row(p, key, title, description, segmented(p, key, current, &choices, enabled, set))
}

/// A row that chooses one of a few values. `choices` are (value, label).
#[allow(clippy::too_many_arguments)]
pub fn select_row(
    ui: &Ui,
    p: Paint,
    layer: &mut Vec<El>,
    key: &'static str,
    title: &str,
    description: Option<&str>,
    current: &str,
    choices: Vec<(String, String)>,
    enabled: bool,
    set: fn(&mut Ui, &'static str, String),
) -> El {
    let labels: Vec<String> = choices.iter().map(|(_, l)| l.clone()).collect();
    let shown = choices.iter().find(|(v, _)| v == current).map(|(_, l)| l.clone()).unwrap_or_else(|| current.to_string());
    let labels = if labels.contains(&shown) { labels } else { [vec![shown.clone()], labels].concat() };
    let open = enabled && ui.open == Some(key);
    let select = Select::new(format!("{key}-select"), &labels)
        .current(Some(&shown))
        .label(title)
        .width(SELECT_W)
        .disabled(!enabled)
        .open(open)
        .on_toggle(move |s: &mut Ui| s.open = if s.open == Some(key) { None } else { Some(key) })
        .on_close(|s: &mut Ui| s.open = None)
        .on_choose(move |s: &mut Ui, label: &str, _| {
            s.open = None;
            if let Some((v, _)) = choices.iter().find(|(_, l)| l == label) {
                set(s, key, v.clone());
            }
        })
        .build(p.k, layer)
        // Filled and as tall as the buttons, as in Waypointer.
        .h(30.0)
        .pl(12.0)
        .pr(8.0)
        .fill(p.t.control_off)
        .radius(radius::MD);
    // Rune's menu does not grow for its labels, so it gets the width of the
    // longest one. The menu is what it put last.
    if open && let Some(menu) = layer.pop() {
        let longest = labels.iter().map(|l| l.chars().count()).max().unwrap_or(0) as f32;
        layer.push(menu.min_w((longest * MENU_CHAR_W + MENU_EXTRA_W).clamp(160.0, 368.0)));
    }
    let r = setting_row(p, key, title, description, select);
    if enabled { r } else { r.opacity(0.5) }
}

/// A small coloured dot, for how something stands.
pub fn dot(color: Color) -> El {
    el().key("dot").size(8.0, 8.0).radius(4.0).fill(color).a11y_hidden()
}

/// Copy where `code` in backticks shows as inline code.
pub fn prose(p: Paint, s: &str) -> El {
    let mut clean = String::with_capacity(s.len());
    let mut spans = Vec::new();
    for (i, part) in s.split('`').enumerate() {
        let start = clean.len();
        clean.push_str(part);
        if i % 2 == 1 && !part.is_empty() {
            spans.push(rune::Span::new(start, clean.len()).mono().bg(p.t.control_off, 2.0));
        }
    }
    text(clean).spans(spans)
}

/// A line of copy to select and paste, in mono.
pub fn code(p: Paint, key: &str, line: &str) -> El {
    row()
        .key(key)
        .px(10.0)
        .py(6.0)
        .radius(radius::SM)
        .fill(p.t.control_track)
        .child(text(line).key("code").font(p.k.ty.mono).color(p.text()).selectable(true).selection_color(p.k.selection()).wrap())
}
