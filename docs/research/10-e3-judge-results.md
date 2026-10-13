# 10 — Experiment E3 results (judgement rules of the real game)

Design: doc 07 §5.2, doc 04 §11. Harness: `tools/realgame` (`e3_script`, `run_experiment`, `analyze_e2` for the
timing check, `analyze_e3`). Game: the user's install, 675×1200 window, guest play, offline, default guest options
(speed 1.5, timing 0). All runs 2026-10-12/13, fully automatic. Run directories: `runs/e3/<run>/` of the **main
checkout** (gitignored). Every input went only through `Local\WACCON_SHARED_BUFFER`.

## 0. Summary

| Question (doc 04 §11) | Answer | Ours (`wacca-game/src/judge`) |
|---|---|---|
| Touch windows (JudgeParameterTable 3/5/6 f) | **Confirmed.** 50 % edges from the window centre: Marvelous −2.97/+3.04 f, Great −4.94/+4.81 f, Good −5.73/+5.86 f (n 567, 7 runs). Widths 6.0 / 9.7 / 11.6 f. | same (3/5/6) |
| Frame-quantised or continuous? | Widths match **continuous** `|offset| ≤ n frames` (6/10/12 f); integer-frame-index judging (`|k| ≤ n`) would give 7/11/13 f — rejected. Great/Good are ~0.15–0.2 f narrower per side than the table, within the method's resolution. | continuous ✔ |
| Hold start | Like touch: Marvelous −3.2/+3.5 f, Great −5.5/+5.5 f (n 47). A start outside its windows whose body is held still gives **Good** (seen from −10 to +8.5 f). | same ✔ |
| Chain window, AdjustRate front = 20 | Marvelous when the 25 ms contact starts −3.96…+4.0 f from the centre, Miss outside (n 38); the early edge is ≈ 3.5–4 f even after allowing for the contact length — **AdjustRate 20 does not shrink the early window** (to 0.8 f). Chains never showed Great/Good. | 4/4 f, AdjustRate ignored ✔ |
| Slide windows (table 5/7, 8/10, 10/10) | Consistent: Marvelous −4.2…+7.2 f, Great at −4.5…−7.5 f, Miss at ≤ −10.5 and ≥ +9.5 f; a missed slide is shown ≈ 10.3 f after a centred hit (late close ≈ 10 f). n 19 — edges ±1 f. Community −5/+5 Marvelous is **not** supported (Marvelous seen at +4…+5 f twice). | table ✔ |
| Snap windows | Only 5 timed snaps (all snap-in): Marvelous at +0.5, +6.5 f, Great +7.5, Miss +10.5, +13.5; miss shown +11 f. Consistent with SnapUp 5/7, 8/10, 10/10 but not a measurement. | table |
| Hold drop allowance (Saturn 215 ms) | **Between 167 and 183 ms (10–11 frames).** A mid-hold release of ≤ 167 ms keeps the start judgement (n 10), ≥ 183 ms gives Good (n 9). | 215 ms ✘ |
| Release before the end / HoldEnd 7/7 row | Releasing **≤ 267 ms** before the end keeps the start judgement (n 16), **300 ms** gives Good (n 3). Fits "the hold is checked until end − 7 f (HoldEnd), and a gap there > ~10 f is a drop": 300 − 117 = 183 ms → drop, 267 − 117 = 150 ms → kept. | release > 215 ms before end → Good ✘ |
| Start missed **and** dropped | **Miss** (start at −8 f, released 300 ms before the end); start missed + gap ≤ 167 ms → Good. | Good ✘ |
| Slide distance threshold (Saturn 1.9 lanes) | A single jump of **≥ 2.0 lanes** hits (n 7), **≤ 1.5 lanes** misses (n 7), mean-position model. | 1.9 ✔ |
| Snap ring movement | **One ring inward** suffices (3→2, 1→0, 3→1, 2→0 all Marvelous); an outward move on a snap-in misses. | one ring ✔ |
| Chord / one-press rule (`ProtectInvolveRockFrameNum −1`) | Not probed (no guest Normal chart with overlapping notes inside one window). Ordinary chords (side by side, same time) were hit by the perfect script's simultaneous contacts in every run. | — |

## 1. Method

