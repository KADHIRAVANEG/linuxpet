#!/usr/bin/env fish
# linuxpet_setup.fish
# Run this from wherever you want your project folder to live.
# Prerequisites: gh auth login, cargo, git

# ─── 1. LOCAL REPO ────────────────────────────────────────────────────────────

cd ~/linuxpet
git init
cargo init --name linuxpet

# Basic .gitignore
echo "/target" >> .gitignore

# Stub README
echo "# linuxpet
A multi-pet Linux desktop companion written in Rust.
Supports cats, dogs, fish and custom plugin sprite packs.
Includes a live system stats HUD (CPU, RAM, network, disk).
" > README.md

git add .
git commit -m "chore: initial scaffold"

# ─── 2. GITHUB REPO ───────────────────────────────────────────────────────────
gh repo create linuxpet \
  --public \
  --description "🐾 Multi-pet Linux desktop companion in Rust — animated overlay with system stats HUD and plugin support" \
  --source=. \
  --remote=origin \
  --push

# ─── 3. LABELS ────────────────────────────────────────────────────────────────
# Remove default labels first (optional — comment out if you want to keep them)
for label in bug documentation duplicate enhancement "good first issue" "help wanted" invalid question wontfix
  gh label delete $label --yes 2>/dev/null
end

gh label create "core"     --color "0075ca" --description "Core engine / state machine"
gh label create "renderer" --color "5319e7" --description "Windowing, graphics, animation"
gh label create "pet"      --color "e4e669" --description "Pet types and sprite logic"
gh label create "plugin"   --color "d93f0b" --description "Plugin / sprite-pack system"
gh label create "stats"    --color "0e8a16" --description "System stats HUD"
gh label create "config"   --color "006b75" --description "Config file and persistence"
gh label create "ci"       --color "bfd4f2" --description "CI/CD and release binaries"
gh label create "docs"     --color "cfd3d7" --description "Documentation and README"

# ─── 4. MILESTONES ────────────────────────────────────────────────────────────
set OWNER (gh api user --jq .login)
set REPO "linuxpet"

gh api repos/$OWNER/$REPO/milestones \
  -f title="v0.1 – Overlay MVP" \
  -f description="Frameless transparent window with animated cat, drag to move, config save." \
  -f due_on="2026-08-15T00:00:00Z"

gh api repos/$OWNER/$REPO/milestones \
  -f title="v0.2 – Multi-pet + Stats HUD" \
  -f description="Pet switcher (cat/dog/fish), right-click menu, live CPU/RAM/network widget." \
  -f due_on="2026-09-15T00:00:00Z"

gh api repos/$OWNER/$REPO/milestones \
  -f title="v0.3 – Plugin System" \
  -f description="TOML manifest + sprite ZIP loader, community pet packs, Lua behaviour hooks." \
  -f due_on="2026-10-15T00:00:00Z"

# ─── 5. ISSUES ────────────────────────────────────────────────────────────────
# v0.1 issues

gh issue create \
  --title "Set up Cargo workspace and crate structure" \
  --body "Create the initial Cargo.toml with workspace layout:
- src/main.rs — entry point
- src/renderer.rs — winit event loop
- src/pet/mod.rs — PetState trait
- src/config.rs — serde config struct
- src/stats.rs — sysinfo polling stub

