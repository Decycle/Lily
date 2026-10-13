# Autonomous run goals (started 2026-10-12 evening)

Priority: **1) play area — fully faithful visual + control port**, 2) menus, 3) everything else.

## Run rules (all agents)
- The laptop is free: launching the real game, recording, synthetic keyboard/mouse are allowed.
- **Live real game = one at a time.** Before launching it, atomically create `runs/LIVE.lock` in the MAIN checkout
  (`C:\Users\<user>\OneDrive\Documents\WACCA\WACCA_slim\runs\LIVE.lock`, content: agent name + time); if it exists and is
  younger than 45 min, wait. Delete it when your live session ends. Leave no game/amdaemon/ffmpeg/RenderDoc running.
- Never write to or delete the main checkout's `data/`, `assets/` or the user's save (`%APPDATA%\wacca-slim`).
  Use your worktree's own `data/` (export with `wacca-convert`) and a temp save file (`--save`).
- `CARGO_BUILD_JOBS=2`; keep pins (CONTRIBUTING.md).

## 1. Play area: visuals
- [x] Extract + disassemble the game's compiled shaders (d3dcompiler_47 `D3DDisassemble`), map shader → element (doc 11 §1)
- [x] RenderDoc capture of a real gameplay frame (draw order, blend states, constants, meshes) — 16 captures, exact camera/transforms, port spec (doc 11 §2–3)
- [x] Real note meshes in our renderer (601 meshes; real camera; ported note shader; bar lines) — ring/lanes in progress
- [~] Port key shaders to WGSL — done: notes (all types, R aura, sync band), hold ribbons + end caps, chevrons, snap arrows, guide lanes + ring, movie, colour pipeline (1–3/255 vs RenderDoc frames); sync connector (opaque, from assets + KING frames); in progress: hit effects
- [ ] Side-by-side audit vs real frames: every difference fixed or listed

## 1. Play area: controls
- [x] E3: real judgement windows per class, hold drop, slide/snap thresholds (doc 10; chord rule + snap-out + Hard not measured)
- [x] Apply E3 to the judge engine (175 ms drop, HoldEnd free window, missed+dropped = Miss; autoplay perfect on all 1,247 charts, mirrored too)
- [~] KING capture: Miss / snap / R / bonus references captured (runs/e2/20261013-043842); matching in progress
- [x] Performance + latency (dGPU, 120 Hz, low-latency audio, input timestamps) — frame pacing: display wait 7.0→1.5 ms/frame; async image decode; F3 overlay
- [x] Timing calibration screen (unit-tested; live tap check blocked by permission system → user to try by hand)

## 2. Menus
- [x] Menu scale = play-ring scale on landscape (circle-matched; off-circle UI uses side space) — menu circle 331→581 px radius on 1920x1200
- [x] Remaining details (Reverse Points from WaccaPointTable — matches real results; user plate; menu timers off by default; title halftone/curved text/glitch approx)

## 3. Finish
- [x] README (how to play, controls, options, troubleshooting, accuracy)
- [ ] Final release build on the desktop launcher + report (done / measured vs estimated / left)
