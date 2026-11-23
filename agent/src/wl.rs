//! Wayland: the bubble as a layer-shell surface above everything, the own
//! lock screen (ext-session-lock) with the bubble on one of its screens,
//! the keyboard and pointer on it, and ext-idle-notify to tell when
//! somebody comes back.

use crate::app::{App, Event};
use crate::bubble::{self, Bubble};
use crate::draw::{self, Canvas};
use crate::lockscreen::{self, LockScreen};
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState, Region};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::calloop::LoopHandle;
use smithay_client_toolkit::reexports::calloop::channel::Sender;
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers};
use smithay_client_toolkit::seat::pointer::{CursorIcon, PointerEvent, PointerEventKind, PointerHandler, ThemeSpec, ThemedPointer};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::session_lock::{SessionLock, SessionLockHandler, SessionLockState, SessionLockSurface, SessionLockSurfaceConfigure};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure};
use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::subcompositor::SubcompositorState;
use smithay_client_toolkit::{
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer, delegate_registry, delegate_seat, delegate_session_lock, delegate_shm, delegate_subcompositor,
    registry_handlers,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tiny_skia::{Pixmap, PixmapMut};
use wayland_client::globals::{GlobalList, registry_queue_init};
use wayland_client::protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_subsurface, wl_surface};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::ext::idle_notify::v1::client::{ext_idle_notification_v1, ext_idle_notifier_v1};
use wayland_protocols_plasma::lockscreen_overlay::v1::client::kde_lockscreen_overlay_v1;

/// Whether the compositor offers a global by this name. For the command
/// line, before deciding whether to fork.
pub fn has_global(name: &str) -> bool {
    let Ok(conn) = Connection::connect_to_env() else { return false };
    let Ok((globals, _)) = registry_queue_init::<Probe>(&conn) else { return false };
    globals.contents().with_list(|l| l.iter().any(|g| g.interface == name))
}

struct Probe;

impl Dispatch<wayland_client::protocol::wl_registry::WlRegistry, wayland_client::globals::GlobalListContents> for Probe {
    fn event(
        _: &mut Self,
        _: &wayland_client::protocol::wl_registry::WlRegistry,
        _: wayland_client::protocol::wl_registry::Event,
        _: &wayland_client::globals::GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

/// A surface the bubble is drawn on: the layer surface over the desktop, or
/// a subsurface of a lock screen.
struct Target {
    surface: wl_surface::WlSurface,
    layer: Option<LayerSurface>,
    sub: Option<wl_subsurface::WlSubsurface>,
    pool: Option<SlotPool>,
    configured: bool,
    scale: i32,
    frame_at: Option<Instant>,
    /// Whether the last buffer had the bubble on it.
    drawn: bool,
}

impl Drop for Target {
    fn drop(&mut self) {
        if let Some(sub) = self.sub.take() {
            sub.destroy();
            self.surface.destroy();
        }
    }
}

/// Whether a frame may be drawn: the compositor took the last one, or it
/// did not say so for a while (a surface it does not show sends nothing).
fn ready(frame_at: Option<Instant>) -> bool {
    frame_at.is_none_or(|t| t.elapsed() > Duration::from_millis(200))
}

/// Draws into a new buffer of `pool`: straight into it when the compositor
/// takes RGBA, else into a pixmap that is copied over as BGRA.
fn render(pool: &mut SlotPool, w: i32, h: i32, scale: f32, abgr: bool, draw: impl FnOnce(&mut Canvas)) -> Option<Buffer> {
    let format = if abgr { wl_shm::Format::Abgr8888 } else { wl_shm::Format::Argb8888 };
    let (buffer, canvas) = pool.create_buffer(w, h, w * 4, format).ok()?;
    if abgr {
        let pm = PixmapMut::from_bytes(canvas, w as u32, h as u32)?;
        draw(&mut Canvas::new(pm, scale));
    } else {
        let mut pixmap = Pixmap::new(w as u32, h as u32)?;
        draw(&mut Canvas::new(pixmap.as_mut(), scale));
        for (d, s) in canvas.as_chunks_mut::<4>().0.iter_mut().zip(pixmap.data().as_chunks::<4>().0) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = s[3];
        }
    }
    Some(buffer)
}

impl Target {
    fn draw(&mut self, shm: &Shm, abgr: bool, qh: &QueueHandle<App>, bubble: &Bubble, at: f64) {
        if !self.configured || !ready(self.frame_at) || (!bubble.visible && !self.drawn) {
            return;
        }
        let s = self.scale.max(1);
        let (w, h) = (bubble::WIDTH as i32 * s, bubble::HEIGHT as i32 * s);
        let pool = match &mut self.pool {
            Some(p) => p,
            None => match SlotPool::new((w * h * 4 * 2) as usize, shm) {
                Ok(p) => self.pool.insert(p),
                Err(_) => return,
            },
        };
        let visible = bubble.visible;
        let Some(buffer) = render(pool, w, h, s as f32, abgr, |cv| {
            cv.pixmap.fill(tiny_skia::Color::TRANSPARENT);
            if visible {
                bubble.draw(cv, at);
            }
        }) else {
            return;
        };
        self.surface.set_buffer_scale(s);
        if buffer.attach_to(&self.surface).is_err() {
            return;
        }
        self.surface.damage_buffer(0, 0, w, h);
        self.surface.frame(qh, self.surface.clone());
        self.surface.commit();
        self.frame_at = Some(Instant::now());
        self.drawn = visible;
    }
}

/// One screen of the own lock screen.
struct LockOutput {
    output: wl_output::WlOutput,
    name: String,
    surface: SessionLockSurface,
    size: Option<(u32, u32)>,
    scale: i32,
    pool: Option<SlotPool>,
    bg: Option<Pixmap>,
    bg_for: Option<(u32, u32)>,
    bubble: Option<Target>,
    dirty: bool,
    frame_at: Option<Instant>,
    clock: String,
}

impl LockOutput {
    fn pixels(&self) -> Option<(u32, u32)> {
        self.size.map(|(w, h)| (w * self.scale.max(1) as u32, h * self.scale.max(1) as u32))
    }

