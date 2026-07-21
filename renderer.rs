use anyhow::Result;
use log::{debug, info, warn};
use softbuffer::{Context as SbContext, Surface};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tiny_skia::{Paint, Pixmap, PixmapPaint, Rect, Transform, ColorU8, PathBuilder, FillRule};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId, WindowLevel};

use crate::config::Config;
use crate::font::BitmapFont;
use crate::pet::{cat::Cat, dog::Dog, fish::Fish, Pet, PetKind, PetState, StateMachine};
use crate::plugin::LoadedPlugin;
use crate::stats::{self, StatsSnapshot, format_bytes};

// ─── Constants ────────────────────────────────────────────────────────────────

const TICK_MS:       u64 = 16;   // ~60 fps target
const HUD_WIDTH:     u32 = 220;
const HUD_HEIGHT:    u32 = 110;
const HUD_PADDING:   i32 = 8;
const MENU_WIDTH:    u32 = 180;
const MENU_ITEM_H:   u32 = 28;

// ─── Menu item ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum MenuItem {
    SwitchPet(PetKind),
    ToggleStats,
    Quit,
}

// ─── Application state ───────────────────────────────────────────────────────

struct App {
    // Core
    config:   Config,
    plugins:  Vec<LoadedPlugin>,

    // Window handles (set after window creation)
    window:   Option<Arc<Window>>,
    surface:  Option<Surface<Arc<Window>, Arc<Window>>>,

    // Pet
    pet:      Box<dyn Pet>,
    sm:       StateMachine,

    // Stats
    stats_rx: std::sync::mpsc::Receiver<StatsSnapshot>,
    latest_stats: Option<StatsSnapshot>,

    // Font
    font:     BitmapFont,

    // Drag state
    dragging:     bool,
    drag_start:   PhysicalPosition<f64>,   // cursor when drag began
    win_start:    (i32, i32),             // window pos when drag began

    // Cursor position (for menu hit-testing)
    cursor_pos: PhysicalPosition<f64>,

    // Right-click menu
    menu_open:  bool,
    menu_items: Vec<MenuItem>,
    menu_hover: Option<usize>,

    // Timing
    last_tick: Instant,
}

