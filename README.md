# Lily

Lily is a faithful native reimplementation of the WACCA Reverse (3.07.01) rhythm game, written in Rust. It runs the
single-player core loop on a PC, ideally with a touchscreen: title → mode → genre → song and difficulty → options →
play → results, with personal bests saved locally.

It reads **your own copy of the game**: charts, music, movies, textures, fonts and note meshes all come from the
game files you supply. This repository contains no game assets. Lily is unofficial and not affiliated with SEGA or
Marvelous ([NOTICE.md](NOTICE.md)).

Not included: multiplayer, cards and login, servers, unlocks, and other arcade-only features (everything is unlocked).

## How faithful is it?

The aim is a copy, not a remake. Values come from the game's own data tables. Everything else was measured on the
real game, driven by injected touch input and recorded ([doc 07](docs/research/07-real-game-harness.md)):

- **Judgement:** windows, hold drop, slide distance, snap rings and chord rules were measured on the real game
  ([doc 10](docs/research/10-e3-judge-results.md)). Fed the same recorded input, Lily's judge agrees with the real
  game on 1,068 of the 1,070 notes that landed more than one frame from a window edge (doc 10 §10.4).
- **Visuals:** the play area uses the game's note meshes and textures, the camera and note speed from RenderDoc
  frame captures, and WGSL ports of the game's compiled shaders. Colours are within 1–3/255 of the captured frames
  ([doc 11](docs/research/11-shaders-and-meshes.md)), and whole frames are compared side by side with the real
  game ([doc 13](docs/research/13-parity-audit.md), menus [doc 14](docs/research/14-menu-parity.md)).
- **Scoring:** a perfect run scores 1,000,000, and grades, lamps, rating and Reverse Points follow the game's tables
  ([doc 02](docs/research/02-data-tables.md), [doc 05](docs/research/05-save-and-song-select.md)).

Anything inferred rather than measured is marked `UNVERIFIED` in the code and docs. Open items are in
[docs/ROADMAP.md](docs/ROADMAP.md).

## Screenshots

No game images are committed to this repository. To see Lily, run it with your own game files.

## Features

- All 395 songs and 1,247 charts of 3.07.01, with genre and song select, sort and filter, preview audio, and jackets.
- Real note meshes, shaders, hit effects, background movies (optional) and the no-movie background scene.
- Touch input on a virtual 60 × 4 sensor ring, like the cabinet. The mouse works as a single finger.
- Options: note speed, timing offset with a tap-along TIMING CHECK, mirror, autoplay, and more.
- Results with grade, lamp, judgement counts and fast/late, saved personal bests, rating and Reverse Points.

## Requirements

- **Your own copy of WACCA Reverse 3.07.01**: the Reverse update archives (3.00.00 base plus 3.01.00 … 3.07.01), or
  an installed or unpacked game.
