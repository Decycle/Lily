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
| Chain window, AdjustRate front = 20 | Marvelous when the 25 ms contact starts −3.96…+4.0 f from the centre, Miss outside (n 38); the early edge is ≈ 3.5–4 f even after allowing for the contact length — **AdjustRate 20 does not shrink the early window** (to 0.8 f). Chains never showed Great/Good. **E5 (§9):** any touch of the arc while the ±4 f window is open hits — a finger resting since long before, or landing early and staying, is Marvelous; no fresh press is needed. | 4/4 f on any touch, AdjustRate ignored ✔ |
| Slide windows (table 5/7, 8/10, 10/10) | Consistent: Marvelous −4.2…+7.2 f, Great at −4.5…−7.5 f, Miss at ≤ −10.5 and ≥ +9.5 f; a missed slide is shown ≈ 10.3 f after a centred hit (late close ≈ 10 f). n 19 — edges ±1 f. Community −5/+5 Marvelous is **not** supported (Marvelous seen at +4…+5 f twice). | table ✔ |
| Snap windows | **E6 (§10):** snap-in early edges M −6.0, Great −8.2, Good −10.2 f (n 23); snap-out M −7.5/+6.0, Great −10.0/+8.3, Good −10.0/+10.5 f (n 21). Table SnapUp 5/7, 8/10, 10/10 and SnapDown 7/5, 10/8, 10/10 to ±1 f; slide early edges −5.5 / −8.1 / −10.4 f (n 107). | table ✔ |
| Hold drop allowance (Saturn 215 ms) | **Between 167 and 183 ms (10–11 frames).** A mid-hold release of ≤ 167 ms keeps the start judgement (n 10), ≥ 183 ms gives Good (n 9). | 215 ms ✘ |
| Release before the end / HoldEnd 7/7 row | Releasing **≤ 267 ms** before the end keeps the start judgement (n 16), **300 ms** gives Good (n 3). Fits "the hold is checked until end − 7 f (HoldEnd), and a gap there > ~10 f is a drop": 300 − 117 = 183 ms → drop, 267 − 117 = 150 ms → kept. | release > 215 ms before end → Good ✘ |
| Start missed **and** dropped | **Miss** (start at −8 f, released 300 ms before the end); start missed + gap ≤ 167 ms → Good. | Good ✘ |
| Slide distance threshold (Saturn 1.9 lanes) | A single jump of **≥ 2.0 lanes** hits (n 7), **≤ 1.5 lanes** misses (n 7), mean-position model. | 1.9 ✔ |
| Snap ring movement | **One ring inward** suffices (3→2, 1→0, 3→1, 2→0 all Marvelous); an outward move on a snap-in misses. | one ring ✔ |
| Chord / one-press rule (`ProtectInvolveRockFrameNum −1`) | **E6 (§10):** chord notes pressed separately each take their own press's timing; one press over a chord hits all of it; one press over two side-by-side notes 9.5 f apart hits both (the earlier one late); overlapping notes 9.5 f apart have their windows cut at the midpoint and a press goes to the note whose window it is in. A finger landing ≥ 8 f before a touch and held is a Miss (41/41). 102 / 107 groups agree, the rest at a window edge. | same ✔ |
| Hard difficulty | **E6:** guests can select Hard (song-select DIFFICULTY button, lanes 30–31); Expert/Inferno stay locked (doc 09 §1.3). Touch windows on Hard: M −3.2/+3.1, Great −5.4/+5.1, Good −6.1/+5.8 f (n 532) = Normal's. | same ✔ |

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
   Hard/Expert charts (difficulty select not automated). **Done in E6 (§10) except Expert (locked for guests).**

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

## 8. E4 parity (offline: injected input → our judge vs the real verdicts)