impl App {
    fn new(config: Config, plugins: Vec<LoadedPlugin>) -> Self {
        let pet = make_pet(&config.pet.kind, &plugins);
        let sm  = StateMachine::new(
            300,  // sleep after 5 minutes idle
            config.stats.alert_sustain_secs as u64,
        );
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
        }
    }

    // ── Window size from current frame ───────────────────────────────────────

    fn sprite_size(&self) -> (u32, u32) {
        let frames = self.pet.frames(self.sm.state);
        if frames.is_empty() {
            return (64, 64);
        }
        let f = &frames[self.sm.frame_index % frames.len()];
        let s = self.config.window.scale;
        ((f.width as f32 * s) as u32, (f.height as f32 * s) as u32)
    }

    // ── Build right-click menu items ─────────────────────────────────────────

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

    // ── Handle a menu selection ──────────────────────────────────────────────

    fn select_menu(&mut self, idx: usize, elp: &ActiveEventLoop) {
        self.menu_open = false;
        match self.menu_items.get(idx).cloned() {
            Some(MenuItem::SwitchPet(kind)) => {
                info!("Switching to {:?}", kind);
                self.pet = make_pet(&kind, &self.plugins);
                self.sm  = StateMachine::new(300, self.config.stats.alert_sustain_secs as u64);
                let _ = self.config.set_pet_kind(kind);
            }
            Some(MenuItem::ToggleStats) => {
                let en = !self.config.stats.enabled;
                let _ = self.config.set_stats_enabled(en);
                self.resize_window();
            }
            Some(MenuItem::Quit) => {
                elp.exit();
            }
            None => {}
        }
    }

    // ── Resize window to fit sprite (+ HUD if enabled) ───────────────────────

    fn resize_window(&self) {
        let Some(win) = &self.window else { return };
        let (sw, sh)  = self.sprite_size();
        let total_w   = if self.config.stats.enabled { sw.max(HUD_WIDTH) } else { sw };
        let total_h   = if self.config.stats.enabled { sh + HUD_HEIGHT } else { sh };
        win.request_inner_size(LogicalSize::new(total_w, total_h));
    }

    // ── Paint everything into the pixel buffer ───────────────────────────────

    fn paint(&mut self) {
        let Some(surface) = &mut self.surface else { return };
        let Some(window)  = &self.window       else { return };

        let (win_w, win_h) = {
            let s = window.inner_size();
            (s.width.max(1), s.height.max(1))
        };

        // Resize softbuffer surface to match window
        if let (Ok(w), Ok(h)) = (
            NonZeroU32::try_from(win_w),
            NonZeroU32::try_from(win_h),
        ) {
            let _ = surface.resize(w, h);
        }

        let mut buf = match surface.buffer_mut() {
            Ok(b)  => b,
            Err(e) => { warn!("surface.buffer_mut: {e}"); return; }
        };

        // Create a tiny-skia pixmap as our drawing canvas
        let mut pixmap = match Pixmap::new(win_w, win_h) {
            Some(p) => p,
            None    => return,
        };

        // ── Paint pet sprite ─────────────────────────────────────────────
        let frames = self.pet.frames(self.sm.state);
        if !frames.is_empty() {
            let frame   = &frames[self.sm.frame_index % frames.len()];
            let (sw, sh) = (frame.width, frame.height);
            let scale   = self.config.window.scale;

            if let Some(sprite) = Pixmap::from_vec(
                frame.pixels.clone(),
                tiny_skia::IntSize::from_wh(sw, sh).unwrap_or_default(),
            ) {
                let flip = if self.pet.walk_direction() < 0.0 { -1.0 } else { 1.0 };
                let tx   = if flip < 0.0 { sw as f32 * scale } else { 0.0 };

                let transform = Transform::from_scale(flip * scale, scale)
                    .post_translate(tx, 0.0);

                pixmap.draw_pixmap(
                    0, 0,
                    sprite.as_ref(),
                    &PixmapPaint::default(),
                    transform,
                    None,
                );
            }
        }

        // ── Paint stats HUD ──────────────────────────────────────────────
        if self.config.stats.enabled {
            if let Some(ref snap) = self.latest_stats {
                let (_, sh) = self.sprite_size();
                self.paint_hud(&mut pixmap, 0, sh as i32, snap.clone());
            }
        }

        // ── Paint right-click menu ───────────────────────────────────────
        if self.menu_open {
            self.paint_menu(&mut pixmap);
        }

        // ── Copy pixmap → softbuffer (BGRA on little-endian) ─────────────
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

        // Semi-transparent background
        let mut bg_paint = Paint::default();
        bg_paint.set_color_rgba8(20, 20, 30, 200);
        let bg_rect = Rect::from_xywh(x as f32, y as f32, HUD_WIDTH as f32, HUD_HEIGHT as f32);
        if let Some(r) = bg_rect {
            pixmap.fill_rect(r, &bg_paint, Transform::identity(), None);
        }

        let text_color = [220u8, 220, 220, 255];

        // ── CPU row ──────────────────────────────────────────────────────
        let row_y = y + p + 14;
        self.font.draw(pixmap, "CPU", x + p, row_y, 12.0, text_color);
        self.paint_bar(pixmap, x + p + 36, row_y - 10, 100, 10, snap.cpu_pct / 100.0, [100, 180, 255, 255]);
        let cpu_label = format!("{:.0}%", snap.cpu_pct);
        self.font.draw(pixmap, &cpu_label, x + p + 142, row_y, 12.0, text_color);

        // ── RAM row ──────────────────────────────────────────────────────
        let row_y = y + p + 32;
        self.font.draw(pixmap, "RAM", x + p, row_y, 12.0, text_color);
        let ram_pct = if snap.ram_total_gb > 0.0 { snap.ram_used_gb / snap.ram_total_gb } else { 0.0 };
        self.paint_bar(pixmap, x + p + 36, row_y - 10, 100, 10, ram_pct, [180, 120, 255, 255]);
        let ram_label = format!("{:.1}G", snap.ram_used_gb);
        self.font.draw(pixmap, &ram_label, x + p + 142, row_y, 12.0, text_color);

        // ── Network row ──────────────────────────────────────────────────
        let row_y = y + p + 52;
        let net_str = format!("↓{}  ↑{}", format_bytes(snap.net_rx_bps), format_bytes(snap.net_tx_bps));
        self.font.draw(pixmap, &net_str, x + p, row_y, 11.0, [100, 220, 180, 255]);

        // ── Disk row ─────────────────────────────────────────────────────
        let row_y = y + p + 68;
        let disk_str = if snap.disk_read_bps == 0 && snap.disk_wrt_bps == 0 {
            "DISK  idle".to_string()
        } else {
            format!("DISK  r:{} w:{}", format_bytes(snap.disk_read_bps), format_bytes(snap.disk_wrt_bps))
        };
        self.font.draw(pixmap, &disk_str, x + p, row_y, 11.0, [220, 180, 100, 255]);

        // ── CPU sparkline ─────────────────────────────────────────────────
        let row_y = y + p + 90;
        self.paint_sparkline(pixmap, x + p, row_y, 200, 14, &snap.cpu_history, [100, 180, 255, 120]);
    }

    fn paint_bar(
        &self,
        pixmap: &mut Pixmap,
        x: i32, y: i32,
        w: u32, h: u32,
        fill: f32,
        color: [u8; 4],
    ) {
        // Background track
        let mut track_paint = Paint::default();
        track_paint.set_color_rgba8(60, 60, 80, 180);
        if let Some(r) = Rect::from_xywh(x as f32, y as f32, w as f32, h as f32) {
            pixmap.fill_rect(r, &track_paint, Transform::identity(), None);
        }

        // Fill
        let fill_w = (w as f32 * fill.clamp(0.0, 1.0)).max(1.0);
        let mut fill_paint = Paint::default();
        fill_paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
        if let Some(r) = Rect::from_xywh(x as f32, y as f32, fill_w, h as f32) {
            pixmap.fill_rect(r, &fill_paint, Transform::identity(), None);
        }
    }

    fn paint_sparkline(
        &self,
        pixmap: &mut Pixmap,
        x: i32, y: i32,
        w: u32, h: u32,
        ring: &crate::stats::RingBuffer,
        color: [u8; 4],
    ) {
        let vals: Vec<f32> = ring.iter().collect();
        if vals.len() < 2 { return; }

        let step  = w as f32 / (vals.len() - 1) as f32;
        let mut pb = PathBuilder::new();
        let mut first = true;

        for (i, &v) in vals.iter().enumerate() {
            let px = x as f32 + i as f32 * step;
            let py = y as f32 + h as f32 - (v / 100.0 * h as f32);
            if first { pb.move_to(px, py); first = false; }
            else      { pb.line_to(px, py); }
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

        // Background
        let mut bg = Paint::default();
        bg.set_color_rgba8(30, 30, 45, 240);
        if let Some(r) = Rect::from_xywh(x as f32, y as f32, MENU_WIDTH as f32, mh as f32) {
            pixmap.fill_rect(r, &bg, Transform::identity(), None);
        }

        for (i, item) in self.menu_items.iter().enumerate() {
            let iy = y + i as i32 * MENU_ITEM_H as i32;

            // Hover highlight
            if self.menu_hover == Some(i) {
                let mut hl = Paint::default();
                hl.set_color_rgba8(60, 80, 130, 200);
                if let Some(r) = Rect::from_xywh(x as f32, iy as f32, MENU_WIDTH as f32, MENU_ITEM_H as f32) {
                    pixmap.fill_rect(r, &hl, Transform::identity(), None);
                }
            }

            let label = match item {
                MenuItem::SwitchPet(PetKind::Cat)         => "🐱  Cat".to_string(),
                MenuItem::SwitchPet(PetKind::Dog)         => "🐶  Dog".to_string(),
                MenuItem::SwitchPet(PetKind::Fish)        => "🐟  Fish".to_string(),
                MenuItem::SwitchPet(PetKind::Plugin(n))   => format!("🔌  {n}"),
                MenuItem::ToggleStats => {
                    if self.config.stats.enabled { "📊  Hide Stats".into() }
                    else                         { "📊  Show Stats".into() }
                }
                MenuItem::Quit => "✕   Quit".to_string(),
            };

            self.font.draw(pixmap, &label, x + 10, iy + 19, 13.0, [220, 220, 220, 255]);
        }
    }
}

