use anyhow::{Context, Result};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;
use log::{info, warn, debug};

use crate::pet::{PetKind, PetState, RgbaFrame};
use crate::pet::cat::decode_gif;

// ─── Manifest ────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PluginManifest {
    pub pet:       ManifestPet,
    pub sprites:   ManifestSprites,
    pub behaviour: Option<ManifestBehaviour>,
}

#[derive(Debug, Deserialize)]
pub struct ManifestPet {
    pub name:    String,
    pub author:  String,
    pub version: String,
}

#[derive(Debug, Deserialize)]
pub struct ManifestSprites {
    pub idle:     String,
    pub walk:     Option<String>,
    pub sleep:    Option<String>,
    pub interact: Option<String>,
    pub alert:    Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ManifestBehaviour {
    pub walk_chance:  Option<f32>,
    pub sleep_after:  Option<u64>,
    pub walk_speed:   Option<f32>,
}

// ─── Loaded plugin ────────────────────────────────────────────────────────────

pub struct LoadedPlugin {
    pub name:       String,
    pub kind:       PetKind,
    pub manifest:   PluginManifest,

    pub idle_frames:     Vec<RgbaFrame>,
    pub walk_frames:     Vec<RgbaFrame>,
    pub sleep_frames:    Vec<RgbaFrame>,
    pub interact_frames: Vec<RgbaFrame>,
    pub alert_frames:    Vec<RgbaFrame>,
}

impl LoadedPlugin {
    pub fn frames(&self, state: PetState) -> &[RgbaFrame] {
        match state {
            PetState::Idle     => &self.idle_frames,
            PetState::Walk     => if self.walk_frames.is_empty() { &self.idle_frames } else { &self.walk_frames },
            PetState::Sleep    => if self.sleep_frames.is_empty() { &self.idle_frames } else { &self.sleep_frames },
            PetState::Interact => if self.interact_frames.is_empty() { &self.idle_frames } else { &self.interact_frames },
            PetState::Alert    => if self.alert_frames.is_empty() { &self.idle_frames } else { &self.alert_frames },
        }
    }
}

// ─── Plugin base directory ────────────────────────────────────────────────────

pub fn plugins_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("linuxpet")
        .join("plugins")
}

// ─── Load all plugins on startup ─────────────────────────────────────────────

pub fn load_all() -> Result<Vec<LoadedPlugin>> {
    let base = plugins_dir();

    if !base.exists() {
        std::fs::create_dir_all(&base)
            .with_context(|| format!("Creating plugins dir: {}", base.display()))?;
        return Ok(vec![]);
    }

    let mut plugins = Vec::new();

    for entry in std::fs::read_dir(&base)
        .with_context(|| format!("Reading plugins dir: {}", base.display()))?
    {
        let entry = entry?;
        let path  = entry.path();

        if !path.is_dir() { continue; }

        match load_one(&path) {
            Ok(plugin) => {
                info!("Loaded plugin: {} v{} by {}", plugin.name, plugin.manifest.pet.version, plugin.manifest.pet.author);
                plugins.push(plugin);
            }
            Err(e) => {
                warn!("Skipping plugin at {}: {}", path.display(), e);
            }
        }
    }

    Ok(plugins)
}

/// Load a single plugin from a directory.
pub fn load_one(dir: &Path) -> Result<LoadedPlugin> {
    let manifest_path = dir.join("manifest.toml");
    if !manifest_path.exists() {
        anyhow::bail!("missing manifest.toml");
    }

    let raw = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("Reading {}", manifest_path.display()))?;

    let manifest: PluginManifest = toml::from_str(&raw)
        .with_context(|| format!("Parsing {}", manifest_path.display()))?;

    // Load required idle sprite
    let idle_path = dir.join(&manifest.sprites.idle);
    if !idle_path.exists() {
        anyhow::bail!("missing idle sprite: {}", idle_path.display());
    }

    let load_optional = |filename: &Option<String>| -> Vec<RgbaFrame> {
        filename.as_ref()
            .map(|f| dir.join(f))
            .filter(|p| p.exists())
            .map(|p| {
                std::fs::read(&p)
                    .map(|bytes| decode_gif(&bytes))
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    };

    let name = manifest.pet.name.clone();
    let kind = PetKind::Plugin(name.to_lowercase().replace(' ', "_"));

    let idle_bytes = std::fs::read(&idle_path)?;

    Ok(LoadedPlugin {
        name,
        kind,
        idle_frames:     decode_gif(&idle_bytes),
        walk_frames:     load_optional(&manifest.sprites.walk),
        sleep_frames:    load_optional(&manifest.sprites.sleep),
        interact_frames: load_optional(&manifest.sprites.interact),
        alert_frames:    load_optional(&manifest.sprites.alert),
        manifest,
    })
}

// ─── Hot-reload watcher ───────────────────────────────────────────────────────

/// Spawned after startup. Sends the path of a newly-added or modified plugin
/// directory whenever the user drops in a new sprite pack.
pub fn spawn_watcher() -> Result<(RecommendedWatcher, Receiver<PathBuf>)> {
    let base = plugins_dir();
    std::fs::create_dir_all(&base)?;

    let (tx, rx) = mpsc::channel::<PathBuf>();

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        if let Ok(event) = res {
            if matches!(event.kind,
                EventKind::Create(_) | EventKind::Modify(_))
            {
                for path in &event.paths {
                    // We only care about manifest files being created/modified
                    if path.file_name().map(|n| n == "manifest.toml").unwrap_or(false) {
                        if let Some(plugin_dir) = path.parent() {
                            debug!("Hot-reload triggered: {}", plugin_dir.display());
                            let _ = tx.send(plugin_dir.to_path_buf());
                        }
                    }
                }
            }
        }
    })?;

    watcher.watch(&base, RecursiveMode::Recursive)?;
    info!("Plugin hot-reload watching: {}", base.display());

    Ok((watcher, rx))
}