    fn place_bubble(&self) {
        if let (Some(t), Some((w, _))) = (&self.bubble, self.size)
            && let Some(sub) = &t.sub
        {
            sub.set_position((w as i32 - bubble::WIDTH as i32) / 2, 0);
        }
    }
}

pub struct Wl {
    pub conn: Connection,
    pub qh: QueueHandle<App>,
    registry: RegistryState,
    outputs: OutputState,
    compositor: CompositorState,
    subcompositor: Option<SubcompositorState>,
    shm: Shm,
    layer_shell: Option<LayerShell>,
    lock_state: SessionLockState,
    pub has_lock: bool,
    seat: SeatState,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<ThemedPointer>,
    idle: Option<ext_idle_notifier_v1::ExtIdleNotifierV1>,
    notification: Option<ext_idle_notification_v1::ExtIdleNotificationV1>,
    overlay: Option<kde_lockscreen_overlay_v1::KdeLockscreenOverlayV1>,
    warned_overlay: bool,
    bubble_surface: Option<Target>,
    pub lock: Option<SessionLock>,
    pub confirmed: bool,
    unlock_wanted: bool,
    locks: Vec<LockOutput>,
    pointer_output: String,
    placed_while_shown: bool,
    pictures: HashMap<String, PathBuf>,
    blur: bool,
    tx: Option<Sender<Event>>,
}

impl Wl {
    pub fn connect(handle: &LoopHandle<'static, App>) -> Option<Wl> {
        let conn = Connection::connect_to_env().ok()?;
        let (globals, queue) = registry_queue_init::<App>(&conn).ok()?;
        let qh = queue.handle();
        let compositor = CompositorState::bind(&globals, &qh).ok()?;
        let subcompositor = SubcompositorState::bind(compositor.wl_compositor().clone(), &globals, &qh).ok();
        let shm = Shm::bind(&globals, &qh).ok()?;
        let has = |name: &str| globals.contents().with_list(|l| l.iter().any(|g| g.interface == name));
        let has_lock = has("ext_session_lock_manager_v1");
        let wl = Wl {
            conn: conn.clone(),
            registry: RegistryState::new(&globals),
            outputs: OutputState::new(&globals, &qh),
            subcompositor,
            shm,
            layer_shell: LayerShell::bind(&globals, &qh).ok(),
            lock_state: SessionLockState::new(&globals, &qh),
            has_lock,
            seat: SeatState::new(&globals, &qh),
            keyboard: None,
            pointer: None,
            idle: bind(&globals, &qh, 1..=2),
            notification: None,
            overlay: bind(&globals, &qh, 1..=1),
            warned_overlay: false,
            bubble_surface: None,
            lock: None,
            confirmed: false,
            unlock_wanted: false,
            locks: Vec::new(),
            pointer_output: String::new(),
            placed_while_shown: false,
            pictures: HashMap::new(),
            blur: false,
            tx: None,
            compositor,
            qh,
        };
        WaylandSource::new(conn, queue).insert(handle.clone()).ok()?;
        Some(wl)
    }