Tool: `judge_parity <run>... [--report r.md] [--table t.csv]` (`tools/realgame/src/bin/judge_parity.rs`, pure parts
in `parity.rs`). Per run it reads the injector log (`inject.jsonl`: every edge/clear record at its **actual write
time**), puts it on the judge clock (`t − δ(block) − c`: δ = the 8-note-block visual timing of `e2.json` relative to
the run mean, c = the run's touch centre from `e3.json`), repeats each state every ¼ frame (the game feeds the judge
every frame) and runs `wacca_game::judge::Judge` as on main (175 ms drop, HoldEnd free window, missed + dropped =
Miss) on the parsed chart. Real verdicts: the resolved test notes of `e3.json`, plus unprobed notes that resolve to
exactly one panel change and keep 0.1 s from every other note edge. Left out as in §1.3/§1.4: notes in a block next
to a clock jump, notes within 0.3 s of a late (> 2 ms) write, and run 20261013-040534. A disagreement within 1 frame
of a window or rule edge (window edges at the input offset; for hold probes also gap − 175 ms, or the release lead
on the judge clock − (175 ms + 7 f)) counts as sampling noise: the game sees input once per 60 Hz frame.

### 8.1 Agreement (7 runs, 845 notes)

| Class | Probed | Perfect play | > 1 f from every edge | Disagreements: noise / unexplained |
|---|---|---|---|---|
| Touch | 489 / 567 (86.2 %) | 63 / 64 | **301 / 302 (99.7 %)** | 78 / 1 |
| Hold | 74 / 87 (85.1 %) | 11 / 18 | **34 / 34** | 20 / 0 |
| Chain | 36 / 38 (94.7 %) | 22 / 24 | **38 / 38** | 4 / 0 |
| Slide | 33 / 35 (94.3 %) | 1 / 1 | **25 / 25** | 2 / 0 |
| Snap | 10 / 10 | – | 7 / 7 | 0 / 0 |

The probed figures are low by design: the sweeps put most test notes on window edges. Away from the edges our judge
agrees on every note but one.

### 8.2 Disagreements

- **Touch (79):** 78 within 1 f of an edge. Ours is the more lenient in 51 (29 early, 22 late), the stricter in 28 —
  the ~0.15–0.3 f narrower Great/Good of §0, at the method's resolution: no rule change. The unexplained one
  (20261013-042834 note 29, input −7.3 f, real Good, ours Miss) is the stray of the §3 sweep (−7.0: 0/0/1/17).
- **Hold (20):** all at start-window edges (an `= start` verdict one tier off) or at the drop boundary. Every
  hold-rule probe (21 gaps, 19 early releases) agrees with the new rules except at an edge: gap 167 ms at start
  −3.1 f (start edge) and release 233 ms before the end at start −3.85 f (real Gr, ours Gd). The latter is E3
  bookkeeping: probe times are **script** times, but the script runs `δ + c` (here 64 ms) ahead of the judge clock,
  so that release is **297 ms** before the end on the judge clock, 5 ms past our 175 ms + 7 f = 292 ms. On the judge
  clock: a release lead ≤ 297 ms keeps the start judgement (n 14), ≥ 347 ms drops (n 4; one at 346 ms is ambiguous,
  its start is on the Great/Good edge) — consistent with §6.2. Missed + dropped → Miss and missed + kept → Good agree.
  7 "perfect play" hold misfits sit in two drift blocks at −4.75 and −5.0 f, on the Great/Good edge (block-level
  sync error < 1 f; chains in the same block show the same shift).
- **Chain (4):** all **real Miss / ours Marvelous**, the 25 ms contact *starting* −4.4…−4.9 f before the note;
  contacts starting at −4.1 f or later are Marvelous on both, −5.4 f or earlier Miss on both. Ours hits a chain if
  the lane is touched on any frame inside ±4 f, so a contact starting up to 5.5 f early still hits; the real early
  edge on the contact start is ≈ −4.1…−4.4 f (§0: −3.96). Each case is < 1 f from an edge, but all go the same way.
- **Slide (2):** at the Marvelous/Great edge (−4.7 f) and the late Miss edge (+9.3 f): noise. Slide jumps (1.0/1.5
  lanes Miss, ≥ 2.0 lanes hit) and all snap ring moves agree.

### 8.3 Hold paths: all joints vs control points

