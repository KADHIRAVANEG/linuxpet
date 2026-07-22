use anyhow::Result;
use log::{info, warn};
use softbuffer::{Context as SbContext, Surface};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tiny_skia::{Paint, Pixmap, PixmapPaint, Rect, Transform, PathBuilder};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId, WindowLevel};

use crate::config::Config;
use crate::font::BitmapFont;
use crate::pet::{cat::Cat, dog::Dog, fish::Fish, Pet, PetKind, StateMachine};
use crate::plugin::LoadedPlugin;
use crate::stats::{self, StatsSnapshot, format_bytes};

// ─── Constants ────────────────────────────────────────────────────────────────

const TICK_MS:     u64 = 16;
const HUD_WIDTH:   u32 = 220;
const HUD_HEIGHT:  u32 = 110;
const HUD_PADDING: i32 = 8;
const MENU_WIDTH:  u32 = 180;
const MENU_ITEM_H: u32 = 28;

// ─── Menu item ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum MenuItem {
    SwitchPet(PetKind),
    ToggleStats,
    Quit,
}

// ─── Application state ───────────────────────────────────────────────────────

struct App {
    config:       Config,
    plugins:      Vec<LoadedPlugin>,
    window:       Option<Arc<Window>>,
    surface:      Option<Surface<Arc<Window>, Arc<Window>>>,
    pet:          Box<dyn Pet>,
    sm:           StateMachine,
    stats_rx:     std::sync::mpsc::Receiver<StatsSnapshot>,
    latest_stats: Option<StatsSnapshot>,
    font:         BitmapFont,
    dragging:     bool,
    drag_start:   PhysicalPosition<f64>,
    win_start:    (i32, i32),
    cursor_pos:   PhysicalPosition<f64>,
    menu_open:    bool,
    menu_items:   Vec<MenuItem>,
    menu_hover:   Option<usize>,
    last_tick:    Instant,
    // Fish drift time accumulator
    fish_t:       f32,
}

impl App {
    fn new(config: Config, plugins: Vec<LoadedPlugin>) -> Self {
        let pet      = make_pet(&config.pet.kind, &plugins);
        let sm       = StateMachine::new(300, config.stats.alert_sustain_secs as u64);
        let stats_rx = stats::spawn();
        Self {
            config,
            plugins,
            window:       None,
            surface:      None,
            pet,
            sm,
            stats_rx,
            latest_stats: None,
            font:         BitmapFont::new(),
            dragging:     false,
            drag_start:   PhysicalPosition::new(0.0, 0.0),
            win_start:    (0, 0),
            cursor_pos:   PhysicalPosition::new(0.0, 0.0),
            menu_open:    false,
            menu_items:   Vec::new(),
            menu_hover:   None,
            last_tick:    Instant::now(),
            fish_t:       0.0,
        }
    }

    fn sprite_size(&self) -> (u32, u32) {
        let frames = self.pet.frames(self.sm.state);
        if frames.is_empty() { return (64, 64); }
        let f = &frames[self.sm.frame_index % frames.len()];
        let s = self.config.window.scale;
        ((f.width as f32 * s) as u32, (f.height as f32 * s) as u32)
    }

    fn build_menu(&mut self) {
        let mut items = vec![
            MenuItem::SwitchPet(PetKind::Cat),
            MenuItem::SwitchPet(PetKind::Dog),
            MenuItem::SwitchPet(PetKind::Fish),
        ];
        for p in &self.plugins {
            items.push(MenuItem::SwitchPet(p.kind.clone()));
        }
        items.push(MenuItem::ToggleStats);
        items.push(MenuItem::Quit);
        self.menu_items = items;
    }

