# open2jam-rs — Project Overview

> O2Jam rhythm game port in Rust with wgpu + egui + oddio + cpal

## Quick Facts

| | |
|---|---|
| **Language** | Rust 2021 Edition |
| **Version** | 0.1.0 |
| **Render** | wgpu 29 (GPU-accelerated, textured sprites, dual blend modes) |
| **GUI** | egui 0.34 + egui-wgpu + egui-winit (no eframe) |
| **Window/Input** | winit 0.30 |
| **Audio** | oddio 0.7.4 + cpal 0.17.3 (low-latency, sample-accurate BGM scheduling) |
| **Audio Formats** | Ogg Vorbis (lewton), WAV (hound) |
| **Parsing** | quick-xml (skin XML), custom binary (OJN/OJM) |
| **Database** | sqlx 0.8 + SQLite (song metadata cache) |
| **Fonts** | Inter (Latin) + Noto Sans CJK SC (CJK), auto-downloaded at build time |
| **Upstream** | `git@github.com:ar-nim/open2jam-rs.git` → `gui-development` |

## What Is This?

A Rust port of [open2jam-modern](../open2jam-modern) — a community reimplementation of the **O2Jam** rhythm game (2002 Korean arcade-style music game).

**Single unified binary** — no process spawning. The menu and game share the same winit window, wgpu device, and event loop:
- **No CLI args** → opens the menu GUI (browse songs, configure options, launch game)
- **`<path.ojn>` arg** → launches game directly with the specified chart
- **`--autoplay`** → enables auto-play mode

## File Structure

```
open2jam-rs/
├── Cargo.toml                         # [workspace] — crates/core, crates/game, crates/ojn
├── tmpdocs/                           # Session notes and planning docs (gitignored)
│   ├── SESSION-1304.md                # Development session log
│   ├── binary-unification.md          # Binary unification plan and status
│   └── gui-milestones.md              # GUI feature milestones
├── crates/
│   ├── core/                          # open2jam-rs-core (shared library)
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── config.rs              # Config JSON (mirrors Java config.json)
│   │       ├── game_options.rs        # VSyncMode, FpsLimiter, Difficulty, SpeedType, etc.
│   │       └── key_bindings.rs        # Key map for K4-K8 + misc
│   │
│   ├── ojn/                           # open2jam-rs-ojn (OJN chart parser)
│   │   └── src/lib.rs                 # OjnHeader, BMP thumbnail extraction
│   │
│   └── game/                          # open2jam-rs (SINGLE unified binary)
│       ├── Cargo.toml
│       ├── build.rs                   # Auto-downloads Inter + Noto Sans CJK fonts at build
│       ├── assets/                    # Inter-Regular.ttf, NotoSansCJKsc-Regular.otf
│       └── src/
│           ├── main.rs                # Entry point: parse args, create App, run
│           ├── lib.rs                 # Module declarations
│           ├── engine.rs              # App struct + winit event loop + wgpu + egui (~2092 lines)
│           ├── game_state.rs          # GameState: clock, chart, notes, scoring (~1610 lines)
│           ├── test_harness.rs
│           ├── cover_shader.wgsl
│           ├── menu/                  # Menu GUI module (formerly separate crate)
│           │   ├── mod.rs
│           │   ├── menu_app.rs        # egui menu (no eframe): song list, config panels
│           │   ├── db/                # SQLite song cache
│           │   │   ├── mod.rs
│           │   │   └── models.rs      # LibraryEntry, CachedChart, ChartScanEntry
│           │   └── panels/            # Configuration UI
│           │       ├── mod.rs
│           │       ├── display_config.rs
│           │       ├── key_bind_editor.rs
│           │       └── modifiers.rs
│           ├── audio/                 # bgm_signal, cache, manager, trigger
│           ├── gameplay/              # judgment, scroll, timing_data
│           ├── parsing/               # chart, ojm, xml
│           ├── render/                # atlas, hud, pipeline, textured_renderer
│           ├── resources/             # clock, key_bindings, skin_assets
│           └── skin/                  # mod.rs, prefab
```