The `.mer` column after the width is the render flag (1 = authored control point, 0 = generated joint); the four E3
charts have 40 flag-1 and 1035 flag-0 joints. With each hold replaced by a straight lane/width line between its
control points (sampled every frame), the generated joints are at most **1.2 lanes** off that line (≤ 0.1 lanes on
3 of 4 charts) and **no verdict changes** on any note of any run (the E3 inputs keep the finger well inside the
arc). E4 cannot tell the two apart on these Normal charts. Recommendation: follow the renderer — judge the hold arc
on the straight path between control points (linear in unwrapped lane and width at `t`), not the step-wise "latest
stored joint" of `hold_arc_at`; with `HoldNode.control` this is a small change in `hold_arc_at` and the autoplay
`hold_finger`. Check a chart whose generated joints stray further (curved Hard/Expert holds) first.

### 8.4 Recommended changes to `wacca-game/src/judge` (not applied here)

1. **Chain early edge (tentative; settled by E5, §9: keep the current rule):** ≈ 4 f on the contact *start* in the game, ≈ 5.5 f in ours. Two rules fit:
   (a) a chain needs a sensor turning on inside its ±4 f window, or (b) touched-while-open with the early window
   shrunk by `AdjustRateFront` 20 % as a factor (4 × 0.8 = 3.2 f; + 1.5 f contact ≈ 4.7 f). They differ for a finger
   already resting on the lane when the window opens: probe that (chains under contacts held −10…+2 f and −10…−3 f)
   before choosing. Until then keep the current rule; the effect is ≤ 1.5 f on early chain touches only.
2. **Hold arc:** straight path between control points (§8.3), once `HoldNode.control` exists (done, §9.4).
3. No change to the touch/hold/slide/snap windows, the 175 ms drop, the HoldEnd free window or missed + dropped =
   Miss: every note > 1 f from an edge agrees.
4. Harness (done): `e3::predict` fed only edge frames, so a lane touched before a chain's window and a hold body held
   between two edges were never seen (holds started early and held came out Miss); `e3::judge_frames` now repeats held
   states every ¼ frame. Plans generated from now on get a corrected `ours` column.

### 8.5 Reproduce

```
cargo build --release -p wacca-realgame --bin judge_parity
judge_parity runs\e3\20261013-023731 runs\e3\20261013-030310 runs\e3\20261013-034614 runs\e3\20261013-035555 ^
  runs\e3\20261013-040029 runs\e3\20261013-042301 runs\e3\20261013-042834 --report parity.md --table parity.csv
```
(from the checkout root, after `analyze_e2` and `analyze_e3`; read-only on the run directories.)

## 9. E5: chains under held and resting fingers (live, 2026-10-13)

Question (§8.4.1): does a chain need a touch-down inside its window (onset rule), is its early window 3.2 f
(`AdjustRateFront` 20 % as a factor), or does any touch while the window is open count?

### 9.1 Probe and runs

New probe `press {from_ms, to_ms}` (`e3::Probe::Press`; spec `ProbeSpec::Press { frames: [[from, to], …] }`): the
chain's scripted contact is replaced by one finger on ring 0 of every lane of its arc from `from` to `to` (relative
to the note); everything else is the perfect script. Specs `scripts/e5-chain.spec.json` (touch-downs −7…−3 f held to
+8 f, resting −18 f → +8 f, −18 → −2, −10 → −3/−2, late touch-downs +2…+6 f held to +15 f) and
`e5-chain-edges.spec.json` (25 ms contacts starting −5.5…−4 f, long contacts −10 f → −5…−3.5 f, late touch-downs
+3…+4.5 f, resting −14 f → +8 f), each with a ½-frame touch sweep −6…+6 f for the run's touch centre. Chart:
メルト Normal (its 21 isolated chains are one stream at 77.6–85.4 s, 0.35 s apart). Runs `runs/e5/<run>` of the main
checkout; validation as §1.3/§1.4 (`analyze_e2 --timing-block 8`, `analyze_e3`; chain table "Chain presses (E5)").

| Run | Spec | Write err p95 / max ms | Touch centre ms | Used |
|---|---|---|---|---|
| 20261013-052909 | e5-chain (seed 51) | 0.01 / 1.0 | 7.6 | ✔ |
| 20261013-053427 | e5-chain (52) | 0.02 / 0.4 | 16.0 | ✔ |
| 20261013-053912 | e5-chain (53) | 0.06 / 6.3 | 107.0 | ✘ chain stream inside a clock-jump block (no timing) |
| 20261013-055132 | e5-chain-edges (61) | 0.01 / 0.8 | 19.7 | ✔ |
| 20261013-055616 | e5-chain-edges (62) | 0.00 / 2.6 | 70.2 | ✘ for edges: 4 clock jumps, run-wide touch centre not valid for the chain block |
| 20261013-060103 | e5-chain-edges (63) | 0.02 / 5.6 (1 late edge, its tests dropped) | 17.7 | ✔ |