- Windows 10 or 11, 64-bit, with a GPU that supports DirectX 12 or Vulkan. A touchscreen is recommended.
  Linux and macOS build from source but are untested ([GUIDE § Porting notes](docs/GUIDE.md#porting-notes)).
- An audio output device that can play 48 kHz.
- About 7 GB of free disk space for the game files Lily uses, plus about 20 GB if you include the background movies.
- Optional: [FFmpeg](https://ffmpeg.org/) on `PATH` for background movies.

## Quick start (players)

1. Download the portable Windows build from the Releases page and unzip it anywhere. You get a `Lily/` folder.
2. Run `lily.exe`. On the first start, setup asks for your game files:
   1. **Pick a folder** that contains either the Reverse `.7z` update archives or an existing game folder.
   2. Lily **verifies** that the files are the expected 3.07.01 data.
   3. It **unpacks only the files it needs** (charts, audio, textures, fonts, meshes; movies are optional).
   4. It **converts** them into its own `data` folder.
   5. **Play.** Setup runs once; afterwards Lily starts straight at the title screen.
3. For background movies, install FFmpeg (`winget install Gyan.FFmpeg`). Without it, songs play without movies.

The full walkthrough, how each note type is judged and troubleshooting are in the [player guide](docs/GUIDE.md).

## Build from source (developers)

```
rustup default stable-x86_64-pc-windows-gnu      # Windows; see CONTRIBUTING.md#toolchain
cargo build --release -p wacca-app
```

With the game's `WindowsNoEditor\Mercury\Content` folder copied to `assets/` (gitignored), convert once and play:

```
cargo run --release -p wacca-convert -- --assets assets --out data --textures --fonts --meshes --export-ui UI/Textures --export-ui Vfx/Textures
cargo run --release -p wacca-app -- --assets assets --data data
```

| `wacca-convert` flag | Writes |
|---|---|
| (always) | `data/songs.json`: song list from `MusicParameterTable` |
| `--textures` | `data/jackets/*.png` |
| `--fonts` | `data/fonts/` (the game's fonts, including the combo-digit font) |
| `--meshes` | `data/meshes/*.obj` (3D note meshes) |
| `--export-ui <dir>` | `data/ui/<dir>/**.png`: every texture under `assets/<dir>` (menus, playfield, notes) |

Some dependencies are pinned to versions that build on the GNU toolchain, so don't run a bare `cargo update`
([CONTRIBUTING.md](CONTRIBUTING.md#toolchain)).

### Command-line flags

| Flag | Meaning |
|---|---|
| `--assets <dir>` | Game `Content` folder (default: `assets` next to the executable) |
| `--data <dir>` | Output of `wacca-convert` (default `data`; next to the executable when `--assets` is not given either) |
| `--save <file>` | Save file (default `%APPDATA%\lily\save.json`, or `save\save.json` next to the executable when portable) |
| `--portable` | Keep the save next to the executable (same as a `portable.txt` file there) |
| `--windowed` | Start in a 450×800 window instead of borderless fullscreen |
| `--autoplay` | Start with AUTOPLAY on |
| `--speed <1.0–6.0>` | Note speed for this session (overrides the saved one) |
| `--offset-ms <N>` | Timing offset in ms for this session, rounded to the TIMING step (8.33 ms) and clamped to ±83 ms |
| `--chart <path>` | Play this chart right away. The path is relative to `--assets`: either `MusicData/<song>/<song>_NN.mer` or `MusicData/<song>` together with `--difficulty` |
| `--difficulty normal\|hard\|expert\|inferno` | Difficulty for `--chart` |
| `--screen title\|mode\|category\|select\|options\|calibrate\|results\|setup` | Open on this screen (for debugging) |
| `--setup-source <dir>` | Open setup and inspect this folder right away (debugging); with `--setup-start` also start it (without videos) |

## Controls

Touch or click works everywhere: tap the on-screen buttons, and drag sideways or use the mouse wheel to turn the
song and genre wheels. Without a touchscreen, the left mouse button is one finger.

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

Save-file-only settings, the save format and environment variables are described in the
[player guide](docs/GUIDE.md#save-file-and-advanced-settings).

## Project layout

| Path | What |
|---|---|
| `crates/wacca-app` | The game: window, touch, screens, play, save IO |
| `crates/wacca-game` | Rules: judgement, score, gauge, grades, rating, Reverse Points, save format |
| `crates/wacca-chart` | `.mer` chart parser and timing |
| `crates/wacca-render` | wgpu renderer: playfield, notes, effects, text, WGSL shader ports (examples: `playfield`, `stagecmp`) |
| `crates/wacca-audio` | CRI ACB/AWB, HCA/ADX decoding, audio output and clock |
| `crates/wacca-video` | `.usm` movie demux and decoding through an external `ffmpeg` |
| `crates/wacca-ue4` | Reader for UE 4.19 cooked packages (DataTables, textures, meshes, materials, particles) |
| `crates/wacca-convert` | Game files → `data/` converter |
| `tools/realgame` | Windows-only harness that drives and records the real game, and compares its judgements with ours |
| `tools/research` | Python scripts used for format research (UE4, CRI, RenderDoc) |
| `docs/` | [Player guide](docs/GUIDE.md), [roadmap](docs/ROADMAP.md), the original [plan](docs/PLAN.md), and research docs [01–14](docs/research/) |

The crate names keep the `wacca-` prefix because they describe the game's formats.

## Research docs

| Doc | Topic |
|---|---|
| [01](docs/research/01-chart-format.md) | `.mer` chart format |
| [02](docs/research/02-data-tables.md) | Data tables: judgement, score, gauge, rating |
| [03](docs/research/03-audio-and-jackets.md) | CRI audio and jackets |
| [04](docs/research/04-gameplay-mechanics.md) | Gameplay mechanics |
| [05](docs/research/05-save-and-song-select.md) | Save data and song select |
| [06](docs/research/06-visuals.md) | Movies, textures, UI |
| [07](docs/research/07-real-game-harness.md) | Real-game harness |
| [08](docs/research/08-e1-results.md), [09](docs/research/09-e2-results.md), [10](docs/research/10-e3-judge-results.md) | Measurements: touch mapping, playfield geometry, judgement |
| [11](docs/research/11-shaders-and-meshes.md), [12](docs/research/12-hit-effects.md) | Shaders, meshes, hit effects |
| [13](docs/research/13-parity-audit.md), [14](docs/research/14-menu-parity.md) | Visual parity audits: play area, menus |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for code rules, tests (`cargo test --workspace`; tests that need real game
files are `#[ignore]` and read `WACCA_ASSETS`), pinned dependencies, and the rule that no game data is ever committed.

## License

The code is released under the [MIT license](LICENSE). WACCA and all game content are © SEGA / Marvelous; Lily is an
unofficial fan project, contains no game assets, and requires your own legally obtained copy of the game. See
[NOTICE.md](NOTICE.md).