    fn select_menu(&mut self, idx: usize, elp: &ActiveEventLoop) {
        self.menu_open = false;
        match self.menu_items.get(idx).cloned() {
            Some(MenuItem::SwitchPet(kind)) => {
                info!("Switching to {:?}", kind);
                self.pet    = make_pet(&kind, &self.plugins);
                self.sm     = StateMachine::new(300, self.config.stats.alert_sustain_secs as u64);
                self.fish_t = 0.0;
                let _ = self.config.set_pet_kind(kind);
            }
            Some(MenuItem::ToggleStats) => {
                let en = !self.config.stats.enabled;
                let _ = self.config.set_stats_enabled(en);
                self.resize_window();
            }
            Some(MenuItem::Quit) => elp.exit(),
            None => {}
        }
    }

    fn resize_window(&self) {
        let Some(win) = &self.window else { return };
        let (sw, sh) = self.sprite_size();
        let total_w  = if self.config.stats.enabled { sw.max(HUD_WIDTH) } else { sw };
        let total_h  = if self.config.stats.enabled { sh + HUD_HEIGHT }  else { sh };
        let _ = win.request_inner_size(LogicalSize::new(total_w, total_h));
    }

    fn paint(&mut self) {
        // Extract everything we need before any mutable borrow of surface
        let win_size = self.window.as_ref().map(|w| w.inner_size());
        let (win_w, win_h) = match win_size {
            Some(s) => (s.width.max(1), s.height.max(1)),
            None    => return,
        };

        // Build pixmap — all immutable borrows happen here
        let mut pixmap = match Pixmap::new(win_w, win_h) {
            Some(p) => p,
            None    => return,
        };

        // Sprite
        let frame_index = self.sm.frame_index;
        let state       = self.sm.state;
        let scale       = self.config.window.scale;
        let walk_dir    = self.pet.walk_direction();
        let frames      = self.pet.frames(state);

        if !frames.is_empty() {
            let frame    = &frames[frame_index % frames.len()];
            let (sw, sh) = (frame.width, frame.height);
            if let Some(size) = tiny_skia::IntSize::from_wh(sw, sh) {
                if let Some(sprite) = Pixmap::from_vec(frame.pixels.clone(), size) {
                    let flip      = if walk_dir < 0.0 { -1.0_f32 } else { 1.0 };
                    let tx        = if flip < 0.0 { sw as f32 * scale } else { 0.0 };
                    let transform = Transform::from_scale(flip * scale, scale)
                        .post_translate(tx, 0.0);
                    pixmap.draw_pixmap(0, 0, sprite.as_ref(), &PixmapPaint::default(), transform, None);
                }
            }
        }

        // Stats HUD
        let stats_enabled  = self.config.stats.enabled;
        let sprite_h       = self.sprite_size().1;
        if stats_enabled {
            if let Some(snap) = self.latest_stats.clone() {
                self.paint_hud(&mut pixmap, 0, sprite_h as i32, snap);
            }
        }

        // Menu
        if self.menu_open {
            self.paint_menu(&mut pixmap);
        }

        // Now do the mutable surface borrow for the final blit
        let Some(surface) = &mut self.surface else { return };
        if let (Ok(w), Ok(h)) = (NonZeroU32::try_from(win_w), NonZeroU32::try_from(win_h)) {
            let _ = surface.resize(w, h);
        }
        let mut buf = match surface.buffer_mut() {
            Ok(b)  => b,
            Err(e) => { warn!("surface.buffer_mut: {e}"); return; }
        };
        for (dst, src) in buf.iter_mut().zip(pixmap.pixels()) {
            *dst = ((src.alpha() as u32) << 24)
                | ((src.red()   as u32) << 16)
                | ((src.green() as u32) << 8)
                |  (src.blue()  as u32);
        }
        let _ = buf.present();
    }

