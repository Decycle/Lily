# 04 — WACCA (Reverse 3.07.01) gameplay mechanics

Scope: behaviour of the core play loop for a native Rust reimplementation — playfield, input,
per-note judgement, combo, score, clear gauge, grades, timing/offsets, play options.
Out of scope: multiplayer, coins, bingo, gates, unlock economy. Table *parsing* is covered by other
docs (`docs/research/tables/*.json`); this doc only cites table values where they define behaviour.

Confidence legend
- **CONFIRMED** – stated by the game's own data/text (tables, tutorial strings, exe symbols) or
  verified empirically here.
- **COMMUNITY** – from a community reimplementation/wiki, consistent with game data but not verified
  against the binary.
- **UNVERIFIED** – plausible guess / single source / contradictory sources. Needs disassembly or
  in-game testing. Every UNVERIFIED item is collected in §11.

Primary sources
- [S1] Game tables, parsed by other agent: `docs/research/tables/*.json` (JudgeParameterTable,
  NormaTable, GameScoreTable, ClearRateTable, RhythmGameRuleTable, MusicParameterTable,
  UserRate*Table, MusicSelectOption*).
- [S2] Table documentation: redive.world (yellowberryHN/redive)
  https://redive.world/info/data/tables.html /
  https://github.com/yellowberryHN/redive/blob/master/docs/info/data/tables.md
- [S3] Saturn — open-source WACCA-like game whose authors tuned behaviour against the real cabinet
  (Unity/C#): https://github.com/Zaphkito/Saturn (files under `Assets/__Scripts/RhythmGame/`,
  notably `ChartElements/Abstract/Note.cs`, `ChartElements/Notes/*.cs`,
  `_Managers/ScoringManager.cs`, `_Managers/NoteManager.cs`, `Loading/ChartLoader.cs`,
  `Misc/TouchState.cs`, `Misc/TouchRingManager.cs`, `Settings/PlayerSettings.cs`).
