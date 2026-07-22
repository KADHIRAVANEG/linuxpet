use std::time::Duration;
use crate::pet::{Pet, PetKind, PetState, RgbaFrame};

// ─── Embedded sprite bytes ────────────────────────────────────────────────────
//
// At build time, cargo embeds the GIF files from assets/cat/ into the binary.
// Replace the placeholder paths with real sprites before releasing.

static IDLE_GIF:     &[u8] = include_bytes!("../../assets/cat/idle.gif");
static WALK_GIF:     &[u8] = include_bytes!("../../assets/cat/walk.gif");
static SLEEP_GIF:    &[u8] = include_bytes!("../../assets/cat/sleep.gif");
static INTERACT_GIF: &[u8] = include_bytes!("../../assets/cat/interact.gif");
static ALERT_GIF:    &[u8] = include_bytes!("../../assets/cat/alert.gif");

// ─── Cat ──────────────────────────────────────────────────────────────────────

pub struct Cat {
    idle_frames:     Vec<RgbaFrame>,
    walk_frames:     Vec<RgbaFrame>,
    sleep_frames:    Vec<RgbaFrame>,
    interact_frames: Vec<RgbaFrame>,
    alert_frames:    Vec<RgbaFrame>,

    walk_direction:  f32,
    walk_loops_done: u32,
    /// How many full walk loops before returning to Idle
    #[allow(dead_code)]
    max_walk_loops:  u32,
}

impl Cat {
    pub fn new() -> Self {
        Self {
            idle_frames:     decode_gif(IDLE_GIF),
            walk_frames:     decode_gif(WALK_GIF),
            sleep_frames:    decode_gif(SLEEP_GIF),
            interact_frames: decode_gif(INTERACT_GIF),
            alert_frames:    decode_gif(ALERT_GIF),

            walk_direction:  1.0,
            walk_loops_done: 0,
            max_walk_loops:  3,
        }
    }
}

impl Pet for Cat {
    fn name(&self) -> &str { "Cat" }
    fn kind(&self) -> PetKind { PetKind::Cat }

    fn frames(&self, state: PetState) -> &[RgbaFrame] {
        match state {
            PetState::Idle     => &self.idle_frames,
            PetState::Walk     => &self.walk_frames,
            PetState::Sleep    => &self.sleep_frames,
            PetState::Interact => &self.interact_frames,
            PetState::Alert    => &self.alert_frames,
        }
    }

    fn tick(&mut self, _dt: Duration, state: PetState) -> Option<PetState> {
        match state {
            PetState::Walk => {
                // StateMachine already advanced the frame index.
                // We just need to count completed loops here.
                // Walk → Idle after max_walk_loops.
                // NOTE: loop counting is tracked externally via frame_index wrapping;
                // this is a stub that the renderer increments. For now we signal
                // "keep walking" by returning None — the renderer calls on_walk_loop_complete.
                None
            }
            PetState::Interact => {
                // Play once — renderer calls this when frame_index wraps back to 0
                Some(PetState::Idle)
            }
            _ => None,
        }
    }

    fn walk_direction(&self) -> f32 { self.walk_direction }

    fn set_walk_direction(&mut self, dir: f32) {
        self.walk_direction  = dir;
        self.walk_loops_done = 0;
    }
}

// ─── GIF decoder ─────────────────────────────────────────────────────────────

/// Decode a GIF byte slice into a Vec of RGBA frames with per-frame delay.
pub fn decode_gif(data: &[u8]) -> Vec<RgbaFrame> {
    use image::AnimationDecoder;
    use image::codecs::gif::GifDecoder;
    use std::io::Cursor;

    let cursor  = Cursor::new(data);
    let decoder = match GifDecoder::new(cursor) {
        Ok(d)  => d,
        Err(e) => {
            log::warn!("GIF decode error: {e}");
            return vec![placeholder_frame()];
        }
    };

    let frames: Vec<RgbaFrame> = decoder
        .into_frames()
        .filter_map(|f| f.ok())
        .map(|frame| {
            // frame.delay() returns (numerator, denominator) in seconds
            let (n, d)  = frame.delay().numer_denom_ms();
            let delay_ms = if d == 0 { 100 } else { n / d };
            let delay    = Duration::from_millis(delay_ms.max(16) as u64);

            let img    = frame.into_buffer();
            let width  = img.width();
            let height = img.height();
            let pixels = img.into_raw(); // already RGBA

            RgbaFrame { pixels, width, height, delay }
        })
        .collect();

    if frames.is_empty() {
        vec![placeholder_frame()]
    } else {
        frames
    }
}

/// 64×64 magenta placeholder — visible if a GIF fails to load.
fn placeholder_frame() -> RgbaFrame {
    let w = 64u32;
    let h = 64u32;
    let pixels = (0..w * h)
        .flat_map(|_| [255u8, 0, 255, 255])
        .collect();
    RgbaFrame {
        pixels,
        width:  w,
        height: h,
        delay:  Duration::from_millis(100),
    }
}