    fn paint_hud(&self, pixmap: &mut Pixmap, x: i32, y: i32, snap: StatsSnapshot) {
        let p = HUD_PADDING;

        let mut bg = Paint::default();
        bg.set_color_rgba8(20, 20, 30, 200);
        if let Some(r) = Rect::from_xywh(x as f32, y as f32, HUD_WIDTH as f32, HUD_HEIGHT as f32) {
            pixmap.fill_rect(r, &bg, Transform::identity(), None);
        }

        let white = [220u8, 220, 220, 255];

        // CPU
        let ry = y + p + 14;
        self.font.draw(pixmap, "CPU", x + p, ry, 12.0, white);
        self.paint_bar(pixmap, x + p + 36, ry - 10, 100, 10, snap.cpu_pct / 100.0, [100, 180, 255, 255]);
        self.font.draw(pixmap, &format!("{:.0}%", snap.cpu_pct), x + p + 142, ry, 12.0, white);

        // RAM
        let ry = y + p + 32;
        self.font.draw(pixmap, "RAM", x + p, ry, 12.0, white);
        let ram_pct = if snap.ram_total_gb > 0.0 { snap.ram_used_gb / snap.ram_total_gb } else { 0.0 };
        self.paint_bar(pixmap, x + p + 36, ry - 10, 100, 10, ram_pct, [180, 120, 255, 255]);
        self.font.draw(pixmap, &format!("{:.1}G", snap.ram_used_gb), x + p + 142, ry, 12.0, white);

        // Network
        let ry = y + p + 52;
        let net = format!("↓{}  ↑{}", format_bytes(snap.net_rx_bps), format_bytes(snap.net_tx_bps));
        self.font.draw(pixmap, &net, x + p, ry, 11.0, [100, 220, 180, 255]);

        // Disk space
        let ry = y + p + 68;
        let disk = format!("DSK {:.0}/{:.0}GB free",
            snap.disk_avail_gb, snap.disk_total_gb);
        self.font.draw(pixmap, &disk, x + p, ry, 11.0, [220, 180, 100, 255]);

        // Sparkline
        self.paint_sparkline(pixmap, x + p, y + p + 90, 200, 14, &snap.cpu_history, [100, 180, 255, 120]);
    }

    fn paint_bar(&self, pixmap: &mut Pixmap, x: i32, y: i32, w: u32, h: u32, fill: f32, color: [u8; 4]) {
        let mut track = Paint::default();
        track.set_color_rgba8(60, 60, 80, 180);
        if let Some(r) = Rect::from_xywh(x as f32, y as f32, w as f32, h as f32) {
            pixmap.fill_rect(r, &track, Transform::identity(), None);
        }
        let fw = (w as f32 * fill.clamp(0.0, 1.0)).max(1.0);
        let mut fp = Paint::default();
        fp.set_color_rgba8(color[0], color[1], color[2], color[3]);
        if let Some(r) = Rect::from_xywh(x as f32, y as f32, fw, h as f32) {
            pixmap.fill_rect(r, &fp, Transform::identity(), None);
        }
    }