- [S4] MercuryMapper chart editor (C#): https://github.com/Yasu3D/MercuryMapper
  (`Rendering/RenderEngine.cs`, `Utils/MathExtensions.cs`).
- [S5] Fan wiki (English, translated from gamerch):
  https://wacca.fandom.com/wiki/Gameplay_Basics , https://wacca.fandom.com/wiki/Game_mechanics ,
  https://wacca.fandom.com/wiki/UI_Guide (read through the MediaWiki API; the HTML is paywalled 402).
- [S6] Japanese gamerch wiki, "ゲームシステム": https://gamerch.com/wacca-wiki/483894 ;
  song page used for note-count check: https://gamerch.com/wacca-wiki/484398
- [S7] In-game tutorial text (Japanese) decoded from `assets/Message/Chara01Message.uexp`
  (keys `TUTORIAL_*` in the matching `.uasset`).
- [S8] Strings from `Mercury-Win64-Shipping.exe`, extracted with
  `tools/research/gameplay_strings.py` (UE4 reflection names of native classes/properties).
- [S9] `Mercury/Config/DefaultEngine.ini` (`bUseFixedFrameRate=True`, `FixedFrameRate=60`).
- [S10] WACCA Rating Analyzer (rating multipliers): https://github.com/shimmand/wacca-rating-analyzer
  (`js/script.js`, `getMultiplierTable`), https://shimmand.github.io/wacca-rating-analyzer/
- [S11] Blog, "ステージXIIIに合格しよう" (hold judging notes):
  https://3qua9la-notebook.hatenablog.com/entry/2020/03/29/184053 (seen via search summary only)
- [S12] MercuryPlayerPlus web chart player: https://github.com/WenqiOfficial/MercuryPlayerPlus

---

## 1. Engine timing model

- The game is UE4 with a **fixed 60 fps tick** [S9]. All judgement windows in JudgeParameterTable
  are integers that line up with **frames at 60 Hz** (3, 5, 6, 7, 8, 10) [S1][S2]; Saturn and
  MercuryMapper both interpret them as `n × 16.667 ms` [S3][S4]. **CONFIRMED (frames), UNVERIFIED
  whether the game judges on frame indices (quantised) or on continuous time.** Exe has
  `JudgeStartFrame`, `JudgeEndFrame`, `JudgeNotePos`, `JudgeNoteSpeed` property names [S8], which
  suggests frame-index based judging.
- Touch input: the real touch controller streams a full 240-sensor frame roughly every **8 ms**
  ([S3] `TouchRingManager.cs` comment). So the game sees input at ~125 Hz but evaluates at 60 Hz.
- Recommendation for the Rust port: run judgement on a continuous millisecond clock with windows
  `frames × 1000/60 ms` (what Saturn does); optionally add a "frame-quantised" mode later once the
  binary is checked.

## 2. Playfield geometry

### 2.1 Ring, lanes, coordinates
- 60 angular **segments** ("lanes"), 6° each. **Segment 0 is at 3 o'clock (right); indices
  increase counter-clockwise; the top is between 14 and 15** [S3 `TouchState.cs`], consistent with
  MercuryMapper drawing `startAngle = position × −6°` in screen space (y down) [S4]. **COMMUNITY.**
- A note occupies `size` consecutive segments `[position, position+size)` mod 60, `size ∈ 1..60`.
  `size == 60` is a full circle (special-cased for swipe detection, §4.4).
- Notes only appear inside the **guide lane** area (tutorial: notes only fall within the guide
  lines) [S7]. Chart **masks** (`.mer` note types 12 = mask add, 13 = mask remove, with direction
  CCW/CW/center for the reveal animation) open/close lanes of the guide-lane background
  ([S3] `Mask.cs`, `NoteManager.ProcessMasks`). **UNVERIFIED** whether a note placed in a masked
  lane is still drawn/judged (assume yes: masks are background only).
- Game-option "Mask" (マスク濃度, 0..+4) is a different thing: it darkens the background/BGA and the
  centre fog [S5 UI Guide][S6]. Table `RhythmGameAppearanceTable` gives guide-lane alpha 600 (no
  video) / 290 (with video) and gradation 3/6 [S1].

### 2.2 Note travel and perspective
- Notes **spawn at the centre and travel outward** to the **judgement line at the outer rim**
  (tunnel/perspective look). Tutorial: hit when the note overlaps the 判定ライン [S7]. **CONFIRMED.**
- Approach time at speed 1.0 measured by the community as **≈3266.7 ms** [S3 `NoteManager.ScrollDuration`]
  or **≈3333.3 ms** [S4 `UpdateVisibleTime`], divided by the hi-speed option value:
  `visible_ms = ~3300 / speed`. Speed option range **1.0–6.0, step 0.1** [S6].
  `RhythmGameRuleTable.NoteSpeedRate = 2.0` [S1] is probably a base constant in the real formula.
  **UNVERIFIED exact constant** (3266.7 vs 3333.3); disassemble the code that reads `NoteSpeedRate`
  / `JudgeNoteSpeed` / `ForceHighSpeedRate` [S8].
- Radial position (perspective): with `x ∈ [0,1]` = linear progress from spawn (0) to judgement
  line (1) in *scaled* time, MercuryMapper uses (attributed to CG505 fitting the real game):
  `r(x) = 3.325·x / (13.825 − 10.5·x)` → r(0)=0, r(1)=1 (normalised to rim radius) [S4
  `MathExtensions.Perspective`]. Saturn equivalently moves notes linearly in Z from −6 to 0 in a
  perspective camera and scales them by `x` [S3]. **COMMUNITY.** Note thickness also scales with r.
- Chart **hi-speed gimmicks** multiply scroll speed from their timestamp (scaled time =
  piecewise-linear integral of hi-speed); **stop** gimmicks freeze scroll; **reverse** gimmicks
  (start / mid / end) make notes play backwards from the rim inward with an ease curve — Saturn uses
  a 0.25 × scroll-duration window for reverse visuals ([S3] `NoteManager.ProcessReverseGimmicks`,
  `ARhythmGameSequencerReverse` in exe [S8]). Visual only; judgement uses real time.
- After a note is hit it disappears immediately; unhit notes keep travelling past the line and are
  culled ~500 ms after their time [S3] (cosmetic).

### 2.3 Visual details useful for 1:1 look
- Note body is drawn slightly shorter than its hit span: MercuryMapper trims ~1 segment off each
  end and draws end caps (`TruncateArc`, ExcludeCaps: start −6°, sweep +12°) [S4]. Real game uses
  pre-built meshes per size 1..60 (`vMsh_*Note00_%02d_s` in exe [S8]). **COMMUNITY.**
- Note thickness option: 5 steps (NoteDesignTable 0–4) [S1][S2].
- Sync (simultaneous) notes get a connector/highlight (`SyncNote` [S8]; Saturn `SyncIndicator`).
- Bar lines: full-circle lines per measure, toggleable [S5].
- Hold body rendered as a surface between hold segments; table `HoldNoteDivisionLength = 2.0`,
  `HoldNoteDivisionLaneWidth = 2` (mesh subdivision) [S1].
- `RhythmGameAppearanceTable.RequiredComboNumForEffect = 200`,
  `ChangeGuideLaneAppearanceComboBorder = 200`: background/guide-lane effect changes at 200 combo [S1].
- Default note colours (fan wiki/gamerch): touch red/pink, chain yellow, hold gold, slide CW orange,
  slide CCW green (inverted with the slide-colour-support option), snap in/up red, snap out/down blue
  [S5 Game_mechanics][S6]. Exact RGB → NotesColorTable (other doc).

## 3. Touch input model

- 60 angles × 4 depth rings = **240 sensors**. Saturn convention: `depthPos 0 = outermost`,
  `3 = innermost (against the screen)` [S3 `TouchState.cs`]. On the cabinet the ring is split into two
  30-column halves (left/right controller) [S3 `TouchRingManager.cs`].
- **All four rings count** for touch/hold/chain (a note is "touched" if any depth in any of its
  segments is pressed) [S3 `PositionedChartElement.Touched`]. Rings only matter individually for
  **snap** detection (§4.5). **COMMUNITY.**
- Touch notes / hold starts need a **new press** (a sensor that went from off→on since the previous
  input frame) inside the note span; fingers already resting do not trigger them [S3
  `ScoringManager` uses `newSegments`]. Chain notes accept resting fingers (§4.2).
  **COMMUNITY / UNVERIFIED for hold start** (one translated source claims holds can be collected by
  simply touching as they pass — see §4.3).
- `RhythmGameRuleTable.ProtectInvolveRockFrameNum = -1` [S1]: name suggests a "巻き込み (involve)
  lock" protection — preventing one press from hitting several nearby notes — **disabled (−1)**.
  So one new press may hit every overlapping note whose window is open. **UNVERIFIED.**
- For the laptop port: map each OS touch contact to an (angle, radius) → (segment, ring); treat a
  contact as covering ±1 segment and the ring it is in (tune), and emulate the 8 ms sensor frame.

## 4. Note types and their judgement

`.mer` note-type IDs (from [S3] `Note.CreateFromNoteID`; parsing details in the chart-format doc):

| ID | Type | Variant |
|----|------|---------|
| 1 / 2 / 20 | Touch | normal / **bonus** / **R** |
| 3 / 21 | Snap **Up** (= "forward", toward the screen / inward, red) | normal / R |
| 4 / 22 | Snap **Down** (= "backward", toward the player / outward, blue) | normal / R |
| 5 / 6 / 23 | Slide **clockwise** (orange) | normal / bonus / R |
| 7 / 8 / 24 | Slide **counter-clockwise** (green) | normal / bonus / R |
| 9 / 25 | Hold start | normal / R |
| 10 | Hold segment (middle/joint) | — |
| 11 | Hold end | — |
| 12 / 13 | Mask add / remove | not a note |
| 14 | End of chart | not a note |
| 16 / 26 | Chain | normal / R |

Note: the exe has `SnapUp→vMX_2_SnapIn` and `SnapDown→vMX_2_SnapOut` materials [S8], and the
tutorial says the (blue) snap is slid *toward yourself* and the red one *toward the screen* [S7].
Clockwise/counter-clockwise mapping of IDs 5/7 is from [S3]/[S4]; the exe calls them
`SlideLeft`/`SlideRight` [S8] — **UNVERIFIED which of Left/Right is CW** (doesn't matter if IDs are
right).

### 4.1 Judgement windows (frames @60 Hz; Front = early, Rear = late) [S1][S2] **CONFIRMED values**

| Row | Marvelous F/R | Great F/R | Good F/R | AdjustRate F/R |
|-----|---------------|-----------|----------|----------------|
| Touch | 3 / 3 | 5 / 5 | 6 / 6 | 100 / 100 |
| Chain | 4 / 4 | 4 / 4 | 4 / 4 | **20** / 100 |
| Hold (start) | 3 / 3 | 5 / 5 | 6 / 6 | 100 / 100 |
| HoldEnd | 7 / 7 | 7 / 7 | 7 / 7 | 100 / 100 |
| SnapUp | 5 / 7 | 8 / 10 | 10 / 10 | 100 / 100 |
| SnapDown | 7 / 5 | 10 / 8 | 10 / 10 | 100 / 100 |
| SlideLeft | 5 / 7 | 8 / 10 | 10 / 10 | 100 / 100 |
| SlideRight | 5 / 7 | 8 / 10 | 10 / 10 | 100 / 100 |

In ms: 3f = 50 ms, 5f = 83.3, 6f = 100, 7f = 116.7, 8f = 133.3, 10f = 166.7.

- Miss: no explicit window; a note is Missed when its last (Good) window closes without a valid
  input [S3].
- Discrepancy: Saturn and MercuryMapper use **Marvelous −5/+5** for slides, while the table says
  5/7 [S3][S4] vs [S1]. Prefer the table. **UNVERIFIED** how `AdjustRateFront/Rear` (percent) is
  applied (Chain front = 20 → maybe the chain's early window is 20 % of 4 f ≈ 0.8 f; or it scales a
  window-overlap cut). Needs disassembly.
- Window boundary inclusivity: Saturn uses `early ≤ err < late`. **UNVERIFIED.**
- `TapBonusTimingTable` / `SlideBonusTimingTable` give BPM-banded `BeatTiming` (1/2/4) [S1][S2];
  exe has `TapBonusManager`, `SlideBonusManager`, `ASlideBonusEffectActor`, `ESlideBonusSeType`
  [S8]. These likely drive the *bonus-note effect/SE* cadence, not judgement. **UNVERIFIED.**

**Overlapping windows.** When a later note's early window overlaps an earlier note on overlapping
segments, Saturn/MercuryMapper split at the midpoint between the two notes' times (the earlier
note's late window and the later note's early window are cut to the midpoint) [S3 `ChartLoader`]
[S4 `CutOverlappingJudgementWindows`]. A note at the same tick as an overlapping **hold end** loses
its early Great/Good windows (only Marvelous-early remains) [S3][S4 `CutEarlyJudgementWindowOnHolds`].
**COMMUNITY**, believed to mimic the cabinet.

