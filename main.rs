mod config;
mod font;
mod pet;
mod plugin;
mod renderer;
mod stats;

use anyhow::Result;
use clap::Parser;
use log::info;

use config::Config;
use pet::PetKind;

/// 🐾 LinuxPet — animated Linux desktop companion
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Pet type to start with: cat | dog | fish | <plugin-name>
    #[arg(long)]
    pub pet: Option<String>,

    /// Starting X position on screen
    #[arg(long)]
    pub x: Option<i32>,

    /// Starting Y position on screen
    #[arg(long)]
    pub y: Option<i32>,

    /// Disable the stats HUD at launch
    #[arg(long, default_value_t = false)]
    pub no_stats: bool,

    /// Scale factor for the pet sprite (0.5 – 3.0)
    #[arg(long)]
    pub scale: Option<f32>,
}

fn main() -> Result<()> {
    // Initialise logger — use RUST_LOG=debug linuxpet for verbose output
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .init();

    let args = Args::parse();
    info!("LinuxPet v{} starting", env!("CARGO_PKG_VERSION"));

    // Load config from ~/.config/linuxpet/config.toml (creates defaults if missing)
    let mut config = Config::load()?;

    // CLI flags override config values
    if let Some(pet_str) = &args.pet {
        config.pet.kind = PetKind::from_str(pet_str);
    }
    if let Some(x) = args.x {
        config.window.x = x;
    }
    if let Some(y) = args.y {
        config.window.y = y;
    }
    if args.no_stats {
        config.stats.enabled = false;
    }
    if let Some(scale) = args.scale {
        config.window.scale = scale.clamp(0.5, 3.0);
    }

    info!("Pet: {:?}  Position: ({}, {})  Stats: {}  Scale: {}",
        config.pet.kind,
        config.window.x, config.window.y,
        config.stats.enabled,
        config.window.scale,
    );

    // Load plugins from ~/.local/share/linuxpet/plugins/
    let plugins = plugin::load_all()?;
    info!("Loaded {} plugin(s)", plugins.len());

    // Hand off to the renderer — this blocks until the window is closed
    renderer::run(config, plugins)?;

    Ok(())
}
