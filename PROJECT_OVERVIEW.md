# open2jam-rs — Project Overview

> O2Jam rhythm game port in Rust with wgpu + oddio + cpal

## Quick Facts

| | |
|---|---|
| **Language** | Rust 2021 Edition |
| **Version** | 0.1.0 (Preview Mode) |
| **Render** | wgpu 29 (GPU-accelerated, textured sprites, dual blend modes) |
| **Window/Input** | winit 0.30 |
| **Audio** | oddio 0.7.4 + cpal 0.17.3 (low-latency) |
| **Audio Formats** | Ogg Vorbis (lewton), WAV (hound) |
| **Parsing** | quick-xml (skin XML), custom binary (OJN/OJM) |
| **Upstream** | `git@github.com:ar-nim/open2jam-rs.git` → `ar-nim/gameplay-logic` |

## What Is This?

A Rust port of [open2jam-modern](../open2jam-modern) — a community reimplementation of the **O2Jam** rhythm game (2002 Korean arcade-style music game).

The project is currently in **preview mode**: no song selection UI, no menus. You launch it with a path to an `.ojn` chart file and it plays in auto-play mode with full judgment, effects, and scoring.

## File Structure

```
open2jam-rs/
├── Cargo.toml                 # Project manifest, dependencies
├── assets/                    # Game assets directory
│   └── audio/
├── test_assets/               # Test fixtures
│   └── README.md
├── src/
│   ├── main.rs                # Entry point — CLI args → App::run()
│   ├── engine.rs              # Frame orchestrator — winit loop, wgpu init, render pipeline
│   ├── game_state.rs          # Game state — clock, chart, note spawning, judgment, effects
│   ├── test_harness.rs        # Test utilities
│   │
│   ├── audio/                 # Audio subsystem
│   │   ├── mod.rs             # Exports AudioManager
│   │   ├── manager.rs         # oddio + cpal host setup, mixer control
│   │   ├── cache.rs           # Decoded sample cache (SoundCache)
│   │   ├── chart_audio.rs     # Chart-to-audio linkage
│   │   └── trigger.rs         # Time-driven audio trigger system (schedule, fire, skip)
│   │
│   ├── gameplay/              # Gameplay mechanics
│   │   ├── mod.rs             # Module exports
│   │   ├── scroll.rs          # Beat-to-pixel scroll math (core BPM-based formula)
│   │   ├── judgment.rs        # Hit detection (COOL/GOOD/BAD/MISS, tap + release)
│   │   └── modifiers.rs       # Game modifiers (Hi-Speed, etc.)
│   │
│   ├── parsing/               # File format parsers
│   │   ├── mod.rs             # Module exports
│   │   ├── ojn.rs             # OJN binary chart parser (.ojn) — notes, BPM changes, measures
│   │   ├── ojm.rs             # OJM binary audio parser (.ojm) — sample extraction
│   │   ├── chart.rs           # Chart data model (TimedEvent, NoteEvent, BpmChangeEvent)
│   │   └── xml.rs             # Skin XML parser (resources.xml) — sprites, entities, effects
│   │
│   ├── render/                # Rendering subsystem (wgpu)
│   │   ├── mod.rs             # Module exports
│   │   ├── atlas.rs           # Texture atlas builder — packs sprite frames into GPU texture
│   │   ├── textured_renderer.rs  # Batch textured quad renderer with dual blend modes (alpha/additive)
│   │   ├── pipeline.rs        # wgpu render pipeline (solid color quads)
│   │   ├── states.rs          # Render pipeline state management
│   │   └── hud.rs             # HUD rendering (score, combo, lifebar, timer, judgment popups)
│   │
│   ├── resources/             # Shared resource types
│   │   ├── mod.rs             # Module exports
│   │   ├── clock.rs           # Game clock — game_time vs render_time, BPM tracking, interpolation
│   │   ├── key_bindings.rs    # Keyboard configuration (S D F J K L ;)
│   │   ├── skin_assets.rs     # Skin asset loading
│   │   ├── chart_model.rs     # Chart data types
│   │   ├── state.rs           # State management utilities
│   │   └── async_loading.rs   # Async asset loading helpers
│   │
│   └── skin/                  # Skin system
│       ├── mod.rs             # Module exports
│       └── prefab.rs          # Note prefab definitions per lane (x, sprite IDs, long note support)
```

## Architecture