### 1.1 Probe scripts (`e3.rs`, `e3_script`)
`e3_script --chart <mer> --spec <spec.json> --out <x.taps.json> [--seed N]` builds the perfect script of doc 09
(`autoplay_frames` → contacts ≥ 25 ms, ≥ 17 ms apart per cell) and changes it on **test notes only**: notes whose span
(holds: start..end) keeps ≥ 0.35 s from every other note and that come after the live-sync point (5 s). Each test note
gets one probe, dealt from a shuffled list per class (`Probe`):

| Probe | Input |
|---|---|
| `shift {ms}` | every contact of the note moved (a hold: its start only) — window sweeps, −8…+8 f in ½ f for touch, ±13 f for slide/snap, −7…+6 f for chain |
| `hold_gap {at_ms, ms}` | all contacts in the hold's lanes removed for `ms` from `at_ms` after the start, then re-pressed |
| `hold_release {ms}` | hold released `ms` before its end |
| `slide_jump {lanes}` | all rings of the trailing-edge lane for 25 ms, then the lane `lanes` ahead (x.5: the two lanes around) for 25 ms |
| `snap_move {from, to}` | whole arc on ring `from` for 25 ms, then on ring `to` (0 = innermost) |

The script also goes through our judge (`predict`); the plan file `x.plan.json` stores test notes, probes and our
verdicts. Specs: `scripts/e3-sweep.spec.json`, `e3-chain.spec.json`, `e3-rules.spec.json`, `e3-hold.spec.json`.

### 1.2 Per-note judgements (`panel.rs`)
The debug panel under the playfield counts Marvelous / Great / Good / Miss (and FAST / LATE). The capture is
lossless and the panel static, so **a counter changed exactly when its digit pixels changed** (no OCR): each
change is a judgement event (`analyze_e3` decodes only a 176×148 crop, ~15 s per run). Each event is assigned to the
note whose expected time (note time + its shift; holds: their whole span) is nearest after removing the run's
display lag (the most common event − note distance). A test note is resolved when exactly one counter changed for it.
Totals agree with the notes (e.g. Fragile: 145 notes → 146 changes, chords counted once).

### 1.3 Clock: the timing check matters
Input is scheduled on the audio clock (live loopback sync, doc 09), but the game judges on its own frame clock,
which **drifts and jumps against the audio**: per-run visual timing (`analyze_e2 --timing-block 8`, δ = note
visuals − audio) drifted by 25–135 ms over a song in most runs and jumped by 60–550 ms in some. Without correction a
sweep is smeared (13 misfits / 110 touches); with the shift corrected by δ of the note's 8-note block (relative to the
run mean) it is clean (2–3 / 135). Tests in a block next to a jump > 30 ms are excluded. Edges are then given
relative to each run's **touch Marvelous centre** (fitted from its own touch sweep): the absolute offset of the script
to the judge clock (input lead, A/V skew, frame phase) is not known to better than a frame, but edges relative to the
touch window are. The 1-frame sampling phase smears every transition over one frame; the fit
(`fit_interval`, fewest misclassified points, edge = middle of the optimal range) gives the 50 % point.

### 1.4 Validation
Every run: injector write error (actual − scheduled edge time) p95 ≤ 2 ms, else the run is excluded; single edges
> 2 ms late remove only the test notes within 0.3 s of them. Visual timing per 8 notes as above.

## 2. Runs