    fn paint_sparkline(&self, pixmap: &mut Pixmap, x: i32, y: i32, w: u32, h: u32,
                        ring: &crate::stats::RingBuffer, color: [u8; 4]) {
        let vals: Vec<f32> = ring.iter().collect();
        if vals.len() < 2 { return; }
        let step = w as f32 / (vals.len() - 1) as f32;
        let mut pb = PathBuilder::new();
        let mut first = true;
        for (i, &v) in vals.iter().enumerate() {
            let px = x as f32 + i as f32 * step;
            let py = y as f32 + h as f32 - (v / 100.0 * h as f32);
            if first { pb.move_to(px, py); first = false; } else { pb.line_to(px, py); }
        }
        if let Some(path) = pb.finish() {
            let mut stroke = tiny_skia::Stroke::default();
            stroke.width = 1.5;
            let mut paint = Paint::default();
            paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
            pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
    }

    fn paint_menu(&self, pixmap: &mut Pixmap) {
        let x  = self.cursor_pos.x as i32;
        let y  = self.cursor_pos.y as i32;
        let mh = MENU_ITEM_H as i32 * self.menu_items.len() as i32;

        let mut bg = Paint::default();
        bg.set_color_rgba8(30, 30, 45, 240);
        if let Some(r) = Rect::from_xywh(x as f32, y as f32, MENU_WIDTH as f32, mh as f32) {
            pixmap.fill_rect(r, &bg, Transform::identity(), None);
        }

        for (i, item) in self.menu_items.iter().enumerate() {
            let iy = y + i as i32 * MENU_ITEM_H as i32;
            if self.menu_hover == Some(i) {
                let mut hl = Paint::default();
                hl.set_color_rgba8(60, 80, 130, 200);
                if let Some(r) = Rect::from_xywh(x as f32, iy as f32, MENU_WIDTH as f32, MENU_ITEM_H as f32) {
                    pixmap.fill_rect(r, &hl, Transform::identity(), None);
                }
            }
            let label = match item {
                MenuItem::SwitchPet(PetKind::Cat)       => "Cat".to_string(),
                MenuItem::SwitchPet(PetKind::Dog)       => "Dog".to_string(),
                MenuItem::SwitchPet(PetKind::Fish)      => "Fish".to_string(),
                MenuItem::SwitchPet(PetKind::Plugin(n)) => format!("Plugin: {n}"),
                MenuItem::ToggleStats => {
                    if self.config.stats.enabled { "Hide Stats".into() } else { "Show Stats".into() }
                }
                MenuItem::Quit => "Quit".to_string(),
            };
            self.font.draw(pixmap, &label, x + 10, iy + 19, 13.0, [220, 220, 220, 255]);
        }
    }
}

// ─── winit ApplicationHandler ─────────────────────────────────────────────────

impl ApplicationHandler for App {
    fn resumed(&mut self, elp: &ActiveEventLoop) {
        let (sw, sh) = (100u32, 100u32);
        let total_w  = if self.config.stats.enabled { sw.max(HUD_WIDTH) } else { sw };
        let total_h  = if self.config.stats.enabled { sh + HUD_HEIGHT }  else { sh };

        let attrs = Window::default_attributes()
            .with_title("LinuxPet")
            .with_inner_size(LogicalSize::new(total_w, total_h))
            .with_position(LogicalPosition::new(self.config.window.x, self.config.window.y))
            .with_decorations(false)
            .with_transparent(true)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_resizable(false);

        let window  = Arc::new(elp.create_window(attrs).expect("create window"));
        let sb_ctx  = SbContext::new(window.clone()).expect("softbuffer context");
        let surface = Surface::new(&sb_ctx, window.clone()).expect("softbuffer surface");

        self.window  = Some(window);
        self.surface = Some(surface);
        info!("Window created");
    }

