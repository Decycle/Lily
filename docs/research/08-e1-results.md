# 08 — Experiment E1 results (touch mapping, latency, minimum tap)

Design: doc 07 §7 (E1). Harness: `tools/realgame` (`run_experiment`, `analyze_e1`). Game: the user's install on
this laptop (675×1200 window, waccon_io.dll as the segatools `mercuryio` DLL). Every input went only through
`Local\WACCON_SHARED_BUFFER`. All runs were made on 2026-10-12, fully automatic: launch, navigation, recording,
stop. Run directories and images are under `runs/` (gitignored, not committed).

## 0. Summary

| Question | Answer |
|---|---|
| Lane/ring mapping of `cells.rs` (Waccon cell → lane/depth) | **Correct for all 240 cells.** Each cell was injected alone and exactly that screen cell lit, every time (240/240). Lane 0 is the segment just above 3 o'clock, lanes run counter-clockwise, ring 0 is innermost. |
| Detection of taps ≥ 17 ms | **100 %** (80 such taps in the E1 and short-tap runs, plus 240 × 100 ms taps in the all-cells sweep). |
| Shortest tap that always registers | **17 ms (one game frame).** Shorter taps register only sometimes, roughly in proportion to their length (3 ms: 4/10, 5 ms: 4/13, 8–14 ms: 60–70 %). This fits one sensor snapshot per 16.7 ms frame. |
| DLL acknowledgement (write → waccon_io.dll takes the record) | mean **1.1 ms**, p95 1.9 ms, max 2.1 ms on an idle system. waccon_io.dll polls at ~1 kHz, which confirms that `timeBeginPeriod` is in effect (doc 07 §8 item 9). |
| Injection → input-test screen (write → first captured frame showing the cell) | median **108–125 ms** per run, p10 102–113 ms. Our capture path adds ~29 ms of this (§3), so the game's own touch → display time on this screen is **~85–95 ms (5–6 frames)**. |
| Release → cell cleared on screen | median 108–125 ms, the same as press. The screen adds no hold or debounce. |

## 1. Procedure

```
set WACCA_INJECT_OK=1
run_experiment --script tools/realgame/scripts/<script>.taps.json --nav tools/realgame/scripts/nav-input-test.nav.json
analyze_e1 runs/<ts>      # prints the table, writes runs/<ts>/e1.json
```

1. `run_experiment` starts the game with `launch.bat`, waits for its window (~24 s) plus 30 s of boot, and makes
   the window topmost.
2. `--nav` drives the test menu with TEST/SERVICE through the shared buffer. After each press it checks a
   grabbed frame (doc 07 §2.5). All 7 runs reached TEST OF INPUT MACHINE on the first try. The check frames are
   saved as `runs/<ts>/nav-*.png`.
3. Recording uses ddagrab at 120 fps, VFR, lossless, with wall-clock timestamps (doc 07 §4.3). The script plays
   after a 1 s lead, then a 2 s tail. Afterwards the game, amdaemon, inject, OverlayWebview and Waccon.Server are
   stopped, and `stop()` waits until they are gone.
4. `analyze_e1` decodes every frame and calibrates the cell grid on the first frame. The fit gave the same result
   in every run: centre (336.5, 562.9), r_in 196.6, r_out 316.4. It reads each cell at 8 points around its centre,
   avoiding the digit; unlit cells read ≈ 0, lit ones ≈ 190, and the threshold is 100. Each press in
   `inject.jsonl` is then matched to the cells that turn on within [write − 30 ms, release + 400 ms]. The
   matching is pure code in `e1.rs`, with unit tests.

Scripts:
- `e1-lanes.taps.json` (doc 07 E1): lanes 0–9 on all rings, 200 ms each; lane 0 one ring at a time, 300 ms
  each; lanes 15/30/45; then lanes 20–23 with 17/34/50/5 ms taps.
- `e1-short-taps.taps.json`: 40 taps on 40 different lanes, all rings, 400 ms apart. Durations
  3/5/8/10/12/14/17/20 ms, 5 of each.
- `e1-all-cells.taps.json`: all 240 cells one at a time, 100 ms each, 250 ms apart.

## 2. Results per run

| Run (`runs/…`) | Script | Detected | Mapping correct | On-latency median / p10 / p90 ms | Off-latency median ms | Ack mean / p95 / max ms | Stray frames |
|---|---|---|---|---|---|---|---|
| 20261012-221737 | e1-lanes | 20/21 (5 ms missed) | 20/20 | 120.1 / 112.1 / 120.1 | 112.1 | 1.07 / 1.68 / 1.92 | 0 |
| 20261012-221930 | e1-lanes | 21/21 | 21/21 | 107.9 / 107.9 / 115.9 | 107.9 | 1.09 / 1.92 / 2.00 | 0 |
| 20261012-222039 | e1-lanes | 20/21 (5 ms missed) | 20/20 | 121.1 / 113.1 / 138.1 | 121.1 | 1.18 / 1.89 / 1.99 | 0 |
| 20261012-222152 | e1-short-taps | 31/40 | 31/31 | 111.3 / 102.3 / 119.3 | 117.3 | 1.01 / 1.89 / 1.91 | 0 |
| 20261012-222303 | e1-short-taps | 22/40 | 22/22 | 113.2 / 113.2 / 122.2 | 121.7 | 1.06 / 1.98 / 2.14 | 0 |
| 20261012-223327 | e1-all-cells | **240/240** | **240/240** | 124.8 / 107.8 / 191.8 | 124.8 | 1.12 / 1.90 / 2.00 | 2 |
| 20261012-222626 | e1-all-cells (**invalid**, CPU saturated) | 172/240 | 172/172 | 211.4 / 149.4 / 363.8 | 202.7 | 1.47 / 3.49 / 29.5 | 274 |

