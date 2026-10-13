# 13 — Play-area visual parity audit

Final side-by-side audit of the play view against the real game, after the ground-truth ports of docs 11–12.
Confidence as in doc 11: **CONFIRMED** = read from assets or RenderDoc captures, **MEASURED** = fitted on recorded
frames, **UNVERIFIED** = inference.

## 1. Method

`tools/research/parity_audit.py <run> <out>` renders our frame for every reference frame of an E2/E3 run and
compares (`stagecmp compare`: real 675² view crop | ours | |diff|×4, mean |diff| per 25 px radius band). Ours is
the `playfield` example's `WACCA_SHOT` with:

* the run's picture lead (`e2.json` `visual_timing`, ~0.09 s, interpolated per block of notes) minus 33 ms:
  analyze_e2's 2D note-edge fit overestimates it; our 3D notes line up best 33 ms earlier (note-annulus offset
  fit, KING −40/−30 ms, Fragile −20…−50 ms; MEASURED),
* `WACCA_SHOT_REPLAY`: the injected touches (`inject.jsonl`) and, for E3 runs, the real per-note judgements
  (`e3.json`); touches are moved by the game's input latency on that clock (0.025 s: the touched-sensor
  rim highlight appears 0.025 s after the edge's script time), hit effects at their script times,
* the song's movie frame behind (planes cut from `assets/Movie/mv_<id>.usm` at the movie time).

Regions (px from the view centre; the ring band is 292–315 px): centre 0–100 (combo, judgement art), inner
100–200, outer 200–275, ring 275–325, rim ≥ 325 (incl. the black corners). Reference sets: E3 Fragile
`runs/e3/20261013-034614` (87 frames, every 2nd), E2 KING `runs/e2/20261013-043842` (70 frames, every 3rd, no
movie). "Before" = commit `8f95d2d` with the same replay; "after" = this branch.

| set | centre | inner | outer | ring | rim | all |
|---|---|---|---|---|---|---|
| Fragile before | 7.46 | 5.06 | 5.02 | 17.42 | 2.77 | 7.15 |
| Fragile after | 5.44 | 2.82 | 2.84 | 7.77 | 0.70 | **3.45** |
| KING before | 94.08 | 32.50 | 15.55 | 20.31 | 2.18 | 21.82 |
| KING after | 92.57 | 30.03 | 14.05 | 11.59 | 0.79 | **18.65** |

KING's centre/inner numbers were the missing no-movie background, since ported (§5: KING all 18.65 → 7.68).

New RenderDoc captures (KING Normal, guest, LIVE.lock protocol, worktree `runs/rdc-king/`, not committed):
frame 9800 (R-note hit), 9103 (held hold), 9574 / 11389 (normal background), 13214 (Good background), dumps
`d<frame>/` (+ `p9800.json` particle vertices).

## 2. Fixed

