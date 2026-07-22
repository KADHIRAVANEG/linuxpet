use std::time::Duration;
use crate::pet::{Pet, PetKind, PetState, RgbaFrame};
use super::cat::decode_gif;

static IDLE_GIF:  &[u8] = include_bytes!("../../assets/fish/idle.gif");
static SWIM_GIF:  &[u8] = include_bytes!("../../assets/fish/swim.gif");
static ALERT_GIF: &[u8] = include_bytes!("../../assets/fish/alert.gif");

pub struct Fish {
    idle_frames:  Vec<RgbaFrame>,
    swim_frames:  Vec<RgbaFrame>,
    alert_frames: Vec<RgbaFrame>,

    /// Accumulated time for sine-wave drift calculation
    pub t: f32,
    /// Walk direction field kept for trait compatibility (unused — fish drifts)
    direction: f32,
}

impl Fish {
    pub fn new() -> Self {
        Self {
            idle_frames:  decode_gif(IDLE_GIF),
            swim_frames:  decode_gif(SWIM_GIF),
            alert_frames: decode_gif(ALERT_GIF),
            t:            0.0,
            direction:    1.0,
        }
    }

    /// Returns the (dx, dy) drift offset from the anchor position.
    /// Renderer adds this to the base window position each frame.
    #[allow(dead_code)]
    pub fn drift(&self) -> (f32, f32) {
        let dx = (self.t * 0.5).sin() * 40.0;  // ±40px horizontal
        let dy = (self.t * 0.8).sin() * 10.0;  // ±10px vertical bob
        (dx, dy)
    }
}

impl Pet for Fish {
    fn name(&self) -> &str { "Fish" }
    fn kind(&self) -> PetKind { PetKind::Fish }

    fn frames(&self, state: PetState) -> &[RgbaFrame] {
        match state {
            PetState::Walk     => &self.swim_frames, // walk → swim for fish
            PetState::Alert    => &self.alert_frames,
            _                  => &self.idle_frames,
        }
    }

    fn tick(&mut self, dt: Duration, state: PetState) -> Option<PetState> {
        // Advance sine time every tick regardless of state
        self.t += dt.as_secs_f32();

        match state {
            PetState::Interact => Some(PetState::Idle),
            PetState::Sleep    => Some(PetState::Idle), // fish don't sleep — return to idle
            _                  => None,
        }
    }

    fn drift_offset(&self) -> f32 {
        // Return horizontal component; renderer calls fish.drift() for full (dx, dy)
        self.drift().0
    }

    fn walk_direction(&self) -> f32 { self.direction }
    fn set_walk_direction(&mut self, dir: f32) { self.direction = dir; }
}