## Architecture

### Unified Single Binary

```
open2jam-rs (single binary, single window, single wgpu device)
├── winit event loop (main thread, macOS compliant)
├── wgpu Device + Queue + Surface (shared)
├── egui-winit (input routing)
├── egui-wgpu (menu rendering → same surface)
└── TexturedRenderer (game rendering → same surface)

Mode AppMenu:
  - egui renders full-screen UI (song list, config panels)
  - Input routed to egui-winit first
  - Consumed events don't reach game

Mode AppPlaying:
  - Game state renders notes/skin/HUD
  - Input routed directly to game key handler
  - ESC exits back to menu

State Transitions:
  Menu ──(PLAY pressed)──▶ Playing
  Playing ──(song ends/ESC)──▶ Menu
```

### Game Engine

```
main() → App::new() → App::run()
                  │
                  ├─ EventLoop (winit, ControlFlow::Poll)
                  │     ├─ resumed()         → init_wgpu + egui-winit setup
                  │     ├─ about_to_wait()   → no-op (redraw requested after render)
                  │     └─ window_event()    → egui input (menu) or key input (game), resize, close
                  │
                  ├─ wgpu Renderer
                  │     ├─ Surface + Device + Queue
                  │     ├─ egui_wgpu::Renderer (menu mode)
                  │     ├─ SkinAtlas — packed texture from skin XML sprites
                  │     ├─ TexturedRenderer — batch quad drawing
                  │     │     ├─ pipeline_alpha — standard alpha blending
                  │     │     └─ pipeline_additive — additive blending (long flare glow)
                  │     └─ HudLayout — HUD position data from skin
                  │
                  ├─ Audio (oddio + cpal)
                  │     ├─ AudioManager → mixer handle + BGM queue producer
                  │     ├─ BgmSignalQueue → single oddio::Signal in mixer
                  │     ├─ SoundCache → decoded OGG/WAV buffers
                  │     └─ ScheduledSignal → per-note wrapper with delay_samples
                  │
                  └─ GameState
                        ├─ Clock → game_time, render_time, BPM, interpolation
                        ├─ Chart → parsed OJN events (notes, BPM changes, measures)
                        ├─ TimingData → velocity tree (BPM-aware beat calculation)
                        ├─ active_notes → tap notes on screen
                        ├─ active_long_notes → long notes (head/body/tail, holding state)
                        ├─ pending_judgments → visual judgment popups
                        ├─ combo_counter → combo wobble animation state
                        ├─ stats → life, score, combo, pill_count, jam_counter, judgment counts
                        ├─ note_click_effects → EFFECT_CLICK sprites (COOL/GOOD)
                        ├─ long_flare_effects → EFFECT_LONGFLARE sprites (additive glow)
                        ├─ handle_key_press() → immediate judgment + keysound
                        └─ handle_key_release() → kill flare, stop holding
```

### Menu GUI (egui, no eframe)

```
App::render_frame() (Menu mode):
  1. egui_winit.take_egui_input() — build input from winit events
  2. egui_ctx.run() — run menu UI layout
  3. egui_ctx.tessellate() — generate draw commands
  4. surface.get_current_texture() — acquire GPU surface
  5. egui_renderer.update_buffers() — upload vertices/textures
  6. egui_renderer.render() — draw to wgpu render pass
  7. queue.submit() + present() — present frame
```

## Core Game Loop