```
main() → App::new() → App::run()
                  │
                  ├─ EventLoop (winit)
                  │     ├─ resumed()         → init_wgpu + GameState::load()
                  │     ├─ about_to_wait()   → request_redraw()
                  │     └─ window_event()    → input (key press/release), resize, close
                  │
                  ├─ wgpu Renderer
                  │     ├─ Surface + Device + Queue
                  │     ├─ SkinAtlas — packed texture from skin XML sprites
                  │     ├─ TexturedRenderer — batch quad drawing
                  │     │     ├─ pipeline_alpha — standard alpha blending
                  │     │     └─ pipeline_additive — additive blending (GL_SRC_ALPHA, GL_DST_ALPHA)
                  │     └─ HudLayout — HUD position data from skin
                  │
                  ├─ Audio (oddio + cpal)
                  │     ├─ AudioManager → mixer handle
                  │     ├─ SoundCache → decoded OGG/WAV buffers
                  │     └─ AudioTriggerSystem → time-driven sample playback (schedule, fire, skip tracking)
                  │
                  └─ GameState
                        ├─ Clock → game_time, render_time, BPM, interpolation
                        ├─ Chart → parsed OJN events (notes, BPM changes)
                        ├─ active_notes → tap notes on screen
                        ├─ active_long_notes → long notes (head/body/tail, holding state)
                        ├─ pending_judgments → visual judgment popups (pop-in animation)
                        ├─ combo_counter → combo wobble animation state
                        ├─ stats → life, score, combo, judgment counts
                        ├─ note_click_effects → EFFECT_CLICK sprites (COOL/GOOD)
                        ├─ long_flare_effects → EFFECT_LONGFLARE sprites (additive glow)
                        └─ auto_judge_notes() / handle_key_press() / handle_key_release()
```

## Core Game Loop

1. **Delta Time** — measured per frame (rounded to prevent cumulative drift)
2. **Game State Update** — advance clock by delta_ms (startup delay phase → gameplay)
3. **Note Spawning** — spawn notes within lead-time window from chart events
4. **Auto-Judge / Input** — auto-play mode judges all notes as COOL; manual mode processes keyboard input
5. **Judgment Processing** — COOL/GOOD triggers effects (click/flare), records stats, updates combo
6. **Long Note Tail Judgment** — auto-release when tail passes judgment line
7. **Audio Processing** — trigger samples at scheduled game times via AudioTriggerSystem
8. **Effect Cleanup** — remove expired click/flare effects (duration-based lifecycle)
9. **Render** — draw skin background → judgment line → notes → effects → HUD (layered order)

## The Scroll Formula

The single most important calculation. Notes scroll based on **BPM**, not fixed speed:

```
distance_px = speed × beats_remaining × (0.8 × viewport_height) / 4
beats_remaining = (target_time_ms - render_time_ms) / (60000 / BPM)
```

**Higher BPM = faster scroll.** A note at 200 BPM moves twice as fast as one at 100 BPM.

The travel time determines spawn lead:
```
travel_time_ms = (4 × viewport_height / (speed × measure_size)) × 60000 / BPM
```

## File Formats

| Format | Extension | Purpose | Parser |
|--------|-----------|---------|--------|
| **OJN** | `.ojn` | Chart — note events, BPM changes, measures, sample IDs | `parsing/ojn.rs` |
| **OJM** | `.ojm` | Audio — individual samples (WAV IDs 0-999, OGG IDs 1000+) | `parsing/ojm.rs` |
| **Skin XML** | `resources.xml` | Sprite definitions, entity layouts, judgment line Y, effect sprites | `parsing/xml.rs` |

**Chart-to-Audio Linkage:** OJN contains `sample_id` values that map to samples within the OJM file. There is **no continuous background track** — BGM is individual samples triggered automatically at the right game time.

## Key Input

Default lane bindings: **S D F J K L ;** for lanes 1–7.

Key press triggers judgment (tap or long note head). Key release evaluates long note tail timing.

## Skin System

- Skin is loaded from `resources.xml` in the Java source directory
- Sprites are packed into a **texture atlas** at startup (GPU texture, UV mapping)
- Notes use per-lane **prefabs** with customizable head/body/tail sprites
- The base skin is 800×600, scaled to fit the window with letterboxing
- **Effect sprites** (EFFECT_CLICK, EFFECT_LONGFLARE) extracted from skin XML with frame count and speed
- **Animation frame speed** parsed as FPS from XML (e.g., `framespeed="60"` → 60fps → 16.67ms/frame)

## Entity State Machine (Notes)

```
Tap Note:
  NOT_JUDGED → (player hits COOL/GOOD/BAD) → JUDGED → cleanup
  NOT_JUDGED → (missed/passed)             → MISSED → cleanup

Long Note:
  NOT_JUDGED → (head hit)     → JUDGED + HOLDING
  HOLDING    → (key release)  → RELEASED → tail judgment → cleanup
  HOLDING    → (tail reached) → auto-release judgment → cleanup
  NOT_JUDGED → (head missed)  → MISSED → cleanup
```

## Judgment System

### Tap Note Judgment
- **COOL**: ±~30ms window (BPM-dependent), +2 life, 200 + combo×10 score, triggers EFFECT_CLICK
- **GOOD**: ±~80ms window, +1 life, 100 score, triggers EFFECT_CLICK
- **BAD**: ±~130ms window, 0 life (Normal: +1), 0 score, breaks combo
- **MISS**: outside all windows, -10 life, 0 score, breaks combo

