//! Setting up a face: a short introduction, the password (polkit's own
//! dialog), then the camera in a circle with the ring of ticks around it,
//! and a tick at the end. Laid out like a phone's setup.

use crate::controls::{Kind, Paint, button, button_tx};
use crate::ui::{Mode, Ui};
use face_unlock::client::Conn;
use face_unlock::i18n::{tr, trf};
use rune::{El, LineCap, Path, Shape, TextStyle, Tx, canvas, col, el, row, text};
use rune_kit::{Spinner, TextInput};
use serde_json::{Value, json};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};

/// Where the setup is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum State {
    #[default]
    Intro,
    Authorizing,
    Starting,
    Center,
    Circle,
    Done,
    Failed,
}

pub struct Enroll {
    pub state: State,
    pub instruction: String,
    pub name: rune::TextEdit,
    /// Eight sectors, clockwise from the top, as seen in the preview.
    pub sectors: [bool; 8],
    /// Where the head points, in the daemon's turn units, x right and y up.
    pub pose: (f32, f32),
    pub face_visible: bool,
    pub has_frame: bool,
    /// The camera, cropped square around the face.
    pub pixels: rune::Pixels,
    /// The connection, for "finish" and to hang up.
    line: Option<Arc<Mutex<Option<UnixStream>>>>,
}

impl Default for Enroll {
    fn default() -> Self {
        Enroll {
            state: State::Intro,
            instruction: String::new(),
            name: rune::TextEdit::new(std::env::var("USER").unwrap_or_default()),
            sectors: [false; 8],
            pose: (0.0, 0.0),
            face_visible: false,
            has_frame: false,
            pixels: rune::Pixels::new(),
            line: None,
        }
    }
}

fn hint_text(hint: &str) -> Option<&'static str> {
    Some(match hint {
        "no-face" => tr("Bring your face into the circle."),
        "one-face" => tr("Only one face, please."),
        "closer" => tr("Move a little closer."),
        "light" => tr("It is too dark. Turn on a light."),
        "bright" => tr("Too bright. Turn away from the light."),
        "still" => tr("Hold still for a moment."),
        "straight" | "hold" => tr("Look straight at the camera."),
        "circle" => tr("Move your head slowly to complete the circle."),
        _ => return None,
    })
}

/// Keeps the face in the middle of the circle and a little over half its
/// size, whatever the distance, smoothed from frame to frame.
struct Crop {
    centre: (f32, f32),
    scale: f32,
}

impl Crop {
    fn apply(&mut self, frame: &Value, rgb: &[u8], w: usize, h: usize, out: &rune::Pixels) {
        if let Some(f) = frame.get("face").filter(|f| f.is_object()) {
            let g = |k: &str| f[k].as_f64().unwrap_or(0.0) as f32;
            let (fx, fy, fw, fh) = (g("x"), g("y"), g("w"), g("h"));
            let aspect = w as f32 / h as f32;
            let side = (fw * aspect).max(fh);
            let target_scale = (0.55 / side.max(0.05)).clamp(1.0, 2.2);
            let target = (fx + fw / 2.0, fy + fh / 2.0);
            self.centre.0 += (target.0 - self.centre.0) * 0.25;
            self.centre.1 += (target.1 - self.centre.1) * 0.25;
            self.scale += (target_scale - self.scale) * 0.2;
        }
        // The shorter side spans the circle, zoomed and centred on the face.
        let side = (w.min(h) as f32 / self.scale).max(8.0);
        let x0 = (self.centre.0 * w as f32 - side / 2.0).clamp(0.0, w as f32 - side);
        let y0 = (self.centre.1 * h as f32 - side / 2.0).clamp(0.0, h as f32 - side);
        const N: usize = 360;
        let mut rgba = vec![255u8; N * N * 4];
        for y in 0..N {
            let sy = ((y0 + (y as f32 + 0.5) * side / N as f32) as usize).min(h - 1);
            for x in 0..N {
                let sx = ((x0 + (x as f32 + 0.5) * side / N as f32) as usize).min(w - 1);
                let i = (sy * w + sx) * 3;
                let o = (y * N + x) * 4;
                rgba[o..o + 3].copy_from_slice(&rgb[i..i + 3]);
            }
        }
        out.set(N as u32, N as u32, rgba);
    }
}

