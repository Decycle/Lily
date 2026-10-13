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

KING's centre/inner numbers are the missing no-movie background (§4.1).

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

1. **No-movie background** (KING centre ≈ 90 levels; most songs have no movie). The game renders a background
   scene through `BgEffectSceneCapture` into a 1080² texture (own bloom, tonemap, FXAA) and draws it on the far
   quad (`BgCaptureTextureRenderTarget2D_Mat`). States (CONFIRMED by captures): normal = `vb_BGS300_00_lp`
   (`vMX_3_BGN03_000/002`, `vMX_3_BGBoss_004_MUD_m_Clp000`; blue grid), Good = `vb_BGS200_00_lp` (`vMX_3_02BGS_*`,
   `vM_1_SAUV_000_MUD_m_02BGS00`; red crystals). KING switches between 64 and 66 s of perfect play (MEASURED:
   gauge reaching the norma, UNVERIFIED), with `vp_bgcup000_00`/`vp_bgcdown000_00`. Other sets (`BGN*`, `BGB*`
   boss, `BGC*` climax) not yet seen. Needs: the capture camera, ~6 materials, bloom, the state rules — a
   separate port.
2. HUD banners not drawn: "BONUSGET" (`BP_Game_bonus`), end-of-song "Missless!/Full Combo" (`uT_FTclear_*`),
   the credit track counter "1/3" on the rim (no credits in our app), the COMBO word's two 0.3-alpha colour
   copies (`combo1`/`comob2` images).
3. R-note effect timing/size ±1 frame; Miss shard onset relies on the replay's popup times (E3 `shown`).
4. CREDIT/COIN overlay at the top of the real frames is the cabinet's free-play overlay, not the play view.
