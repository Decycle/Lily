# WACCA_slim

A native Rust reimplementation of WACCA Reverse (3.07.01) for a Windows touchscreen PC. It covers the single-player
core loop: title → mode → genre → song and difficulty → options → play → results, with personal bests saved locally.
Multiplayer, cards, servers, unlocks and other arcade-only features are not included (see [docs/PLAN.md](docs/PLAN.md) §1).

**You need your own copy of the original game.** This repository contains no game files. Charts, audio, movies,
textures, fonts and meshes are all read from your install. Personal use only.

## Quick start (Windows)

1. **Toolchain.** Install Rust with `winget install Rustlang.Rustup`, then select the GNU toolchain:
   `rustup default stable-x86_64-pc-windows-gnu`. Some dependencies are pinned to versions that build on this
   toolchain, so don't run a bare `cargo update` ([CONTRIBUTING.md](CONTRIBUTING.md#toolchain)).
2. **FFmpeg** (optional, needed for background movies): `winget install Gyan.FFmpeg`.
3. **Game files.** Copy the game's `WindowsNoEditor\Mercury\Content` folder to `assets\` in the repository root.
   `assets/` and `data/` are gitignored.
4. **Convert, one time** (writes `data\`):
   ```
   cargo run --release -p wacca-convert -- --assets assets --out data --textures --fonts --meshes --export-ui UI/Textures --export-ui Vfx/Textures
   ```
   | Flag | Writes |
   |---|---|
   | (always) | `data/songs.json`: song list from `MusicParameterTable` |
   | `--textures` | `data/jackets/*.png` |
   | `--fonts` | `data/fonts/` (the game's fonts, incl. the combo-digit font) |
   | `--meshes` | `data/meshes/*.obj` (3D note meshes) |
   | `--export-ui <dir>` | `data/ui/<dir>/**.png`: every texture under `assets/<dir>` (menus, playfield, notes) |
5. **Play:**
   ```
   cargo run --release -p wacca-app -- --assets assets --data data
   ```

### `wacca-app` flags

| Flag | Meaning |
|---|---|
| `--assets <dir>` | Game `Content` folder (required) |
| `--data <dir>` | Output of `wacca-convert` (default `data`) |
| `--save <file>` | Save file (default `%APPDATA%\wacca-slim\save.json`) |
| `--windowed` | Start in a 450×800 window instead of borderless fullscreen |
| `--autoplay` | Start with AUTOPLAY on |
| `--speed <1.0–6.0>` | Note speed for this session (overrides the saved one) |
| `--offset-ms <N>` | Timing offset in ms for this session, rounded to the TIMING step (8.33 ms) and clamped to ±83 ms |
| `--chart <path>` | Play this chart right away. The path is relative to `--assets`: either `MusicData/<song>/<song>_NN.mer` or `MusicData/<song>` together with `--difficulty` |
| `--difficulty normal\|hard\|expert\|inferno` | Difficulty for `--chart` |
| `--screen title\|mode\|category\|select\|options\|calibrate\|results` | Open on this screen (for debugging) |

## How to play

Notes travel from the centre outward. Hit them as they reach the ring at the edge. The ring has 60 lanes and 4
depth rings of sensors, like the cabinet. A note covers an arc, and touching anywhere inside that arc counts.

| Note | What it needs |
|---|---|
| Touch | A new touch on the note's arc in time |
| Hold | Touch the start in time, then keep a finger on the (moving) hold until it ends. You don't need to release at the end. A gap longer than 175 ms drops the hold to Good, or to Miss if the start was also missed |
| Slide (arrows along the ring) | Swipe along the ring in the arrow direction, at least ~2 lanes |
| Snap in (arrows to the centre) | Move the finger at least one sensor ring inward |
| Snap out (arrows to the edge) | Move the finger at least one sensor ring outward |
| Chain | Just be touching the arc when it arrives. A resting finger counts. Chains are only Marvelous or Miss |
| R notes | Judged like their base type, but worth double score |

Judgements are Marvelous / Great / Good / Miss. Touch windows are ±50 / 83 / 100 ms, and the other note types follow
the game's `JudgeParameterTable` ([doc 04 §4](docs/research/04-gameplay-mechanics.md)).

**Without a touchscreen:** the left mouse button is one finger. Click to tap, hold the button for holds, and drag
for slides and snaps. Chords need more than one finger.

## Controls

Touch or click works everywhere: tap the on-screen buttons, and drag sideways or use the mouse wheel to turn the
song and genre wheels.

| Screen | Keys |
|---|---|
| Anywhere | **F11** or **Alt+Enter**: toggle fullscreen · **F3**: performance overlay |
| Menus (general) | **Enter**/**Space**: confirm (GO) · **Esc**/**Backspace**: back (on the title screen this quits) |
| Genre select | **←/→**: genre · **↑/↓**: difficulty · **D**: cycle the difficulty · **S**: sort · **F**: filter |
| Song select | **←/→** or wheel: song · **↑/↓**: difficulty · **D**: cycle through the song's charts · **PageUp/PageDown**: previous/next genre · **S**: sort (title → level → version) · **F**: show only songs that have the chosen difficulty |
| Options | **↑/↓**: row · **←/→**: change the value. Tap left or right of a value to step it down or up. Pick TIMING CHECK to open it |
| Timing check | **Space** or a tap anywhere: tap on each click · **Enter**/APPLY: use the suggestion · **Esc**/BACK: leave without changing it |
| Play | **Esc**: give up (the result is still recorded) · **Space**: pause/resume · **↑/↓**: note speed ±0.5 for this play only |
| Results | **Enter**/**Esc**: back to song select · **R** or RETRY: play again |

## Options

The options screen opens after you pick a song. Settings are saved when you start the song.

| Option | Default | What it does |
|---|---|---|
| SPEED | 4.0 | Note speed 1.0–6.0 in steps of 0.5. At 1.0, a note takes 3.33 s from the centre to the ring |
| TIMING | +0.0 | Judgement offset from −10.0 to +10.0 in steps of 0.1. One unit is 8.33 ms, so the full range is ±83 ms. Positive values mean you have to hit earlier (use this if you are judged late) |
| TIMING CHECK | | A 120 BPM click track. Tap along at least 16 times. The median error, with outliers removed, is shown as a suggested TIMING. APPLY saves it. This measures audio, touch and your own latency together |
| MIRROR | OFF | Plays the chart mirrored left to right, both visually and for judging |
| AUTOPLAY | OFF | Perfect automatic play. It is not saved, and autoplay results are not recorded |
| BG MOVIE | ON | Shows the song's background movie (needs FFmpeg) |
| BAR LINES | ON | Measure lines on the playfield, as in the real game |
| TOUCH GUIDE | OFF | Faint depth-ring circles and highlights on the sensors you are pressing (not in the real game) |

These settings are only in the save file (edit it while the app is closed):

| Key in `settings` | Default | What it does |
|---|---|---|
| `menuTimers` | `false` | Show the arcade's menu countdowns and continue automatically when they run out (mode 30 s, song select 99 s, options 25 s, results 15 s) |
| `playerName`, `playerTitle` | `"PLAYER"`, `""` | Name and title on the results screen's player plate |

## Save file

`%APPDATA%\wacca-slim\save.json`, or the path given with `--save`. It is versioned JSON and written atomically, with
the previous version kept as `save.json.bak`. It stores:

- `settings`: the options above.
- `records`, keyed `<songId>:<difficulty>`: best score and its judgement counts, best lamp (clear / missless /
  full combo / all marvelous), best grade, best combo, lowest miss count, and counters for plays, clears, lamps and
  grades. Every play counts, including give-ups.
- `reversePoints`: total RP earned.

Rating and player level are calculated from these values, not stored. If the save can't be read, the app doesn't
overwrite it and plays without saving. Spec: [doc 05 §2](docs/research/05-save-and-song-select.md).

## Troubleshooting

| Symptom | Cause / fix |
|---|---|
| `error: audio: ...` at start | There is no default output device, or it can't play 48 kHz (there is no resampler). Connect or enable a device, or set it to 48 kHz in Windows Sound settings |
| No background movies (log: `ffmpeg not found`) | Install FFmpeg (`winget install Gyan.FFmpeg`) or set `WACCA_FFMPEG=C:\path\to\ffmpeg.exe`. Without it, songs play without movies |
| Song select is empty | `data/songs.json` is missing. Run `wacca-convert`, and pass `--data` if `data` isn't in the current folder |
| Flat 2D notes / plain colours / missing jackets | `data/meshes`, `data/ui/...` or `data/jackets` is missing. Rerun the convert command with all its flags. The app falls back to 2D notes and flat colours |
| Fullscreen is black, stutters or is slow (laptops with two GPUs) | Run with `--windowed` and press F11 later, or pick the GPU yourself: `WGPU_POWER_PREF=high` and/or `WGPU_BACKEND=vulkan` (or `dx12`). F3 shows frame times. The adapter in use is logged at start |
| Notes judged consistently early or late | Run TIMING CHECK in the options, or set TIMING yourself |

Environment variables:

| Variable | Effect |
|---|---|
| `WACCA_FFMPEG` | Path to `ffmpeg.exe`. Otherwise the app looks on `PATH`, then in winget's install folder |
| `WACCA_PRESENT=immediate` | Disable vsync (uncapped frame rate, tearing) |
| `WACCA_PACE=0` | Turn off vsync frame pacing (the app normally waits until shortly before the next refresh to cut input latency) |
| `WGPU_BACKEND`, `WGPU_POWER_PREF` | wgpu's GPU selection (`vulkan`/`dx12`, `high`/`low`) |
| `RUST_LOG` | Log level (default `info`). `RUST_LOG=wacca_app=debug` logs every judgement, and `RUST_LOG=wacca_app::perf=debug` logs the F3 stats every 2 s |

## Accuracy

The goal is a faithful copy. Values are taken from the game's own data tables, and the rest is measured on the real
game, which is driven by injected touch input and recorded ([doc 07](docs/research/07-real-game-harness.md)).

- **Measured and matching:** touch mapping (all 240 sensors) and the minimum tap length
  ([doc 08](docs/research/08-e1-results.md)); a perfect run scores 1,000,000, lane 0 and its direction, and
  playfield geometry ([doc 09](docs/research/09-e2-results.md)); judgement windows, hold drop (175 ms), slide distance
  and snap ring rules ([doc 10](docs/research/10-e3-judge-results.md)). Fed the same input as the real game, our judge
  agrees on every note more than one frame from a window edge except 1 of 406 (doc 10 §8).
- **Visuals:** the play area uses the game's own note meshes and textures, the camera and note speed from RenderDoc
  captures, and WGSL ports of the real shaders. Colours are within 1–3/255 of captured frames
  ([doc 11](docs/research/11-shaders-and-meshes.md)). Hit effects and menus are approximations.
- **Not measured yet:** the chord / one-press rule, snap-out windows, the TIMING unit and sign, and some gauge
  details. These are marked `UNVERIFIED` in the code and docs ([doc 04 §11](docs/research/04-gameplay-mechanics.md),
  [PLAN §5](docs/PLAN.md)). Progress: [docs/GOALS.md](docs/GOALS.md).

## Layout

| Path | What |
|---|---|
| `crates/wacca-app` | The game: window, touch, screens, play, save IO |
| `crates/wacca-game` | Rules: judgement, score, gauge, grades, rating, RP, save format |
| `crates/wacca-chart` | `.mer` chart parser and timing |
| `crates/wacca-render` | wgpu renderer: playfield, notes, effects, text (examples: `playfield`, `stagecmp`) |
| `crates/wacca-audio` | CRI ACB/AWB, HCA/ADX decoding, audio output and clock |
| `crates/wacca-video` | `.usm` movie demux and decoding through an external `ffmpeg` |
| `crates/wacca-ue4` | Reader for UE 4.19 cooked packages (DataTables, textures, meshes) |
| `crates/wacca-convert` | `assets/` → `data/` converter |
| `tools/realgame` | Harness that drives and records the real game: `run_experiment`, `e2_script`, `e3_script`, `analyze_e1/e2/e3`, `judge_parity` |
| `tools/research` | Python scripts used for format research (UE4, CRI, RenderDoc) |
| `docs/` | [PLAN](docs/PLAN.md), [GOALS](docs/GOALS.md), and the research docs `docs/research/01–11` |

## For developers

Read [CONTRIBUTING.md](CONTRIBUTING.md) for code rules, tests (`cargo test --workspace`; tests that need the real
assets are `#[ignore]` and read `WACCA_ASSETS`), pinned dependencies, and the rule that no game data is ever
committed.
