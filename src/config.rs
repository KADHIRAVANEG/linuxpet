use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use log::{info, warn};

use crate::pet::PetKind;

// ─── Top-level config ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub window: WindowConfig,
    pub pet:    PetConfig,
    pub stats:  StatsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowConfig {
    /// Connector name of the monitor the pet lives on (e.g. "DP-1")
    pub monitor: Option<String>,
    /// X position — relative to monitor top-left
    pub x:       i32,
    /// Y position — relative to monitor top-left
    pub y:       i32,
    /// Sprite scale factor (0.5 – 3.0)
    pub scale:   f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PetConfig {
    /// Built-in type or plugin name
    pub kind:       PetKind,
    /// Seconds to hold the first frame before animating
    pub wait_secs:  u32,
    /// Walk speed in pixels per second
    pub walk_speed: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsConfig {
    /// Whether the stats HUD is shown
    pub enabled:       bool,
    /// CPU % that triggers the alert animation (sustained for alert_sustain_secs)
    pub alert_cpu_pct: f32,
    /// Seconds CPU must stay above threshold before triggering alert
    pub alert_sustain_secs: u32,
}

// ─── Defaults ────────────────────────────────────────────────────────────────

impl Default for Config {
    fn default() -> Self {
        Self {
            window: WindowConfig::default(),
            pet:    PetConfig::default(),
            stats:  StatsConfig::default(),
        }
    }
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            monitor: None,
            x:       100,
            y:       100,
            scale:   1.0,
        }
    }
}

impl Default for PetConfig {
    fn default() -> Self {
        Self {
            kind:       PetKind::Cat,
            wait_secs:  5,
            walk_speed: 60.0,
        }
    }
}

impl Default for StatsConfig {
    fn default() -> Self {
        Self {
            enabled:             false,
            alert_cpu_pct:       85.0,
            alert_sustain_secs:  3,
        }
    }
}

// ─── Load / Save ─────────────────────────────────────────────────────────────

impl Config {
    /// Path: ~/.config/linuxpet/config.toml
    pub fn path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("linuxpet")
            .join("config.toml")
    }

    /// Load from disk, creating defaults if the file doesn't exist.
    pub fn load() -> Result<Self> {
        let path = Self::path();

        if !path.exists() {
            info!("Config not found — writing defaults to {}", path.display());
            let default = Config::default();
            default.save()?;
            return Ok(default);
        }

        let raw = fs::read_to_string(&path)
            .with_context(|| format!("Reading config: {}", path.display()))?;

        let config: Config = toml::from_str(&raw)
            .unwrap_or_else(|e| {
                warn!("Config parse error ({}), using defaults: {}", path.display(), e);
                Config::default()
            });

        Ok(config)
    }

    /// Write atomically: write to .tmp then rename — safe on crash.
    pub fn save(&self) -> Result<()> {
        let path = Self::path();

        // Ensure directory exists
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)
                .with_context(|| format!("Creating config dir: {}", dir.display()))?;
        }

        let toml_str = toml::to_string_pretty(self)
            .context("Serialising config to TOML")?;

        // Atomic write: write to temp file then rename
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, toml_str)
            .with_context(|| format!("Writing temp config: {}", tmp.display()))?;
        fs::rename(&tmp, &path)
            .with_context(|| format!("Renaming config: {}", path.display()))?;

        Ok(())
    }

    /// Update window position and save.
    pub fn set_position(&mut self, x: i32, y: i32) -> Result<()> {
        self.window.x = x;
        self.window.y = y;
        self.save()
    }

    /// Update pet kind and save.
    pub fn set_pet_kind(&mut self, kind: PetKind) -> Result<()> {
        self.pet.kind = kind;
        self.save()
    }

    /// Update stats visibility and save.
    pub fn set_stats_enabled(&mut self, enabled: bool) -> Result<()> {
        self.stats.enabled = enabled;
        self.save()
    }
}