1. **Delta Time** — measured per frame (rounded to prevent cumulative drift)
2. **Game State Update** — advance clock by delta_ms (startup delay phase → gameplay)
3. **Note Spawning** — spawn notes within lead-time window from chart events (2x travel time + 500ms)
4. **Auto-Judge / Input** — auto-play mode judges all notes as COOL; manual mode processes keyboard input via `handle_key_press()`
5. **Judgment Processing** — COOL/GOOD triggers effects (click/flare), records stats, updates combo, pill system
6. **Long Note Tail Judgment** — auto-release when tail passes judgment line (runs in BOTH modes)
7. **BGM Lookahead Scheduling** — scan chart 500ms ahead, push BgmCommand to rtrb queue with sample-accurate delay
8. **Audio Callback** — cpal drains rtrb, creates ScheduledSignal, BgmSignalQueue mixes all active notes
9. **Effect Cleanup** — remove expired click/flare effects (duration-based or killed lifecycle)
10. **Render** — draw skin background → lane effects → notes → static_keyboard → pressed overlays → long notes → effects → HUD (layered order)

## The Scroll Formula

### Static BPM (used when no BPM changes exist)
Notes scroll based on **BPM**, not fixed speed:

```
distance_px = speed × beats_remaining × (0.8 × judgment_line_y) / 4
beats_remaining = (target_time_ms - render_time_ms) / (60000 / BPM)
```

### BPM-Aware (used when chart has BPM changes — matches Java HiSpeed)
Uses a velocity tree (`TimingData`) that stores BPM change points with cumulative beat counts:

```
beats = timing.getBeat(target_time) - timing.getBeat(render_time)
distance_px = speed × beats × (0.8 × judgment_line_y) / 4
```

`getBeat()` uses binary search to find the correct BPM segment and correctly accumulates beats across all intermediate BPM changes. This ensures scroll speed changes smoothly when BPM shifts mid-chart.

**measure_size = 0.8 × judgment_line_y = 0.8 × 480 = 384** — matches Java HiSpeed's hardcoded `measureSize = 385` (the +1 is the original game's 1px overlap tweak).

**Higher BPM = faster scroll.** A note at 200 BPM moves twice as fast as one at 100 BPM.

The travel time determines spawn lead:
```
travel_time_ms = (4 × judgment_line_y / (speed × measure_size)) × 60000 / BPM
spawn_lead = 2 × travel_time + 500ms   // 2x ensures notes appear at very top even at low BPM/1x speed
```

## File Formats

| Format | Extension | Purpose | Parser |
|--------|-----------|---------|--------|
| **OJN** | `.ojn` | Chart — note events, BPM changes, time signatures, measures, sample IDs | `parsing/ojn.rs` |
| **OJM** | `.ojm` | Audio — individual samples (WAV IDs 0-999, OGG IDs 1000+) | `parsing/ojm.rs` |
| **Skin XML** | `resources.xml` | Sprite definitions, entity layouts, judgment line Y, effect sprites | `parsing/xml.rs` |

**Chart-to-Audio Linkage:** OJN contains `sample_id` values that map to samples within the OJM file. BGM is composed from individual samples triggered at precise game times via the BGM lookahead scheduler (rtrb queue → ScheduledSignal → BgmSignalQueue).

## BGM Scheduling Architecture

```
Main Thread (per frame):                     Audio Thread (cpal callback):
─────────────────────────                    ─────────────────────────────
1. Scan chart events 500ms ahead             1. BgmSignalQueue.sample() fires
   for playable notes                        2. Drain rtrb consumer → create
2. For each note found:                         ScheduledSignal with delay_samples
   a. Calculate delay_samples =              3. For each active signal:
      ms_to_samples(note_time - now)            a. Sample into temp buffer
   b. Push BgmCommand {                         b. ACCUMULATE into output buffer
      frames, delay_samples,                     (add samples, don't overwrite)
      volume, pan, source_id }                   c. source_id match → STEAL VOICE
      to rtrb producer                           (replace old signal)
3. Continue rendering                        4. Remove finished signals
                                             5. Mixer writes accumulated output
```

**Key insight:** BgmSignalQueue is a SINGLE `oddio::Signal` that manages multiple notes internally. The mixer sees it as one entity. Inside `sample()`, it mixes all active notes by **accumulation**, preventing the overwrite bug where simultaneous BGM notes would erase each other.

