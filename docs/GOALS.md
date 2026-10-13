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
- [ ] Extract + disassemble the game's compiled shaders (d3dcompiler_47 `D3DDisassemble`), map shader → element
- [ ] RenderDoc capture of a real gameplay frame (draw order, blend states, constants, meshes)
- [ ] Real note/ring meshes in our renderer
- [ ] Port key shaders to WGSL (notes, holds, chevrons, ring/rim, hit effects, movie overlay, post-process)
- [ ] Side-by-side audit vs real frames: every difference fixed or listed

## 1. Play area: controls
- [ ] E3: real judgement windows per class, hold drop, slide/snap thresholds, chord rule
- [ ] Apply E3 to the judge engine (autoplay stays perfect on all charts, mirrored too)
- [ ] KING capture: Miss / snap / R / bonus effect references, then match them
- [ ] Performance + latency (dGPU, 120 Hz, low-latency audio, input timestamps)
- [ ] Timing calibration screen

## 2. Menus
- [ ] Menu scale = play-ring scale on landscape (circle-matched; off-circle UI uses side space)
- [ ] Remaining details (Reverse Points, player level/title, countdowns, title glitch)

## 3. Finish
- [ ] README (how to play, controls, options)
- [ ] Final release build on the desktop launcher + report (done / measured vs estimated / left)