    pub fn has_layer(&self) -> bool {
        self.layer_shell.is_some()
    }

    fn abgr(&self) -> bool {
        self.shm.formats().contains(&wl_shm::Format::Abgr8888)
    }

    fn output_named(&self, name: &str) -> Option<wl_output::WlOutput> {
        if name.is_empty() {
            return None;
        }
        self.outputs.outputs().find(|o| self.outputs.info(o).and_then(|i| i.name).as_deref() == Some(name))
    }

    fn scale_of(&self, output: Option<&wl_output::WlOutput>) -> i32 {
        match output {
            Some(o) => self.outputs.info(o).map_or(1, |i| i.scale_factor),
            None => self.outputs.outputs().filter_map(|o| self.outputs.info(&o).map(|i| i.scale_factor)).max().unwrap_or(1),
        }
    }

    fn empty_input(&self, surface: &wl_surface::WlSurface) {
        if let Ok(region) = Region::new(&self.compositor) {
            surface.set_input_region(Some(region.wl_region()));
        }
    }

    // -----------------------------------------------------------------------
    // The bubble over the desktop
    // -----------------------------------------------------------------------

    /// The layer surface is made when the bubble opens and goes when it has
    /// closed: it moves only when it opens, never in the middle of a scan.
    pub fn show_bubble(&mut self, shown: bool, screen: &str) {
        if !shown {
            self.bubble_surface = None;
            return;
        }
        if self.bubble_surface.is_some() {
            return;
        }
        let Some(shell) = &self.layer_shell else { return };
        // Not connected right now: as if none was picked, and the
        // compositor puts it on the screen in use.
        let output = self.output_named(screen);
        let surface = self.compositor.create_surface(&self.qh);
        let layer = shell.create_layer_surface(&self.qh, surface, Layer::Overlay, Some("on-screen-display"), output.as_ref());
        layer.set_anchor(Anchor::TOP);
        layer.set_size(bubble::WIDTH as u32, bubble::HEIGHT as u32);
        // Sit at the very top edge, over a top panel if there is one.
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        self.empty_input(layer.wl_surface());
        // KWin shows it above its lock screen only when asked before it is
        // mapped.
        match &self.overlay {
            Some(o) => o.allow(layer.wl_surface()),
            None => {
                if !self.warned_overlay && std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().split(':').any(|d| d == "KDE") {
                    self.warned_overlay = true;
                    eprintln!("KWin does not let this program show above the lock screen; is its desktop file installed?");
                }
            }
        }
        layer.commit();
        let scale = self.scale_of(output.as_ref());
        self.bubble_surface = Some(Target { surface: layer.wl_surface().clone(), layer: Some(layer), sub: None, pool: None, configured: false, scale, frame_at: None, drawn: false });
    }

    pub fn draw_bubbles(&mut self, bubble: &Bubble, at: f64) {
        let abgr = self.abgr();
        if let Some(t) = &mut self.bubble_surface {
            t.draw(&self.shm, abgr, &self.qh, bubble, at);
        }
        for o in &mut self.locks {
            if let Some(t) = &mut o.bubble {
                t.draw(&self.shm, abgr, &self.qh, bubble, at);
            }
        }
    }

    // -----------------------------------------------------------------------
    // The own lock screen
    // -----------------------------------------------------------------------

