# Lily player guide

This guide covers setting up Lily, how each note is judged, and what to do when something goes wrong. For an
overview of the project, see the [README](../README.md).

## 1. Requirements

- **Your own copy of WACCA Reverse**, 3.00.00 or newer, in one of these forms:
  - the Reverse archives (`.7z` files): the 3.00.00 base, which is required, plus any updates from 3.01.00 up to
    3.07.01. Updates are optional; each adds songs (base only: 331 songs, all updates: 395), or
  - an installed or unpacked game (a folder containing `WindowsNoEditor\Mercury\Content`).

  Lily contains no game files and cannot play without them. Use a copy you obtained legally.
- Windows 10 or 11 (64-bit) with a DirectX 12 or Vulkan GPU.
- A touchscreen is recommended. A mouse works as a single finger, which is enough for most charts but not chords.
- An audio device that can output 48 kHz.
- Disk space: about 7 GB for the files Lily uses, plus about 20 GB for background movies if you want them.
- Optional: FFmpeg for background movies (`winget install Gyan.FFmpeg`).

## 2. Getting Lily

Download the portable Windows zip from the project's Releases page and unzip it anywhere you can write to (not
`Program Files`). You get a `Lily/` folder; run `lily.exe` inside it. Nothing is installed, and you can delete the
folder to remove Lily. Your save stays in `%APPDATA%\lily\save.json`.

## 3. First-run setup

The first time you start Lily, it shows a setup screen instead of the title:

1. **Pick a folder.** Choose the folder that contains your Reverse `.7z` archives (3.00.00 and any updates), or the
   folder of an installed or unpacked game.
2. **Verify.** Lily checks that it found the expected files and version. If something is missing, for example an
   update archive, it tells you which one.
3. **Unpack.** From archives, Lily unpacks only the files it needs (charts, music, sound effects, textures, fonts,
   meshes, tables). Background movies are optional; leave them out to save about 20 GB and some time.
4. **Convert.** Lily converts what it needs into its own `data` folder (song list, jackets, fonts, meshes, UI
   textures). This takes a few minutes.
5. **Play.** Setup is done; Lily goes to the title screen. Later starts skip setup.

Your game archives or game folder are only read, never changed.

## 4. How to play

Notes travel from the centre outward. Hit them as they reach the ring at the edge. The ring has 60 lanes and 4
depth rings of sensors, like the cabinet. A note covers an arc, and touching anywhere inside that arc counts.

The touch area is a band from a little over halfway out to the edge of the screen; touches nearer the centre do
nothing. Turn on **TOUCH GUIDE** in the options to see it.

During play, the **II** button in the top-right corner (or Space) pauses. The pause menu has RESUME, RETRY and QUIT;
retry and quit end the play without saving it. The real game has no pause, only automatic GIVE UP settings.

Judgements are **Marvelous / Great / Good / Miss**. For touches the windows are ±50 / ±83 / ±100 ms. The other note
types use the game's own `JudgeParameterTable` values, which were measured on the real game
([doc 10](research/10-e3-judge-results.md)).

### Touch (and R touch)

Put a finger down on the note's arc in time. It must be a **new** touch: a finger that was already resting there
does not count, so lift and tap.

### Hold

A hold has a start note and a ribbon that may move around the ring.

- The hold is **judged on the start press**, like a touch: a fresh touch-down on the start in time. A finger that
  was already resting on the start when it arrives gives **Good**.
- Then **keep a finger on the ribbon** as it moves. Short slips are forgiven: a gap of up to **175 ms** keeps your
  start judgement. A longer gap drops the hold to Good, or to Miss if the start was also missed.
- **You don't need to release at the end.** The last 7 frames (about 117 ms) of the hold are not checked, so
  letting go slightly early is fine.

### Slide (arrows along the ring)

Swipe along the ring in the arrow direction. The slide is judged **once**, as soon as about **2 lanes** of movement
in the arrow direction registers. You don't need to trace the whole arc.

### Snap in / snap out (arrows to the centre / to the edge)

Move your finger **at least one sensor ring** inward (snap in) or outward (snap out). Moving the wrong way misses.

### Chain

Just be touching the arc while the chain passes. A **resting finger counts**; no new press is needed. Chains are
only ever **Marvelous or Miss**.

### R notes

