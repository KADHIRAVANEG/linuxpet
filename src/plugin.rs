use anyhow::{Context, Result};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use log::{info, warn, debug};

use crate::pet::{PetKind, PetState, RgbaFrame};
use crate::pet::cat::decode_gif;

// ─── Manifest ────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PluginManifest {
    pub pet:       ManifestPet,
    pub sprites:   ManifestSprites,
    #[allow(dead_code)]
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
#[allow(dead_code)]
pub struct ManifestBehaviour {
    pub walk_chance: Option<f32>,
    pub sleep_after: Option<u64>,
    pub walk_speed:  Option<f32>,
}

// ─── Loaded plugin ────────────────────────────────────────────────────────────

#[allow(dead_code)]
pub struct LoadedPlugin {
    pub name:            String,
    pub kind:            PetKind,
    pub manifest:        PluginManifest,
    pub idle_frames:     Vec<RgbaFrame>,
    pub walk_frames:     Vec<RgbaFrame>,
    pub sleep_frames:    Vec<RgbaFrame>,
    pub interact_frames: Vec<RgbaFrame>,
    pub alert_frames:    Vec<RgbaFrame>,
}

impl LoadedPlugin {
    #[allow(dead_code)]
    pub fn frames(&self, state: PetState) -> &[RgbaFrame] {
        match state {
            PetState::Idle     => &self.idle_frames,
            PetState::Walk     => if self.walk_frames.is_empty()    { &self.idle_frames } else { &self.walk_frames },
            PetState::Sleep    => if self.sleep_frames.is_empty()   { &self.idle_frames } else { &self.sleep_frames },
            PetState::Interact => if self.interact_frames.is_empty(){ &self.idle_frames } else { &self.interact_frames },
            PetState::Alert    => if self.alert_frames.is_empty()   { &self.idle_frames } else { &self.alert_frames },
        }
    }
}

// ─── Paths ───────────────────────────────────────────────────────────────────

pub fn plugins_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("linuxpet")
        .join("plugins")
}

// ─── Load all ────────────────────────────────────────────────────────────────

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
        let path = entry?.path();
        if !path.is_dir() { continue; }
        match load_one(&path) {
            Ok(p)  => {
                info!("Loaded plugin: {} v{} by {}", p.name, p.manifest.pet.version, p.manifest.pet.author);
                plugins.push(p);
            }
            Err(e) => warn!("Skipping plugin at {}: {}", path.display(), e),
        }
    }
    Ok(plugins)
}

pub fn load_one(dir: &Path) -> Result<LoadedPlugin> {
    let manifest_path = dir.join("manifest.toml");
    if !manifest_path.exists() {
        anyhow::bail!("missing manifest.toml");
    }

    let raw: String = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("Reading {}", manifest_path.display()))?;
    let manifest: PluginManifest = toml::from_str(&raw)
        .with_context(|| format!("Parsing {}", manifest_path.display()))?;

    let idle_path = dir.join(&manifest.sprites.idle);
    if !idle_path.exists() {
        anyhow::bail!("missing idle sprite: {}", idle_path.display());
    }

    let load_opt = |f: &Option<String>| -> Vec<RgbaFrame> {
        f.as_ref()
            .map(|n| dir.join(n))
            .filter(|p| p.exists())
            .and_then(|p| std::fs::read(&p).ok())
            .map(|b| decode_gif(&b))
            .unwrap_or_default()
    };

    let name = manifest.pet.name.clone();
    let kind = PetKind::Plugin(name.to_lowercase().replace(' ', "_"));
    let idle_bytes = std::fs::read(&idle_path)?;

    Ok(LoadedPlugin {
        name,
        kind,
        idle_frames:     decode_gif(&idle_bytes),
        walk_frames:     load_opt(&manifest.sprites.walk),
        sleep_frames:    load_opt(&manifest.sprites.sleep),
        interact_frames: load_opt(&manifest.sprites.interact),
        alert_frames:    load_opt(&manifest.sprites.alert),
        manifest,
    })
}

// ─── Hot-reload watcher ───────────────────────────────────────────────────────

#[allow(dead_code)]
pub fn spawn_watcher() -> Result<(RecommendedWatcher, Receiver<PathBuf>)> {
    let base = plugins_dir();
    std::fs::create_dir_all(&base)?;

    let (tx, rx) = mpsc::channel::<PathBuf>();

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        if let Ok(event) = res {
            if matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
                for path in &event.paths {
                    if path.file_name().map(|n| n == "manifest.toml").unwrap_or(false) {
                        if let Some(plugin_dir) = path.parent() {
                            debug!("Hot-reload: {}", plugin_dir.display());
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