    pub fn lock(&mut self, wallpaper: &str, blur: bool, tx: Sender<Event>) -> bool {
        if self.lock.is_some() {
            return true;
        }
        match self.lock_state.lock(&self.qh) {
            Ok(l) => self.lock = Some(l),
            Err(_) => {
                eprintln!("the compositor has no ext_session_lock_manager_v1, so this cannot lock the screen");
                return false;
            }
        }
        self.confirmed = false;
        self.unlock_wanted = false;
        self.pictures = face_unlock::wallpaper::for_lock(wallpaper);
        self.blur = blur;
        self.tx = Some(tx);
        let outputs: Vec<_> = self.outputs.outputs().collect();
        for o in outputs {
            self.add_lock_output(o);
        }
        true
    }

    fn add_lock_output(&mut self, output: wl_output::WlOutput) {
        let Some(lock) = &self.lock else { return };
        if self.locks.iter().any(|o| o.output == output) {
            return;
        }
        let info = self.outputs.info(&output);
        let name = info.as_ref().and_then(|i| i.name.clone()).unwrap_or_default();
        let scale = info.map_or(1, |i| i.scale_factor);
        let surface = self.compositor.create_surface(&self.qh);
        let surface = lock.create_lock_surface(surface, &output, &self.qh);
        self.locks.push(LockOutput {
            output,
            name,
            surface,
            size: None,
            scale,
            pool: None,
            bg: None,
            bg_for: None,
            bubble: None,
            dirty: true,
            frame_at: None,
            clock: String::new(),
        });
    }

    /// Unlocks. Unlocking before the compositor has confirmed the lock is a
    /// protocol error: then it follows as soon as it has, and this returns
    /// false.
    pub fn unlock(&mut self) -> bool {
        let Some(lock) = &self.lock else { return false };
        if !self.confirmed {
            self.unlock_wanted = true;
            return false;
        }
        lock.unlock();
        self.release();
        true
    }

    fn release(&mut self) {
        self.locks.clear();
        self.lock = None;
        self.confirmed = false;
        self.placed_while_shown = false;
        // Out to the compositor now: an agent started only for this lock
        // quits straight after.
        let _ = self.conn.flush();
    }

    pub fn dirty_locks(&mut self) {
        for o in &mut self.locks {
            o.dirty = true;
        }
    }

    /// Draws the screens of the lock that changed, the time included.
    pub fn draw_locks(&mut self, screen: &LockScreen, at: f64) {
        if self.locks.is_empty() {
            return;
        }
        let abgr = self.abgr();
        let (clock, _) = lockscreen::now_text();
        for o in &mut self.locks {
            let Some((w, h)) = o.size else { continue };
            if !(o.dirty || o.clock != clock) || !ready(o.frame_at) {
                continue;
            }
            let s = o.scale.max(1);
            let (pw, ph) = (w as i32 * s, h as i32 * s);
            let pool = match &mut o.pool {
                Some(p) => p,
                None => match SlotPool::new((pw * ph * 4) as usize, &self.shm) {
                    Ok(p) => o.pool.insert(p),
                    Err(_) => continue,
                },
            };
            let bg = o.bg.as_ref();
            let Some(buffer) = render(pool, pw, ph, s as f32, abgr, |cv| lockscreen::draw(cv, w as f64, h as f64, bg, screen, at)) else { continue };
            let surface = o.surface.wl_surface();
            surface.set_buffer_scale(s);
            if buffer.attach_to(surface).is_err() {
                continue;
            }
            surface.damage_buffer(0, 0, pw, ph);
            surface.frame(&self.qh, surface.clone());
            surface.commit();
            o.frame_at = Some(Instant::now());
            o.dirty = false;
            o.clock = clock.clone();
        }
    }