### Long Note Release Judgment
- Evaluated against tail time when player releases key or tail passes judgment line
- Same timing windows as tap notes
- Head judgment propagates: if head was BAD/MISS, release auto-MISS

### Combo System
- COOL/GOOD increases combo counter
- BAD/MISS resets combo to 0
- Combo counter has wobble animation (pop-in + slide)
- Jam counter (combo milestone) shows briefly on certain thresholds

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
- **Duration**:
  - **Autoplay**: equals actual hold time (`tail_time - head_time`)
  - **Manual play**: uses sprite-based duration (frame_count × frame_speed_ms)
- **Blend mode**: **additive blending** (GL_SRC_ALPHA, GL_DST_ALPHA) for vibrant glow effect
- **Cleanup**: removed when `is_active()` returns false, or killed on key release / miss

### PendingJudgment (Judgment Popup)
- **Triggered on**: any judgment (COOL/GOOD/BAD/MISS)
- **Position**: per-lane, from skin XML HUD layout
- **Animation**: pop-in (50%→100% scale over 100ms), stays full size for 750ms
- **Behavior**: instant-replace — new judgment kills previous one immediately

## Current Status — What Works

- [x] Window creation & wgpu rendering
- [x] OJN chart parsing (notes, BPM changes, measures, sample IDs)
- [x] OJM audio decoding (WAV + OGG samples)
- [x] Texture atlas building from skin XML sprites
- [x] Beat-based note scrolling (BPM-dependent)
- [x] Note spawning & cleanup (lead-time calculation)
- [x] Auto-play mode (auto-judge all notes as COOL)
- [x] Long note rendering (head/body/tail with stretchable body)
- [x] Keyboard input → judgment (press + release)
- [x] Tap note judgment (COOL/GOOD/BAD/MISS with timing windows)
- [x] Long note head + tail judgment (hold + release evaluation)
- [x] EFFECT_CLICK rendering (positioned on judgment line, correct speed, alpha blending)
- [x] EFFECT_LONGFLARE rendering (positioned at skin Y, additive blending, dynamic duration)
- [x] Animation looping (modulo-based, matches Java AnimatedEntity)
- [x] Effect lifecycle (duration-based cleanup)
- [x] Combo counter with wobble animation
- [x] Jam counter (combo milestone popup)
- [x] Score calculation (200 + combo×10 for COOL, 100 for GOOD)
- [x] Life / health system (HP gain/loss per judgment)
- [x] HUD rendering (score, combo, lifebar, timer, judgment popups)
- [x] Audio trigger system (time-driven sample playback)
- [x] Startup delay animation (2000ms lifebar fill)
- [x] Dual blend mode pipelines (alpha + additive)
- [ ] Song selection menu
- [ ] Skin selection UI
- [ ] Audio latency compensation
- [ ] Stop channels (chart events that pause audio)
- [ ] Hi-Speed modifier (UI + scroll adjustment)
- [ ] Note judgment windows (COOL/GOOD/BAD/MISS text popups — partially done)
- [ ] Max combo counter display

## How to Run

```bash
# Basic usage — auto-play a chart
cargo run -- /path/to/song.ojn

# Requirements:
#   - .ojn file (chart)
#   - .ojm file (audio) with matching name in same directory
#   - Skin XML at ../open2jam-modern/src/resources/resources.xml
```

## Dependencies Explained

| Dependency | Purpose |
|---|---|
| `wgpu` | GPU rendering (Vulkan/Metal/DX12/WebGPU abstraction) |
| `winit` | Cross-platform window creation & input events |
| `oddio` | Low-latency audio mixing (hotswap + buffer ring) |
| `cpal` | Cross-platform audio device (output to speakers) |
| `lewton` | Pure-Rust Ogg Vorbis decoder |
| `hound` | WAV file reader/writer |
| `quick-xml` | Fast XML parsing (skin definitions) |
| `image` | PNG/JPEG loading for sprite textures |
| `pollster` | Sync-on-async (block on wgpu init) |
| `bytemuck` | Safe POD type casting for GPU buffers |
| `thiserror` + `anyhow` | Error handling (thiserror for lib, anyhow for app) |
| `log` + `env_logger` | Logging infrastructure |

## Design Principles

1. **Time-based, not frame-based** — all positions derived from game clock
2. **No singletons** — dependency injection via explicit state
3. **Separate time authorities** — game time for logic, render time (+interpolation) for visuals
4. **Skin XML is the authority for visuals** — layout, dimensions, sprite mappings
5. **Match Java open2jam behavior** — judgment logic, effect lifecycle, animation looping, blend modes

## Key Implementation Details

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