**source_id deduplication:** When a BgmCommand arrives with a source_id matching an already-active signal, the old signal is removed (voice-steal) and the new one takes its place. This prevents phase cancellation from overlapping identical samples in dense streams.

## Key Input

Default lane bindings: **S D F Space J K L** for lanes 1–7. Configurable via the menu's key binding editor (press-to-capture).

- **Key press** → `handle_key_press()` → finds next unjudged note in lane → judges if within ±bad_window → plays keysound → shows effect
- **Key release** → `handle_key_release()` → kills long note flare, stops holding
- **OS key repeat** is suppressed: if `pressed_lanes[lane]` is already true, the press is ignored (holding ≠ pressing)
- Keysound only fires when the press is within the judgment window (matching O2Jam: pressing too early/late produces no sound)

## Skin System

- Skin is loaded from `resources.xml` in the Java source directory
- Sprites are packed into a **texture atlas** at startup (GPU texture, UV mapping)
- Notes use per-lane **prefabs** with customizable head/body/tail sprites
- The base skin is 800×600, scaled to fit the window with letterboxing
- **Effect sprites** (EFFECT_CLICK, EFFECT_LONGFLARE) extracted from skin XML with frame count and speed
- **PRESSED_NOTE entities** split into two groups:
  - **Lane effects** (y < judgment_line_y, e.g., y=215): drawn BEFORE notes, behind them
  - **Keyboard overlays** (y >= judgment_line_y, e.g., y=487): drawn AFTER static_keyboard, on top
- **Animation frame speed** parsed as FPS from XML (e.g., `framespeed="60"` → 60fps → 16.67ms/frame)

## Entity State Machine (Notes)

```
Tap Note:
  NOT_JUDGED → (player hits COOL/GOOD/BAD) → JUDGED → cleanup
  NOT_JUDGED → (missed/passed)             → MISSED → cleanup

Long Note:
  NOT_JUDGED → (head hit)     → JUDGED + HOLDING + flare triggered
  HOLDING    → (key release)  → RELEASED → tail judgment → flare killed → cleanup
  HOLDING    → (tail reached) → auto-release judgment → flare killed → cleanup
  NOT_JUDGED → (head missed)  → MISSED → cleanup
```

## Judgment System

### Tap Note Judgment (192 TPB — windows scale with BPM)
- **COOL**: ±6/192 measures ≈ ±50ms @ 150 BPM, +2 life, 200 + combo×10 score, triggers EFFECT_CLICK
- **GOOD**: ±18/192 measures ≈ ±150ms @ 150 BPM, +1 life, 100 + combo×5 score, triggers EFFECT_CLICK
- **BAD**: ±25/192 measures ≈ ±208ms @ 150 BPM, -5 life (Hard), 4 score, breaks combo
- **MISS**: outside all windows, -30 life (Hard), -10 score, breaks combo

### Long Note Release Judgment
- Evaluated against tail time when player releases key or tail passes judgment line
- ±24/192 measures for BAD window (slightly stricter than tap notes)
- Same scoring as tap notes

### Pill (Buffer) System
- Every **15 consecutive Cools** awards a pill/buffer (max 5 stored)
- When a **Bad** is judged and pills > 0: converts to Cool and consumes one pill
- Good/Miss resets the consecutive Cools counter

### Combo System
- COOL/GOOD increases combo counter
- BAD/MISS resets combo to 0
- Combo counter has wobble animation (pop-in + slide)
- Jam counter (combo milestone) shows briefly on certain thresholds
- Jam combo: every 100 jam_counter = 1 jam combo (multiplier for scoring)

## Visual Effects System

### EFFECT_CLICK (Tap Note Hit)
- **Triggered on**: COOL or GOOD judgment for tap notes
- **Sprite**: from skin XML (e.g., `effect_click_1`), typically 11 frames
- **Position**: centered on note X, centered on judgment line Y
- **Animation**: loops continuously through frames until duration expires
- **Duration**: calculated from sprite data: `frame_count × frame_speed_ms`
- **Blend mode**: standard alpha blending
- **Cleanup**: removed when `is_active()` returns false (elapsed > duration)

