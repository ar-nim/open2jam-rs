# open2jam-rs — Project Overview

> O2Jam rhythm game port in Rust with wgpu + oddio + cpal

## Quick Facts

| | |
|---|---|
| **Language** | Rust 2021 Edition |
| **Version** | 0.1.0 (Preview Mode) |
| **Render** | wgpu 29 (GPU-accelerated, textured sprites) |
| **Window/Input** | winit 0.30 |
| **Audio** | oddio 0.7.4 + cpal 0.17.3 (low-latency) |
| **Audio Formats** | Ogg Vorbis (lewton), WAV (hound) |
| **Parsing** | quick-xml (skin XML), custom binary (OJN/OJM) |

## What Is This?

A Rust port of [open2jam-modern](../open2jam-modern) — a community reimplementation of the **O2Jam** rhythm game (2002 Korean arcade-style music game).

The project is currently in **preview mode**: no song selection UI, no menus. You launch it with a path to an `.ojn` chart file and it plays in auto-play mode.

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
│   ├── engine.rs              # Frame orchestrator — winit loop, wgpu init, render, skin loading
│   ├── game_state.rs          # Game state — clock, chart, note spawning, judgment
│   ├── test_harness.rs        # Test utilities
│   │
│   ├── audio/                 # Audio subsystem
│   │   ├── mod.rs             # Exports AudioManager
│   │   ├── manager.rs         # oddio + cpal host setup
│   │   ├── cache.rs           # Decoded sample cache (SoundCache)
│   │   ├── chart_audio.rs     # Chart-to-audio linkage
│   │   └── trigger.rs         # Time-driven audio trigger system
│   │
│   ├── gameplay/              # Gameplay mechanics
│   │   ├── mod.rs             # Module exports
│   │   ├── scroll.rs          # Beat-to-pixel scroll math (core formula)
│   │   ├── judgment.rs        # Hit detection logic
│   │   └── modifiers.rs       # Game modifiers (Hi-Speed, etc.)
│   │
│   ├── parsing/               # File format parsers
│   │   ├── mod.rs             # Module exports
│   │   ├── ojn.rs             # OJN binary chart parser (.ojn)
│   │   ├── ojm.rs             # OJM binary audio parser (.ojm)
│   │   ├── chart.rs           # Chart data model
│   │   └── xml.rs             # Skin XML parser (resources.xml)
│   │
│   ├── render/                # Rendering subsystem (wgpu)
│   │   ├── mod.rs             # Module exports
│   │   ├── atlas.rs           # Texture atlas builder from sprite frames
│   │   ├── textured_renderer.rs  # Batch textured quad renderer
│   │   ├── pipeline.rs        # wgpu render pipeline
│   │   ├── states.rs          # Render pipeline state management
│   │   └── hud.rs             # HUD rendering
│   │
│   ├── resources/             # Shared resource types
│   │   ├── mod.rs             # Module exports
│   │   ├── clock.rs           # Game clock (game time vs render time)
│   │   ├── key_bindings.rs    # Keyboard configuration
│   │   ├── skin_assets.rs     # Skin asset loading
│   │   ├── chart_model.rs     # Chart data types
│   │   ├── state.rs           # State management utilities
│   │   └── async_loading.rs   # Async asset loading helpers
│   │
│   └── skin/                  # Skin system
│       ├── mod.rs             # Module exports
│       └── prefab.rs          # Note prefab definitions per lane
```

## Architecture

```
main() → App::new() → App::run()
                  │
                  ├─ EventLoop (winit)
                  │     ├─ resumed()     → init_wgpu + GameState::load()
                  │     ├─ about_to_wait() → request_redraw()
                  │     └─ window_event() → input, resize, close
                  │
                  ├─ wgpu Renderer
                  │     ├─ Surface + Device + Queue
                  │     ├─ TextureAtlas (packed sprite sheet)
                  │     └─ TexturedRenderer (batch quad drawing)
                  │
                  ├─ Audio (oddio + cpal)
                  │     ├─ AudioManager → mixer handle
                  │     ├─ SoundCache → decoded OGG/WAV buffers
                  │     └─ AudioTriggerSystem → time-driven sample playback
                  │
                  └─ GameState
                        ├─ Clock → game_time, render_time, BPM
                        ├─ Chart → parsed OJN events
                        ├─ active_notes → tap notes on screen
                        ├─ active_long_notes → long notes on screen
                        └─ ScrollSystem → beat → pixel conversion
```

## Core Game Loop

1. **Delta Time** — measured per frame (rounded to prevent cumulative drift)
2. **Game State Update** — advance clock by delta_ms
3. **Note Spawning** — spawn notes within lead-time window from chart events
4. **Audio Processing** — trigger samples at scheduled game times
5. **Note Cleanup** — remove notes that passed the judgment line
6. **Render** — draw skin background, then notes, then HUD (layered order)

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
| **Skin XML** | `resources.xml` | Sprite definitions, entity layouts, judgment line Y | `parsing/xml.rs` |

**Chart-to-Audio Linkage:** OJN contains `sample_id` values that map to samples within the OJM file. There is **no continuous background track** — BGM is individual samples triggered automatically at the right game time.

## Key Input

Default lane bindings: **S D F J K L ;** for lanes 1–7.

## Skin System

- Skin is loaded from `resources.xml` in the Java source directory
- Sprites are packed into a **texture atlas** at startup
- Notes use per-lane **prefabs** with customizable head/body/tail sprites
- The base skin is 800×600, scaled to fit the window with letterboxing

## Entity State Machine (Notes)

```
NOT_JUDGED → (player hits)    → JUDGED
NOT_JUDGED → (missed/passed)  → MISSED → KILLED (cleanup)
Long Note:  → JUDGED → HOLDING → RELEASED → KILLED
```

## Current Status — What Works

- [x] Window creation & wgpu rendering
- [x] OJN chart parsing
- [x] OJM audio decoding
- [x] Texture atlas building from skin XML sprites
- [x] Beat-based note scrolling
- [x] Note spawning & cleanup
- [x] Auto-play audio triggers
- [x] Long note rendering (head/body/tail)
- [x] Keyboard input → judgment
- [ ] Judgment windows (COOL/GOOD/BAD)
- [ ] Scoring & combo system
- [ ] Health bar / life system
- [ ] Song selection menu
- [ ] Skin selection UI
- [ ] Audio latency compensation
- [ ] Stop channels
- [ ] Hi-Speed modifier (UI)

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

## Design Principles

1. **Time-based, not frame-based** — all positions derived from game clock
2. **No singletons** — dependency injection via explicit state
3. **Separate time authorities** — game time for logic, render time (+offset) for visuals
4. **Skin XML is the authority for visuals** — layout, dimensions, sprite mappings