fn decode(b64: &str) -> Option<(Vec<u8>, usize, usize)> {
    let bytes = unbase64(b64)?;
    let opts = zune_jpeg::zune_core::options::DecoderOptions::default().jpeg_set_out_colorspace(zune_jpeg::zune_core::colorspace::ColorSpace::RGB);
    let mut dec = zune_jpeg::JpegDecoder::new_with_options(zune_jpeg::zune_core::bytestream::ZCursor::new(&bytes), opts);
    let px = dec.decode().ok()?;
    let (w, h) = dec.dimensions()?;
    (px.len() == w * h * 3).then_some((px, w, h))
}

fn unbase64(text: &str) -> Option<Vec<u8>> {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut buf, mut bits) = (0u32, 0);
    for c in text.bytes().take_while(|&c| c != b'=') {
        buf = buf << 6 | T.iter().position(|&b| b == c)? as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

impl Ui {
    pub fn enroll_start(&mut self) {
        if self.enroll.line.is_some() {
            return;
        }
        let e = &mut self.enroll;
        e.sectors = [false; 8];
        e.has_frame = false;
        e.state = State::Starting;
        e.instruction = tr("Starting the camera…").into();
        let name = e.name.text.trim().to_string();
        let Some(host) = self.host.clone() else { return };
        let conn = match Conn::open(&json!({"cmd": "enroll", "name": name})) {
            Ok(c) => c,
            Err(_) => {
                self.enroll_failed("unreachable", "");
                return;
            }
        };
        let line = Arc::new(Mutex::new(conn.closer()));
        e.line = Some(line.clone());
        let pixels = e.pixels.clone();
        std::thread::spawn(move || {
            let mut crop = Crop { centre: (0.5, 0.5), scale: 1.0 };
            let result = conn.result(|ev| {
                if ev["event"] == "frame" {
                    if let Some((rgb, w, h)) = ev["jpeg"].as_str().and_then(decode) {
                        crop.apply(ev, &rgb, w, h, &pixels);
                        host.app.update_if(|s, _| !std::mem::replace(&mut s.enroll.has_frame, true));
                    }
                    return;
                }
                let ev = ev.clone();
                host.app.update(move |s, _| s.enroll_event(&ev));
            });
            host.app.update(move |s, tx| s.enroll_finished(&result, tx));
        });
    }

    fn enroll_event(&mut self, e: &Value) {
        let en = &mut self.enroll;
        match e["event"].as_str().unwrap_or("") {
            "authorizing" => {
                en.state = State::Authorizing;
                en.instruction = tr("Confirm with your password to add a face.").into();
            }
            "authorized" => {
                en.state = State::Starting;
                en.instruction = tr("Starting the camera…").into();
            }
            "started" => {
                en.state = State::Center;
                en.instruction = hint_text("straight").unwrap_or_default().into();
            }
            "pose" => {
                en.face_visible = e["face"] == true;
                if en.face_visible {
                    en.pose = (e["x"].as_f64().unwrap_or(0.0) as f32, e["y"].as_f64().unwrap_or(0.0) as f32);
                }
            }
            "hint" => {
                if let Some(t) = hint_text(e["hint"].as_str().unwrap_or("")) {
                    en.instruction = t.into();
                }
            }
            "captured" => {
                if e["pose"] == "center" {
                    en.state = State::Circle;
                    en.instruction = hint_text("circle").unwrap_or_default().into();
                } else if let Some(i) = e["sector"].as_u64().filter(|i| *i < 8) {
                    en.sectors[i as usize] = true;
                }
            }
            _ => {}
        }
    }

    fn enroll_finished(&mut self, r: &Value, _tx: &mut Tx) {
        self.enroll.line = None;
        if r["ok"] == true {
            self.enroll.sectors = [true; 8];
            self.enroll.state = State::Done;
            self.enroll.instruction = tr("Face Unlock is set up.").into();
            self.refresh();
            return;
        }
        let reason = r["reason"].as_str().unwrap_or("").to_string();
        if reason == "cancelled" {
            return;
        }
        self.enroll_failed(&reason, r["message"].as_str().unwrap_or(""));
    }

    fn enroll_failed(&mut self, reason: &str, message: &str) {
        let text = match reason {
            "denied" => {
                let mut t = tr("A face can only be added with your password.").to_string();
                if self.snap.desktop.own_locker() {
                    t.push(' ');
                    t.push_str(tr("No password window? Start a polkit agent, for example hyprpolkitagent."));
                }
                t
            }
            "camera" => trf("The camera could not be used: %1", &[&message]),
            "models" => tr("The face recognition models are missing. Reinstall the package.").into(),
            "timeout" => tr("That took too long. Try again, in good light.").into(),
            "unreachable" => tr("The face unlock service is not running. Turn face unlock on first.").into(),
            "busy" => tr("The camera is busy with another scan. Try again in a moment.").into(),
            other => trf("The face could not be added (%1).", &[&other]),
        };
        self.enroll.state = State::Failed;
        self.enroll.instruction = text;
    }

    pub fn enroll_finish_now(&mut self) {
        if let Some(line) = &self.enroll.line
            && let Some(s) = line.lock().unwrap_or_else(|e| e.into_inner()).as_mut()
        {
            let _ = s.write_all(b"{\"cmd\":\"finish\"}\n");
        }
    }

    /// Stops a setup that runs, and leaves the page.
    pub fn enroll_cancel(&mut self, tx: &mut Tx) {
        if let Some(line) = self.enroll.line.take()
            && let Some(s) = line.lock().unwrap_or_else(|e| e.into_inner()).take()
        {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
        let done = self.enroll.state == State::Done;
        match self.mode {
            Mode::Setup => {
                crate::ui::answer(if done { 0 } else { 1 }, "");
                tx.quit();
            }
            Mode::Onboarding => {
                if done {
                    self.next_step();
                } else {
                    self.enroll = Enroll::default();
                }
            }
            _ => {
                self.sub = None;
                self.enroll = Enroll::default();
            }
        }
    }

    pub fn enroll_can_finish(&self) -> bool {
        let n = self.enroll.sectors.iter().filter(|s| **s).count();
        self.enroll.state == State::Circle && (4..8).contains(&n)
    }
}

const STAGE: f32 = 340.0;
const PREVIEW: f32 = 244.0;
const TICKS: usize = 80;

/// The ring of ticks around the camera, as on a phone. A sector lights up
/// once the head has pointed that way long enough; a few ticks follow where
/// the head points now, so it is clear which way is still missing.
fn ring(p: Paint, e: &Enroll) -> El {
    let accent = p.accent;
    let idle = p.t.control_off;
    let c = STAGE / 2.0;
    let inner = STAGE / 2.0 - 26.0;
    let done = e.state == State::Done;
    let tracking = e.state == State::Circle && e.face_visible;
    let head = {
        let a = e.pose.0.atan2(e.pose.1).to_degrees();
        if a < 0.0 { a + 360.0 } else { a }
    };
    let reach = e.pose.0.hypot(e.pose.1).min(1.0);
    let mut shape = Shape::new();
    if done {
        let mut path = Path::new();
        for i in 0..=96 {
            let a = i as f32 / 96.0 * std::f32::consts::TAU;
            let pt = [c + (inner + 10.0) * a.sin(), c - (inner + 10.0) * a.cos()];
            path = if i == 0 { path.move_to(pt) } else { path.line_to(pt) };
        }
        shape = shape.stroke_path(&path, 5.0, LineCap::Round, accent);
    } else {
        for i in 0..TICKS {
            let deg = i as f32 * 360.0 / TICKS as f32;
            let lit = e.sectors[((deg / 45.0).round() as usize) % 8];
            let glow = if tracking && !lit {
                let mut d = (deg - head).abs();
                if d > 180.0 {
                    d = 360.0 - d;
                }
                reach * (1.0 - d / (6.0 * 360.0 / TICKS as f32)).max(0.0)
            } else {
                0.0
            };
            let len = if lit { 18.0 } else { 10.0 + 8.0 * glow };
            let a = deg.to_radians();
            let (s, co) = a.sin_cos();
            let from = [c + inner * s, c - inner * co];
            let to = [c + (inner + len) * s, c - (inner + len) * co];
            let color = if lit { accent } else if glow > 0.0 { idle.mix(accent, glow) } else { idle };
            shape = shape.line(from, to, 3.0, LineCap::Round, color);
        }
    }
    canvas(shape).key("ring").size(STAGE, STAGE).absolute(0.0, 0.0).a11y_hidden()
}

/// A tick that is drawn once done.
fn check(p: Paint) -> El {
    let u = 150.0 / 100.0;
    let pts = vec![[28.0 * u, 52.0 * u], [43.0 * u, 67.0 * u], [73.0 * u, 35.0 * u]];
    canvas(Shape::new().stroke_with(pts, false, 150.0 * 0.07, LineCap::Round, p.accent))
        .key("check")
        .size(150.0, 150.0)
        .absolute((STAGE - 150.0) / 2.0, (STAGE - 150.0) / 2.0)
        .a11y_hidden()
}

/// The setup, for a window of its own or inside another.
pub fn view(ui: &Ui, p: Paint) -> El {
    let e = &ui.enroll;
    let scanning = matches!(e.state, State::Center | State::Circle);
    let done = e.state == State::Done;
    let mut stage = el().key("stage").size(STAGE, STAGE).self_center();
    if scanning || done {
        let preview = el()
            .key("preview")
            .size(PREVIEW, PREVIEW)
            .radius(PREVIEW / 2.0)
            .fill(p.k.c.layer_02)
            .absolute((STAGE - PREVIEW) / 2.0, (STAGE - PREVIEW) / 2.0)
            .opacity(if done { 0.25 } else { 1.0 })
            .a11y_hidden();
        let preview = if e.has_frame { preview.pixels(&e.pixels) } else { preview };
        stage = stage.child(preview).child(ring(p, e));
        if done {
            stage = stage.child(check(p));
        }
    } else {
        // Before the camera, and when something went wrong: the face.
        let glyph = el()
            .key("glyph")
            .size(132.0, 132.0)
            .radius(66.0)
            .center()
            .fill(if e.state == State::Failed { p.k.c.danger.bg } else { p.k.c.layer_02 })
            .absolute((STAGE - 132.0) / 2.0, (STAGE - 132.0) / 2.0)
            .child(
                rune::icon(&crate::icons::SCAN_FACE, 64.0)
                    .key("face")
                    .color(if e.state == State::Failed { p.k.c.danger.fg } else { p.k.c.text_accent }),
            );
        stage = stage.child(glyph);
        if matches!(e.state, State::Authorizing | State::Starting) {
            stage = stage.child(Spinner::new().size(24.0).build(p.k).key("spinner").absolute((STAGE - 24.0) / 2.0, STAGE - 60.0));
        }
    }

    let title = match e.state {
        State::Intro => tr("Set up your face"),
        State::Authorizing => tr("Confirm it is you"),
        State::Done => tr("You are all set"),
        State::Failed => tr("Setup did not finish"),
        _ => "",
    };
    let body = match e.state {
        State::Intro => tr("Look at the camera, then move your head slowly in a circle. Your face is turned into numbers on this computer and never stored as a picture.").to_string(),
        State::Done if ui.mode == Mode::Main => tr("Lock the screen and look at it to try it out.").to_string(),
        _ => e.instruction.clone(),
    };
    let texts = col()
        .key("texts")
        .w(420.0)
        .self_center()
        .gap(10.0)
        .children((!title.is_empty()).then(|| text(title).key("title").font(TextStyle::new(24.0, 650.0).lh(30.0)).color(p.text()).align(rune::Align::Center).wrap()))
        .child(
            text(body)
                .key("instruction")
                .font(if scanning { TextStyle::new(15.0, 520.0).lh(21.0) } else { p.k.ty.ui })
                .color(if scanning { p.text() } else { p.muted() })
                .align(rune::Align::Center)
                .wrap(),
        )
        .children((e.state == State::Intro).then(|| {
            TextInput::new(&e.name)
                .placeholder(tr("Name (optional)"))
                .label(tr("Name (optional)"))
                .width(Some(260.0))
                .on_edit(|s: &mut Ui| &mut s.enroll.name)
                .on_submit(|s: &mut Ui, _| s.enroll_start())
                .build(p.k)
                .key("name")
                .self_center()
                .mt(8.0)
        }));

    let mut buttons = row().key("buttons").gap(10.0).self_center();
    if !done {
        buttons = buttons.child(button_tx(p, "cancel", tr("Cancel"), Kind::Plain, true, |s: &mut Ui, tx: &mut Tx| s.enroll_cancel(tx)).h(36.0).px(18.0));
    }
    if ui.enroll_can_finish() {
        buttons = buttons.child(button(p, "finish", tr("Finish now"), Kind::Secondary, true, |s: &mut Ui| s.enroll_finish_now()).h(36.0).px(18.0));
    }
    if matches!(e.state, State::Intro | State::Failed) {
        let label = if e.state == State::Failed { tr("Try again") } else { tr("Get started") };
        buttons = buttons.child(button(p, "start", label, Kind::Primary, true, |s: &mut Ui| s.enroll_start()).h(36.0).px(22.0));
    }
    if done {
        let label = if ui.mode == Mode::Onboarding { tr("Continue") } else { tr("Done") };
        buttons = buttons.child(button_tx(p, "done", label, Kind::Primary, true, |s: &mut Ui, tx: &mut Tx| s.enroll_cancel(tx)).h(36.0).px(22.0));
    }
    col().key("setup").w_full().pt(24.0).pb(24.0).gap(20.0).child(stage).child(texts).child(buttons)
}
