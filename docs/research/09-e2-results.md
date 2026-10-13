# 09 — Experiment E2 results (real song, perfect input, playfield constants)

Design: doc 07 §7 (E2). Harness: `tools/realgame` (`e2_script`, `run_experiment --sync-chart`, `analyze_e2`). Game:
the user's install, 675×1200 window, guest play, offline. All runs 2026-10-12, fully automatic (launch → guest
navigation → song → results → stop). Run directories are under `runs/e2/` of the worktree (gitignored, game images
never committed).

## 0. Summary

| Question | Answer |
|---|---|
| Song / chart | **フラジール (1041), Normal Lv 1**, 127 BPM, 145 notes, all ≥ 0.236 s apart (121 touch, 12 holds, 8 chains, 4 slides), first note 9.45 s, has a background movie. Reachable for a guest without unlocks. |
| Song-time sync (loopback audio vs our decoded BGM) | Live detection works every time: coarse r 0.82–0.93, waveform r **0.993–0.994**; offline re-alignment equals the live one to < 0.01 ms; residuals at 25/50/75 % of the song **0.0 ms** (r 0.96–0.98). Found 3.59 s after BGM sample 0 (first input at 9.45 s). |
| Real-game result for perfect injected input | With every edge sent **110 ms before** its audio-derived chart time: **145/145 Marvelous, 1,000,000, MASTER, FAST/LATE 0/0 — twice** (runs `20261012-234943`, `-235435`). With 0 ms lead: 11 M / 1 Gr / 16 Gd / 117 Miss (input ~100 ms late). With 50 ms lead: 14 M / 113 Gr / 18 Gd / 0 Miss, FAST 97 / LATE 34 (see §3: a 115 ms clock jump mid-song). |
| Approach time (note spawn → judgement line) | **2.184 s at the guest speed 1.5 → 3.276 s at speed 1.0** (3 runs: 3.2751 / 3.2768 / 3.2765). Our current constant: 3.3 s (UNVERIFIED) — 0.7 % too long. |
| Perspective constant `k` in `r = (1-k)x/(1-kx)` | **0.750** (3 runs: 0.750 / 0.750 / 0.750; 0.748 in the disturbed run). Ours: 10.5/13.825 = 0.7595. Fit RMS 1.25–1.30 px over ~20,000 samples of 121 notes. |
| Lane 0 / direction | **Confirmed**: lane 0 starts at 3 o'clock, lanes counter-clockwise. Arc-centre offset −0.003…−0.008° (sd 1.0–1.5°, n ≈ 9,000 per run) from our convention. |
| Playfield geometry in the 675×1200 window | Centre **(337.5, 565.3)**, rim inner edge (judgement line) **291.8 px** (4 runs, ±0.05 px); rim band 292–315 px. |
| Visual vs audio clock | Notes reach the line **87–92 ms before** their loopback-audio chart time (frame stamps; display is a few ms earlier still). Same order as the 110 ms input lead that gives all Marvelous: on this PC the game's frame/judge clock runs ~0.1 s ahead of its audible audio. |
| Result files | The game writes **no PlayLog** offline: only `CreditLog.csv`, `PayingLog.csv`, `UpdateChecker.csv` (+ backups/hashes) change. Results are read from the result screen and from the always-on debug panel under the playfield (per-judgement counts incl. R notes, FAST/LATE, combo, score, hi-speed). |

## 1. Method

### 1.1 Perfect input (`e2_script`, `perfect.rs`)
`wacca_game::judge::autoplay_frames` → per-cell contacts → each contact held ≥ 34 ms (E1 minimum 17 ms × 2), keeping
≥ 17 ms to the same cell's next contact; a contact that hands over to another one at its end (snap/slide first
phase, moving hold finger) is lengthened backwards so the motion still happens on the note time. Written as
`scripts/e2-fragile-normal.taps.json` in chart time (165 taps). Played through our own judge it scores 1,000,000.

### 1.2 Live song start (`sync.rs`, `run_experiment --sync-chart`)
Chosen approach (doc 07 §3.2): detect the song start live from loopback audio. The loopback stream (with WASAPI
QPC capture stamps per packet, `audio_packets.csv`) is matched against our decoded BGM: Pearson correlation of a
1 ms energy envelope of the first 1.5 s of sound (coarse, threshold 0.6), confirmed 200 ms later and refined with a
250 ms waveform template (±6 ms, must reach r ≥ 0.5). Edges are then scheduled at
`bgm0 + (chart_t − #OFFSET) − input_lead`. Detection costs ~3.6 s after BGM sample 0, so the first input must be
later than that (Fragile: 9.45 s). `#OFFSET` is 0 for this chart, so its sign is still untested.