### 4.2 Touch
- Hit = new press anywhere within the note span while a window is open; judgement from timing error
  [S7: "anywhere inside the area is OK"][S3]. Early/late recorded for non-Marvelous (FAST/SLOW
  display) [S5][S6].

### 4.3 Hold (start / segments / end)
- **A hold produces exactly ONE judgement and ONE combo/note count.** Verified empirically: the
  note counts on gamerch for ワールズエンド・ダンスホール (NORMAL 200 / HARD 333 / EXPERT 692,
  R = 22 / 36 / 40) [S6] equal `count(all note records) − hold segments (10) − hold ends (11) −
  masks (12/13) − end (14)` for `S01-032_00/01/02.mer`, with R counts including type 25.
  **CONFIRMED.** Same counting is used by MercuryPlayerPlus [S12].
- Start is judged like a touch (3/5/6 frames). The player then follows the (possibly curving /
  resizing) hold surface. The **end does not have to be released** (tutorial: you don't need to let
  go at the end) [S7]; gamerch says the same [S6].
- Releasing too long mid-hold downgrades the result; blog [S11]: holding breaks give Good or Miss,
  "the end point has no judgement". Saturn model [S3 `HoldNote.Judge`]:
  - never touched at all → **Miss**;
  - start missed but body held later, or dropped for > **215 ms** (Saturn `LeniencyMs`, tuned by
    testing) at any point → **Good**;
  - otherwise → the start judgement.
  - Judgement & combo increment are applied **at the hold end time**.
  **COMMUNITY / UNVERIFIED** (the HoldEnd 7/7 row suggests the game instead allows ≈117 ms of
  slack before the end; exe names `ReHoldEffectName`, `ReHoldSEName`, `HoldMarvelousName`,
  `bHoldWhenFinished` [S8] indicate a "re-hold" state). Disassemble `AHoldStartNote` /
  `AHoldEndNote` / `AHoldJointNote` (source path `GameMain/RhythmGame/Note/HoldStartNote.cpp` is in
  the exe [S8]).