// ─── winit ApplicationHandler ─────────────────────────────────────────────────

impl ApplicationHandler for App {
    fn resumed(&mut self, elp: &ActiveEventLoop) {
        let (sw, sh)  = (100u32, 100u32); // initial size, resized after first frame
        let total_w   = if self.config.stats.enabled { sw.max(HUD_WIDTH) } else { sw };
        let total_h   = if self.config.stats.enabled { sh + HUD_HEIGHT }  else { sh };

        let attrs = Window::default_attributes()
            .with_title("LinuxPet")
            .with_inner_size(LogicalSize::new(total_w, total_h))
            .with_position(LogicalPosition::new(self.config.window.x, self.config.window.y))
            .with_decorations(false)
            .with_transparent(true)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_resizable(false);

        let window = Arc::new(elp.create_window(attrs).expect("create window"));
        let sb_ctx = SbContext::new(window.clone()).expect("softbuffer context");
        let surface = Surface::new(&sb_ctx, window.clone()).expect("softbuffer surface");

        self.window  = Some(window);
        self.surface = Some(surface);

        info!("Window created");
    }

    fn window_event(&mut self, elp: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            // ── Close ───────────────────────────────────────────────────────
            WindowEvent::CloseRequested | WindowEvent::Destroyed => {
                elp.exit();
            }

            // ── Redraw ──────────────────────────────────────────────────────
            WindowEvent::RedrawRequested => {
                // Drain stats channel
                while let Ok(snap) = self.stats_rx.try_recv() {
                    self.latest_stats = Some(snap);
                }

                // Tick state machine
                let dt       = self.last_tick.elapsed();
                self.last_tick = Instant::now();
                let cpu_pct  = self.latest_stats.as_ref().map(|s| s.cpu_pct).unwrap_or(0.0);
                self.sm.tick(dt, self.pet.as_mut(), cpu_pct, self.config.stats.alert_cpu_pct);

                // Fish drift — move window to follow sine wave
                if self.config.pet.kind == PetKind::Fish {
                    if let Some(fish) = self.pet.as_any_mut().and_then(|a| a.downcast_mut::<Fish>()) {
                        let (dx, dy) = fish.drift();
                        if let Some(win) = &self.window {
                            let pos = LogicalPosition::new(
                                self.config.window.x + dx as i32,
                                self.config.window.y + dy as i32,
                            );
                            win.set_outer_position(pos);
                        }
                    }
                }

                self.paint();
                // Schedule next tick
                elp.set_control_flow(ControlFlow::WaitUntil(
                    Instant::now() + Duration::from_millis(TICK_MS),
                ));
                if let Some(win) = &self.window {
                    win.request_redraw();
                }
            }

            // ── Cursor moved ─────────────────────────────────────────────────
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_pos = position;

                if self.dragging {
                    let dx = position.x - self.drag_start.x;
                    let dy = position.y - self.drag_start.y;
                    let new_x = (self.win_start.0 as f64 + dx) as i32;
                    let new_y = (self.win_start.1 as f64 + dy) as i32;

                    if let Some(win) = &self.window {
                        win.set_outer_position(LogicalPosition::new(new_x, new_y));
                    }
                }

                // Update menu hover
                if self.menu_open {
                    let mx = self.cursor_pos.x as i32;
                    let my = self.cursor_pos.y as i32;
                    self.menu_hover = self.menu_items.iter().enumerate().find(|(i, _)| {
                        let iy = my + *i as i32 * MENU_ITEM_H as i32;
                        mx >= 0 && mx <= MENU_WIDTH as i32
                            && my >= iy && my < iy + MENU_ITEM_H as i32
                    }).map(|(i, _)| i);
                }
            }

