# 02 — WACCA data tables (UE4 DataTables)

Source: `assets/Table/*.uasset` + `*.uexp` (read-only). Parser: `tools/research/ue4_datatable.py`.
JSON dumps: `docs/research/tables/<Table>.json` (all 103 tables).

Tags used below: **[TABLE]** = value read directly from the game data. **[WEB]** = from a cited community source.
**UNVERIFIED** = my interpretation, not confirmed by data or by a second source.

---

## 1. Parser approach

The format is the cooked UE4 split-export package:
- **Version fields.** `LegacyFileVersion = -7`, `FileVersionUE4 = 0` (unversioned), no custom versions.
- **Package flags.** `PackageFlags = 0x80000000` (FilterEditorOnly).
- **Property serialization.** It is tagged, not unversioned, which makes it self-describing.

**`.uasset` (summary).**
- Reads magic, the legacy and UE3 versions, the UE4 and licensee versions, the custom-version array, TotalHeaderSize, FolderName, PackageFlags, then `NameCount/NameOffset`.
- Reads `GatherableText` (count, offset), `Export` (count, offset) and `Import` (count, offset). There is no LocalizationId because the editor-only filter flag is set.
- Name map: each entry is an `FString` followed by a 4-byte hash pair.
- Import table entries are 28 bytes: `ClassPackage FName, ClassName FName, OuterIndex i32, ObjectName FName`. The row-struct name comes from the `ScriptStruct` import.

**`.uexp` (the single DataTable export, at offset 0).**
1. A tagged property list for the UObject. It contains only `RowStruct: ObjectProperty`, terminated by `None`.
2. An `int32` for the object-GUID flag (always 0).
3. An `int32` row count, then one entry per row: `FName RowName` followed by a tagged property list terminated by `None`.
4. The file ends with the 4-byte package magic `C1 83 2A 9E`. The parser checks that exactly 4 trailing bytes remain.

**Tag layout.**
- Common fields: `FName name, FName type, i32 size, i32 arrayIndex`.
- Type-specific extras: Struct adds `FName structName` plus a 16-byte GUID. Bool adds a `u8` value. Byte and Enum add `FName enumName`. Array and Set add `FName innerType`. Map adds `FName keyType` and `FName valueType`.
- Every tag then has a `u8 hasGuid` (and a 16-byte GUID if it is set).

**Supported value types.**
- Int, UInt32, Int64, UInt64, Int8/16, UInt16, Float, Double, Bool, Str, Name.
- Text: history −1, 0 and 11 (StringTable).
- Byte (raw `u8` or enum `FName`), Enum.
- Soft object and class paths, Object references (as an import/export index).
- Struct: native Vector, Vector2D, Rotator, Color, LinearColor, Guid, DateTime, IntPoint and Quat, with everything else read as nested tagged properties.
- Array, Set and Map.

**Robustness.** The tag size is always trusted. If a value fails to parse, or its size does not match, the raw hex is stored under `__unparsed__` or `__size_mismatch__` rather than aborting.

**Usage (stdlib only):**
```
python -I tools/research/ue4_datatable.py --dir assets/Table docs/research/tables
python -I tools/research/ue4_datatable.py assets/Table/JudgeParameterTable.uasset   # prints JSON
```

**Result: 103 of 103 tables parsed cleanly.** None failed, every table had 0 unparsed properties, and every table ended with exactly 4 trailing bytes. In practice the data only uses these types: Int, UInt32, Int64, UInt64, Float, Bool, Str, Byte (raw) and ObjectProperty (RowStruct). Every user-facing string, including song titles, is an `StrProperty` rather than an FText.

---

## 2. Judgement windows — `JudgeParameterTable` (row struct `JudgeParameterTableData`)

All fields are `IntProperty`. Raw values **[TABLE]**:

| Row        | Marv F/R | Great F/R | Good F/R | AdjustRate F/R |
|------------|----------|-----------|----------|----------------|
| Touch      | 3 / 3    | 5 / 5     | 6 / 6    | 100 / 100 |
| Hold (head)| 3 / 3    | 5 / 5     | 6 / 6    | 100 / 100 |
| HoldEnd    | 7 / 7    | 7 / 7     | 7 / 7    | 100 / 100 |
| Chain      | 4 / 4    | 4 / 4     | 4 / 4    | **20** / 100 |
| SlideLeft  | 5 / 7    | 8 / 10    | 10 / 10  | 100 / 100 |
| SlideRight | 5 / 7    | 8 / 10    | 10 / 10  | 100 / 100 |
| SnapUp     | 5 / 7    | 8 / 10    | 10 / 10  | 100 / 100 |
| SnapDown   | **7 / 5**| **10 / 8**| 10 / 10  | 100 / 100 |