### 1.3 Navigation (`scripts/e2-guest-fragile.nav.json`, new `touch` and `seek` steps)
Coin → START (touch lanes 44–46) → guest dialog YES → tutorial dialog NO (lanes 40–41) → slide help SKIP → mode
select GO → category select: 4× right arrow (lanes 49–50) to TOUHOU, GO → `seek`: left arrow until the centred
jacket matches `data/jackets/S01/uT_J_S01_041.png` (8×8 cell-mean difference ≤ 30; correct ≈ 3–6, wrong ≥ 40) →
GO → options screen (small jacket check) → `scripts/e2-start.nav.json` (GO) after recording has started. Screen
checks are colour rules (dialog YES button, category bar) or jacket matches. Worked 4/4 times after the script was
finished. Pitfalls found: a GO touch wider than 3 lanes also hits the scroll arrows; every menu has a timer that
auto-advances. Expert/Inferno are locked for guests (checked on うっせぇわ).

### 1.4 Analysis (`analyze_e2`)
Frame chart time = `(first_frame_ms + pts − bgm0_ms)/1000 + #OFFSET` (FFmpeg wall-clock stamps, no capture-latency
correction). Rim: longest saturated run 250–340 px out on 120 rays → circle fit. Notes: isolated touch notes
(no other note on the centre lane within ±0.4 s) in the lower half; note-colour runs along the lane-centre ray;
outer edge vs time-to-hit fitted with `x = 1 − (τ+δ)/T`, `r = R(1−k)x/(1−kx)` (`R` = rim inner edge; the curve
has only 3 degrees of freedom, so `R` is measured, not fitted). Run matching uses a tolerance relative to the
expected radius and a δ pre-search, then two passes with outlier rejection. The same pipeline on **our** playfield,
rasterised on the same geometry (`raster.rs`), recovers T = 2.191 s (true 2.200) and k = 0.759 (true 0.7595) with
0.4 px RMS — the method is accurate to ~0.4 %; correcting the real T by that bias gives 3.29 s at speed 1.

Rim consistency: the first analyses reported centre y 536 / radius 321.5 px. That detector only accepted the
bluish bottom part of the rim and fitted a circle to the lower half only; the rim's colour changes from blue
(bottom) to pink (sides). The fixed detector gives (337.5, 565.2–565.3) / 291.8–291.9 px in all four song runs.

## 2. Runs

| Run | Input lead | Sync coarse / fine r | Result | δ (visual − audio) | T @1.5 / k / RMS |
|---|---|---|---|---|---|
| 20261012-234221 | 0 ms | 0.923 / 0.994 | 11 M, 1 Gr, 16 Gd, 117 Miss, LATE 5 | −91 ms | 2.183 s / 0.750 / 1.30 px |
| 20261012-234943 | 110 ms | 0.923 / 0.994 | **145 M**, AM, 1,000,000 | −92 ms | 2.184 s / 0.750 / 1.26 px |
| 20261012-235435 | 110 ms | 0.933 / 0.993 | **145 M**, AM, 1,000,000 | −87 ms | 2.185 s / 0.750 / 1.25 px |
| 20261012-235920 | 50 ms | 0.925 / 0.993 | 14 M, 113 Gr, 18 Gd, FAST 97 / LATE 34 | −85 → **+30 ms** | (2.272 s / 0.748 / 6.0 px, invalid) |

Per-segment δ (20 notes each) is −80…−90 ms throughout the clean runs; in the 50 ms run it jumps from −85 to +30 ms
between notes 40 and 60 (≈ 30–40 s) and stays there. Before the jump the taps were ~60 ms late (LATE Greats), after
it ~55 ms early (FAST Greats), which explains the counts and confirms FAST = early. The machine was idle; this is a
discontinuity between the game's audio and its frame/judge clock (e.g. an audio hiccup). Consequence: audio-only
sync can be off by > 100 ms after such an event; the visual phase (δ) must be checked per run.

## 3. Constants vs our current values

| Constant | Ours (`wacca-render/src/layout.rs`) | Measured | Note |
|---|---|---|---|
| `APPROACH_SECS` (speed 1.0) | 3.3 (UNVERIFIED) | 3.276 s raw, 3.29 s bias-corrected (spread 0.002 s) | between the community 3.2667 and 3.3333; 3.28 recommended |
| perspective `k` | 10.5/13.825 = 0.7595 | 0.750 (spread < 0.001) | position error of the old value ≤ 2 px at 292 px |
| lane 0 / direction | 3 o'clock, CCW | same (±0.01°) | remove the UNVERIFIED note in `layout.rs` and `cells.rs` |
| judgement line | radius 1 | rim inner edge, 291.8 px; rim band to 315 px | real rim is a thick magenta band, ours a thin white line |
| default guest options | — | speed 1.5, mask off, timing 0, give-up off | from the options screen |