Two late-touch-down chains (+5.2/+5.4 f, real M) are left out: the next chain's −18 f contact started 3 f after
them on overlapping lanes (the −18 f probes were replaced by −14 f in the second spec).

### 9.2 Results (frames from the run's touch centre on the judge clock, drift-corrected)

- **Resting / landing early and staying: always hit.** Fingers down from −2.7 to −19 f and held to ≥ +5.8 f: 26 / 26
  Marvelous (7 of them resting since −14…−19 f). Resting from −18 f and lifted at −2.6/−2.8 f: Marvelous. → the
  **onset rule is rejected**; no fresh press is needed.
- **Early side: the last touch counts, not the touch-down.** Contacts lifted before the note (M = hit, x = Miss):

  | Run | Long (down from −10 f) lifted at | 25 ms contact lifted at |
  |---|---|---|
  | 052909 | −3.56 M, −2.56 M ×3 | – |
  | 053427 | −3.79 M, −2.79 M ×2 | – |
  | 055132 | −5.28 … −3.78 x (7) | −4.28 x, −3.78 x ×2, −3.28 M/x, −2.78 M |
  | 060103 | −6.14 … −4.24 x (7) | −5.14 x, −4.64 x ×2, −4.14 M/x, −3.64 M |

  Long and short contacts lifted at the same time get the same verdict, so the hit depends on whether the lane is
  touched on a frame inside the window, not on when the touch began.
- **Late side: touch-downs** +1.2…+3.4 f: 16 / 17 Marvelous (one Miss at +3.2); +3.7, +3.7, +4.2 f M (055132);
  +4.2, +4.4 f Miss. 50 % ≈ +3.5…+4.2 f.
- **Window width, independent of the touch-centre calibration:** within each run the early 50 % point (on the lift)
  and the late one (on the touch-down) are ≥ 7.0–7.5 f apart (052909 ≥ 7.5, 053427 ≈ 7.0, 055132 ≥ 7.5, 060103
  ≥ 7.5). With the game sampling once per 60 Hz frame, a window `[a, b]` has its 50 % points at `a + ½` and `b − ½`,
  so `b − a` ≥ 8.0–8.5 f: **±4 f**. A 3.2 f early window would give ≈ 6.2 f between the 50 % points — **rejected**.
- The absolute early edge moves between runs (≈ −3.3 f in 055132, ≈ −4.1 f in 060103, < −3.8 f in 052909/053427)
  together with the late edge: the per-block clock correction is good to ±½–1 f in a chain stream. That also
  explains E4's four chain disagreements (§8.2: all one-sided at −4.4…−4.9 f, all in the メルト chain stream of two
  E3 runs).

**Rule:** a chain is hit (Marvelous, no timing) if any sensor of its arc is on at any judge frame while its ±4 f
window is open — a resting finger, a finger landing early and staying, or a contact lifted inside the window. This
is the rule `wacca-game` already had; no change, now with a test per measured case (`judge/tests.rs`:
`chain_finger_resting_long_before_hits`, `chain_finger_landing_early_and_staying_hits`,
`chain_early_edge_is_on_the_last_touch`, `chain_late_touch_down`).

### 9.3 Parity (`judge_parity`, before = main a3f2cfb, after = this branch)

7 valid E3 runs + 4 valid E5 runs (052909, 053427, 055132, 060103), 1,267 notes:

| Class | > 1 f from every edge | Disagreements: noise / rule? |
|---|---|---|
| Touch | 398 / 399 | 128 / 1 (the §8.2 stray) |
| Hold | 78 / 78 | 20 / 0 |
| Chain | **84 / 84** | 9 / 0 |
| Slide | 28 / 28 | 2 / 0 |
| Snap | 7 / 7 | 0 / 0 |

