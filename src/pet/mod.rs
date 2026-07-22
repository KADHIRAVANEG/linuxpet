pub mod cat;
pub mod dog;
pub mod fish;

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

// ─── Pet kind (serialisable for config) ──────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PetKind {
    Cat,
    Dog,
    Fish,
    /// Community plugin — name matches manifest.pet.name (lowercase)
    Plugin(String),
}

impl PetKind {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "cat"  => PetKind::Cat,
            "dog"  => PetKind::Dog,
            "fish" => PetKind::Fish,
            other  => PetKind::Plugin(other.to_owned()),
        }
    }
}

// ─── Pet state ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PetState {
    Idle,
    Walk,
    Sleep,
    /// Triggered by left-click — plays once then returns to Idle
    Interact,
    /// Triggered when CPU stays above threshold
    Alert,
}

// ─── RGBA frame (one decoded GIF frame) ──────────────────────────────────────

#[derive(Clone)]
pub struct RgbaFrame {
    pub pixels: Vec<u8>,   // raw RGBA bytes
    pub width:  u32,
    pub height: u32,
    /// How long to show this frame (from GIF metadata)
    pub delay:  Duration,
}

// ─── Pet trait ────────────────────────────────────────────────────────────────

#[allow(dead_code)]
pub trait Pet: Send {
    fn name(&self) -> &str;
    fn kind(&self) -> PetKind;

    /// Frames for a given state. Falls back to Idle frames if the state
    /// has no dedicated sprite (e.g. Fish has no Walk sprite).
    fn frames(&self, state: PetState) -> &[RgbaFrame];

    /// Called every tick with the elapsed time. Returns the new state if
    /// the pet itself wants to transition (e.g. walk completes).
    /// The caller (state machine) is responsible for external triggers
    /// like CPU alerts and user clicks.
    fn tick(&mut self, dt: Duration, state: PetState) -> Option<PetState>;

    /// Walk direction: +1.0 = right, -1.0 = left
    fn walk_direction(&self) -> f32 { 1.0 }
    fn set_walk_direction(&mut self, dir: f32);

    /// Current horizontal offset from anchor (used by fish sine drift)
    fn drift_offset(&self) -> f32 { 0.0 }
}

// ─── State machine ────────────────────────────────────────────────────────────

/// Drives state transitions and frame advancement for any Pet.
pub struct StateMachine {
    pub state:        PetState,
    pub frame_index:  usize,
    pub last_frame:   Instant,

    /// How long we have been in the current state
    state_elapsed:    Duration,

    /// Cumulative idle time (resets on any non-idle state)
    idle_elapsed:     Duration,

    /// Alert: how long CPU has been above threshold
    alert_elapsed:    Duration,
    /// Alert: how long CPU has been below threshold (for recovery)
    recovery_elapsed: Duration,

    /// Config mirrors
    pub sleep_after_secs:       u64,
    pub alert_sustain_secs:     u64,
    pub alert_recovery_secs:    u64,
    pub walk_chance_per_sec:    f32,
}

impl StateMachine {
    pub fn new(sleep_after_secs: u64, alert_sustain_secs: u64) -> Self {
        Self {
            state:                PetState::Idle,
            frame_index:          0,
            last_frame:           Instant::now(),
            state_elapsed:        Duration::ZERO,
            idle_elapsed:         Duration::ZERO,
            alert_elapsed:        Duration::ZERO,
            recovery_elapsed:     Duration::ZERO,
            sleep_after_secs,
            alert_sustain_secs,
            alert_recovery_secs:  5,
            walk_chance_per_sec:  0.05,
        }
    }