## 4. Side-by-side (`runs/e2/<run>/compare/NN-<t>.png`, real | ours)

10 moments per run, ours rasterised on the measured centre/radius at the frame's chart time. Note radii agree
within a few px (as the fit says). Visible differences: the real rim is a thick glowing magenta band (ours: thin
white ring plus sensor guides); real notes are thicker with cyan end caps; masked lanes show the movie/background
panel in the real game (ours: black) and the real game draws no dark guide-lane sectors; the movie fills the circle
behind a translucent tunnel; the combo counter ("65 COMBO") is small and centred above the middle; a horizontal
bar near the top centre and a score row with "o" digits run along the top rim.

## 5. Reference set for the visual-parity audit

Every analysed run has `reference/NNNN-<label>.png` (full 675×1200 frames, nearest captured frame) plus
`reference/frames.csv` (`target,label,chart_t_target,frame,pts_s,chart_t,file`) and `frames.csv` for every video
frame (`frame,pts_s,chart_t`); the full 120 fps lossless `video.mkv` is kept. Targets (`reference.rs`): every 2 s,
the song start and end (+1 s, +4 s), the first two hits of each note kind at −200…+800 ms (approach, hit effect,
judgement popup appear/fade; holds also mid-body and end), and combo 10/50/100/145. Best set:
**`runs/e2/20261012-234943/reference/`** (all Marvelous, 175 frames); `-235435` is the repeat.
Fragile covers touch, slide cw/ccw, hold, chain. **Not covered:** snap in/out and bonus/R notes — the only reachable
charts with them need more navigation (うっせぇわ Hard lacks snaps; Expert/Inferno are locked for guests). A
candidate is ぺこみこ大戦争!! Normal/Hard (FOR BEGINNERS, 1 step right of the default cursor: snap in, bonus) — one
more live run.

## 6. Recommended code changes

1. `wacca-render/src/layout.rs`: `APPROACH_SECS = 3.28`, perspective `k = 0.750` (keep the form
   `r = (1−k)x/(1−kx)`), drop the UNVERIFIED notes on approach time, curve form and lane 0; same for `cells.rs`.
2. Visuals (for the parity audit): thick rim band instead of the thin white ring; no dark guide sectors (masked
   lanes reveal the background); smaller centred combo text.
3. Harness: after live audio sync, also lock to the visual phase (δ from note positions) — audio↔game offset is
   ~−90 ms here and can jump by 115 ms mid-song. A live variant of the note fit (a few notes suffice) could correct
   the injector during the song.
4. The default player timing offset of our app should not try to model this PC's 0.1 s A/V skew; it is a property of
   the real game's audio path on this machine, not of WACCA.

## 7. Open items

- Snap/bonus/R reference frames (one more run, §5).
- `#OFFSET` sign (needs a chart with a non-zero offset).
- Judgement window edges vs frame quantisation: a δ sweep run (doc 07 §5.2) now only needs per-note readout of the
  debug panel or popups; the panel's FAST/LATE counters make a coarse sweep possible from totals alone.
- Hold/chain/slide rules were all Marvelous with autoplay-shaped input (34 ms contacts, ring 0 taps, ring 3 hold
  finger) — consistent with our rules but not a test of their edges.

- After merging main (textured notes, sprites), `raster.rs` still draws every vertex as a flat colour with an empty
  `SpriteSheet` (UVs and additive/mask flags ignored). The numbers above were measured on the pre-merge flat
  renderer; the side-by-sides should move to a headless wgpu render of the real renderer.

## 8. Reproduce

```
cargo build --release -p wacca-realgame
e2_script --chart <assets>\MusicData\S01-041\S01-041_00.mer --out tools\realgame\scripts\e2-fragile-normal.taps.json
set WACCA_INJECT_OK=1 & set WACCA_ASSETS=<assets>
run_experiment --script tools\realgame\scripts\e2-fragile-normal.taps.json --nav tools\realgame\scripts\e2-guest-fragile.nav.json
  --start-nav tools\realgame\scripts\e2-start.nav.json --sync-chart <assets>\MusicData\S01-041\S01-041_00.mer
  --input-lead-ms 110 --tail-ms 30000 --out-dir runs\e2        (run from a directory with data\jackets)
analyze_e2 runs\e2\<ts>                                         (e2.json, compare\, reference\, result-*.png)
```