| Run (`runs/e3/…`) | Chart | Probes | Write err p95 / max ms | Visual timing | Touch centre ms | Used |
|---|---|---|---|---|---|---|
| 20261013-023731 | フラジール N | sweeps | 0.06 / 47 (1 tap; 2 tests dropped) | drift −78→−35, jump at 109 s | 22.2 | ✔ |
| 20261013-030310 | メルト N | sweeps (chains) | 0.01 / 0.4 | jumps at 65 s, 128 s | 41.7 | ✔ (blocks near jumps excluded) |
| 20261013-034614 | フラジール N | sweeps | 0.00 / 0.4 | flat −85…−100 | −14.6 | ✔ |
| 20261013-035555 | マトリョシカ N | sweeps (holds) | 0.01 / 3.3 | drift −105→−40 | 4.6 | ✔ |
| 20261013-040029 | 乙女解剖 N | hold/slide/snap probes | 0.13 / 4.4 | drift −75→+60 | 57.1 | ✔ |
| 20261013-040534 | 砂の惑星 N | hold/slide/snap probes | **7.3 / 27** | chaotic (±300 ms) | — | **✘ excluded** (other agents' build + app ran concurrently) |
| 20261013-042301 | 乙女解剖 N | hold-gap fine probes | 0.14 / 5.2 | drift −78→+7 | 41.9 | ✔ |
| 20261013-042834 | メルト N | chain sweep | 0.01 / 0.8 | drift −90→+10 | 42.2 | ✔ |

Plus 5 runs aborted in navigation while the seek was being fixed (§5). 1 of 8 song runs excluded.

## 3. Window sweeps (frames from the touch centre, ½-frame bins: M/Gr/Gd/Miss)

Touch (n 567):
```
-8.0: 0/0/0/12  -7.0: 0/0/1/17  -6.5: 0/0/3/14  -6.0: 0/0/6/10  -5.5: 0/3/10/2  -5.0: 0/6/10/1
-4.5: 0/13/6/0  -4.0: 1/14/2/0  -3.5: 2/17/0/0  -3.0: 8/10/0/0  -2.5: 13/3/0/1  -2.0 … +2.0: all M (1 stray)
+2.5: 10/4/0/0  +3.0: 9/6/0/0   +3.5: 1/15/0/0  +4.0: 1/16/1/0  +4.5: 0/10/2/0  +5.0: 0/6/5/2
+5.5: 0/1/12/1  +6.0: 0/0/5/9   +6.5: 0/0/0/12  +7.0: 0/0/0/9
```
A missed touch is shown 6.5 f (median, n 174) after a centred hit would be: the Good window closes at +6 f and
the miss is decided then (plus the ½ f sampling bias).

Hold start (n 47): M −3.5…+4.0, Great −5.5…−3.0 and +2.5…+5.5, then only Good (body held) out to ±10 f.
Chain (n 38): M −3.5…+3.5, Miss ≤ −4.5 and +4.5 (a missed chain is shown 5.4 f after centre, ~1 f before a missed
touch: late window ≈ 4–5 f). Slide (n 19) and snap (n 5): see §0.

## 4. Hold, slide and snap probes

Each probed note is otherwise played at script offset 0, which lands at the "start" offset on the judge clock
(frames from the touch centre; drift-corrected), so the start alone would get the "start" judgement.

| Probe | Start offset (f) | Start alone | Real | Ours |
|---|---|---|---|---|
| hold gap 67 ms at +150 ms | −3.8 | Gr | Gr | M |
| hold gap 100 ms | −3.8 / −8.0 | Gr / Miss | Gr / Gd | M / M |
| hold gap 133 ms | −0.4 / −3.2 / −4.8 | M / Gr / Gr | M / Gr / Gr | M |
| hold gap 150 ms | −3.5 / −4.0 | Gr | Gr / Gr | M |
| hold gap 167 ms | −3.1 / −3.6 / −5.2 / −8.0 | Gr / Gr / Gd / Miss | M / Gr / Gd / Gd | M |
| hold gap 183 ms | −3.5 / −3.7 | Gr | **Gd / Gd** | M |
| hold gap 200 ms | −0.2 / −3.2 | M / Gr | **Gd / Gd** | M |
| hold gap 217–400 ms | −0.2…−2.9 (5 holds) | M | **Gd** ×5 | Gd |
| released 33–200 ms before end | −1.8…−4.1 (10 holds) | M / Gr | = start ×10 | M |
| released 150 ms before end | −8.0 | Miss | Gd | M |
| released 233 ms before end | −1.8 / −3.9 | M / Gr | M / Gr | **Gd** |
| released 267 ms before end | −0.4 / −4.8 | M / Gr | M / Gd | Gd |
| released 300 ms before end | −2.8 / −3.5 / −4.0 | M / Gr / Gr | **Gd ×3** | Gd |
| released 300 ms before end | −8.0 | Miss | **Miss** | Gd |
| slide jump 1.0 lanes | −4.7 / −6.3 | | Miss ×2 | Miss |
| slide jump 1.5 lanes | −0.4 … −4.8 (5) | | **Miss ×5** | Miss |
| slide jump 2.0 lanes | −0.9 … −4.8 (5) | | **M ×5** | M |
| slide jump 2.5 / 3.0 / 4.0 lanes | | | M (Gr at −6.3 f) | M |
| snap-in ring 3→2, 1→0, 3→1, 2→0 | | | M ×4 | M |
| snap-in ring 0→3 (outward) | | | Miss | Miss |

Notes: the one 267 ms-before-end Good had its start at −4.8 f, on the Great/Good edge (−4.94), so it is probably the
start's Good. The 167 ms gap rows are all "= start", the 183 ms rows all Good: with ~1 f sampling jitter on both
the release and the re-press, the allowance is **≈ 10–11 f (167–183 ms)**.

## 5. Navigation fixes (harness)

- The song carousel is the **whole song list** sorted by category (= `songs.json` order); a category only sets the
  start (its first song). The category screen's default focus is not stable, so `nav` now **seeks the category by
  image** (`data/nav/cat-<name>.png`, a crop of that category's tile, made from captured frames; not committed),
  then seeks the jacket. `seek` has a `back` step (other direction, 2× steps) and every step's frame is saved.
  VOCALOID tail songs (フラジール, マトリョシカ, 乙女解剖, …) are reached from TOUHOU going left; メルト/KING from
  VOCALOID going right. Menus time out after ~90 s, so long seeks fail.
- `run_experiment --predict-start-ms N` (start the injector at GO + N without audio sync) is implemented, but the
  GO → BGM-sample-0 delay measured in 6 runs is **bimodal: 11.9–12.2 s or 41.8–42.8 s** (the GO touch is sometimes not
  taken and the options timer starts the song) with ±0.5 s spread, so it is not usable for 1-frame timing. `meta.json`
  records `sync.start_press_ms` in every song run.

### Visual references (for the play-area port)
Every song run above has `reference/NNNN-<label>.png` + `reference/frames.csv` and `frames.csv` (chart time per
video frame; `analyze_e2`'s export), the 120 fps lossless `video.mkv` and the result screens. E3 runs contain
deliberately mistimed notes, i.e. Great/Good/Miss popups and combo breaks. KING (snap in/out, R, bonus, one Miss):
`runs/e2/20261013-043842/` (doc 09 §5).

## 6. Recommended changes to `wacca-game/src/judge` (not applied here)

1. `HOLD_DROP_LENIENCY`: 0.215 → **0.175 s** (10.5 frames; measured between 167 and 183 ms). Keep it UNVERIFIED-free
   with a reference to this doc.
2. Hold end: stop checking drops at **`end − HoldEnd` (7 frames, `JudgeTable::hold_end`)** — a release in the last
   7 frames is free, and a release earlier than that counts as a gap up to `end − 7 f`. Concretely in `decide`:
   `if h.released_at.is_some_and(|r| (end - 7.0 * FRAME_SECS) - r > HOLD_DROP_LENIENCY) { h.dropped = true }`, and
   `track_hold` should ignore release/re-press after `end − 7 f`. This turns the measured 233/267 ms-before-end
   cases from Good into "start judgement" and keeps 300 ms as Good.
3. Hold result: `(touched, start missed, dropped)` → **Miss** (now Good). Keep: start missed but held → Good; dropped
   with a hit start → Good; otherwise the start judgement.
4. Keep: touch/hold/chain/slide/snap windows from `JudgeParameterTable` in continuous time (`frames × 1/60 s`);
   `AdjustRate` stays unapplied (chain early window measured ≈ 4 f, not 0.8 f) — replace the UNVERIFIED notes on
   these with a pointer to this doc. Slide threshold 1.9 and one-ring snaps are consistent with the game
   (threshold between 1.5 and 2.0 lanes).
5. Open (not measured): chord / involve-lock rule, snap-out windows and early slide/snap Great edges (few samples),
   Hard/Expert charts (difficulty select not automated).

## 7. Reproduce

```
cargo build --release -p wacca-realgame
e3_script --chart <assets>\MusicData\S01-041\S01-041_00.mer --spec tools\realgame\scripts\e3-sweep.spec.json ^
  --out tools\realgame\scripts\e3-fragile-a.taps.json --seed 1
set WACCA_INJECT_OK=1 & set WACCA_ASSETS=<assets>
run_experiment --script tools\realgame\scripts\e3-fragile-a.taps.json --nav tools\realgame\scripts\e3-guest-fragile.nav.json ^
  --start-nav tools\realgame\scripts\e2-start.nav.json --sync-chart <mer> --input-lead-ms 110 --tail-ms 30000 --out-dir <runs\e3>
analyze_e2 <run> --compare 0 --timing-block 8     (timing check → e2.json; also reference/ frames)
analyze_e3 <run>... --report report.md            (panel.json, e3.json per run; pooled report)
```
(run from a directory with `data/jackets` and `data/nav`; take `runs/LIVE.lock` first, doc GOALS.md.)