    fn request_wallpaper(&mut self, i: usize) {
        let o = &mut self.locks[i];
        let Some(px) = o.pixels() else { return };
        if o.bg_for == Some(px) {
            return;
        }
        o.bg_for = Some(px);
        o.bg = None;
        let (Some(path), Some(tx)) = (face_unlock::wallpaper::pick(&self.pictures, &o.name).map(|p| p.to_path_buf()), self.tx.clone()) else { return };
        let (name, blur) = (o.name.clone(), self.blur);
        // Loaded aside, so a big picture does not hold up the lock.
        std::thread::spawn(move || {
            let pixmap = lockscreen::background(&path, px.0, px.1, blur);
            let _ = tx.send(Event::Wallpaper(name, px, pixmap));
        });
    }

    pub fn set_wallpaper(&mut self, output: &str, size: (u32, u32), pixmap: Option<Pixmap>) {
        for o in &mut self.locks {
            if o.name == output && o.bg_for == Some(size) {
                o.bg = pixmap;
                o.dirty = true;
                return;
            }
        }
    }

    /// Which screen of the lock shows the bubble: the one picked, else the
    /// one with the pointer, else the first. It moves only while closed.
    pub fn place_lock_bubble(&mut self, screen: &str, shown: bool) {
        if self.locks.is_empty() {
            return;
        }
        if shown && self.placed_while_shown {
            return;
        }
        self.placed_while_shown = shown;
        let names: Vec<&str> = self.locks.iter().map(|o| o.name.as_str()).collect();
        let pick = if names.contains(&screen) {
            screen.to_string()
        } else if names.contains(&self.pointer_output.as_str()) {
            self.pointer_output.clone()
        } else {
            names[0].to_string()
        };
        if self.locks.iter().any(|o| o.name == pick && o.bubble.is_some()) {
            return;
        }
        for o in &mut self.locks {
            o.bubble = None;
        }
        let Some(sc) = &self.subcompositor else { return };
        let Some(i) = self.locks.iter().position(|o| o.name == pick) else { return };
        let parent = self.locks[i].surface.wl_surface().clone();
        let (sub, surface) = sc.create_subsurface(parent, &self.qh);
        sub.set_desync();
        self.empty_input(&surface);
        let o = &mut self.locks[i];
        o.bubble = Some(Target { surface, layer: None, sub: Some(sub), pool: None, configured: true, scale: o.scale, frame_at: None, drawn: true });
        o.place_bubble();
        o.dirty = true;
    }

    fn lock_index(&self, surface: &wl_surface::WlSurface) -> Option<usize> {
        self.locks.iter().position(|o| o.surface.wl_surface() == surface)
    }

    // -----------------------------------------------------------------------
    // Idle notifications
    // -----------------------------------------------------------------------

    /// Asks to be told when there is input after `calm` ms of none. False
    /// when the compositor cannot tell.
    pub fn watch_idle(&mut self, calm: u32) -> bool {
        self.stop_idle();
        let (Some(idle), Some(seat)) = (&self.idle, self.seat.seats().next()) else { return false };
        // Version 1 has only the notification that inhibitors hold back.
        let n = if idle.version() >= 2 { idle.get_input_idle_notification(calm, &seat, &self.qh, ()) } else { idle.get_idle_notification(calm, &seat, &self.qh, ()) };
        self.notification = Some(n);
        true
    }

    pub fn stop_idle(&mut self) {
        if let Some(n) = self.notification.take() {
            n.destroy();
        }
    }
}

fn bind<I: Proxy + 'static>(globals: &GlobalList, qh: &QueueHandle<App>, version: std::ops::RangeInclusive<u32>) -> Option<I>
where
    App: Dispatch<I, ()>,
{
    globals.bind::<I, App, ()>(qh, version, ()).ok()
}

// ---------------------------------------------------------------------------
// The keys and the pointer on the lock screen
// ---------------------------------------------------------------------------

impl App {
    pub fn key(&mut self, event: KeyEvent) {
        if !self.own_locked() {
            return;
        }
        self.note_input();
        if self.screen.busy {
            return;
        }
        match event.keysym {
            Keysym::Return | Keysym::KP_Enter | Keysym::ISO_Enter => self.submit(),
            Keysym::BackSpace => {
                self.screen.password.pop();
                self.screen.message.clear();
            }
            Keysym::Escape => {
                self.screen.clear_password();
                self.screen.message.clear();
            }
            _ => {
                if let Some(text) = event.utf8.filter(|t| !t.is_empty() && !t.chars().any(char::is_control))
                    && self.screen.password.len() < 1024
                {
                    self.screen.password.push_str(&text);
                    self.screen.message.clear();
                }
            }
        }
        if let Some(wl) = self.wl.as_mut() {
            wl.dirty_locks();
            wl.draw_locks(&self.screen, draw::ms());
        }
    }