    /// Main tick — call every frame with elapsed time and current CPU %.
    /// Returns the state to render (may have changed).
    pub fn tick(
        &mut self,
        dt:       Duration,
        pet:      &mut dyn Pet,
        cpu_pct:  f32,
        alert_threshold: f32,
    ) -> PetState {
        self.state_elapsed += dt;

        // ── CPU alert tracking ────────────────────────────────────────────
        if cpu_pct >= alert_threshold {
            self.alert_elapsed   += dt;
            self.recovery_elapsed = Duration::ZERO;
        } else {
            self.recovery_elapsed += dt;
            self.alert_elapsed    = Duration::ZERO;
        }

        let should_alert =
            self.alert_elapsed.as_secs() >= self.alert_sustain_secs
            && self.state != PetState::Interact;

        let should_recover =
            self.state == PetState::Alert
            && self.recovery_elapsed.as_secs() >= self.alert_recovery_secs;

        // ── State transitions ─────────────────────────────────────────────
        let next = match self.state {
            PetState::Idle => {
                self.idle_elapsed += dt;

                if should_alert {
                    Some(PetState::Alert)
                } else if self.idle_elapsed.as_secs() >= self.sleep_after_secs {
                    Some(PetState::Sleep)
                } else {
                    // Random walk: check per-second probability
                    let walk_prob = self.walk_chance_per_sec * dt.as_secs_f32();
                    if rand_bool(walk_prob) {
                        // Randomise walk direction
                        let dir = if rand_bool(0.5) { 1.0_f32 } else { -1.0_f32 };
                        pet.set_walk_direction(dir);
                        Some(PetState::Walk)
                    } else {
                        None
                    }
                }
            }

            PetState::Walk => {
                if should_alert {
                    Some(PetState::Alert)
                } else {
                    // Let the pet decide when walk finishes (after N loops)
                    pet.tick(dt, PetState::Walk)
                }
            }

            PetState::Sleep => {
                if should_alert {
                    Some(PetState::Alert)
                } else {
                    None // stays asleep until clicked
                }
            }

            PetState::Interact => {
                // Play once then return to Idle
                pet.tick(dt, PetState::Interact)
            }

            PetState::Alert => {
                if should_recover {
                    Some(PetState::Idle)
                } else {
                    None
                }
            }
        };

        if let Some(new_state) = next {
            self.transition(new_state);
        }

        // ── Advance animation frame ───────────────────────────────────────
        let frames = pet.frames(self.state);
        if !frames.is_empty() {
            let delay = frames[self.frame_index % frames.len()].delay;
            if self.last_frame.elapsed() >= delay {
                self.frame_index = (self.frame_index + 1) % frames.len();
                self.last_frame  = Instant::now();
            }
        }

        self.state
    }

    /// Handle left-click from the user.
    pub fn on_click(&mut self) {
        match self.state {
            // Wake from sleep or cancel walk on click
            PetState::Sleep | PetState::Walk => self.transition(PetState::Interact),
            PetState::Idle                   => self.transition(PetState::Interact),
            // Double-click during interact: re-trigger
            PetState::Interact               => {
                self.frame_index = 0;
                self.state_elapsed = Duration::ZERO;
            }
            _ => {}
        }
    }

    /// Hard-set the pet to a specific state (e.g. from right-click menu).
    pub fn transition(&mut self, new_state: PetState) {
        if self.state != new_state {
            self.state         = new_state;
            self.frame_index   = 0;
            self.state_elapsed = Duration::ZERO;
            self.last_frame    = Instant::now();

            // Reset idle accumulator when leaving idle
            if new_state != PetState::Idle {
                self.idle_elapsed = Duration::ZERO;
            }
        }
    }
}

// ─── Minimal LCG-based float rand (no rand crate dependency) ─────────────────
//
// We only need a simple probability check — no need to pull in the rand crate.

use std::cell::Cell;
use std::time::SystemTime;

thread_local! {
    static RNG_STATE: Cell<u64> = Cell::new({
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(12345)
    });
}

/// Returns true with probability `p` (0.0 – 1.0).
fn rand_bool(p: f32) -> bool {
    if p <= 0.0 { return false; }
    if p >= 1.0 { return true;  }

    let x = RNG_STATE.with(|s| {
        // Xorshift64
        let mut v = s.get();
        v ^= v << 13;
        v ^= v >> 7;
        v ^= v << 17;
        s.set(v);
        v
    });

    // Map to [0, 1)
    let f = (x >> 11) as f32 / (1u64 << 53) as f32;
    f < p
}