**Crates to add:**
\`\`\`toml
winit = \"0.30\"
softbuffer = \"0.4\"
tiny-skia = \"0.11\"
image = { version = \"0.25\", features = [\"gif\"] }
serde = { version = \"1\", features = [\"derive\"] }
toml = \"0.8\"
sysinfo = \"0.30\"
\`\`\`" \
  --label "core" \
  --milestone "v0.1 – Overlay MVP"

gh issue create \
  --title "Create frameless transparent overlay window (X11 + Wayland)" \
  --body "Using winit + softbuffer, create a frameless always-on-top window with per-pixel transparency.

- Window decorations: none
- Always on top: true
- Transparent background: true
- Target: X11 first, then test Wayland via XWayland

Reference: winit WindowAttributes::with_decorations(false), with_transparent(true)" \
  --label "renderer" \
  --milestone "v0.1 – Overlay MVP"

gh issue create \
  --title "Implement GIF frame decoder and animation loop" \
  --body "Decode a GIF file into individual RGBA frames using the \`image\` crate.

- Read frame delay from GIF metadata
- Loop frames using std::time::Instant
- Blit current frame to softbuffer surface with tiny-skia
- Handle first-frame static hold (configurable seconds)" \
  --label "renderer" \
  --milestone "v0.1 – Overlay MVP"

gh issue create \
  --title "Add drag-to-move with mouse input" \
  --body "Handle left mouse button drag in the winit event loop:

- WindowEvent::MouseInput (pressed) — record offset
- WindowEvent::CursorMoved — move window by delta
- Store last position in config after drag ends" \
  --label "renderer" \
  --milestone "v0.1 – Overlay MVP"

gh issue create \
  --title "Config file: save and restore pet position and pet type" \
  --body "Create ~/.config/linuxpet/config.toml with serde:

\`\`\`toml
[window]
x = 1800
y = 900

[pet]
type = \"cat\"
wait_secs = 5
\`\`\`

- Read on startup, write on exit / after drag
- Create file with defaults if missing" \
  --label "config" \
  --milestone "v0.1 – Overlay MVP"

gh issue create \
  --title "Bundle built-in cat sprite assets" \
  --body "Add default cat sprites to assets/cat/:
- idle.gif (static / slow blink loop)
- walk.gif
- sleep.gif

Source: create simple placeholder GIFs or port from myCat's open assets.
Assets must be embedded at compile time using \`include_bytes!\` so the binary is self-contained." \
  --label "pet" \
  --milestone "v0.1 – Overlay MVP"

# v0.2 issues

gh issue create \
  --title "PetState trait and state machine (idle → walk → sleep → interact)" \
  --body "Define a trait each pet type implements:

\`\`\`rust
pub trait Pet {
    fn name(&self) -> &str;
    fn current_frame(&self) -> &[u8]; // RGBA
    fn tick(&mut self, dt: Duration);
    fn on_click(&mut self);
}
\`\`\`

State machine:
- idle (default, loops idle.gif)
- walk (random horizontal drift, loops walk.gif)
- sleep (after N minutes idle)
- interact (on left click, plays react.gif once)" \
  --label "core,pet" \
  --milestone "v0.2 – Multi-pet + Stats HUD"

gh issue create \
  --title "Add Dog and Fish pet types with placeholder sprites" \
  --body "Implement Dog and Fish structs that satisfy the Pet trait.
- src/pet/dog.rs
- src/pet/fish.rs
- assets/dog/ and assets/fish/ with idle GIFs

Fish behaviour: gentle bob animation, no walking state — stays in place." \
  --label "pet" \
  --milestone "v0.2 – Multi-pet + Stats HUD"

gh issue create \
  --title "Right-click context menu (pet switcher + quit)" \
  --body "On right-click, show an overlay menu with:
- Switch pet → Cat / Dog / Fish / (installed plugins)
- Toggle stats HUD
- Quit

Options:
1. Use a second borderless winit window as menu
2. Use tray-icon crate
3. Simple custom painted rectangle overlay

Start with option 1 (pure winit, no extra deps)." \
  --label "core,renderer" \
  --milestone "v0.2 – Multi-pet + Stats HUD"

gh issue create \
  --title "System stats HUD — CPU, RAM, network, disk" \
  --body "Spawn a background thread using sysinfo that polls every 1 second and sends data via std::sync::mpsc to the render loop.

Display near the pet as a small semi-transparent HUD panel:
- CPU % bar
- RAM used / total
- Net rx/tx rate
- Disk read/write rate

Render with tiny-skia (filled rects for bars, text via a pixel font or embedded bitmap font).

Toggle via right-click menu and config flag." \
  --label "stats" \
  --milestone "v0.2 – Multi-pet + Stats HUD"

gh issue create \
  --title "Persist pet type selection to config.toml" \
  --body "When user switches pet via right-click menu, write the new type to config.toml immediately so it persists across restarts.

Also add a 'stats_enabled' boolean to config." \
  --label "config" \
  --milestone "v0.2 – Multi-pet + Stats HUD"

# v0.3 issues

gh issue create \
  --title "Plugin manifest spec (TOML + sprite ZIP format)" \
  --body "Define the plugin format:

\`\`\`toml
# plugins/foxgirl/manifest.toml
[pet]
name = \"Fox Girl\"
author = \"community\"
version = \"1.0.0\"

[sprites]
idle  = \"idle.gif\"
walk  = \"walk.gif\"
sleep = \"sleep.gif\"

[behaviour]
walk_chance = 0.3   # probability per tick of entering walk state
sleep_after = 300   # seconds idle before sleep
\`\`\`

Sprite ZIP contains the GIF files and manifest.toml.
Plugins live in ~/.local/share/linuxpet/plugins/<name>/" \
  --label "plugin" \
  --milestone "v0.3 – Plugin System"

gh issue create \
  --title "Plugin loader — discover, validate and load sprite packs at runtime" \
  --body "Scan ~/.local/share/linuxpet/plugins/ on startup.
For each directory:
1. Parse manifest.toml
2. Validate required sprite files exist
3. Decode GIF frames into memory
4. Register as a selectable pet in the right-click menu

Show a warning (stderr log) and skip if a plugin is malformed." \
  --label "plugin" \
  --milestone "v0.3 – Plugin System"

gh issue create \
  --title "Example community plugin: Fox sprite pack" \
  --body "Create a reference plugin under plugins/foxgirl/ in the repo:
- manifest.toml following the spec
- placeholder GIF sprites (can be simple pixel art)
- README inside the folder explaining how to install

This doubles as documentation-by-example for plugin authors." \
  --label "plugin,docs" \
  --milestone "v0.3 – Plugin System"

gh issue create \
  --title "GitHub Actions: build and release binaries (x86_64 + aarch64)" \
  --body "Create .github/workflows/release.yml:

Triggers: push to tag v*

Matrix:
- ubuntu-latest → x86_64-unknown-linux-gnu
- ubuntu-latest (cross) → aarch64-unknown-linux-gnu

Steps:
1. cargo build --release
2. Strip binary
3. gh release upload artifact

Also add a CI workflow that runs on every PR: cargo check + cargo test + cargo clippy" \
  --label "ci" \
  --milestone "v0.3 – Plugin System"

gh issue create \
  --title "Write README: install, usage, plugin authoring guide" \
  --body "README sections:
- Demo GIF (record with peek or byzanz)
- Install: cargo install linuxpet, AUR package (future), prebuilt binary
- Usage: command-line flags, right-click menu
- Plugin authoring: manifest spec, folder structure, example
- Building from source
- Contributing

Keep it concise. Link to the wiki for deeper docs." \
  --label "docs" \
  --milestone "v0.3 – Plugin System"

# ─── 6. CREATE KANBAN PROJECT AND LINK IT ─────────────────────────────────────
set PROJECT_NUM (gh project create --owner "@me" --title "LinuxPet Kanban" --format json | python3 -c "import sys,json; print(json.load(sys.stdin)['number'])")

echo ""
echo "✅ Done! Project board number: $PROJECT_NUM"
echo "   View at: https://github.com/users/$OWNER/projects/$PROJECT_NUM"
echo ""
echo "Next: add your issues to the project board:"
echo "  gh project item-add $PROJECT_NUM --owner \"@me\" --url <issue-url>"
echo ""
echo "Or open the board and use 'Add item' to bulk-add all issues from $OWNER/linuxpet"