    /// Where the pointer is goes first: a scan this starts shows its bubble
    /// there.
    fn pointer_on(&mut self, output: String) {
        let screen = self.cfg.bubble_screen.clone();
        let shown = self.shown;
        if let Some(wl) = self.wl.as_mut()
            && wl.pointer_output != output
        {
            wl.pointer_output = output;
            if !shown {
                wl.place_lock_bubble(&screen, false);
                wl.draw_locks(&self.screen, draw::ms());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The handlers
// ---------------------------------------------------------------------------

impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &wl_surface::WlSurface, factor: i32) {
        let Some(wl) = self.wl.as_mut() else { return };
        if let Some(t) = &mut wl.bubble_surface
            && &t.surface == surface
            && t.scale != factor
        {
            t.scale = factor;
            t.frame_at = None;
            t.drawn = true;
        }
        self.kick();
    }

    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &wl_surface::WlSurface, _: u32) {
        let Some(wl) = self.wl.as_mut() else { return };
        if let Some(t) = &mut wl.bubble_surface
            && &t.surface == surface
        {
            t.frame_at = None;
        }
        for o in &mut wl.locks {
            if o.surface.wl_surface() == surface {
                o.frame_at = None;
            }
            if let Some(t) = &mut o.bubble
                && &t.surface == surface
            {
                t.frame_at = None;
            }
        }
    }

    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}

    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.wl.as_mut().expect("wayland").outputs
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        let screen = self.cfg.bubble_screen.clone();
        let shown = self.shown;
        if let Some(wl) = self.wl.as_mut()
            && wl.lock.is_some()
        {
            wl.add_lock_output(output);
            // The picked screen may be the one that just came.
            if !shown {
                wl.place_lock_bubble(&screen, false);
            }
        }
    }

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        let Some(wl) = self.wl.as_mut() else { return };
        let info = wl.outputs.info(&output);
        if let Some(i) = wl.locks.iter().position(|o| o.output == output) {
            let o = &mut wl.locks[i];
            if let Some(info) = info {
                o.name = info.name.unwrap_or_default();
                if o.scale != info.scale_factor {
                    o.scale = info.scale_factor;
                    if let Some(t) = &mut o.bubble {
                        t.scale = info.scale_factor;
                    }
                    o.dirty = true;
                    wl.request_wallpaper(i);
                }
            }
        }
    }

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        let screen = self.cfg.bubble_screen.clone();
        let shown = self.shown;
        if let Some(wl) = self.wl.as_mut() {
            let had = wl.locks.iter().any(|o| o.output == output && o.bubble.is_some());
            wl.locks.retain(|o| o.output != output);
            if had {
                wl.placed_while_shown = false;
                wl.place_lock_bubble(&screen, shown);
            }
        }
    }
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        if let Some(wl) = self.wl.as_mut()
            && wl.bubble_surface.as_ref().and_then(|t| t.layer.as_ref()) == Some(layer)
        {
            wl.bubble_surface = None;
        }
    }

    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface, _: LayerSurfaceConfigure, _: u32) {
        if let Some(wl) = self.wl.as_mut()
            && let Some(t) = &mut wl.bubble_surface
            && t.layer.as_ref() == Some(layer)
        {
            t.configured = true;
            t.frame_at = None;
            t.drawn = true;
        }
        self.kick();
    }
}