Before and after are identical (the chain rule did not change and the hold path changes no verdict on these
charts). Chain disagreements away from the edges: **0** (E3 alone: 38 / 38 as in §8.1; E5 alone: 46 / 46). The nine
edge cases: the four E3 ones of §8.2 and five E5 ones within 0.1–0.8 f of an edge, in both directions (real Miss /
ours M at a −10 → −3.5 f lift, a 25 ms contact lifted at −3.3 f and a +3.2 f touch-down; real M / ours Miss at a
+4.2 f touch-down and a 25 ms contact lifted at −4.1 f). Run 055616 alone: chains 21 / 21. E5 notes in all: 190 / 190 agree > 1 f from an edge.

### 9.4 Hold path = straight between control points

`judge::hold_path_at`: a hold body is judged on the lane span at `t` interpolated linearly (unwrapped lane and
width) between the authored control points (`HoldNode.control`; the first and last node always count), rounded to
whole lanes; generated joints (`control = false`) are ignored — as the game draws it (doc 11 §3.4, §8.3 here). The
autoplay finger follows the same path (sampled every ¼ frame, moving only when the path leaves it). Tests:
`hold_path_is_straight_between_control_points`, `hold_body_judged_on_the_control_path`,
`autoplay_follows_the_control_path`. Autoplay corpus (1,247 charts, normal and mirrored): all perfect.

### 9.5 Reproduce

```
e3_script --chart <assets>\MusicData\S03-048\S03-048_00.mer --spec tools\realgame\scripts\e5-chain.spec.json ^
  --out tools\realgame\scripts\e5-melt-51.taps.json --seed 51          (e5-chain-edges.spec.json: seeds 61–63)
run_experiment … (as §7, nav e3-guest-melt.nav.json; needs data\nav\cat-vocaloid.png and the jacket)
analyze_e2 <run> --compare 0 --timing-block 8 ; analyze_e3 <runs>… --report report.md
judge_parity <7 E3 runs> runs\e5\20261013-052909 runs\e5\20261013-053427 runs\e5\20261013-055132 ^
  runs\e5\20261013-060103 --report parity.md --table parity.csv
```
`data\nav\cat-vocaloid.png` (not committed) is the crop `[0.28, 0.36, 0.44, 0.2]` of
`runs\e3\20261013-042834\nav-11-category-vocaloid.png`.

## 10. E6: chords, close pairs, snap-out, slide/snap early edges, Hard (live, 2026-10-13)

### 10.1 Probes, charts, runs
- **Two-note groups** (`e3::isolated_pairs`, classes `chord` / `chordoverlap` / `pair` / `pairoverlap`): two touches
  (or touch + hold start) starting ≤ 0.19 s apart whose joint span keeps the spec isolation from every other note.
  The test note is the earlier one, its partner the next note. Probes remove both notes' scripted input and add
  `group_press {cover, ms}` (one 25 ms contact on ring 0 of `union` = shortest arc over both / `first` / `second` /
  `overlap` lanes, at the second note + `ms`) or `group_split {a, b}` (first arc on ring 0 at its time + a, second
  arc on ring 1 at its time + b). `analyze_e3` pools every panel change near either note into the test note;
  `judge_parity` compares the set of changed counters with ours for both notes on the injected input (two equal
  judgements on one frame change one counter once). Miss-only panel changes are now matched ~6 f earlier
  (`e3_analysis::MISS_DELAY`): a miss is shown when its window closes, which put the second note's miss nearer the
  next note in the first E6 analysis.
- **Held touches:** `press {from, to}` now also applies to touch notes (finger on ring 0 of the arc from `from` to
  `to` frames).
- Window sweeps as in §1.1; snap-out notes are their own class (`snapout`) from now on.
- **Hard:** the guest song select has a DIFFICULTY button at lanes 30–31 (left edge); one touch switches Normal →
  Hard (`nav-*-difficulty-hard.png` shows HARD Lv. 8 / 8+ on every run). Navigation: `e6-guest-*-hard.nav.json`
  (VOCALOID by image; TOUHOU = one more right step from VOCALOID, so no TOUHOU tile image is needed).
  Expert/Inferno: not tried again; locked for guests per doc 09 §1.3.