### EFFECT_LONGFLARE (Long Note Hold)
- **Triggered on**: COOL or GOOD judgment for long note head
- **Sprite**: from skin XML (e.g., `longflare`), typically 15 frames
- **Position**: centered on note X, top-aligned at skin XML entity Y (e.g., y="460")
- **Animation**: loops continuously through frames until duration expires
- **Duration**: sprite-based (frame_count × frame_speed_ms)
- **Blend mode**: **additive blending** (GL_SRC_ALPHA, GL_DST_ALPHA) for vibrant glow effect
- **Cleanup**: killed on key release, tail pass, or miss — removed from active list

### PendingJudgment (Judgment Popup)
- **Triggered on**: any judgment (COOL/GOOD/BAD/MISS)
- **Position**: per-lane, from skin XML HUD layout
- **Animation**: pop-in (50%→100% scale over 100ms), stays full size for 750ms
- **Behavior**: instant-replace — new judgment kills previous one immediately
- **Pill conversion**: when a pill converts Bad → Cool, the popup shows "COOL" (the effective judgment)

## Hybrid Phase-Locked Clock

The audio hardware is the sovereign authority for timing:

```
Audio Thread (cpal callback):
  samples_played += frames_per_callback    // atomic counter
  last_callback_instant = now.elapsed()    // atomic timestamp

Main Thread (rendering):
  T_audio   = (samples_played / sample_rate) × 1000
  T_wall    = (base_instant.elapsed() - callback_timestamp) in ms
  T_visual  = T_audio + T_wall             // smooth, continuous, monotonic
```

- `samples_played` never resets (except on `stream.play()`) — only ever increases
- `T_visual` provides continuous interpolation between discrete audio callback steps
- At 144Hz+ rendering, this eliminates micro-stutter while staying phase-locked to audio
- cpal buffer: 128 samples (~2.9ms latency at 44.1kHz)

## Current Status — What Works

### Infrastructure
- [x] Single unified binary (menu + game, same process, same window)
- [x] wgpu 29 + egui 0.34 integration (egui-wgpu renders to same surface)
- [x] winit 0.30 event loop (ControlFlow::Poll for immediate redraw delivery)
- [x] macOS compliant (all rendering on main thread, single window)
- [x] Config-driven settings: speed, difficulty, keybindings, resolution, fullscreen, VSync, FPS limiter
- [x] Auto-download fonts at build time (Inter + Noto Sans CJK SC)
- [x] Explicit 9-step resource cleanup prevents double-panic on exit
- [x] Hybrid spin-sleep frame limiter (matches Java open2jam-modern approach)
- [x] 24 passing tests (14 DB + 10 key capture)

### Menu (egui, no eframe)
- [x] Song library management (add/remove/rescan folders via `rfd::FileDialog`)
- [x] SQLite-backed song metadata cache (`~/.config/open2jam-rs/songcache.db`)
- [x] Virtualized `TableBuilder` song list with sticky headers, resizable columns, 60+ fps
- [x] Sortable columns (Name, Artist, Level, BPM, Length, Genre) with cached sort
- [x] Column visibility persistence saved to config.json
- [x] Genre filter dropdown (11 OJN genre codes)
- [x] BMP cover thumbnail extraction and lazy loading
- [x] Per-difficulty metadata (levels, durations, note counts)
- [x] Difficulty selection (sticky across song selection and sessions, persisted to config)
- [x] Auto-select last-opened library on restart
- [x] Press-to-capture key binding editor (7-key layout)
- [x] Display config: resolution dropdown with native detection, custom resolution
- [x] Menu/Game fullscreen toggles (independent)
- [x] VSync mode selector: On (AutoVsync), Fast (Mailbox), Off (AutoNoVsync)
- [x] FPS limiter: X1/X2/X4/X8 (dynamic based on monitor refresh rate)
- [x] Power-of-two buffer size slider