impl SessionLockHandler for App {
    fn locked(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {
        let Some(wl) = self.wl.as_mut() else { return };
        if wl.lock.is_none() {
            return;
        }
        wl.confirmed = true;
        let unlock = wl.unlock_wanted;
        self.lock_confirmed();
        if unlock {
            self.own_unlock();
        }
    }

    fn finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {
        let Some(wl) = self.wl.as_mut() else { return };
        let Some(lock) = &wl.lock else { return };
        // Refused (another lock screen is up) or ended by the compositor.
        if wl.confirmed {
            lock.unlock();
        } else {
            eprintln!("the compositor did not let this lock the screen; is another lock screen running?");
        }
        wl.release();
        self.released();
    }

    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: SessionLockSurface, configure: SessionLockSurfaceConfigure, _: u32) {
        let Some(wl) = self.wl.as_mut() else { return };
        let Some(i) = wl.lock_index(surface.wl_surface()) else { return };
        let o = &mut wl.locks[i];
        o.size = Some(configure.new_size);
        o.dirty = true;
        o.place_bubble();
        wl.request_wallpaper(i);
        wl.draw_locks(&self.screen, draw::ms());
        self.kick();
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.wl.as_mut().expect("wayland").shm
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.wl.as_mut().expect("wayland").seat
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, capability: Capability) {
        let handle = self.handle.clone();
        let Some(wl) = self.wl.as_mut() else { return };
        if capability == Capability::Keyboard && wl.keyboard.is_none() {
            wl.keyboard = wl.seat.get_keyboard_with_repeat(qh, &seat, None, handle, Box::new(|app: &mut App, _, event| app.key(event))).ok();
        }
        if capability == Capability::Pointer && wl.pointer.is_none() {
            let surface = wl.compositor.create_surface(qh);
            wl.pointer = wl.seat.get_pointer_with_theme(qh, &seat, wl.shm.wl_shm(), surface, ThemeSpec::default()).ok();
        }
    }

    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, capability: Capability) {
        let Some(wl) = self.wl.as_mut() else { return };
        if capability == Capability::Keyboard
            && let Some(k) = wl.keyboard.take()
        {
            k.release();
        }
        if capability == Capability::Pointer
            && let Some(p) = wl.pointer.take()
        {
            p.pointer().release();
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for App {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {}

    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {}

    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        self.key(event);
    }

    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}

    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: Modifiers, _: u32) {}
}

impl PointerHandler for App {
    fn pointer_frame(&mut self, conn: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for e in events {
            let Some(wl) = self.wl.as_mut() else { return };
            let Some(i) = wl.lock_index(&e.surface) else { continue };
            let name = wl.locks[i].name.clone();
            match e.kind {
                PointerEventKind::Enter { .. } => {
                    if let Some(p) = &wl.pointer {
                        let _ = p.set_cursor(conn, CursorIcon::Default);
                    }
                    self.pointer_on(name);
                }
                PointerEventKind::Motion { .. } | PointerEventKind::Press { .. } | PointerEventKind::Axis { .. } => {
                    self.pointer_on(name);
                    self.note_input();
                }
                _ => {}
            }
        }
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.wl.as_mut().expect("wayland").registry
    }
    registry_handlers![OutputState, SeatState];
}

impl Dispatch<ext_idle_notifier_v1::ExtIdleNotifierV1, ()> for App {
    fn event(_: &mut Self, _: &ext_idle_notifier_v1::ExtIdleNotifierV1, _: ext_idle_notifier_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ext_idle_notification_v1::ExtIdleNotificationV1, ()> for App {
    fn event(app: &mut Self, n: &ext_idle_notification_v1::ExtIdleNotificationV1, event: ext_idle_notification_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        // Only ever sent after idled: calm first, then this.
        if let ext_idle_notification_v1::Event::Resumed = event
            && app.wl.as_ref().and_then(|w| w.notification.as_ref()) == Some(n)
        {
            if let Some(wl) = app.wl.as_mut() {
                wl.stop_idle();
            }
            app.idle_resumed();
        }
    }
}

impl Dispatch<kde_lockscreen_overlay_v1::KdeLockscreenOverlayV1, ()> for App {
    fn event(_: &mut Self, _: &kde_lockscreen_overlay_v1::KdeLockscreenOverlayV1, _: kde_lockscreen_overlay_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

delegate_compositor!(App);
delegate_subcompositor!(App);
delegate_output!(App);
delegate_shm!(App);
delegate_layer!(App);
delegate_session_lock!(App);
delegate_seat!(App);
delegate_keyboard!(App);
delegate_pointer!(App);
delegate_registry!(App);