- Hold body touch test: any sensor inside the *current interpolated* segment span (Saturn checks the
  span of the active segment, not interpolated) [S3].

### 4.4 Chain
- Only **Marvelous or Miss** [S5 Game_mechanics]; window 4 f each side [S1].
- Resting fingers count: "you can just keep pressing beforehand" [S7]. Saturn: note is marked
  *touched* if any segment is pressed at any moment inside the window; it is awarded Marvelous when
  the clock reaches the note time (not earlier), Miss if the window closes untouched [S3
  `ChainNote.cs`, `ScoringManager`]. Chains are excluded from FAST/SLOW stats [S3]. **COMMUNITY.**

### 4.5 Snap (up = inward/toward screen, down = outward/toward player)
- Needs a **radial movement across the 4 depth rings** within the note span. Saturn [S3
  `SnapNote.Snapped`]: compare previous vs current touch frame; for each single segment and each
  pair of adjacent segments in the span, compute min/max pressed depth; Snap-Up (forward) fires if
  min or max depth **increased** (moved inward), Snap-Down if it **decreased**. One ring of movement
  is enough. **COMMUNITY.**
- Windows asymmetric: wider on the side the hand naturally lags (SnapUp late 7 f, SnapDown early 7 f).

### 4.6 Slide (clockwise / counter-clockwise)
- Swipe along the ring in the arrow direction while the note is in its window [S7].
- Saturn's empirical algorithm [S3 `SwipeNote.cs`]: per frame compute the **average offset** (in
  segments, measured from the note's start edge in the swipe direction) of all pressed sensors inside
  the note (each pressed depth counts separately); track the minimum average seen during the window;
  the slide is hit when `current_avg − min_avg > 1.9` segments (and < 30, to reject wrap-arounds).
  "1.9 chosen by testing with the original game". Full-circle (size 60) slides use four virtual
  size-30 sub-notes at 0/15/30/45. Hit time = time the threshold is crossed. **COMMUNITY.**
