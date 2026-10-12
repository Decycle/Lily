# 08 — Experiment E1 results (touch mapping, latency) — IN PROGRESS

Design: doc 07 §7 (E1). Harness: `tools/realgame` (`run_experiment`, `analyze_e1`). Game: the user's install on
this laptop (675×1200 window, waccon_io.dll as segatools `mercuryio`), driven only through
`Local\WACCON_SHARED_BUFFER`. Images referenced below live under `runs/` (gitignored, not committed).

**Status (2026-10-12):** capture, navigation and the analysis tool are done and checked on live frames; the three
recorded E1 runs have **not** been made yet — live runs were paused by the user before they started. Everything
below marked *pending* needs those runs.

## 1. Procedure (fully automatic)

```
set WACCA_INJECT_OK=1
run_experiment --script tools/realgame/scripts/e1-lanes.taps.json --nav tools/realgame/scripts/nav-input-test.nav.json
analyze_e1 runs/<ts>
```

1. `run_experiment` launches the game (`launch.bat`), waits for the window and 30 s of boot, makes it topmost.
2. `--nav`: TEST → test menu (check: red cursor on CREDIT LOG) → SERVICE ×2 (check: red DEVICE TEST MENU) → TEST
   (check: red TEST OF SOUND·SPEAKER) → SERVICE (check: red TEST OF INPUT MACHINE) → TEST (check: white cell grid,
   no red menu cursor). Frames of every check are saved as `nav-*.png` (doc 07 §2.5).
3. Recording starts (ddagrab 120 fps VFR, lossless, doc 07 §4.3) plus loopback audio; the tap script plays after a
   1 s lead; 2 s tail; everything is stopped (game, amdaemon, inject, OverlayWebview, Waccon.Server).
4. `analyze_e1` decodes every frame, calibrates the grid on the first frame (`testscreen::fit`: centre and
   radii from the radial profile), reads every cell's brightness at 8 points around its centre (avoiding the
   digit; unlit ≈ 0, lit ≈ 190, threshold 100), and compares with `inject.jsonl`
   (`e1::analyze`): per tap detected / observed cells / mapping / on-latency (first frame showing it − press
   write) / off-latency / visible duration / DLL ack latency; plus frames with unexplained lit cells.

## 2. Results so far (live probe, single frames)

Probe: `wacca-realgame inject` with lane 0 (all rings) and lanes 20–22 (ring 0) held 2 s, frame grabbed in the
middle; idle frame before it.

| Check | Result |
|---|---|
| Grid calibration on the idle frame | centre (336.5, 562.9), r_in 196.6, r_out 316.4 px (score 162) |
| Lit cells, idle frame | none (max cell level 0) |
| Lit cells during the probe | lane 0 rings 0–3 and lanes 20, 21, 22 ring 0 — **exactly the injected set** (lit level 192, unlit max 0) |
| Lane 0 position / direction | just above 3 o'clock; lanes 20–22 at 120–138° counter-clockwise (upper left) → **`cells.rs` lane mapping confirmed** for these cells |
| Ring order | depth bit 0 lights the **innermost** ring → ring order confirmed |
| DLL ack latency (42 publishes) | mean 1.19 ms, p95 1.99 ms, max 2.02 ms |
| Write timing (actual − scheduled) | mean 0.000 ms, max 0.001 ms |

Frames: `runs/e1-probe/idle-input-test.png`, `runs/e1-probe/probe-lane0-lanes20-22.png` (crops of the input-test
screen), full frames `runs/e1-probe/nav0.png` (test menu), `nav1.png` (device test menu), `nav2.png` (input
test), `touch1.png` (probe).

## 3. Pending (needs the three recorded runs)

| Measure | Run 1 | Run 2 | Run 3 |
|---|---|---|---|
| Taps detected (of 21) | pending | pending | pending |
| Mapping correct (lanes 0–9, 15/30/45, rings of lane 0) | pending | pending | pending |
| On-latency mean / p95 (write → first frame) | pending | pending | pending |
| Shortest tap that shows (5 / 17 / 34 / 50 ms) | pending | pending | pending |
| Ack latency mean / p95 | pending | pending | pending |

## 4. Known limits

- Video timestamps are FFmpeg wall-clock reads (after hwdownload), so on-latency includes a few ms of capture
  delay and the 120 Hz composition; resolution ≈ 8 ms. Windows.Graphics.Capture QPC stamps (doc 07 §4) would
  remove this.
- The input-test screen shows raw sensor state; it confirms Waccon cell → game sensor mapping, not gameplay lane
  judging (E2).
- Another session drove a native app with synthetic mouse input on the same desktop during development; Waccon's
  mouse source would OR such clicks into the game's touches. `stray_frames` in `e1.json` flags them.