            // ── Mouse buttons ────────────────────────────────────────────────
            WindowEvent::MouseInput { button, state, .. } => {
                match (button, state) {
                    // Left press: start drag or dismiss menu
                    (MouseButton::Left, ElementState::Pressed) => {
                        if self.menu_open {
                            // Check if clicked on a menu item
                            let cy = self.cursor_pos.y as i32;
                            if let Some(idx) = (0..self.menu_items.len()).find(|&i| {
                                let iy = i as i32 * MENU_ITEM_H as i32;
                                cy >= iy && cy < iy + MENU_ITEM_H as i32
                                    && self.cursor_pos.x as i32 >= 0
                                    && (self.cursor_pos.x as u32) <= MENU_WIDTH
                            }) {
                                let idx = idx;
                                self.select_menu(idx, elp);
                            } else {
                                self.menu_open = false;
                            }
                        } else {
                            // Begin drag
                            self.dragging   = true;
                            self.drag_start = self.cursor_pos;
                            if let Some(win) = &self.window {
                                let pos = win.outer_position().unwrap_or_default();
                                self.win_start = (pos.x, pos.y);
                            }
                            // Trigger interact animation
                            self.sm.on_click();
                        }
                    }

                    // Left release: end drag, save position
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

                    // Right press: open context menu
                    (MouseButton::Right, ElementState::Pressed) => {
                        self.build_menu();
                        self.menu_open  = true;
                        self.menu_hover = None;
                    }

                    _ => {}
                }
            }

            // ── Keyboard ─────────────────────────────────────────────────────
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
        if let Some(win) = &self.window {
            win.request_redraw();
        }
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
            // Find the matching loaded plugin and wrap it
            plugins
                .iter()
                .find(|p| p.kind == PetKind::Plugin(name.clone()))
                .map(|_p| {
                    // For now fall back to Cat until PluginPet wrapper is built in issue #13
                    warn!("Plugin pet '{}' found but PluginPet wrapper not yet implemented — using Cat", name);
                    Box::new(Cat::new()) as Box<dyn Pet>
                })
                .unwrap_or_else(|| {
                    warn!("Plugin '{}' not found — falling back to Cat", name);
                    Box::new(Cat::new())
                })
        }
    }
}