R notes (with a glowing aura) are judged like their base type but are worth **double** in the score.

### Without a touchscreen

The left mouse button is one finger: click to tap, hold the button for holds, and drag for slides and snaps.

## 5. Score, grades and lamps

- The score is out of **1,000,000**, which means every note Marvelous. Marvelous is worth 100 % of a note, Great
  70 %, Good 50 %, Miss 0. R notes count twice. A hold counts once. There is no combo bonus.
- Grades: Master 1,000,000, SSS+ 990,000, SSS 980,000, SS+ 970,000, SS 950,000, S+ 930,000, S 900,000,
  AAA 850,000, AA 800,000, A 700,000, then B and C.
- **Clear** means the gauge ended at or above the chart's clear line. Lamps, from lowest to highest:
  - **Clear**: gauge cleared.
  - **Missless**: cleared with 5 misses or fewer.
  - **Full Combo**: no misses.
  - **All Marvelous**: every note Marvelous.
- Rating is calculated from your best 15 new and best 35 old charts, as in the game. Reverse Points (RP) are earned
  after every play.

## 6. Options

The options screen opens after you pick a song. Settings are saved when you start the song.

| Option | Default | What it does |
|---|---|---|
| SPEED | 4.0 | Note speed 1.0–6.0 in steps of 0.5. At 1.0, a note takes 3.33 s from the centre to the ring |
| TIMING | +0.0 | Judgement offset from −10.0 to +10.0 in steps of 0.1. One unit is 8.33 ms (±83 ms in total). Positive values mean you have to hit earlier: raise it if you are judged late |
| TIMING CHECK | | Opens the timing check (below) |
| MIRROR | OFF | Mirrors the chart left to right, both visually and for judging |
| AUTOPLAY | OFF | Perfect automatic play. Not saved; autoplay results are not recorded |
| BG MOVIE | ON | Shows the song's background movie (needs FFmpeg and the movie files) |
| BAR LINES | ON | Measure lines on the playfield, as in the real game |
| TOUCH GUIDE | OFF (ON on iPad and Android) | Shows the touch area during play: a bright inner circle where it starts (touches nearer the centre do nothing), the 4 depth rings, a line between lanes, and the sensors you are pressing lit up (not in the real game) |

**TIMING CHECK** plays a 120 BPM click track. Tap along (a tap anywhere, or Space) at least 16 times. Lily takes
the median of your errors, with outliers removed, and suggests a TIMING value. APPLY (or Enter) saves it; BACK (or
Esc) leaves without changing anything. The check measures audio latency, touch latency and your own timing
together, so do it with the same screen and speakers you play with.

