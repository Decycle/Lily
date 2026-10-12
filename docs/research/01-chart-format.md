# 01: WACCA chart format (`.mer`)

Status: research complete for parsing and timing. Gameplay semantics of reverse, stop and masks are only partly verified (marked **UNVERIFIED** below).
Scope: `assets/MusicData/<SongId>/<SongId>_NN.mer`, 1,247 files and 407 songs.
Tools: `tools/research/mer_stats.py`, `tools/research/mer_holds.py`, `tools/research/grep_strings.py`. Run each with `python -I <script> <path>`. All three are read-only.

---

## 0. Sources and confidence levels

Each claim is marked with one of these sources:

| Tag | Source |
|---|---|
| **[GAME]** | Unreal reflection strings from the original binary `C:\Games\WACCA\WindowsNoEditor\Mercury\Binaries\Win64\Mercury-Win64-Shipping.exe`, extracted with `grep_strings.py`. These are the game's own enum and field names. UE reflection lists enum members in declaration order, and the numeric values below match the data exactly. |
| **[DATA]** | Statistics over all 1,247 charts (`mer_stats.py`, `mer_holds.py`) |
| **[MM]** | MercuryMapper, the community chart editor for "Mercury" (WACCA's internal name): https://github.com/Yasu3D/MercuryMapper. Files used: `Data/FormatHandler.cs` (MerHandler), `Data/ChartElement.cs`, `Data/Chart.cs`, `Enums/*.cs`, `Rendering/RenderEngine.cs` |
| **[MP]** | MercuryPlayer / MercuryPlayerPlus, a browser chart player: https://github.com/WenqiOfficial/MercuryPlayerPlus (`main.js`), originally https://yellowberryhn.github.io/MercuryPlayer/ |
| **[WacK]** | WacK, a WACCA-like game with a `.mer` parser: https://spag-xalextra.duckdns.org/muskit/WacK/src/branch/master/Scripts/Data/Mer/Mer.cs, plus `song-pkg-format.md` in the same repository |
| **[TUT]** | MercuryMapper charting tutorial: https://siamesederp.github.io/MercuryMapper-Tutorial/Charting/ |
| other | reDIVE docs: https://github.com/yellowberryHN/redive (`docs/guide/custom-charts.md`) |

---

## 1. File-level structure

```
#MUSIC_SCORE_ID 0
#MUSIC_SCORE_VERSION 0
#GAME_VERSION
#MUSIC_FILE_PATH MER_BGM_S00_003
#OFFSET 0.003200
#MOVIEOFFSET 0.000000
#BODY
   0    0    2 180.000000
   0    0    3    4    4
   0  120    1   12    0   30   30    1    2
   ...
  86    0    1   14  520    0   60    1
```

### 1.1 Encoding and line format [DATA]

- 1,244 files are ASCII/UTF-8 with LF line endings, no BOM, and no blank lines.
- The 3 tutorial charts `S99-001_0{0,1,2}.mer` are UTF-16LE with a BOM and CRLF line endings. A parser must sniff the BOM: `FF FE` means UTF-16LE.
- Columns are separated by runs of spaces (right-aligned, width 4). Split on whitespace.
- Numbers can have leading zeros (`002`, `000` in the tutorial).
- **Garbage suffix:** `S99-001_01/02.mer` line 72 reads ` 048  480zxx    1   18 ...`. The game evidently parses with C `atoi`/`strtol` semantics (leading numeric prefix). The parser should do the same and use 0 when no numeric prefix exists, instead of rejecting the line.
- **SVN merge-conflict markers:** all three `S02-221_0N.mer` files contain `<<<<<<< .mine`, `||||||| .r22894`, `=======` and `>>>>>>> .r22915` lines in the header, with three `#OFFSET` lines (0.001000, 0.000000, 0.001). Rules: ignore header lines that do not start with `#`; for repeated tags, the last one wins (here 0.001). The actual game behaviour is **UNVERIFIED**, but any choice is within 1 ms.
- Body rows are **not guaranteed to be time-sorted**: 273 files have rows out of order (typically extra gimmick rows such as time signatures or hi-speeds appended out of order). Sort all events with a stable sort on `(measure*1920 + tick)` after parsing.

### 1.2 Header tags [DATA][GAME]

The game's header enum is `EMusicDataHeaderType`: `MUSIC_SCORE_ID, MUSIC_SCORE_DATA_VERSION, GAME_VERSION, MUSIC_FILE_PATH, START_OFFSET, MOVIE_START_OFFSET`. The struct fields are `MusicScoreId, MusicScoreDataVersion, MusicFilePath, MusicStartOffset, MovieStartOffset`.

| Tag | Meaning | Observed values |
|---|---|---|
| `#MUSIC_SCORE_ID` | id (unused) | `0` (1,223 files), `1` (24) |
| `#MUSIC_SCORE_VERSION` | data version | `0`/`1` |
| `#GAME_VERSION` | often empty | empty or `1` |
| `#MUSIC_FILE_PATH` | BGM asset name, e.g. `MER_BGM_S00_003` maps to `assets/Sound/Bgm/MER_BGM_S00_003.uasset/.uexp`. Several songs share one (`MER_BGM_S02_077` is used by 9 files). | |
| `#OFFSET` | `MusicStartOffset`, in seconds (float) | min −0.06205, max 3.0001. 260 files have `0.000000`. Most values are below 0.02. A cluster of about one measure length: 1.2 (200 BPM), 1.5 (160), 1.818 (132), 2.0 (90 BPM in 6/8), 3.0001 (80) |
| `#MOVIEOFFSET` | `MovieStartOffset` for the background video, in seconds | mostly 0. Values seen include 1.2 and 1.182266. |
| `#BODY` | start of the event rows | present in every file |

Extra tags appear only in the tutorial (`S99-001`): `#MUSIC_ID, #MUSIC_NAME(_RUBY) @JPN/@ENG ..., #ARTIST_NAME(_RUBY), #MUSIC_SCORE_CREATOR_NAME(_RUBY), #JACKET_IMAGE_PATH, #DIFFICULTY, #DISPLAY_LEVEL, #CREAR_NORMA_RATE [sic], #DISPLAY_BPM`. Ignore them. Song metadata such as level and title comes from the game tables, not from `.mer`.

`#OFFSET` can differ between difficulties of the same song (S03-011: −0.06205 / −0.06205 / −0.06205 / 0.01491), so it must be read per chart.

### 1.3 Difficulty suffix [GAME][DATA]

`EDifficulty { Unknown, Normal, Hard, Expert, Inferno }`. The binary contains the path format string `%s/%s_%02d%s` with `.mer`.

| Suffix | Difficulty | Files |
|---|---|---|
| `_00` | NORMAL | 407 |
| `_01` | HARD | 407 |
| `_02` | EXPERT | 407 |
| `_03` | INFERNO | **26** |

Songs with Inferno: S00-026, S00-032, S01-044, S01-056, S01-057, S01-076, S01-105, S01-205, S01-250, S01-251, S02-012, S02-021, S02-204, S02-235, S02-236, S02-243, S03-011, S03-023, S03-044, S03-045, S03-046, S03-047, S03-058, S03-062, S03-064, S03-089.

The S99-00x folders are system charts: S99-001 is the tutorial (UTF-16, contains the tutorial-only note kind 18). S99-002 and S99-003 are short system or demo charts (S99-002_00 is 15 s long).

---

## 2. Body rows

Every row starts with three integers [GAME] (`EMusicScoreBaseDataType`):

| Column | Game name | Meaning |
|---|---|---|
| 1 | `BAR_INDEX` | measure, ≥ 0 |
| 2 | `BAR_DIVISION_INDEX` | tick within the measure, 0..1919 [DATA: max 1919, none out of range]. **1920 ticks per measure, regardless of time signature.** |
| 3 | `DATA_KIND` | row kind (below) |

Absolute tick = `measure * 1920 + tick`.

### 2.1 Row kinds (column 3) [GAME][DATA][MM][MP]

`EMusicScoreDataKind` (declaration order = value):

| Value | Game name | MM name | Extra columns | Rows | Files |
|---|---|---|---|---|---|
| 0 | `INVALID` | | | 0 | 0 |
| 1 | `NOTE` | note | see §2.2 (5 or 6 more columns) | 1,382,340 | 1,247 |
| 2 | `BPM_CHANGE` | BpmChange | `bpm: f32` (`ChangeBpm`) | 2,500 | 1,247 |
| 3 | `BEAT_CHANGE` | TimeSigChange | `numerator [denominator]` (`ChangeBeatNumerator`, `ChangeBeatDenominator`) | 2,875 | 1,247 |
| 4 | `STOP` | (none) | none | **0** | 0 |
| 5 | `SPEED_CHANGE` | HiSpeedChange | `rate: f32` (`SpeedChangeRate`) | 351 | 39 |
| 6 | `REVERSE_START` | ReverseEffectStart | none | 2 | 2 |
| 7 | `REVERSE_END` | ReverseEffectEnd | none | 2 | 2 |
| 8 | `REVERSE_NOTE_END` | ReverseNoteEnd | none | 2 | 2 |
| 9 | `STOP_START` | StopStart | none | 8 | 2 |
| 10 | `STOP_END` | StopEnd | none | 8 | 2 |

Details:
- **BPM** (kind 2): 240 distinct values from 65 to **1962** (gimmick tempos exist). The value is a float with 6 decimals. In 4 cases there are multiple BPM rows on the same tick; the last row in file order wins (MM uses `LastOrDefault` at tick 0).
- **Time signature** (kind 3): 359 rows give only the numerator (`0 0 3 4`). The denominator then defaults to 4 [MM, MP]. Values seen include 4/4, 3/4, 2/4, 5/4, 6/4, 7/4, 8/4, 9/4, 10/4, 11/4, 12/4, 13/4, 16/4, 18/4, 1/4, 7/8, 5/8, 6/8, 3/8, 4/8 and 1/8. Several files stack 6 time-signature rows on tick 0 (S02-082, S02-083: 4/4, 3/4, 4/4, 3/4, 3/4, 4/4). **The last row on a tick wins.**
- Every chart has a BPM row and a time-signature row at tick 0 [DATA: 0 exceptions].
- **Hi-speed** (kind 5): 51 distinct values: negative (−1.2, −1.0, −0.75, −0.5, −0.25), near-zero (1e-5 to 0.01, used as quasi-stops), 0.1 to 3.0, and 20.0 (S01-044 Inferno). Rates are piecewise constant and multiply the visual scroll speed only. Their effect on timing is **UNVERIFIED** in the strict sense, but every tool treats them as visual only, and charts place notes inside slowed regions as normal.
- **Reverse** (6/7/8) always appears as an ordered triple `start ≤ end ≤ note_end` [DATA: 2 of 2 files]:
  - S02-082_02: 66:0 / 73:0 / 82:0
  - S03-019_02: 0:6 / 1:1913 / 2:240

  No playable notes lie inside `[start, end)`. 104 notes lie in `[end, note_end]` (S02-082).
- **Stop** (9/10) always appears in pairs `start < end` [DATA: S02-083_02, S02-255_02]. No notes lie strictly inside a stop window. Notes sit only on the start tick.
- Kind 4 (`STOP`) is never used.

### 2.2 Note rows (kind 1) [GAME][DATA]

```
measure tick 1 note_kind note_index lane lane_width visible_editor [extra]
```

| Column | Game name (`EMusicScoreNoteDataType` / struct field) | Meaning |
|---|---|---|
| 4 | `NOTE_KIND` / `NoteKind` | note kind (§2.3) |
| 5 | `NOTE_INDEX` / `NoteIndex` | unique id. **In every file it equals 0..N−1 in file order of note rows** [DATA: 0 violations]. Use it only for link resolution. |
| 6 | `LANE_INDEX` / `LaneIndex` | start lane 0..59 [DATA: min 0, max 59]. The ring has 60 lanes of 6° each. |
| 7 | `LANE_WIDTH` / `LaneWidth` | number of lanes covered, 1..60. The note covers lanes `lane, lane+1, …, lane+width−1` (mod 60). The data contains hold joints with width 0 and 2 (treat as ≥1; MM clamps to 1..60). 60 = full ring. |
| 8 | `VISIBLE_EDITOR` / `bVisibleScoreEditor` | 0 or 1. It is 1 on every note **except** hold joints, where 646,109 of 725,292 are 0. MM calls this "RenderSegment", MP calls it "change point". See §3. |
| 9 | kind-specific (optional) | Holds 9/10/25: `ChainRearNoteIndex` (next note index in the hold). Masks 12/13: `GuideLaneDirection`. Otherwise absent. |

Column counts [DATA]: 8 columns for kinds 1–8, 11, 14, 16 and 20–24. 9 columns for 9, 10, 12, 13 and 25. Exception: the tutorial writes 9 columns on **every** note with a trailing `0`, including hold ends (12 cases) and chains. **Interpret column 9 only for kinds 9, 10, 25, 12 and 13; ignore it elsewhere.**

### 2.3 Note kinds (column 4) [GAME][DATA]

`ENoteKind` (declaration order = value). "S" = the R-note variant ("_S" in the game, 2× score per [TUT]).

| Id | Game name | Common name / MM | Rows | Files | Judged? |
|---|---|---|---|---|---|
| 0 | `NOTE_KIND_INVALID` | | 0 | | |
| 1 | `TOUCH` | Touch | 312,777 | 1,238 | yes |
| 2 | `TOUCH_BONUS` | Touch, Bonus | 15,086 | 948 | yes |
| 3 | `SNAP_UP` | Snap **in** (toward centre), red; MM "SnapForward" | 11,821 | 913 | yes |
| 4 | `SNAP_DOWN` | Snap **out** (toward rim), blue; MM "SnapBackward" | 3,472 | 512 | yes |
| 5 | `SLIDE_LEFT` | Slide, orange; MM "SlideClockwise" | 14,762 | 1,053 | yes |
| 6 | `SLIDE_LEFT_BONUS` | Slide CW, Bonus | 3,779 | 916 | yes |
| 7 | `SLIDE_RIGHT` | Slide, green; MM "SlideCounterclockwise" | 14,741 | 1,057 | yes |
| 8 | `SLIDE_RIGHT_BONUS` | Slide CCW, Bonus | 3,928 | 917 | yes |
| 9 | `HOLD_START` | Hold start | 79,712 | 1,224 | yes |
| 10 | `HOLD_JOINT` | Hold segment / joint (path point) | 725,292 | 1,222 | no (path) |
| 11 | `HOLD_END` | Hold end (release) | 80,989 | 1,224 | yes (release, per [TUT]) |
| 12 | `GUIDE_LANE_BEGIN` | Mask **add** (lanes appear); MM "MaskAdd" | 25,616 | 1,233 | no |
| 13 | `GUIDE_LANE_END` | Mask **remove** (lanes vanish); MM "MaskRemove" | 23,125 | 1,026 | no |
| 14 | `EOF` | End of chart | 1,247 (one per file) | 1,247 | no |
| 15 | `GUIDE_SAME_TIME` | runtime only (simultaneous-note guide line) | 0 | | |
| 16 | `CHAIN` | Chain | 49,632 | 995 | yes |
| 17 | `GUIDE_LANE` | runtime only | 0 | | |
| 18 | `TUTORIAL_TAG` | tutorial step trigger, always lane 50, width 10 | 90 | 3 (S99-001 only) | no |
| 19 | `BAR_LINE` | runtime only (measure lines) | 0 | | |
| 20 | `TOUCH_S` | Touch R | 7,034 | 365 | yes |
| 21 | `SNAP_UP_S` | Snap-in R | 3,213 | 342 | yes |
| 22 | `SNAP_DOWN_S` | Snap-out R | 677 | 135 | yes |
| 23 | `SLIDE_LEFT_S` | Slide CW R | 1,696 | 304 | yes |
| 24 | `SLIDE_RIGHT_S` | Slide CCW R | 1,683 | 307 | yes |
| 25 | `HOLD_START_S` | Hold start R | 1,277 | 195 | yes |
| 26 | `CHAIN_S` | Chain R | 614 | 105 | yes |

Notes:
- Holds have no bonus variant in data. Snap bonus does not exist (MM writes snap bonus as plain 3/4). Chain bonus also does not exist.
- **EOF row**: `M T 1 14 idx 0 60 1`. It occurs exactly once per file, and no note is ever later than EOF [DATA]. Use it as the chart end and the song-end trigger.
- [TUT] gameplay notes, useful for later docs: R notes play a louder hit sound and are worth 2× score. Bonus notes (Touch and Slide only) play a different hit sound, have no score effect, and add extra life/gauge.
- Direction naming is **UNVERIFIED** against the physical cabinet. The game calls 5 "left" and 7 "right". MM renders 5 as clockwise and 7 as counter-clockwise, with lane index increasing **counter-clockwise** and lane 0 at the 3-o'clock position in MM's screen space (MM `RenderEngine.cs`: `startAngle = Position * -6`, `sweep = Size * -6` in Skia's clockwise-positive angle space). From the player's view at the bottom of the ring, "left" = clockwise, which is consistent. The physical lane-0 position and orientation on the real touch ring must be confirmed by the input/hardware research.

---

## 3. Holds

Hold = `HOLD_START` (9 or 25) → zero or more `HOLD_JOINT` (10) → `HOLD_END` (11), linked through column 9 (`ChainRearNoteIndex` = index of the **next** note in the hold).

Validation over all files [DATA]:
- 80,989 hold chains; every chain walks cleanly from start to end. There are no dangling links, no cycles, no orphan joints or ends, and no index referenced twice.
- **Links never go backwards in time.** 441 links connect two notes on the **same tick** ("jagged"/spike hold art, e.g. S01-025_02 has several joints at 2:1440 with widths 54/56/58). The renderer must tolerate zero-duration segments.
- Chain length is 2–hundreds of notes (33,863 holds are start+end only; 2,219 have ≥ 50 nodes).
- Adjacent hold nodes always differ by < 5 lanes in position (shortest circular distance). **6,946 links wrap across the 59→0 boundary** (raw |Δlane| > 30). Unwrap by choosing the shortest circular delta (as MP does) before interpolating.
- Column 8 on joints: 0 = generated intermediate joint (not shown in the official editor), 1 = authored control point. `mer_holds.py` compared the 646,000 flag-0 joints with linear interpolation (by tick, of unwrapped lane and of width) between the surrounding flag-1/start/end nodes:
  - lane: 95.8 % exact, 4.0 % within ±1, and 0.2 % further off (79 off by ≥ 10)
  - width: 92.8 % exact, 7.0 % within ±1, and the rest further off

  So the flag-0 joints are (nearly) a pre-baked linear sampling of the path between control points. **Recommendation: render and judge using all joints as stored** (the game's struct name `bVisibleScoreEditor` says the flag only controls editor visibility). The flag can be ignored at runtime. **UNVERIFIED** whether the game interpolates between joints linearly in lane space (assumed). MP re-interpolates linearly between flag-1 points; MM draws only flag-1 points by default.
- Hold rendering/judging between nodes: linearly interpolate `lane_start` (unwrapped) and `width` by time (or by scroll distance under hi-speed; MP does this when a hold crosses a negative hi-speed).

---

## 4. Masks (guide lanes)

Kinds 12 (`GUIDE_LANE_BEGIN`, "MaskAdd") and 13 (`GUIDE_LANE_END`, "MaskRemove"). Column 6 = lane, column 7 = width, column 9 = `GuideLaneDirection`.

`EGuideLaneDirection` [GAME]: `0 = PLUS`, `1 = MINUS`, `2 = PLUS_MINUS`. Counts [DATA]: 0: 8,077, 1: 8,042, 2: 32,622.

Semantics [MM `DrawMaskEffect`, MP `laneToggleList`]:
- Each of the 60 lanes has a boolean "visible/on" state. **Initial state: all off**, so MM paints the mask fill over every lane that is off.
- MaskAdd turns lanes `lane..lane+width−1` on. MaskRemove turns them off.
- It is animated as a sweep at about **one lane per 8.33 ms** (MP: `1000/60/2` ms per lane; MM: 8 ms per lane, 4 ms for centre mode because both halves move together):
  - `0 PLUS`: from `lane` toward increasing index (MM calls this counter-clockwise).
  - `1 MINUS`: from `lane+width−1` toward decreasing index (clockwise).
  - `2 PLUS_MINUS`: from the centre outward to both ends. MP uses centre→out for Add and edges→in for Remove; MM uses centre→out for both. **UNVERIFIED** which one matches the game.
- [TUT]: "Size 1 CW/CCW or size 2 Center masks appear instantly. Other masks rotate into position."
- Charts virtually always begin with a MaskAdd: 771 files at tick 0, 376 within the first measure, 86 later. **14 charts have no MaskAdd at all** (S01-028 ×3, S01-053 _00/_01, S02-057 ×3, S02-061 ×3, S02-201 ×3). In MM/MP these would show a fully masked ring, which seems implausible for the real game. **UNVERIFIED**: the game may instead default to all lanes on when a chart has no guide-lane events. Check these charts against gameplay video before finalizing.
- Masks are purely visual and are not judged.

---

## 5. Time model

### 5.1 Chart time (seconds) from (measure, tick): exact algorithm

Assumptions: BPM is quarter notes per minute, and a measure lasts `4 * num/den` quarter notes. [MM][MP][WacK] all agree. [DATA] sanity check: computed chart lengths to EOF are 15.0 s min (S99-002_00), median 135.3 s and max 226.9 s, which match typical WACCA song lengths.

```
seconds_per_measure(bpm, num, den) = 240.0 * num / (den * bpm)     // = 4*num/den beats * 60/bpm
```

Algorithm (precompute once per chart):

1. Parse all kind-2 and kind-3 rows into `(abs_tick, kind, value, file_order)`. Sort stably by `abs_tick` (file order breaks ties).
2. Collapse rows sharing a tick: for each tick, the last BPM row gives that tick's BPM and the last time-signature row gives its time signature (if any).
3. Require a BPM and a time signature at tick 0. All charts have them; as a fallback use 120 BPM and 4/4. A numerator-only time signature means `den = 4`.
4. Build `segments: Vec<TempoSegment { start_tick, start_sec, bpm, num, den }>`:
   ```
   cur = (tick 0, sec 0, bpm0, num0, den0)
   for each subsequent change tick t (ascending):
       sec_t = cur.start_sec + (t - cur.start_tick) as f64 / 1920.0 * seconds_per_measure(cur.bpm, cur.num, cur.den)
       push cur; cur = (t, sec_t, new bpm (or cur.bpm), new num/den (or cur's))
   push cur
   ```
5. `tick_to_sec(t)`: binary-search the last segment with `start_tick <= t`, then
   `seg.start_sec + (t - seg.start_tick) / 1920 * seconds_per_measure(seg)`.

   Use f64 throughout. MP rounds to whole ms per segment and MM uses f32; do not copy either.
6. Inverse `sec_to_tick` (for editors or seek): binary-search on `start_sec` and invert the linear formula.

Note that a time-signature change mid-measure is legal in the format, but the 1920-ticks-per-measure grid continues from the change point. The formula above handles this because it measures from `seg.start_tick`.

STOP_START/STOP_END and SPEED_CHANGE do **not** affect chart time (see 5.3). Reverse does not either.

### 5.2 Audio alignment: `#OFFSET`

The game field name is `MusicStartOffset`. [MP]: `chart_ms = audio_ms + OFFSET*1000`, i.e.

```
audio_time = chart_time - OFFSET          // seconds
chart_time = audio_time + OFFSET
```

So the BGM starts playing when the chart clock reaches `OFFSET`. With a negative OFFSET the audio starts before chart time 0.

Strong supporting evidence [DATA]: all "large" offsets equal **exactly one 4/4 measure at the chart's starting BPM** (1.2 s @200, 1.5 @160, 1.818 @132, 1.73 @139, 1.548 @155, 2.0 @90 in 6/8 = 3 quarter notes × 0.667 s, 3.0001 @80). Those charts have an empty lead-in measure while the BGM file starts directly with music. In every such chart the first note is at measure ≥ 1, which matches. Confidence: high. Still **UNVERIFIED** against real audio. Calibrate once by aligning a chart's first note against the decoded BGM onset.

`#MOVIEOFFSET` (`MovieStartOffset`) is presumably the same relation for the background movie. **UNVERIFIED**.

### 5.3 Visual scroll position (hi-speed, stop, reverse)

Notes approach from the centre to the rim (the judgement line) at the ring edge. Scroll is **time-based** (not beat-based): [MM] normalises by `startBpm/bpm * ratio/startRatio`, and [MP] integrates over milliseconds. BPM changes therefore do not change visual note speed; only SPEED_CHANGE and STOP do.

Define the speed multiplier `hs(t)` as a piecewise-constant function of chart time:
- It starts at 1.0.
- At each SPEED_CHANGE: `hs = rate` (can be negative or near zero).
- At STOP_START: `hs = 0`.
- At STOP_END: `hs` = the most recent SPEED_CHANGE rate before this tick, or 1.0 [MM; MP uses 1].

Scroll distance `D(t) = ∫_0^t hs(τ) dτ`. Precompute per segment, as for tempo.

A note at chart time `t_n` is drawn at depth `d = (D(t_n) − D(now)) / V`, where `V` is the visible window (in "scroll seconds") at the user's note-speed setting: 0 = at the judgement line, 1 = spawn point at the centre. Draw only notes with `0 ≤ d ≤ 1`. [MM] uses `V = 3.333 s / NoteSpeed` (comment: "approximately"). The exact speed table and the radial perspective curve are **UNVERIFIED** and belong in a separate rendering doc. With negative `hs`, `D` decreases, so notes recede toward the centre. With hs = 20, notes zoom in.

Reverse (REVERSE_START `rs`, REVERSE_END `re`, REVERSE_NOTE_END `rn`; times in chart seconds). [MP] implementation; the game's exact visual is **UNVERIFIED**. The game has `EReversePlayType { NONE, PLAY, STOP, NOTES_STOP }`, which suggests a small state machine.
- For `rs < now < re`: replace `now` with `now' = re + (re − now) * (rn − re) / (re − rs)`, which runs backwards from `rn` to `re`. Draw only notes with `t_n ≤ rn` (MP limits the draw distance to `rn − now'`).
- Effect: the notes of `[re, rn]` appear at the rim and rewind toward the centre during `[rs, re)`. After `re` they play forward normally.
- No judged notes exist inside `[rs, re)` [DATA], so judgement is unaffected.

---

## 6. Proposed Rust data model

```rust
pub const TICKS_PER_MEASURE: u32 = 1920;
pub const LANES: u8 = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BeatPos { pub measure: u32, pub tick: u16 }         // tick < 1920
impl BeatPos { pub fn abs_tick(self) -> u64 { self.measure as u64 * 1920 + self.tick as u64 } }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Difficulty { Normal = 0, Hard = 1, Expert = 2, Inferno = 3 }   // file suffix _00.._03

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bonus { None, Bonus, R }          // Bonus = life bonus (2,6,8); R = "_S" kinds (20..26)

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlideDir { Cw, Ccw }              // 5/6/23 = Cw ("LEFT"), 7/8/24 = Ccw ("RIGHT")  (UNVERIFIED physical)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapDir { In, Out }               // 3/21 = In ("UP", red), 4/22 = Out ("DOWN", blue)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskDir { Plus, Minus, Center }   // GuideLaneDirection 0/1/2 (default Center if missing)

/// Raw note kind exactly as in the file (keeps 18 etc.)
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawNoteKind {
    Touch = 1, TouchBonus = 2, SnapUp = 3, SnapDown = 4,
    SlideLeft = 5, SlideLeftBonus = 6, SlideRight = 7, SlideRightBonus = 8,
    HoldStart = 9, HoldJoint = 10, HoldEnd = 11,
    GuideLaneBegin = 12, GuideLaneEnd = 13, Eof = 14, GuideSameTime = 15,
    Chain = 16, GuideLane = 17, TutorialTag = 18, BarLine = 19,
    TouchS = 20, SnapUpS = 21, SnapDownS = 22, SlideLeftS = 23, SlideRightS = 24,
    HoldStartS = 25, ChainS = 26,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arc { pub lane: u8, pub width: u8 }   // lane 0..59, width 1..60 (clamp 0 -> 1)

#[derive(Clone, Debug, PartialEq)]
pub enum NoteBody {
    Touch,
    Snap(SnapDir),
    Slide(SlideDir),
    Chain,
    Hold { nodes: Vec<HoldNode> },   // nodes[0] = start, last = end; joints in between
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoldNode {
    pub pos: BeatPos,
    pub time: f64,          // chart seconds (filled after timing)
    pub arc: Arc,
    pub lane_unwrapped: f32, // lane after shortest-path unwrapping relative to the previous node
    pub editor_visible: bool,// col 8; informational only
}

#[derive(Clone, Debug, PartialEq)]
pub struct Note {               // a judged object
    pub pos: BeatPos,
    pub time: f64,              // chart seconds
    pub arc: Arc,
    pub bonus: Bonus,
    pub body: NoteBody,
    pub file_index: u32,        // NOTE_INDEX of the head row
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaskEvent { pub pos: BeatPos, pub time: f64, pub arc: Arc, pub add: bool, pub dir: MaskDir }

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gimmick {
    Bpm(f64),
    TimeSig { num: u32, den: u32 },
    HiSpeed(f64),
    StopStart, StopEnd,
    ReverseStart, ReverseEnd, ReverseNoteEnd,
}

#[derive(Clone, Copy, Debug)]
pub struct TempoSegment { pub start_tick: u64, pub start_sec: f64, pub bpm: f64, pub num: u32, pub den: u32 }
#[derive(Clone, Copy, Debug)]
pub struct ScrollSegment { pub start_sec: f64, pub start_dist: f64, pub speed: f64 }
#[derive(Clone, Copy, Debug)]
pub struct ReverseSection { pub start: f64, pub end: f64, pub note_end: f64 }

#[derive(Clone, Debug)]
pub struct ChartHeader {
    pub music_score_id: i32, pub music_score_version: i32, pub game_version: String,
    pub music_file_path: String,  // BGM cue/asset, e.g. "MER_BGM_S00_003"
    pub offset: f64,              // MusicStartOffset (s): audio_time = chart_time - offset
    pub movie_offset: f64,
    pub extra: Vec<(String, String)>, // unknown tags (tutorial)
}

#[derive(Clone, Debug)]
pub struct Chart {
    pub header: ChartHeader,
    pub notes: Vec<Note>,               // sorted by time; holds collapsed into one Note
    pub masks: Vec<MaskEvent>,          // sorted by time
    pub gimmicks: Vec<(BeatPos, Gimmick)>, // sorted, raw
    pub tempo: Vec<TempoSegment>,
    pub scroll: Vec<ScrollSegment>,
    pub reverses: Vec<ReverseSection>,
    pub tutorial_tags: Vec<BeatPos>,    // kind 18
    pub end: BeatPos, pub end_time: f64, // EOF (kind 14)
}
```

Parsing rules (summary):
1. Sniff the BOM (UTF-16LE/BE or UTF-8), normalise CRLF, and split lines.
2. Header: lines before `#BODY` starting with `#`. Each is `#TAG value…` and the last one wins. Ignore other lines (merge markers).
3. Body: split on whitespace and parse each token as an atoi-style leading number (int or float). Skip lines with fewer than 3 tokens. Ignore unknown kinds or note kinds, but log them.
4. Stable-sort everything by abs tick. Build tempo segments as in §5.1, then fill `time` for every object.
5. Holds: index map `NOTE_INDEX → row`. For each 9 or 25, follow column 9 until a kind-11 row. Collapse the chain into one `Note::Hold`. Mark joints and ends as consumed. Tolerate missing links by ending the hold at the last reachable node and warning.
6. Chains 16/26 are separate notes. Kind 14 gives `end`. Kind 18 gives `tutorial_tags`. Kinds 15, 17 and 19 should never appear in files.

---

## 7. Statistical summary (all 1,247 files)

Run: `python -I tools/research/mer_stats.py assets/MusicData`

- Files 1,247; songs 407; body rows 1,388,088; note rows 1,382,340.
- Row kinds: 1: 1,382,340 · 2: 2,500 · 3: 2,875 · 5: 351 (39 files) · 6/7/8: 2 each (2 files) · 9/10: 8 each (2 files) · 0 and 4: none.
- Note kinds: see the §2.3 table. Most common: hold joint 725 k, touch 313 k, hold end 81 k, hold start 80 k, chain 50 k, masks 49 k.
- Lanes 0–59; tick ≤ 1919; widths 1–60 (hold joints 0–60). Typical touch widths: 10 (127 k), 8, 15, 12. Slides typically 15/20.
- Hold integrity: 0 errors; 441 same-tick links; 6,946 wrap-around links.
- Unsorted files 273; UTF-16 files 3; files with merge-conflict markers 3; lines with garbage suffixes 2; numerator-only time signatures 359.
- Chart duration to EOF: 15.0 s to 226.9 s, median 135.3 s.

Run: `python -I tools/research/mer_holds.py assets/MusicData` for the hold interpolation and mask statistics in §3 and §4.

---

## 8. Open questions / UNVERIFIED items

1. **OFFSET sign.** Inferred as `audio = chart − OFFSET` from the field name, MP's code and the one-measure pattern. Confirm against decoded audio. (The BGM is in `Sound/Bgm/*.uasset/.uexp`, CRI-wrapped; decoding is a separate task.)
2. **Physical lane orientation.** Where lane 0 sits on the cabinet ring and which direction indices increase. MM convention: 3 o'clock, counter-clockwise. Must match touch-input mapping.
3. **Initial mask state** for the 14 charts without MaskAdd, and the add/remove sweep shape for PLUS_MINUS.
4. **Reverse and stop visuals**: exact game behaviour (MP model documented). Also whether hi-speed applies during reverse.
5. **Note-speed scale** (MM's 3.333 s at speed 1.0) and the perspective curve. These belong to the rendering research.
6. Whether the game uses flag-0 hold joints directly or re-interpolates. Recommendation: use them as stored.
7. Judgement windows: [TUT] gives frame tables (e.g. Touch Marvelous ±3 frames, Good ±6; Chain ±4 frames, Marvelous only; hold release window 250 ms) and notes that adjacent notes cut off each other's windows. These belong in a judgement doc and should be cross-checked against game tables (`assets/Table`).