**Units: frames at 60 fps (1 frame = 16.667 ms).**
- **Evidence.** The community-measured Touch windows are ±50.01 ms, ±83.35 ms and ±100.02 ms, which is exactly 3, 5 and 6 × 16.67 ms ([rhythmgames.wsx.moe/wiki/Wacca](https://rhythmgames.wsx.moe/wiki/Wacca)). Elsewhere the engine also uses frame counts (`RhythmGameRuleTable.ProtectInvolveRockFrameNum`).
- **Front/Rear meaning.** "Front" = early and "Rear" = late. This is UNVERIFIED, but it is the natural reading of the names. SnapDown is the mirror image of SnapUp.

Windows in ms (frames × 1000/60):

| Note | Marvelous | Great | Good | Miss beyond |
|---|---|---|---|---|
| Touch / Hold head | ±50.0 | ±83.3 | ±100.0 | ±100 ms |
| Slide L/R, SnapUp | −83.3 / +116.7 | −133.3 / +166.7 | ±166.7 | |
| SnapDown | −116.7 / +83.3 | −166.7 / +133.3 | ±166.7 | |
| HoldEnd (release) | ±116.7 (all tiers equal) | | | |
| Chain | ±66.7 (all tiers equal, so it is effectively Marvelous or Miss) | | | |

- **Miss beyond.** A note judged later than the Good window counts as Miss. This is UNVERIFIED: no Miss window is defined in the table.
- **Chain and HoldEnd.** All tiers are equal for these, so in practice the result is binary: Marvelous inside the window, Miss outside it. This is UNVERIFIED. It is consistent with the Missless Fighters wiki, which says chains have no attack judgement and a lenient follow-through, and that releasing a hold early degrades the result gradually.
- **`AdjustRateFront/Rear` (percent).** UNVERIFIED. My best guess is a scale applied to the Front and Rear windows, or to the player's timing offset, per note type. Chain has Front = 20, which would make it heavily early-lenient or early-asymmetric. That matches the wiki's note that chains have been more early-lenient since Lily R ([Missless Fighters: Game Mechanics](https://wiki.misslessfighters.club/wiki/Game_Mechanics)).

**Judge-timing option (`JudgeLineSettingTable`).**
- 201 rows, `ParamName` running from "－１０" to "＋１０" in steps of 0.1 (full-width strings).
- ARTEMiS calls this `judge_line_timing_adjust` with range −10..10 ([ARTEMiS const.py](https://gitea.tendokyu.moe/Hay1tsme/artemis/raw/branch/develop/titles/wacca/const.py)).
- Unit UNVERIFIED. Frames would be consistent with the windows table (±166 ms).

---

## 3. Score — `GameScoreTable` (single row `Data`)

| Field | Value |
|---|---|
| MaxScore | 1000000 |
| NotesScoreRate | 1.0 |
| JudgeScoreRateMarverous / Great / Good / Miss | 1.0 / 0.7 / 0.5 / 0.0 |
| CrearBonusScoreRate, AchievementBonusScoreRate | 0.0 |
| CrearBonusRate{HalfCombo,FullCombo,Master,SSS,SS,S} | all 0.0 |
| TargetAchievementRate{Master,SSS,SS,S} | 1.0 / 0.97 / 0.93 / 0.90 |

**Formula.**
- The note rates come from the table **[TABLE]**. Every combo and achievement bonus is 0, so the score is purely judgement-based.
- Reconstructed formula (UNVERIFIED in rounding and note weights):
  `score = floor(1,000,000 × Σ_i(w_i × rate(j_i)) / Σ_i w_i)`
  - `rate` = 1.0 for Marvelous, 0.7 for Great, 0.5 for Good, 0 for Miss.
  - `w_i` = 1 per judged object. The Missless Fighters wiki says R (bonus) notes count double and that hold and chain status does not change the weight ([Missless Fighters](https://wiki.misslessfighters.club/wiki/Game_Mechanics)).
- **Judged objects.** Whether a hold contributes its head and its end (HoldEnd is a separate judge row) as two objects is UNVERIFIED. The chart parser should count judged objects.
- **`TargetAchievementRate*`** (S 0.90, SS 0.93, SSS 0.97) disagree with `ClearRateTable` (SS = 950k). They look like a legacy, pre-"+" rank set and are probably unused because every bonus is 0. UNVERIFIED.

---

## 4. Grades (score ranks) — `ClearRateTable`

Note: `GradeTable` and `GradePartsTable` are player *titles* (称号), not score ranks.

| Rank | RequiredScore | RequiredHitCount |
|---|---|---|
| Master | 1,000,000 | 0 |
| SSS+ | 990,000 | 0 |
| SSS | 980,000 | 0 |
| SS+ | 970,000 | 0 |
| SS | 950,000 | 0 |
| S+ | 930,000 | 0 |
| S | 900,000 | 0 |
| AAA | 850,000 | 0 |
| AA | 800,000 | 0 |
| A | 700,000 | 0 |
| B | 0 | **10** |
| C | 0 | **1** |
| D | 0 | 0 |

- **Score thresholds.** Master through A match the Missless Fighters wiki exactly.
- **B and C disagree with the wiki.** The wiki says B needs 30,001+ and C needs 1+. The table instead gates B and C on a hit count (≥10 hits and ≥1 hit). UNVERIFIED: the hit-count reading is my interpretation.
- **ARTEMiS grade enum** (1–13): D, C, B, A, AA, AAA, S, SS, SSS, MASTER, S_PLUS, SS_PLUS, SSS_PLUS.

**Combo lamps.**
- `RhythmGameRuleTable.FlawlesslyMissCount = 5` is the MISSLESS threshold (≤5 misses), which the wiki confirms.
- Full Combo = 0 misses. All Marvelous = every judgement Marvelous.
- ARTEMiS stores the client flags `flagCleared`, `flagMissless`, `flagFullcombo` and `flagAllMarvelous`.

---

## 5. Clear condition — `MusicParameterTable.ClearNormaRate*`

Each chart carries a gauge "norma" (pass line) as a fraction:

| Difficulty | Normal | Hard | Expert (`Extreme`) | Inferno |
|---|---|---|---|---|
| Typical value (392/395 songs) | 0.45 | 0.55 | 0.80 | 0.80 |
| Exceptions | 0.85 (3 test charts) | 0.85 (3 test charts), 0.45 (tutorial) | — | — |

- **Clear rule.** The song is CLEARED if the life gauge is at or above the norma when the song ends. The gauge starts empty and fills as you hit notes, and failing does not end the credit. Sources: the Moegirl wiki (via search) and Missless Fighters.
- **Gauge mechanics (UNVERIFIED).** No table contains the gauge increments per judgement. The best guess is that the gauge fills like the score ratio (Σ rate / N), with misses possibly draining it, since Missless Fighters says "Misses deplete it". This needs measuring or binary RE.

---

## 6. Player rating

**Tables [TABLE].**
- `UserRateCoefficientTable`: score at or above the threshold maps to a coefficient.
  - 990k → 4.0, 980k → 3.75, 970k → 3.5, 960k → 3.25, 950k → 3.0, 940k → 2.75, 920k → 2.5, 900k → 2.0.
  - 850k → 1.5, 800k → 1.0, 700k → 0.8, 600k → 0.7, 500k → 0.6, 400k → 0.5, 300k → 0.4, 200k → 0.3, 100k → 0.2, 1 → 0.1, 0 → 0.0.
- `UserRateTargetTable`:
  - `新楽曲` (new songs): RequiredDate 2021081007 (the Reverse launch), CountMusicNum **15**, coefficient 1.0.
  - `旧楽曲` (old songs): RequiredDate 0, CountMusicNum **35**, coefficient 1.0.
- `UserRateStepTable` (rating colour tiers): 虹 rainbow 2500, 金 gold 2200, 銀 silver 1900, 銅 bronze 1600, 紫 purple 1300, 赤 red 1000, 黄 yellow 600, 青 blue 300, 黒 black 0.
- `LaunchDateTable` (version dates, format YYYYMMDDHH): 1.00 = 2019071805, 1.50 = 2020012307, 2.00 = 2020091707, 2.50 = 2021031107, 3.00 = 2021081007.

**Formula.**
- **Per chart:** `chart_rate = internal_level × coeff(score)`.
- **Player total:** sum of the best 15 charts from new songs plus the best 35 from old songs.
  - Missless Fighters describes the same 15 + 35 split and says the total is "rounded up".
  - ARTEMiS stores each song rating as `floor(level × coeff × 10)`, i.e. tenths ([ARTEMiS base.py](https://gitea.tendokyu.moe/Hay1tsme/artemis/raw/branch/develop/titles/wacca/base.py)). Its coefficient bands are a coarser approximation (10k steps, below 900k → 0).
  - rg-stats `wacca-rate.ts` matches the table down to 850k but uses 1.0 for everything below 850k ([rg-stats](https://app.unpkg.com/rg-stats@1.2.1/files/src/algorithms/wacca-rate.ts)).
  - **The game table above is authoritative.**
- **Open points (UNVERIFIED).**
  - Whether only one chart per song counts ("CountMusicNum").
  - How "new" is decided. The likely rule is a release or activation date ≥ 2021081007, which corresponds roughly to VersionNo 5.
  - The exact rounding.
- **Sanity check.** The maximum is about 14.1 × 4 × 50 = 2820, which is consistent with rainbow at 2500.

---

## 7. Song metadata — `MusicParameterTable` (row struct `MusicParameterTableData`, 395 rows, row name = UniqueID)

| Field | Type | Meaning / values |
|---|---|---|
| UniqueID | UInt32 | Song ID (3…3094; 99001–99099 are tutorial/test songs) |
| MusicMessage / ArtistMessage / Rubi | Str | Title, artist, kana reading (used for sorting) |
| CopyrightMessage | Str | Always "-" |
| VersionNo | UInt32 | 1 = WACCA, 2 = S, 3 = Lily, 4 = Lily R, 5 = Reverse, 0 = test (ARTEMiS uses 0-based versions) |
| AssetDirectory | Str | e.g. `S03-030` (chart and audio folder under `Content/MusicData/`) |
| AssetFullPath | Str | Developer's path, `D:/project/Mercury/Mercury/Content//MusicData/S03-030` (internal codename "Mercury") |
| MovieAssetName[Hard/Expert/Inferno] | Str | Background movie (`mv_S02_204`, "-" = none, "" = same as base) |
| JacketAssetName | Str | e.g. `S03/uT_J_S03_30` (texture asset path) |
| bValidCulture_* (11) | Bool | Region availability (ja_JP, en_US, zh_Hant_TW, en_HK, en_SG, ko_KR, CN variants, Offline, NoneActive) |
| bRecommend | Bool | Shown in the "recommended" list (31 songs) |
| WaccaPointCost | Int | 0 / 1000 / 2000 WP purchase cost |
| bCollaboration, bWaccaOriginal, Reserved | Byte | Always 0 (unused) |
| TrainingLevel | Byte | 0–3. UNVERIFIED: beginner/"training" bucket, which correlates with chart difficulty |
| Bpm | Str | Display BPM string (e.g. "150"; single values only in this dump) |
| HashTag | Str | e.g. `Nim_WAC` (purpose UNVERIFIED) |
| NotesDesigner{Normal,Hard,Expert,Inferno} | Str | Chart designers |
| Difficulty{Normal,Hard,Extreme,Inferno}Lv | Float | **Internal level with a decimal** (e.g. 12.6). 0.0 = no chart; 26 songs have Inferno. The display uses "+" for a fraction of .7 or more (wiki) |
| ClearNormaRate{…} | Float | Gauge clear line (§5) |
| PreviewBeginTime / PreviewSeconds | Float | Preview start in seconds and its length (usually 10 s) |
| ScoreGenre | Int | 0 = Anime/POP, 1 = Vocaloid, 2 = Touhou, 3 = 2.5D, 4 = Variety, 5 = Original, 6 = HARDCORE TANO\*C, −1 = test. The mapping was inferred from titles and is a high-confidence guess |
| MusicTagForUnlock0..9 | Int | Tag IDs. Tag0 duplicates the genre with another numbering (6 ↔ 0, 4 ↔ 1, 8 ↔ 2, 5 ↔ 3, 7 ↔ 4, 1 ↔ 5, 10000 ↔ 6). Tags 10001+ are event/collab/unlock tags (UNVERIFIED) |
| WorkBuffer | UInt64 | Always 0 |

Song counts by version: v1 105, v2 66, v3 74, v4 58, v5 88, test 4.

**`UnlockMusicTable`** (447 rows, `Data_<MusicId>_<n>`; n = 1..3 gives per-period variants).
- Fields: `MusicId, AdaptStartTime, AdaptEndTime (Int64, YYYYMMDDHH), bRequirePurchase, RequiredMusicOpenWaccaPoint (0/3000/5000/9000), bVipPreOpen, NameTag, ExplanationTextTag ("ゲート" = gate unlock), ItemActivateStartTime, ItemActivateEndTime, bIsInitItem, GainWaccaPoint`.
- For a personal reimplementation: everything is unlockable. Treat `bIsInitItem = true` with `bRequirePurchase = false` as the default unlock state.

**`UnlockInfernoTable`** (26 rows).
- Fields: `MusicId, bRequirePurchase, RequiredInfernoOpenWaccaPoint (718…30000), bVipPreOpen, NameTag, ItemActivateStart/EndTime, bIsInitItem, GainWaccaPoint`.

---

## 8. Other gameplay-relevant tables

**`RhythmGameRuleTable`.**
- `ProtectInvolveRockFrameNum = -1`, `NoteSpeedRate = 2.0`, `FlawlesslyMissCount = 5` (Missless), `BasicExp = 25`, `AdaptedStartDashLevel = 4`.
- `NoteSpeedRate` is probably a global multiplier on the user's speed option (1.0–6.0 per ARTEMiS). UNVERIFIED.
- "Start dash" is a beginner XP or level feature. UNVERIFIED.

**`RhythmGameAppearanceTable`.**
- `RequiredComboNumForEffect = 200`, `NormaPassEffectWaitTime = 10.0`, `ChangeGuideLaneAppearanceComboBorder = 200`.
- GuideLaneBrightness: Alpha 600 (normal) / 290 (movie); Gradation 3 / 6.
- `HoldNoteDivisionLength = 2.0`, `HoldNoteDivisionLaneWidth = 2`. These are mesh tessellation settings for hold bodies (UNVERIFIED).

**`RhythmGamePosTable` (UE units).**
- NoteAppearPos (0, −600, 0). Notes spawn at depth 600, i.e. the tunnel origin.
- ComboEffectPos and NormaEffectPos: (0, −820, 0).

**`SlideBonusTimingTable` / `TapBonusTimingTable`.**
- Each maps a `BpmBorder` (≤75, 100, …, 275, 9999) to a `BeatTiming`:
  - Slide: 1, 1, 2, 2, 2, 4, 4, 4, 4, 4.
  - Tap: 1 up to 175 BPM, 2 above that.
- Meaning UNVERIFIED. It is probably the interval of the repeating bonus or "R-note" visual/SE effect as a beat subdivision depending on BPM.

**`NotesColorTable`.**
- Default colour IDs: 5 Touch (`NormalNote_Color1`), 6 Chain, 7 Hold, 1 SnapUp, 2 SnapDown, 3 SlideR, 4 SlideL.
- 1001–1006 are support colours. `MusicSelectOptionNoteColorTable` maps the option index to these IDs.

**`NoteDesignTable`.** 5 designs (`uT_KZ_10..14` textures).

**Option tables.**
- `BarLineTable`, `RateInfoTable` and `PlayerLevelInfoTable` are OFF/ON toggles.
- `ScoreInfoTable` has 2 modes (`SCORE_INFO_PARAM_*_00/01`, labels in the localisation tables).
- All of them use the generic `MusicSelectOptionSettingTable` struct `{ParamName: Str, ParamMes: Str}` (string-table keys).

**Volume tables.**
- `BGMVolumeTable`, `GuideVolumeTable`, `Touch/Hold/Slide/Snap/Chain/SNoteVolumeTable` and `BonusEffectVolumeTable`.
- Each has 11 steps, 0–100 in steps of 10.

**`MusicSelectOption*` menus.**
- Top: Game / Display / Design / Sound.
- Game: Speed, JudgeLine, Mask, BackMovie, BonusEffect, Mirror, GiveUp.
- Sound: 11 entries. Design: 14. Display: 14. Beginner presets: 4.

**`WaccaPointTable`.** WP awarded per rank (D … Master) × level 1–15. Examples: Master on level 15 gives 2100; S on level 1 gives 900.

**`LevelUpTable`.** XP needed per level bracket (50 … 1000).

---

## 9. Sources

- Rhythm Game Database wiki, WACCA (judge windows in ms): https://rhythmgames.wsx.moe/wiki/Wacca
- Missless Fighters WACCA wiki, Game Mechanics (score rates, ranks, Missless ≤5, rate 15 + 35): https://wiki.misslessfighters.club/wiki/Game_Mechanics
- ARTEMiS WACCA module: https://gitea.tendokyu.moe/Hay1tsme/artemis/raw/branch/develop/titles/wacca/const.py and .../base.py
- rg-stats WACCA rate: https://app.unpkg.com/rg-stats@1.2.1/files/src/algorithms/wacca-rate.ts (docs: https://rgstats.readthedocs.io/en/latest/algorithms/wacca-rate/)
- Moegirl wiki, WACCA (gauge behaviour, via search summary only): https://zh.moegirl.org.cn/WACCA