Controls for every screen are listed in the [README](../README.md#controls).

## 7. Save file and advanced settings

The save is `%APPDATA%\lily\save.json` (a save from the old `wacca-slim` folder is migrated), or the path given
with `--save`. It is versioned JSON, written atomically, with the previous version kept as `save.json.bak`. If the
save can't be read, Lily doesn't overwrite it and plays without saving. It stores:

- `settings`: the options above.
- `records`, keyed `<songId>:<difficulty>`: best score and its judgement counts, best lamp, best grade, best combo,
  lowest miss count, and counters for plays, clears, lamps and grades. Every play counts, including give-ups.
- `reversePoints`: total RP earned.

These settings exist only in the save file (edit it while Lily is closed):

| Key in `settings` | Default | What it does |
|---|---|---|
| `menuTimers` | `false` | Show the arcade's menu countdowns and continue automatically when they run out (mode 30 s, song select 99 s, options 25 s, results 15 s) |
| `playerName`, `playerTitle` | `"PLAYER"`, `""` | Name and title on the results screen's player plate |

Environment variables:

| Variable | Effect |
|---|---|
| `WACCA_FFMPEG` | Path to `ffmpeg.exe`. Otherwise Lily looks on `PATH`, then in winget's install folder |
| `WACCA_PRESENT=immediate` | Disable vsync (uncapped frame rate, tearing) |
| `WACCA_PACE=0` | Turn off vsync frame pacing (Lily normally waits until shortly before the next refresh to cut input latency) |
| `WGPU_BACKEND`, `WGPU_POWER_PREF` | wgpu's GPU selection (`vulkan`/`dx12`, `high`/`low`) |
| `RUST_LOG` | Log level (default `info`). `RUST_LOG=lily=debug` logs every judgement |

## 8. Troubleshooting

| Symptom | Cause / fix |
|---|---|
| Setup says files are missing | Make sure the folder holds the base archive 3.00.00 (updates are optional), or a complete game folder with `WindowsNoEditor\Mercury\Content` |
| `error: audio: ...` at start | No default output device, or it can't play 48 kHz (there is no resampler). Enable a device, or set it to 48 kHz in Windows Sound settings |
| No background movies (log: `ffmpeg not found`) | Install FFmpeg (`winget install Gyan.FFmpeg`) or set `WACCA_FFMPEG` to `ffmpeg.exe`. Movies also need the movie files from setup |
| Song select is empty | The converted song list is missing. Run setup again, or, from source, run `wacca-convert` and pass `--data` |
| Flat 2D notes, plain colours or missing jackets | Converted meshes, UI textures or jackets are missing. Rerun the conversion with all its flags |
| Fullscreen is black, stutters or is slow (laptops with two GPUs) | Start with `--windowed` and press F11 later, or pick the GPU: `WGPU_POWER_PREF=high` and/or `WGPU_BACKEND=vulkan` (or `dx12`). F3 shows frame times |
| Notes judged consistently early or late | Run TIMING CHECK in the options, or set TIMING yourself |
| Chords don't work with the mouse | The mouse is one finger. Use a touchscreen |

## 9. Building from source

1. Install Rust: `winget install Rustlang.Rustup`, then `rustup default stable-x86_64-pc-windows-gnu`. Some
   dependencies are pinned for this toolchain; don't run a bare `cargo update`
   ([CONTRIBUTING.md](../CONTRIBUTING.md#toolchain)).
2. Copy the game's `WindowsNoEditor\Mercury\Content` folder to `assets\` in the repository root (gitignored).
3. Convert once:
   ```
   cargo run --release -p wacca-convert -- --assets assets --out data --textures --fonts --meshes --export-ui UI/Textures --export-ui Vfx/Textures
   ```
4. Play:
   ```
   cargo run --release -p wacca-app --bin lily -- --assets assets --data data
   ```

Command-line flags are listed in the [README](../README.md#command-line-flags).

## 10. Porting notes

Lily is built on cross-platform crates: **wgpu** (DirectX 12, Vulkan, Metal), **winit** (windows, touch, mouse,
keyboard) and **cpal** (audio). CI builds and tests it on Windows, Linux and macOS, but only Windows has been played
on.

- **Linux:** needs ALSA development headers to build (`libasound2-dev`) and a Vulkan driver. Touch arrives through
  winit (Wayland or X11). The save goes to `$XDG_DATA_HOME/lily/save.json` (or `~/.local/share/...`).
- **macOS:** uses the Metal backend. cpal's audio stream is not `Send` there; the app only uses it from the main
  thread. The UI uses the game's own fonts (exported by `wacca-convert --fonts`); the Windows system fonts used as a
  last fallback for a few symbols are simply skipped when absent.
- **Without Windows**, pass the game files on the command line (`--assets`, `--data`) and run `wacca-convert`
  yourself; the setup's folder picker is a Windows dialog, and the archive unpacking uses 7-Zip, which the portable
  build bundles only for Windows.
- **Windows-only pieces:** the folder-picker dialog (Win32 FFI), the 7-Zip binary in the portable build, and the
  real-game harness in `tools/realgame` (raw kernel32/user32 FFI, shared memory, WASAPI loopback recording). The
  harness compiles elsewhere but reports "needs Windows" for live use.
- **iPad:** an Xcode app around the same game, in [`ios/`](../ios/README.md). Setup there downloads a *Lily pack*
  (`assets/` + `data/` made on a desktop with `tools/make-pack.sh`, shared with `tools/serve-pack.sh`) from an
  address you enter; `--setup-url <address>` does the same on desktops (not Windows). No background movies (no
  `ffmpeg` process on iOS; they would need an in-process MPEG-1 decoder).
- **Android:** a `NativeActivity` app around the same game, in [`android/`](../android/README.md), with the same
  pack download as iPad (address typed into a native dialog). Vulkan, with OpenGL ES as a fallback; audio through
  AAudio (cpal). No background movies yet.