### Game Engine
- [x] Window creation & wgpu rendering
- [x] OJN chart parsing (notes, BPM changes, measures, sample IDs, BMP thumbnails)
- [x] OJM audio decoding (WAV + OGG samples)
- [x] Texture atlas building from skin XML sprites
- [x] Beat-based note scrolling (BPM-dependent, velocity tree for BPM changes)
- [x] Note spawning & cleanup (lead-time calculation, bad_window + 100ms safety margin)
- [x] Auto-play mode (auto-judge all notes as COOL)
- [x] Long note rendering (head/body/tail with stretchable body, head-clamped at judgment line)
- [x] Manual input mode (keyboard → immediate judgment → keysound + effects)
- [x] Tap note judgment (COOL/GOOD/BAD/MISS with 192 TPB timing windows)
- [x] Long note head + tail judgment (hold + release evaluation)
- [x] Pill/buffer system (15 consecutive Cools → pill, converts Bad → Cool)
- [x] OS key repeat suppression (holding ≠ pressing)
- [x] EFFECT_CLICK rendering (positioned on judgment line, correct speed, alpha blending)
- [x] EFFECT_LONGFLARE rendering (positioned at skin Y, additive blending, killed on release/miss)
- [x] PRESSED_NOTE rendering (lane effects behind notes, keyboard overlays on top)
- [x] Animation looping (modulo-based, matches Java AnimatedEntity)
- [x] Effect lifecycle (duration-based or killed)
- [x] Combo counter with wobble animation
- [x] Jam counter (combo milestone popup)
- [x] Score calculation (200 + combo×10 for COOL, 100 + combo×5 for GOOD)
- [x] Life / health system (HP gain/loss per judgment, difficulty-based curves)
- [x] HUD rendering (score, combo, lifebar, timer, judgment popups, timebar)
- [x] Audio trigger system (time-driven sample playback)
- [x] Startup delay animation (2000ms lifebar fill)
- [x] Dual blend mode pipelines (alpha + additive)
- [x] BGM signal queue with proper mixing — multiple concurrent notes mix by accumulation
- [x] source_id deduplication — same-lane keysounds steal voice instead of overlapping
- [x] Fractional measure size (channel 0) — time signature events parsed and applied per measure
- [x] BPM-aware velocity tree — scroll correctly accounts for mid-chart BPM changes
- [x] Audio clock synchronization — stream starts paused, play() resets counters after startup delay
- [x] Sample-accurate BGM scheduling — lookahead scheduler pushes commands with delay_samples
- [x] CPU usage monitor — callback timing (avg/max/budget logged every ~10s)
- [x] 1-based measure conversion — OJN 0-based measures converted to game's 1-based system
- [x] Correct scroll measure_size — 0.8 × judgment_line_y (384) matches Java HiSpeed (385)
- [x] Correct z-order — notes → static_keyboard → pressed overlays (original layer order)
- [x] Config persistence — auto-save to `~/.config/open2jam-rs/config.json`
- [ ] Skin selection UI
- [ ] Audio latency compensation (manual offset adjustment)
- [ ] Stop channels (chart events that pause audio)
- [ ] Hi-Speed modifier (UI + scroll adjustment)
- [ ] Note judgment text popups (COOL/GOOD/BAD/MISS text from skin — sprites only, no text yet)
- [ ] Max combo counter display

## How to Run

### Menu (recommended)
```bash
# Launch the unified app — opens menu GUI
cargo run
```

### Game (direct)
```bash
# Manual input mode (default) — play with keyboard (S D F Space J K L)
cargo run -- /path/to/song.ojn

# Auto-play mode — watch the game play itself
cargo run -- /path/to/song.ojn --autoplay

# Requirements:
#   - .ojn file (chart)
#   - .ojm file (audio) with matching name in same directory
#   - Skin XML at resources/ (project root)
```

### Release Build
```bash
cargo build --release
./target/release/open2jam-rs                  # menu
./target/release/open2jam-rs /path/to/song.ojn  # game directly
```

