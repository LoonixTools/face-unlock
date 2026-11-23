//! Try it: one scan, with what every check sees while it runs. The bubble
//! shows it too, when the agent runs.

use crate::controls::{GUTTER, Kind, Paint, ROW_PX, TITLE, button, card, page, section, setting_row};
use crate::ui::Ui;
use face_unlock::client::Conn;
use face_unlock::i18n::{tr, trf};
use rune::{El, col, el, row, text};
use serde_json::{Value, json};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct Trial {
    pub running: bool,
    pub face: bool,
    pub hint: String,
    pub score: Option<f64>,
    pub frame: Option<Value>,
    pub result: Option<Value>,
    line: Option<Arc<Mutex<Option<UnixStream>>>>,
}

impl Trial {
    pub fn stop(&mut self) {
        if let Some(line) = self.line.take()
            && let Some(s) = line.lock().unwrap_or_else(|e| e.into_inner()).take()
        {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
        self.running = false;
    }
}

impl Ui {
    pub fn trial_start(&mut self) {
        if self.trial.running {
            return;
        }
        self.trial = Trial { running: true, ..Default::default() };
        let Some(host) = self.host.clone() else { return };
        let conn = match Conn::open(&json!({"cmd": "verify", "purpose": "test", "verbose": true})) {
            Ok(c) => c,
            Err(_) => {
                self.trial.running = false;
                self.trial.result = Some(face_unlock::client::failure("unreachable"));
                return;
            }
        };
        self.trial.line = Some(Arc::new(Mutex::new(conn.closer())));
        std::thread::spawn(move || {
            let r = conn.result(|e| {
                let e = e.clone();
                host.app.update(move |s, _| match e["event"].as_str().unwrap_or("") {
                    "face" => s.trial.face = true,
                    "hint" => s.trial.hint = e["hint"].as_str().unwrap_or("").into(),
                    "frame" => {
                        if let Some(sc) = e["score"].as_f64() {
                            s.trial.score = Some(sc);
                        }
                        s.trial.frame = Some(e);
                    }
                    _ => {}
                });
            });
            host.app.update(move |s, _| {
                s.trial.running = false;
                s.trial.line = None;
                s.trial.result = Some(r);
                s.refresh();
            });
        });
    }
}

fn meter(p: Paint, key: &'static str, title: &str, description: &str, value: f64, shown: String) -> El {
    let (w, h) = (200.0, 6.0);
    let fill = w * value.clamp(0.0, 1.0) as f32;
    let track = el()
        .key("bar")
        .role("progressbar")
        .label(title)
        .a11y_value(shown.clone())
        .size(w, h)
        .radius(h / 2.0)
        .fill(p.t.control_off)
        .child(el().key("fill").size(fill, h).radius(h / 2.0).fill(p.accent).motion(rune::tok::motion::SNAPPY).a11y_hidden());
    let bar = col()
        .key("meter")
        .w(w)
        .gap(6.0)
        .child(text(shown).key("value").font(crate::controls::DESCRIPTION).color(p.muted()).self_end().a11y_hidden())
        .child(track);
    setting_row(p, key, title, Some(description), bar)
}

pub fn pane(ui: &Ui, p: Paint) -> El {
    let t = &ui.trial;
    let f = t.frame.as_ref();
    let n = |k: &str| f.and_then(|f| f[k].as_f64()).unwrap_or(0.0);
    let status_line = if t.running {
        match t.hint.as_str() {
            "blink" => tr("Recognised. Now blink once, or turn your head a little."),
            "look" => tr("Look at the screen."),
            "closer" => tr("Move closer to the camera."),
            "light" => tr("It is too dark to see your face."),
            _ if t.face => tr("Face found."),
            _ => tr("Look at the camera. Blink once, or turn your head a little."),
        }
        .to_string()
    } else if let Some(r) = &t.result {
        if r["ok"] == true {
            let mut s = trf(
                "Recognised as %s (match %s) in %s ms.",
                &[&r["name"].as_str().unwrap_or(""), &format!("{:.3}", r["score"].as_f64().unwrap_or(0.0)), &r["ms"].as_i64().unwrap_or(0)],
            );
            let how = match r["liveness"].as_str().unwrap_or("") {
                "blink" => tr("blink"),
                "depth" => tr("head turn"),
                _ => "",
            };
            if !how.is_empty() {
                s.push(' ');
                s.push_str(&trf("Sign of life: %s", &[&how]));
            }
            s
        } else {
            let mut s = face_unlock::text::reason(r["reason"].as_str().unwrap_or(""));
            if r.get("lockout").is_some() {
                s.push(' ');
                s.push_str(tr("That was too many tries. Face unlock is paused until you unlock with your password."));
            }
            s
        }
    } else {
        tr("Look at the camera and see what it sees. Every check shows how close it is.").to_string()
    };
    let ok = t.result.as_ref().is_some_and(|r| r["ok"] == true);
    let failed = t.result.as_ref().is_some_and(|r| r["ok"] != true);
    let action = if t.running {
        button(p, "stop", tr("Stop"), Kind::Secondary, true, |s: &mut Ui| s.trial.stop())
    } else {
        button(p, "start", if t.result.is_some() { tr("Try again") } else { tr("Start") }, Kind::Primary, true, |s: &mut Ui| s.trial_start())
    };
    let head = card(
        p,
        "status",
        vec![
            row()
                .key("line")
                .min_h(64.0)
                .px(ROW_PX)
                .py(12.0)
                .gap(16.0)
                .center_y()
                .child(
                    rune::icon(if ok { &crate::icons::CHECK_CIRCLE } else if failed { &crate::icons::WARNING } else { &crate::icons::SCAN_FACE }, 22.0)
                        .key("icon")
                        .color(if ok { p.k.c.success.fg } else if failed { p.k.c.danger.fg } else { p.k.c.icon_base }),
                )
                .child(text(status_line).key("text").font(TITLE).color(p.text()).wrap().grow(1.0).shrink())
                .child(action),
        ],
    );
    let yaw = n("yaw");
    let score = t.score.unwrap_or(0.0);
    page("try")
        .child(head)
        .child(section(
            p,
            "checks",
            tr("What the checks see"),
            vec![
                meter(p, "match", tr("Match"), tr("How much the face looks like yours. Over about 0.5 counts."), score, t.score.map(|s| format!("{s:.3}")).unwrap_or_else(|| "–".into())),
                meter(p, "turn", tr("Turn of the head"), tr("Up to 25 degrees counts as looking at the screen."), (yaw.abs() / 45.0).min(1.0), format!("{yaw:.0}°")),
                meter(p, "eyes", tr("Eyes open"), tr("Closed eyes do not unlock."), n("eyes") / 0.5, format!("{:.2}", n("eyes"))),
                meter(p, "depth", tr("Depth"), tr("A head that turns shows its nose sticking out. A photo cannot."), n("depth"), format!("{:.2}", n("depth"))),
                meter(p, "blink", tr("Blink"), tr("A blink is a sign of life too."), n("blink"), format!("{:.2}", n("blink"))),
                meter(p, "glare", tr("Glare"), tr("A screen or glossy paper throws back light. Full means a fake."), n("glare"), format!("{:.2}", n("glare"))),
                meter(p, "edge", tr("Edge of a device"), tr("The frame of a phone or a tablet around the face. Full means a fake."), n("device"), format!("{:.2}", n("device"))),
            ],
        ))
        .child(crate::controls::prose(p, tr("The terminal shows the same with `face-unlock test`.")).key("hint").font(crate::controls::DESCRIPTION).color(p.faint()).mx(ROW_PX).mt(-8.0))
        .pt(GUTTER - 24.0)
}
