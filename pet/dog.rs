use std::time::Duration;
use crate::pet::{Pet, PetKind, PetState, RgbaFrame};
use super::cat::decode_gif;

static IDLE_GIF:     &[u8] = include_bytes!("../../assets/dog/idle.gif");
static WALK_GIF:     &[u8] = include_bytes!("../../assets/dog/walk.gif");
static SLEEP_GIF:    &[u8] = include_bytes!("../../assets/dog/sleep.gif");
static INTERACT_GIF: &[u8] = include_bytes!("../../assets/dog/interact.gif");
static ALERT_GIF:    &[u8] = include_bytes!("../../assets/dog/alert.gif");

pub struct Dog {
    idle_frames:     Vec<RgbaFrame>,
    walk_frames:     Vec<RgbaFrame>,
    sleep_frames:    Vec<RgbaFrame>,
    interact_frames: Vec<RgbaFrame>,
    alert_frames:    Vec<RgbaFrame>,
    walk_direction:  f32,
}

impl Dog {
    pub fn new() -> Self {
        Self {
            idle_frames:     decode_gif(IDLE_GIF),
            walk_frames:     decode_gif(WALK_GIF),
            sleep_frames:    decode_gif(SLEEP_GIF),
            interact_frames: decode_gif(INTERACT_GIF),
            alert_frames:    decode_gif(ALERT_GIF),
            walk_direction:  1.0,
        }
    }
}

impl Pet for Dog {
    fn name(&self) -> &str { "Dog" }
    fn kind(&self) -> PetKind { PetKind::Dog }

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
            PetState::Interact => Some(PetState::Idle),
            _                  => None,
        }
    }

    fn walk_direction(&self) -> f32 { self.walk_direction }

    fn set_walk_direction(&mut self, dir: f32) {
        self.walk_direction = dir;
    }
}