## Dependencies Explained

| Dependency | Purpose |
|---|---|
| `wgpu` | GPU rendering (Vulkan/Metal/DX12/WebGPU abstraction) |
| `egui` + `egui-wgpu` + `egui-winit` | Immediate-mode GUI (menu) rendered to same wgpu surface |
| `winit` | Cross-platform window creation & input events |
| `oddio` | Low-latency audio mixing (hotswap + buffer ring) |
| `cpal` | Cross-platform audio device (output to speakers) |
| `rtrb` | Lock-free SPSC queue for BGM scheduling (main → audio thread) |
| `lewton` | Pure-Rust Ogg Vorbis decoder |
| `hound` | WAV file reader/writer |
| `quick-xml` | Fast XML parsing (skin definitions) |
| `image` | PNG/JPEG loading for sprite textures |
| `pollster` | Sync-on-async (block on wgpu init) |
| `bytemuck` | Safe POD type casting for GPU buffers |
| `thiserror` + `anyhow` | Error handling (thiserror for lib, anyhow for app) |
| `log` + `env_logger` | Logging infrastructure |
| `sqlx` + `sqlite` | Song metadata cache (persistent) |
| `tokio` | Async runtime for DB operations |
| `rfd` | Native file dialog (folder picker for library) |
| `walkdir` | Recursive directory scanning |
| `sha2` | File hashing for cache invalidation |
| `serde` + `serde_json` | Config serialization |

## Design Principles

1. **Time-based, not frame-based** — all positions derived from game clock
2. **No singletons** — dependency injection via explicit state
3. **Separate time authorities** — audio hardware for playback, hybrid clock for rendering
4. **Skin XML is the authority for visuals** — layout, dimensions, sprite mappings
5. **Match Java open2jam behavior** — judgment logic, effect lifecycle, animation looping, blend modes, layer order
6. **Real-time safe audio** — no allocations, locks, or panics in cpal callback
7. **Event-driven input** — judgment fires immediately on key press (not buffered for later matching)
8. **Single binary** — no process spawning, shared wgpu device, unified event loop
9. **No eframe** — egui used bare for frame-accurate timing control

## Key Implementation Details

### 192 TPB Judgment System
All judgment windows are defined as fractions of a measure (192 ticks per beat, 4 beats per measure):

```
COOL_MEASURES      = 6/192   ≈ ±0.03125 measures
GOOD_MEASURES      = 18/192  ≈ ±0.09375 measures
BAD_MEASURES_TAP   = 25/192  ≈ ±0.13021 measures
BAD_MEASURES_RELEASE = 24/192 ≈ ±0.125 measures
```

Converted to milliseconds at the current BPM:
```
window_ms = measures × 4 × 60000 / BPM
```

At 150 BPM: COOL ≈ ±50ms, GOOD ≈ ±150ms, BAD ≈ ±208ms.
At 90 BPM: COOL ≈ ±83ms, GOOD ≈ ±250ms, BAD ≈ ±347ms.

This **BPM elasticity** means slow songs have wider windows (more forgiving), fast songs have narrower windows (stricter).

### Frame Speed Parsing
XML `framespeed` attribute is in **FPS**, not milliseconds. Conversion:
```
frame_speed_ms = 1000.0 / framespeed_value
```
- `effect_click_1`: 60fps → 16.67ms/frame (11 frames, ~183ms per cycle)
- `longflare`: 33.3fps → 30.03ms/frame (15 frames, ~450ms per cycle)

### Effect Animation Loop
Effects use **modulo** for frame selection, not clamping:
```rust
frame_index = (elapsed / frame_speed_ms) % frame_count
```
This matches Java `AnimatedEntity.move()`: `sub_frame %= frames.size()`. Effects loop continuously until their duration expires and they're cleaned up.

### Blend Mode Switching
The `TexturedRenderer` maintains two separate render pipelines:
- `pipeline_alpha`: Standard alpha blending (`src_alpha * src + (1-src_alpha) * dst`)
- `pipeline_additive`: Additive blending (`src_alpha * src + dst_alpha * dst`)