- Charts (chart stats: new `chart_stats <songs.json> <assets> --difficulty Hard`): no Normal or Hard chart has
  isolated touch-touch pairs closer than 0.19 s except 砂の惑星 Hard (overlapping pairs 9.5 f apart, 24–32 per run);
  KING Hard has 85 isolated side-by-side chords; 四次元跳躍機関 Hard (TOUHOU) the most isolated snap-outs (8) plus
  9 snap-ins and 23 slides. Overlapping simultaneous touch pairs (`chordoverlap`) are not isolated on any reachable
  chart: not measured.

| Run (`runs/e6/…`) | Chart (Hard) | Spec (seed) | Write err p95 / max ms | Visual timing | Touch centre ms | Used |
|---|---|---|---|---|---|---|
| 20261013-192905 | KING | e6-mixed (81): chords, sweeps | 1.22 / 36 (26 late edges, their tests dropped) | −85→0 ms drift, then δ swings −400…+160 ms after 100 s | 29.1 | ✔ before 100 s |
| 20261013-193804 | 砂の惑星 | e6-pair (101): pairs, slide sweep | 0.00 / 2.2 | jumps at 54, 99, 121 s | 65.1 | ✔ except 99–121 s |
| 20261013-194706 | 四次元跳躍機関 | e6-snap (91) | 0.00 / 0.2 | flat −70…−90 | 2.1 | ✔ |
| 20261013-195356 | 四次元跳躍機関 | e6-snap-edges (92) | 0.00 / 0.4 | jumps at 11, 13, 76–122 s | 18.4 | ✔ (blocks next to jumps excluded) |
| 20261013-200154 | 四次元跳躍機関 | e6-snap-edges (93) | 0.00 / 6.0 (2 late edges) | flat | 15.0 | ✔ |
| 20261013-200944 | 砂の惑星 | e6-pair (102) | 0.00 / 0.3 | jumps at 71, 75 s | −22.9 | ✔ |
| 20261013-201831 | 四次元跳躍機関 | e6-touch-held (111) | 0.00 / 0.9 | flat | 19.2 | ✔ |

The two exclusions are chosen from the timing check, not the verdicts: in 192905 after 100 s and 193804 between the
99 s and 121 s jumps, the per-block δ (−400…+160 ms, +122…+153 ms) is evidently not the judge clock (perfectly
played notes there come out 6–9 f off on the corrected clock).

### 10.2 Chords and one-press rule (frames on the judge clock)
- **Side-by-side chords (KING, 52 groups before 100 s, all agree):** one press over both arcs hits both with the
  press's timing (−0.4…0 f: one Marvelous change; +3.6…+4.0 f: one Great change). Split presses are judged each on
  its own press: (0, +4) → M + Gr, (−4, +4) → Gr + Gr, (+2, 0) → M + M, (0, −4) → M + Gr — the first press does not
  carry the second note.
- **Overlapping touches 9.5 f apart (砂の惑星, 43 / 48 agree):** a press over both (or over either arc alone, which
  overlaps the other) up to +4.1…+4.7 f after the first note hits the first only (Great; the second is missed unless
  pressed again); from +4.9…+5.4 f after the first (= −4.6…−4.1 f before the second, 5 groups) it hits only the
  second (Great), and the first is missed — although +5.4 f is inside a lone touch's Good window. The switch lies at
  4.7–4.9 f, the midpoint (4.75 f): **the windows of overlapping notes are cut at the midpoint**, as `cut_overlaps`
  does, and a press goes to the note whose (cut) window it falls in. Disagreements: four at the Marvelous/Great edge
  (−2.6…−3.7 f) and one outlier: a union press at +4.4 f after the first gave Great + Great (ours Great + Miss; the
  second note at −5.1 f, outside its cut early window — two Great changes 25 ms apart). Three other presses at
  +4.1…+4.7 f did not hit the second note, so this stays a single unexplained case.