| element | before | after | status / evidence |
|---|---|---|---|
| Lanes at the song start | song-start frame all 43.9 | 1.4 | fixed: mask events at ≤ 0 s are in place from the lead-in (Fragile's add-all at 0 s shows every lane before the song; we swept 0.25 s). Mid-song sweeps MEASURED at 1 lane / (1/120 s) as before. |
| HUD rim outside the ring | band 300–325 23.8, 325+ 7.35 (Fragile) | 8.3, 1.85 | fixed: no opaque dark rim; the 3D view shows through to 331 px. The time gauge (`BP_Game_time` → `uM_RGguage000`, CONFIRMED: black × 0.5, band `|d − 0.96| < 0.01`) darkens the unplayed part ×0.55 at 321.7–327.9 px; black beyond 330.75 px; HUD centre 2 px above the 3D centre (MEASURED, edges ±2 px top/bottom). Drawn in the composition in display values. |
| Held hold ribbon colour | Fragile 0084 (hold body) all 9.1 | 3.0 | fixed: held holds use `vMX_2_Hold_004` (the exe loads Hold_001/004/002; 004 = tile screened with itself ×1.3·(1.2 − 0.2(w/430)²), α 0.3–0.6, cut below w 197): yellow as in the game. `Hold_002` (desaturated) not used (UNVERIFIED: dropped state). |
| Ring under the key beam | Fragile 0084 ring 17.8 | 8.8 | fixed: the `vMX_2_Tap_000` line cut is `round_pi` (ceil), not round → the beam has a gap over the ring, which stays magenta. (UE separate translucency + recombine was checked: no later ring draw.) |
| Key beam / rim highlight timing | 0.3 s beam fade, 0.2 s rim | 0.17 s, both | fixed (MEASURED): beam and rim go out together, at once, 0.169–0.175 s after the release (25 ms tap at 9.38 s, hold release at 68.97 s). |
| Rim highlight look | edge 1.076–1.09 + grey band | 3 bands | fixed (MEASURED): white 314.7–322.3, grey (128) 322.3–328.8, white 328.8–330.3 px around the HUD centre. |
| Hold-end hit effect | not drawn | drawn | fixed: holds pop over their end arc when judged (Marvelous at 69.0 s, Great at 98.3 s in Fragile); app `hit_arc`. Fragile 0160 all 5.8 → 3.2. |
| R-note hit (`vb_SnoteHit_00`) | not drawn; KING 0120 inner 60.7, all 25.2 | 17.3, 12.0 | fixed (CONFIRMED frame 9800): component at (0, −610, 0), unrotated, drawn before the guide lanes; `ParticleModuleParameterDynamic` parsed; `vM_1_Base_003` and `vMX_3_RankUp_000` ported. Grid size, colours and wave match; remaining diff is the background behind it. |
| Camera-facing sprite orientation | v up | v down | fixed (CONFIRMED frame 9800 eid 2782 UVs/corners): UE's sprite up is the view's down, rotation clockwise on screen. |
| Edge softness (TAA vs 4x MSAA) | edge px 20.4 | 15.8 | fixed: 3×3 Gaussian σ 0.55 in the composition (fitted on frame 12365, minimum at σ 0.45–0.6); 9 texel loads per pixel. Fragile all 4.80 → 4.32 with it. |
| Hold-held glow brightness | flicker mean 0.4 | 0.6 | fixed (CONFIRMED frame 9103 eid 4008): flicker = Max + (Min − Max)·sin; side-mask uniforms confirmed (U_pos 0.46, axis (1, 0)). |
| Clear gauge | translucent mauve bar, diamonds | opaque bar | fixed (MEASURED per pixel): 5 px, border rows, track (42,45,57), fill (255,149,218), 2 px norma mark + 18 px fade, slanted caps, 1.3 px lower. |
| Combo digits | dark outline | none | fixed: the game's digits have no outline. Fragile 0036 centre 4.5 → 2.6. |
| Bonus shard spread | scaled offsets: shards reach the centre | 180–290 px | fixed (MEASURED, KING bonus frames): world-space sprite emitters (`vo_btap000_*`, Bhitpt, Miss) place particles without the component scale (UNVERIFIED in UE terms); bonus frames outer 10.6 → 10.3. |
| R-note aura blend | additive | SrcAlpha/InvSrcAlpha | fixed (CONFIRMED frame 10405 eid 3835): colour × red, opacity sat(red); no longer brightens the ring under it. |

## 3. Checked, no change

* Mask sweep speed and direction: 1 lane per 1/120 s, first lanes change ≤ 1 game frame after the event (MEASURED,
  Fragile 22.68 s remove). The earlier "sweep state at capture times" difference was the song-start case.
* Hold sparks: lengths match the KING hold body frame within a pixel or two once the glow is right (local-space
  emitters keep the 1.7 scale, validated on frame 9009 in doc 12).
* Judgement art and COMBO word position/size: within 1 px (Fragile 0036/0017).
* Sync connector: no darker centre visible at 675 px after the TAA filter (KING 39.04 s R pair).

## 4. Remaining

1. ~~No-movie background~~: ported, see §5. Left: the state-change bursts (`vp_bgcup000_00` /
   `vp_bgcdown000_00`, the yellow triangles at the switch), the end-of-song states (`ClimaxEffect`,
   `FullCombo`, `Missless`, `AllMarvelous` → `vb_BGC*`, `vp_Clear00*`), Inferno (`vb_BGB*`) and multiplayer
   sets, FXAA on the capture.
2. HUD banners not drawn: "BONUSGET" (`BP_Game_bonus`), end-of-song "Missless!/Full Combo" (`uT_FTclear_*`),
   the credit track counter "1/3" on the rim (no credits in our app), the COMBO word's two 0.3-alpha colour
   copies (`combo1`/`comob2` images).
3. R-note effect timing/size ±1 frame; Miss shard onset relies on the replay's popup times (E3 `shown`).
4. CREDIT/COIN overlay at the top of the real frames is the cabinet's free-play overlay, not the play view.

## 5. No-movie background scene (`wacca-render::bg`, `bg.wgsl`)

Sources: `Map/RhythmGame.umap` (install `Content`, read with `cargo run -p wacca-ue4 --example dump_objects`),
the RenderDoc KING frames 11389 (Normal) and 13214 (Good; dumps `runs/bg/d11389|d13214`, raw targets via
`runs/bg/rdc_raw.py`, D3D11 pixel debugging via `runs/bg/rdc_debugpix.py`), the E2/E3 videos.

**Level** (CONFIRMED, `RhythmGame.umap`): one `BackGroundEffectActor` per state, each a `ParticleSystemComponent`:
Normal `vb_BGS300_00_lp`, Good `vb_BGS200_00_lp` (both at (0, −10000, 10000), yaw 180°), MultiNormal
`vb_BGS300_01_lp`, Bad `vb_BGC001_00_lp`, Climax `vb_BGC002_00_lp`, InfernoNormal/Good `vb_BGB001/000_00_lp`,
Up/Down `vp_bgcup000_00` / `vp_bgcdown000_00` (0, −410, 10000), Opening `vp_stgame000_00`, FullCombo /
AllMarvelous / Missless `vp_Clear00*`. The systems have `WarmupTime` 8 s. `BgEffectSceneCapture`: a
`SceneCapture2D` at (0, 190, 10000) looking down −Y, `FOVAngle` 45°, `SCS_FinalColorLDR` into
`cTRT_BackgroundEffect`; `BGNormal_PP` / `BGGood_PP` / `BGClimax_PP` only override depth of field (no DOF pass in
the captured frames).

**Capture** (CONFIRMED, frame 11389 eids 650–1003): depth prepass, base pass of the active system, bloom chain
(computed, unused), then a tonemapper permutation that only multiplies by the exposure (1.0) and writes linear
colour into a B8G8R8A8 UNORM target (so: `saturate`, 8-bit), FXAA into the 1080² R16F `91966`. The play view draws
it on the `BgCaptureTextureRenderTarget2D_Mat` quad (±811 cm at Y = −621, eid 2783: emissive = capture colour) —
the movie's place; the final `pow(1/2.2)` then applies as for the movie. Guide-lane fade without a movie:
`PercentageAlpha` 600, `WidthGradation` 3 (eid 3314; was UNVERIFIED).

**Particles** (`wacca-ue4` / `wacca-render::particles` additions): `EmitterOrigin` (backdrop at local Y 1000 →
w 11190, NDC ±0.9385 — eid 677's clip coordinates), `WarmupTime` (0.032 s steps), `ParticleModuleAcceleration`,
`ParticleModuleMeshRotation` (CONFIRMED by the cube instance rows: applied with the opposite sign to
`RollPitchYawRange`), cylinder start velocity = outward offset × scale (MEASURED: the triangles' spread and the
R-note sparks of frame 9800), mesh vertex colours (the cubes' WPO weights).

**Materials** (ported from the base-pass PS disassembly with the captured cbuffers, annotated asm in
`runs/bg/d*/annot`): Normal `vMX_3_BGN03_000` backdrop sprite (radial glow, beat-flickering triangle pattern,
rotating light layer, dark rim by the bar, tile grid, moving line layers), `vMX_3_BGN03_002` ring sprites,
`vMX_3_BGBoss_004` dotted lines; Good `vMX_3_02BGS_002` tunnel walls, `vMX_3_02BGS_000` cube clusters (with their
`WorldPositionOffset` pulse; the compiled VS reads the MPC Beat for both its bar and beat terms),
`vM_1_SAUV_000` triangles. The Good walls and cubes are default-lit: the pixel debugger (eid 647) shows no
indirect light (volumetric lightmap zero), no reflection, only the level's directional light (towards +Y,
intensity 1.1): `emissive + (base/π + GGX(roughness 0.5, F0 0.04)) · 1.1 · N·L` with base = saturate(emissive),
ported exactly (face normal from derivatives).

**State** (MEASURED on the videos, `runs/bg/bgtrace.py` + `states.py`): Normal from the start, Good from the
first judgement at which the clear gauge reaches the norma: KING at 65.06 s (our gauge 0.4534 at 64.34 s, 0.4585
at 65.06 s), song 31 (E3 `035555`) at 91.9 s (ours 0.4537 at 91.61 s, next judgement 91.90 s) — so the real gauge
runs slightly below our model's (UNVERIFIED gauge points, `wacca-game` unchanged). Rule implemented:
`gauge ≥ norma` (`BgState::of_gauge`), switching instantly (the video changes within one 120 fps frame);
switching back on a drop is UNVERIFIED (the level's `Down` actor suggests it). The materials' MPC
`MPC_BpmSignal` Bar / Beat are taken as the measure / beat phase (UNVERIFIED: the captured values do not fit
one phase), `View.GameTime` as the chart time.

**Audit** (`parity_audit.py --bg-good-at S`, the switch time from the run's video; "before" =
`--env WACCA_SHOT_BG=off`, i.e. the previous black background):

| set (frames; Good from) | part | centre | inner | outer | ring | rim | all |
|---|---|---|---|---|---|---|---|
| KING E2 `043842` (70; 65.1 s) | before, Normal part | 83.57 | 24.40 | 11.21 | 11.38 | 0.66 | 16.09 |
| | after, Normal part | 11.40 | 8.35 | 8.92 | 11.30 | 0.67 | 7.22 |
| | before, Good part | 128.72 | 52.43 | 25.63 | 12.47 | 1.30 | 28.94 |
| | after, Good part | 16.56 | 13.85 | 11.78 | 11.37 | 0.95 | 9.51 |
| | **before → after, all** | 92.60 → 12.44 | 30.01 → 9.45 | 14.09 → 9.49 | 11.59 → 11.31 | 0.79 → 0.72 | **18.66 → 7.68** |
| song 31 E3 `035555` (76; 91.9 s) | before → after, Normal | 116.56 → 10.19 | 68.63 → 8.21 | 48.10 → 14.63 | 42.29 → 19.13 | 2.36 → 1.08 | 43.43 → 10.24 |
| | before → after, Good | 157.65 → 19.67 | 105.62 → 21.12 | 86.25 → 21.40 | 69.55 → 23.64 | 7.12 → 2.32 | 70.21 → 16.50 |
| | **all** | 123.05 → 11.69 | 74.47 → 10.24 | 54.12 → 15.70 | 46.59 → 19.84 | 3.12 → 1.28 | **47.65 → 11.23** |
| 3048 E3 `030310` (107; 99.6 s) | before → after, Normal | 84.11 → 11.14 | 29.65 → 9.86 | 18.24 → 13.47 | 16.08 → 14.28 | 1.11 → 1.01 | 20.03 → 9.34 |
| | before → after, Good | 132.73 → 21.53 | 64.15 → 19.87 | 49.68 → 27.83 | 38.83 → 25.76 | 3.88 → 2.79 | 43.70 → 18.52 |
| | **all** | 98.65 → 14.24 | 39.97 → 12.85 | 27.64 → 17.76 | 22.89 → 17.72 | 1.94 → 1.54 | **27.11 → 12.08** |
| 3048 E3 `042834` (107; 86.3 s) | before → after, Normal | 85.96 → 10.54 | 29.51 → 8.39 | 18.06 → 11.52 | 16.17 → 13.47 | 1.04 → 0.89 | 20.09 → 8.32 |
| | before → after, Good | 128.06 → 20.85 | 62.59 → 18.29 | 43.72 → 21.71 | 30.95 → 19.36 | 3.40 → 2.28 | 39.83 → 15.17 |
| | **all** | 102.10 → 14.49 | 42.18 → 12.18 | 27.89 → 15.42 | 21.84 → 15.73 | 1.94 → 1.42 | **27.65 → 10.94** |

(Every 3rd reference frame; the E3 runs' outcomes without a real judgement are left to the replay's
default — `parity_audit.py` crashed on them before.)

Remaining differences: particle phases (positions are random; the Good cube clusters pass at other times),
the Good triangles are drawn ~1.8× the captured size (GPU sprite size, UNVERIFIED), FXAA, and the end-of-song
states (§4.1).