Effects call `set_blend_mode(BlendMode::Additive)` before drawing the longflare, then reset to `Alpha` afterward. This creates the vibrant glow effect for held long notes.

### Instant-Replace Judgments
Pending judgments use "instant replace" behavior: `clear_pending_judgments()` is called before adding a new one. This matches Java open2jam where the previous judgment entity is killed immediately.

### Fractional Measure Size (Channel 0)
OJN channel 0 contains a float value that scales the measure duration. E.g., `0.75` means the measure is 75% of normal size. The value is **per-measure** — it resets to `1.0` at each measure boundary unless a new channel 0 event overrides it. This matches Java's `frac_measure` behavior in `construct_velocity_tree()`.

Measure duration formula:
```
measure_duration_ms = 240000.0 * frac_measure / BPM
```
Where `240000 = 4 * 60 * 1000` (4 beats × 60 seconds × 1000ms).

### Audio Stream Lifecycle
1. `AudioManager::new()` — creates cpal stream but does NOT start it (`active = false`)
2. Startup animation plays (2000ms lifebar fill)
3. `GameState::update()` detects startup complete → sets `startup_audio_pending = true`
4. `engine.rs` sees the flag → calls `audio_mgr.play()`
5. `play()` resets `samples_played` to 0 (compensates ALSA starting callback early) → calls `stream.play()` → `active = true`
6. BGM scheduling only begins after `is_active() == true` (prevents stale delay values)

### Song End Timing
The game ends when `game_time >= end_time_ms`, where:
```
end_position = ceil(max(measure + position across all events)) + 1
end_time_ms  = ((end_position - refPosition) / bpm × 240000) + refTime
```

OJN measures are stored 0-based in the file, but the game uses 1-based coordinates (measure 1 = game clock 0:00). The `+1` is added to each measure before computing the formula, matching the C++ `block.Measure + 1` conversion.

### CPU Usage Monitor
Callback timing is tracked atomically in every cpal callback:
- `max_callback_us`: peak duration since startup (compare-exchange update)
- `avg_callback_us`: exponential moving average (alpha = 0.01)
- Logged every ~10 seconds on the main thread (zero impact on audio thread)
- Budget at 128 samples / 44.1kHz ≈ 2902µs; healthy is <20%, danger is >50%

### cleanup_notes Safety Margin
Notes are kept for `bad_window + 100ms` after passing the judgment line. This ensures that late key presses (up to ~340ms after the note target at 130 BPM) can still find and judge the note. Previously notes were removed the instant they passed the judgment line, causing the "2nd note not judged" bug.

### Resource Cleanup Order (9 Steps)
Prevents double-panic segfaults on exit:
```
1. egui_renderer (uses wgpu device/queue)
2. egui_winit (clipboard/OS resources)
3. egui_ctx (font textures)
4. game GPU resources (textures, atlases, pipelines)
5. surface (child of device)
6. game_state (audio triggers, sound cache)
7. audio manager (cpal stream)
8. menu app (SQLite pool, background threads)
9. render state (device/queue — parent of everything)
```

### Event Loop Architecture
- `ControlFlow::Poll` — continuously processes events (VSync/frame limiter already throttles FPS)
- `request_redraw()` called after each `render_frame()` completes
- Frame limiter runs AFTER rendering (hybrid spin-sleep)
- In Menu mode: input routed to egui-winit first; consumed events don't reach game
- In Playing mode: input routed directly to game key handler
- ESC in game mode calls `cleanup()` and exits event loop

### Font System
- **Inter** (primary, position 0) — Latin characters
- **egui defaults** (position 1+) — includes emoji fonts
- **Noto Sans CJK SC** (last, fallback) — CJK characters
- Auto-downloaded at build time from GitHub releases
- Cached in `crates/game/assets/` (gitignored)
- Subsequent builds reuse cached fonts with zero download overhead