    fn window_event(&mut self, elp: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => elp.exit(),

            WindowEvent::RedrawRequested => {
                // Drain stats channel
                while let Ok(snap) = self.stats_rx.try_recv() {
                    self.latest_stats = Some(snap);
                }

                // Tick
                let dt = self.last_tick.elapsed();
                self.last_tick = Instant::now();
                let cpu_pct = self.latest_stats.as_ref().map(|s| s.cpu_pct).unwrap_or(0.0);
                self.sm.tick(dt, self.pet.as_mut(), cpu_pct, self.config.stats.alert_cpu_pct);

                // Fish drift — pure sine, no downcast needed
                if self.config.pet.kind == PetKind::Fish {
                    self.fish_t += dt.as_secs_f32();
                    let dx = (self.fish_t * 0.5).sin() * 40.0_f32;
                    let dy = (self.fish_t * 0.8).sin() * 10.0_f32;
                    if let Some(win) = &self.window {
                        win.set_outer_position(LogicalPosition::new(
                            self.config.window.x + dx as i32,
                            self.config.window.y + dy as i32,
                        ));
                    }
                }

                self.paint();
                elp.set_control_flow(ControlFlow::WaitUntil(
                    Instant::now() + Duration::from_millis(TICK_MS),
                ));
                if let Some(win) = &self.window { win.request_redraw(); }
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_pos = position;
                if self.dragging {
                    let dx    = position.x - self.drag_start.x;
                    let dy    = position.y - self.drag_start.y;
                    let new_x = (self.win_start.0 as f64 + dx) as i32;
                    let new_y = (self.win_start.1 as f64 + dy) as i32;
                    if let Some(win) = &self.window {
                        win.set_outer_position(LogicalPosition::new(new_x, new_y));
                    }
                }
                if self.menu_open {
                    let (mx, my) = (self.cursor_pos.x as i32, self.cursor_pos.y as i32);
                    self.menu_hover = (0..self.menu_items.len()).find(|&i| {
                        let iy = my + i as i32 * MENU_ITEM_H as i32;
                        mx >= 0 && mx <= MENU_WIDTH as i32
                            && my >= iy && my < iy + MENU_ITEM_H as i32
                    });
                }
            }

            WindowEvent::MouseInput { button, state, .. } => {
                match (button, state) {
                    (MouseButton::Left, ElementState::Pressed) => {
                        if self.menu_open {
                            let cy = self.cursor_pos.y as i32;
                            if let Some(idx) = (0..self.menu_items.len()).find(|&i| {
                                let iy = i as i32 * MENU_ITEM_H as i32;
                                cy >= iy && cy < iy + MENU_ITEM_H as i32
                                    && self.cursor_pos.x as i32 >= 0
                                    && (self.cursor_pos.x as u32) <= MENU_WIDTH
                            }) {
                                self.select_menu(idx, elp);
                            } else {
                                self.menu_open = false;
                            }
                        } else {
                            self.dragging   = true;
                            self.drag_start = self.cursor_pos;
                            if let Some(win) = &self.window {
                                let pos = win.outer_position().unwrap_or_default();
                                self.win_start = (pos.x, pos.y);
                            }
                            self.sm.on_click();
                        }
                    }
                    (MouseButton::Left, ElementState::Released) => {
                        if self.dragging {
                            self.dragging = false;
                            if let Some(win) = &self.window {
                                if let Ok(pos) = win.outer_position() {
                                    let _ = self.config.set_position(pos.x, pos.y);
                                }
                            }
                        }
                    }
                    (MouseButton::Right, ElementState::Pressed) => {
                        self.build_menu();
                        self.menu_open  = true;
                        self.menu_hover = None;
                    }
                    _ => {}
                }
            }

            WindowEvent::KeyboardInput { event, .. } => {
                use winit::keyboard::{Key, NamedKey};
                if event.state == ElementState::Pressed {
                    if let Key::Named(NamedKey::Escape) = event.logical_key {
                        self.menu_open = false;
                    }
                }
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, elp: &ActiveEventLoop) {
        if let Some(win) = &self.window { win.request_redraw(); }
        elp.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(TICK_MS),
        ));
    }
}

// ─── Entry point ─────────────────────────────────────────────────────────────

pub fn run(config: Config, plugins: Vec<LoadedPlugin>) -> Result<()> {
    let event_loop = EventLoop::new()?;
    let mut app    = App::new(config, plugins);
    event_loop.run_app(&mut app)?;
    Ok(())
}

// ─── Pet factory ─────────────────────────────────────────────────────────────

fn make_pet(kind: &PetKind, plugins: &[LoadedPlugin]) -> Box<dyn Pet> {
    match kind {
        PetKind::Cat  => Box::new(Cat::new()),
        PetKind::Dog  => Box::new(Dog::new()),
        PetKind::Fish => Box::new(Fish::new()),
        PetKind::Plugin(name) => {
            if plugins.iter().any(|p| p.kind == PetKind::Plugin(name.clone())) {
                warn!("Plugin '{}' found but wrapper not yet implemented — using Cat", name);
            } else {
                warn!("Plugin '{}' not found — falling back to Cat", name);
            }
            Box::new(Cat::new())
        }
    }
}