- Bonus slides (IDs 6/8) are gilded; they only change gauge gain/effects (§6).

### 4.7 R notes and bonus notes
- **R notes** (IDs 20–26, added in Lily R): judged exactly like their base type; worth **2× score
  weight** of a normal note; rainbow "R effect" on the note and cabinet LEDs (toggle: SNote effect
  option, `SNoteEffectTable`) [S5][S6][S3]. Exe: `JudgeCountContainsSNoteCount`,
  `RESULT_MESSAGE_RNOTES` [S8] (results screen counts R notes).
- **Bonus notes** (IDs 2, 6, 8 — gilded touch/slide): **no score effect**; when the *Bonus effect*
  option is ON they add more to the clear gauge (NormaTable `NormalBonus`/`SlideBonus` rows) and play
  bonus SE/effects [S5 UI Guide][S6][S1]. With the option OFF they behave as normal notes.

## 5. Combo

- +1 for every judged note that is not Miss (Marvelous/Great/Good), including chains; a hold adds
  +1 at its end (§4.3). **Miss resets combo to 0.** [S3][S6][S5]
- Max combo = total note count as defined in §4.3 (exe: `MusicMaxCombo`, `MaxComboNum` [S8]).
- Combo has **no effect on score** [S5][S6].
- Exe also has `CrearBonusRateHalfCombo` etc. in GameScoreTable, all **0** → no clear/combo bonus
  score in practice [S1][S2].

## 6. Clear (norma) gauge

- The game has a **norma gauge**; the song is **CLEARED if, when the song ends, the gauge is at or
  above the norma line**, otherwise FAILED [S7 tutorial][wacca.marv.jp howto]. No mid-song fail
  (except the optional Give-up, §9). **CONFIRMED.**
- Norma line per chart from MusicParameterTable: `ClearNormaRateNormal` (default **0.45**),
  `ClearNormaRateHard` (**0.55**), `ClearNormaRateExtreme` (Expert, **0.80**),
  `ClearNormaRateInferno` (**0.80**) [S1][S2]. **CONFIRMED** (per-song values may differ; read them).