In all three E1 runs, every tap of 17 ms or longer was detected and correctly mapped. That covers lanes 0–9, the
rings of lane 0 one at a time, lanes 15/30/45, and the 17/34/50 ms taps. A detected cell was never in the wrong
place; every detected tap had a correct mapping.

The run marked invalid ran while other sessions were compiling Rust and the CPU sat at 100 %. Injector writes
were up to 17 ms late, acks up to 30 ms, and the capture timestamps lagged, so detection and latency fell apart.
The run was repeated once the machine was idle (CPU mean 32 % during the run): 240/240. The p90 of that repeat
(192 ms) is still higher than in the shorter runs. This looks like capture or encoder backlog over a 64 s
lossless recording rather than game behaviour: the off-latency shifts the same way, and the visible durations
stay correct. **Run experiments only when nothing else is loading the machine.**

### Minimum tap duration (5 runs pooled)

| Tap ms | Registered | Rate | Expected if sampled once per 16.7 ms frame |
|---|---|---|---|
| 3 | 4/10 | 40 % | 18 % |
| 5 | 4/13 | 31 % | 30 % |
| 8 | 6/10 | 60 % | 48 % |
| 10 | 7/10 | 70 % | 60 % |
| 12 | 6/10 | 60 % | 72 % |
| 14 | 7/10 | 70 % | 84 % |
| 17 | 13/13 | 100 % | 100 % |
| 20 | 10/10 | 100 % | 100 % |
| 34 | 3/3 | 100 % | 100 % |
| 50 | 3/3 | 100 % | 100 % |

When a short tap registers, it shows for exactly one game frame (16–17 ms). Taps of 17 ms or more always
register, and the screen shows them for as long as they were held (visible time = held time ± one capture tick).
This supports "one sensor snapshot per game frame" (doc 07 §1.4). The sample is small, so the sub-frame rates
are only roughly proportional to tap length. **Harness rule: hold every scripted touch ≥ 17 ms; keep the doc 07
default of 34 ms for margin.**

## 3. Capture latency calibration

The on-latency above includes our capture path. To measure that path, a borderless WinForms window was flipped
black/white 16 times while the same ddagrab pipeline recorded it, and the logged flip times were compared with
the first frame showing each flip. The result was 29 ms mean (23–40 ms). That covers WinForms paint, DWM
composition at 120 Hz, the ddagrab tick and hwdownload. The game presents through the same compositor, so the
game's own sensor → input-test display latency is about 115 − 29 ≈ **85–95 ms**. That is 5–6 frames, made up of
the serial buffer, the game tick and the render queue. Video timestamps fall on ddagrab's 120 Hz tick (8.3 ms
steps). Windows.Graphics.Capture QPC stamps would remove most of the capture term (doc 07 §4).

## 4. Frames

- Navigation (full frames): `runs/e1-probe/nav0.png` (test menu), `nav1.png` (device test menu), `nav2.png`
  (input test, idle). Each run also saves its own `nav-*.png` check frames.
- `runs/e1-probe/probe-lane0-lanes20-22.png`: crop of the probe with lane 0 on all rings and lanes 20–22 on ring 0.
- `runs/20261012-221737/crop-lane0-ring1.png`: lane 0, ring 1 only (second ring from the centre).
- `runs/20261012-221737/crop-lane30-allrings.png`: lane 30, just below 9 o'clock.
- `runs/20261012-221737/crop-lane20-17ms.png`: the 17 ms tap on lane 20.
- The screen's digits count touches per cell, which makes them a free cross-check of how often each cell
  registered.

## 5. Consequences

- `cells.rs` is confirmed; its UNVERIFIED note can go once E2 confirms gameplay lanes agree with the test screen.
- Scripted touches must be ≥ 1 frame (17 ms). For judgement experiments (E2), any sub-frame timing is quantised by
  the game's frame snapshot.
- The DLL is not a latency term (~1 ms). End to end, our injector → game display takes ~85–95 ms on this screen.
  Gameplay judging may use the input earlier than the display; E2 measures that against audio.
- The machine must be idle during runs. A future improvement is to log CPU load and injector write-error stats
  in `meta.json` and flag runs whose writes are > 2 ms late.