- **Side-by-side touches 9.5 f apart (7 groups):** one press over both 5.6 f after the first gave Good (first, late)
  + Great (second, early) — one press hits an earlier open note and a later one when their arcs do not overlap, as
  ours (the hit note's lanes are consumed, the rest go on).
- **Held finger on a touch (四次元, 41 notes):** a finger landing 7.8–16.9 f before the note and held to −6…+6 f is
  a **Miss** every time (41 / 41): a touch needs a press inside its window (our rule).

### 10.3 Windows (Hard; frames from each run's touch centre, 50 % points, `runs/e6/report.md`)

| Class | Marvelous | ≥ Great | ≥ Good | n | Table |
|---|---|---|---|---|---|
| Touch | −3.24 / +3.13 | −5.40 / +5.07 | −6.08 / +5.80 | 532 | 3/3, 5/5, 6/6 (Normal E3: −2.97/+3.04, −4.94/+4.81, −5.73/+5.86) |
| Slide (early side) | −5.5 | −8.1 | −10.4 | 107 | 5, 8, 10 |
| Snap in (early side) | −6.0 | −8.2 | −10.2 | 23 | 5, 8, 10 |
| Snap out | −7.5 / +6.0 | −10.0 / +8.3 | −10.0 / +10.5 | 21 | 7/5, 10/8, 10/10 |

Snap-out samples: −10.5 Miss ×2, −9.5 Gr, −7.5 M/Gr, −3.5…+5.0 M ×9, +7.0 Gr ×2, +8.0 / +8.5 Gr/Gd, +10.5 Gd/Miss —
the late Marvelous edge lies between +5 and +7 f (fit midpoint +6), consistent with 5. The snap-in Marvelous edge
rests on one Marvelous at −6.0 f (next to Great at −5.5 and −6.5 f). **Hard uses Normal's windows; slide and snap
rows match `JudgeParameterTable` to ±1 f.**

### 10.4 Parity (`judge_parity`, 18 runs: 7 E3 + 4 E5 + 7 E6; exclusions of §10.1)

| Class | Agree / compared | > 1 f from every edge |
|---|---|---|
| Touch | 1,435 / 1,649 | 747 / 749 |
| Hold | 154 / 174 | 86 / 86 |
| Chain | 168 / 182 | 111 / 111 |
| Slide | 147 / 165 | 90 / 90 |
| Snap in | 36 / 42 | 22 / 22 |
| Snap out | 25 / 29 | 14 / 14 |
| Two-note groups (E6) | 102 / 107 | – (5 at an edge, §10.2) |

The two touch disagreements away from an edge are early strays at −7.3 and −7.4 f (real Good, ours Miss; the §8.2
one and one in 195356), 1.3–1.4 f past the Good edge. E6 alone: touch 349 / 350, slide 63 / 63, snap in 15 / 15, snap
out 14 / 14, chain 31 / 31, hold 8 / 8 away from the edges.

**No rule changes:** the chord rule, the window cut between overlapping notes and every window agree with the real
game. The UNVERIFIED notes on `Judge::press` and `cut_overlaps` are replaced by pointers here; new tests
`chord_notes_are_judged_on_their_own_presses`, `close_overlapping_pair_press_goes_to_the_nearer_note`,
`one_press_hits_an_earlier_open_note_beside_the_target`, harness `e3::tests::group_probes_reach_our_judge` (and `resting_finger_does_not_hit_touch` now cites §10.2).
Autoplay corpus unchanged (judge untouched).

Open: overlapping simultaneous touches (no isolated case on a guest chart), the one +4.4 f double hit above,
Expert/Inferno (locked for guests).

### 10.5 Reproduce
```
chart_stats data\songs.json <assets> --difficulty Hard > hard.tsv
e3_script --chart <assets>\MusicData\S03-005\S03-005_01.mer --spec tools\realgame\scripts\e6-mixed.spec.json --out tools\realgame\scripts\e6-king-h81.taps.json
e3_script … S00-030_01.mer --spec e6-pair.spec.json (seeds 101, 102); S02-225_01.mer --spec e6-snap.spec.json (91),
  e6-snap-edges.spec.json (92, 93), e6-touch-held.spec.json (111)
run_experiment … --nav tools\realgame\scripts\e6-guest-{king,suna,yojigen}-hard.nav.json   (as §7; data\nav\cat-vocaloid.png and the jackets)
analyze_e2 <run> --compare 0 --timing-block 8 ; analyze_e3 <runs>… --report runs\e6\report.md
judge_parity <7 E3 runs> <4 E5 runs> <7 E6 runs> --report runs\e6\parity-before.md --table runs\e6\parity-before.csv
```
Note: the fourth run (195356) was started 2 min 13 s after the previous one released the lock (rule: ≥ 3 min);
all others kept the gap.