- Gauge points per judgement (NormaTable, fields named SuperJust/Just/Good/Miss = Marvelous/Great/
  Good/Miss) [S1]:

  | Row | Marv | Great | Good | Miss |
  |-----|------|-------|------|------|
  | Normal (touch) | 10 | 7 | 4 | −5 |
  | NormalBonus | 15 | 10 | 7 | −5 |
  | Hold | 10 | 7 | 4 | −5 |
  | Chain | 2 | 1 | 1 | −5 |
  | Slide | 10 | 7 | 4 | −5 |
  | SlideBonus | 15 | 10 | 7 | −5 |
  | Flick (= snap) | 10 | 7 | 4 | −5 |

- **Gauge maximum: UNVERIFIED.** A Japanese catalogue wiki states that an all-theoretical play fills
  the gauge exactly to full (search summary of https://w.atwiki.jp/gcmatome/pages/7605.html). Best
  hypothesis: `NormaPointMax = Σ over notes of the Marvelous value of its *non-bonus* row`;
  gauge% = clamp(points, 0, max)/max; bonus notes (option ON) can overfill → reach the line sooner.
  Exe names supporting this: `NormaPoint`, `NormaPointMax`, `NormaPointMaxBonus`,
  `OriginalNormaPointMax`, `bValidNormaBonusByBonusNotes`, `NormaRate`, `NormaUpRate`,
  `NormaDownRate`, `AdditionalMissNorma` [S8]. Disassemble the function that initialises
  `NormaPointMax` (xref from NormaTable row lookups).
- Display: gauge in the centre; when above the line it turns red and the background turns yellow
  (search summary of NamuWiki, UNVERIFIED); `NormaPassEffectWaitTime = 10.0` s [S1].

## 7. Score, grades, clear lamps

- **Score = 1,000,000 × Σ(wᵢ·rᵢ) / Σ(wᵢ)**, where `rᵢ` = 1.0 / 0.7 / 0.5 / 0 for Marv/Great/Good/
  Miss (GameScoreTable `JudgeScoreRate*`) and `wᵢ` = 2 for R notes, 1 otherwise
  [S1][S2][S5][S6][S3]. `MaxScore = 1000000`. **CONFIRMED** (weights), rounding **UNVERIFIED**
  (Saturn uses integer floor of `1e6·cur/max`; the real game might accumulate per-note floats).
  Score display: *plus* (counts up) or *minus* (starts at 1,000,000, shows max attainable) [S5].
- **Achievement rate**: not a separate gameplay metric in single play; GameScoreTable
  `TargetAchievementRate*` (1/0.97/0.93/0.9) are targets for other systems [S2]. Treat "achievement
  %" = score/10,000.
- **Grades** (ClearRateTable, Reverse) [S1][S6]: MASTER 1,000,000 · SSS+ ≥990,000 · SSS ≥980,000 ·
  SS+ ≥970,000 · SS ≥950,000 · S+ ≥930,000 · S ≥900,000 · AAA ≥850,000 · AA ≥800,000 ·
  A ≥700,000 · **B: RequiredHitCount 10** · **C: RequiredHitCount 1** · D otherwise. (B/C are by
  number of hit notes, not score — **UNVERIFIED semantics** of "hit count"; older wiki tables quote
  B/C score bands, which belong to older versions.)
- **Clear lamps** [S5][S6]: All Marvelous (all notes Marvelous), Full Combo (0 Miss), Missless
  (1–5 Miss; `RhythmGameRuleTable.FlawlesslyMissCount = 5`), Clear, Failed. **UNVERIFIED** whether
  FC/Missless/AM are awarded on a *failed* gauge (fandom says "Clear with …", so assume they require
  clear; AM/FC automatically fill the gauge anyway).
- **Player rating** (not needed for gameplay, documented for completeness):
  per-chart rate = `chart_constant × coef(score)` with coef from UserRateCoefficientTable
  (≥990k 4.0, 980k 3.75, 970k 3.5, 960k 3.25, 950k 3.0, 940k 2.75, 920k 2.5, 900k 2.0, 850k 1.5,
  800k 1.0, 700k 0.8, 600k 0.7, 500k 0.6, 400k 0.5, 300k 0.4, 200k 0.3, 100k 0.2, ≥1 0.1, 0 → 0)
  [S1][S10]; player rate = Σ best 15 "new" charts (songs dated ≥ 2021-08-10 07:00, i.e. Reverse) +
  Σ best 35 "old" charts (UserRateTargetTable) [S1][S10]. Chart constants = `Difficulty*Lv` floats
  in MusicParameterTable [S2]. Rank badge thresholds: UserRateStepTable [S2].

## 8. Timing / offsets

- **Chart offset**: `.mer` header `#OFFSET` (seconds; audio start offset) and `#MOVIEOFFSET`
  (BGA) — chart-format doc. Saturn adds `#OFFSET` to the BGM clock [S3 `TimeManager.BgmTime`].
- **Player judgement offset** ("判定調整", option `JUDGE_LINE`): JudgeLineSettingTable has 201
  entries **−10.0 … +10.0 step 0.1, default index 100 (= 0)** [S1][S2]; it shifts judgement timing
  [S6]. **Unit UNVERIFIED**: Saturn's "Classic" mode code uses `1000/12 ≈ 83.3 ms per 1.0`, but its
  own comment says 8.33 ms per 1.0 (½ frame) — contradictory [S3 `PlayerSettings.cs`]. ±83 ms total
  range (8.33 ms/unit) is the more plausible. Disassemble the reader of JudgeLineSettingTable.
  Sign convention (does + mean "I hit late, move window later"?) also **UNVERIFIED**.
- Saturn assumes ~30 ms of inherent cabinet input latency in its "Classic" offset mode and a static
  −10 ms audio offset [S3] — useful only as a hint for a PC default.
- Port recommendation: separate `audio_offset_ms` (judgement clock vs audio) and `visual_offset_ms`
  (render clock vs judgement), plus the WACCA-style −10.0..+10.0 option mapped onto the judgement
  clock.

## 9. Song-select flow and play options

Flow (single play) [S5 UI Guide][wacca.marv.jp]: mode select → song select (sort, genres; locked
songs) → chart select (NORMAL / HARD / EXPERT / INFERNO; Expert normally unlocked by S on Hard) →
**options screen with live preview of the chart** → play → result (judgement counts, FAST/SLOW,
score, grade, lamp, R-note count) → next track (3 tracks/credit; irrelevant for the port).

Options — four menus (MusicSelectOptionTop: Game / Display info / Design / Sound) [S1]:

- **Game** (MusicSelectOptionGameSetting): Speed (1.0–6.0, step 0.1) · Judge line timing
  (−10.0..+10.0) · Mask density (none, +1..+4) · BGA movie (on/off/ask) · Bonus effect (on/off) ·
  Mirror (on/off; mirrors left↔right across the vertical axis; with segment 0 at 3 o'clock this
  is `pos' = (30 − pos − size) mod 60` (derived, UNVERIFIED), CW↔CCW slides swap, snaps unchanged;
  table can re-enable vertical/both mirrors [S2]) · Give-up (GiveupSettingTable 0–5: off, no-touch,
  S border, SS border, SSS border, personal-best border [S6]; "auto-fail when the condition can no
  longer be reached" [S2]; exe `ContinueMissCountNoInput` [S8]).
- **Display info**: Judgement position (centre / top / bottom / off; JudgeInfoPositionTable 0–3) ·
  FAST/SLOW detail (on/off; shown only for non-Marvelous) · Guide-line interval (None, A–G;
  GuideLineIntervalTable 0–7) · Guide-line mask/brightness (0–5) · Widget translucency (0–5) · Bar
  lines (on/off) · Centre info (CenterInfoTable 0–7: combo / score / border etc.) · Score display
  (plus/minus) · (rate, level, stage-up icon, multi rank, gate/bingo skip — ignore).
- **Design**: My colour (cabinet colour theme) · Note thickness (1–5) · per-type note colours
  (touch, chain, slide L, slide R, snap up, snap down, hold) · Slide colour invert · Touch effect
  "pop" · Touch effect "shoot" · Key beam · S/R-note effect.
- **Sound**: Touch SE set · BGM vol · Guide vol (pre-song metronome ticks) · per-note-type SE volume
  (touch, hold, slide, snap, chain, bonus, R) · Character voice.

## 10. Implementation checklist (behavioural contract for the Rust core)

1. Clock: ms, 60 Hz frame windows from JudgeParameterTable; offsets as §8.
2. Note list sorted by time; precompute per-note `[earliest, latest]` with overlap splitting (§4.1).
3. Input: 60×4 bool grid per input frame; derive `pressed_now`, `new_presses`, previous frame.
4. Per input frame, scan open notes in time order:
   touch/hold-start → new press in span; chain → any press in window, award at note time;
   snap → depth delta in span; slide → average-offset delta > 1.9; miss when window closes.
5. Holds: track held/dropped with leniency; finalise at end time (§4.3).
6. Score/gauge/combo updates on each judgement; lamps & grade at end; clear iff gauge ≥ norma rate.
7. Render: centre→rim perspective `r(x)`, hi-speed/stop/reverse scaled time, masks on guide lanes.

## 11. Open questions / needs disassembly (UNVERIFIED list)

| # | Question | Where to look in `Mercury-Win64-Shipping.exe` |
|---|----------|-----------------------------------------------|
| 1 | Is judging frame-quantised (60 Hz) or continuous? Window edge inclusivity? | `ANoteJudge`, `UFuncJudge`, uses of `JudgeStartFrame/JudgeEndFrame` |
| 2 | Meaning of `AdjustRateFront/Rear` (Chain front = 20) | function reading `JudgeParameterTableData` (xref the UTF‑16 string `JudgeParameterTable`) |
| 3 | Hold drop/re-hold leniency, HoldEnd 7/7 role, resulting judgement | `AHoldStartNote`, `AHoldJointNote`, `AHoldEndNote` (`HoldStartNote.cpp` path string) |
| 4 | Slide distance threshold & snap ring-delta rule | `ASlideNoteBase`, `ASnapNoteBase` (`SlideVel`, `FlickInitSpeedRate`) |
| 5 | Can resting fingers collect hold starts? Touch "new press" rule | `ATouchNote`, `AHoldStartNote`, `GetTouchState` |
| 6 | Gauge max (`NormaPointMax`) formula, clamping, bonus handling | xrefs of `NormaTableData`, `NormaPointMax`, `bValidNormaBonusByBonusNotes` |
| 7 | Exact approach-time constant and use of `NoteSpeedRate = 2.0` | `JudgeNoteSpeed`, `ForceHighSpeedRate`, `ARhythmGameSequencerNormal` |
| 8 | Judge-line offset unit (ms per 0.1) and sign | reader of `JudgeLineSettingTable` |
| 9 | Score rounding (per-note float accumulation vs final floor) | `UGameScoreManager`, `SetTotalScore` |
| 10 | B/C grade "RequiredHitCount" semantics; lamps on failed plays | ClearRateTable reader, result code |
| 11 | `ProtectInvolveRockFrameNum = −1` (multi-hit protection) | RhythmGameRuleTable reader |
| 12 | Tap/SlideBonusTimingTable purpose | `UTapBonusManager`, `USlideBonusManager` |
| 13 | Are notes in masked lanes drawn/judged? | `AGuideLane*`, mask handling in sequencer |
| 14 | Slide Marvelous window: table 5/7 vs community −5/+5 | in-game test |

How to confirm without installing heavy tools here: the exe keeps UE4 reflection data, so each
native class above has a `Z_Construct_UClass_*` / `StaticClass` registration that names its
properties; a disassembler (Ghidra or IDA Free, installed by the user later) can xref the UTF‑16
table-path strings (e.g. `DataTable'/Game/Table/JudgeParameterTable.JudgeParameterTable'` at file
offset 0x023cc340) and property names (e.g. `NormaPointMax`) to find the consuming functions.
Lighter alternatives: `rizin`/`radare2` with `/w` + `axt`, or a Python `capstone` script (not
installed). Dynamic option: since the game runs under segatools, a segatools-style hook DLL (or
Cheat Engine) could log `NormaPoint` / combo / judgement fields while playing a known chart.
Empirical tests on the user's PC (offset, slide distance, hold release) can settle #4, #5, #8, #14
without any reverse engineering.
