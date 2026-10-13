# 05 — Save file, song select, results (WACCA Reverse, single-player)

Status: research draft, 2026-10-12. Items marked **UNVERIFIED** are inferred or from a single
community source. Items marked **[table]** come from game DataTables, either parsed by the table
agent (`docs/research/tables/*.json`) or read with `tools/research/peek_props.py` /
`dump_fstrings.py`. Items marked **[artemis]** come from the ARTEMiS server source.

Scope: song/chart select, difficulty select, play options, play, results, persisted personal
bests. Out of scope: multiplayer, credits, bingo, sugoroku, gates, trophies/titles, events, login.

---

## 0. Sources

- ARTEMiS WACCA module (Hay1tsme/artemis, branch `develop`):
  - schema: https://gitea.tendokyu.moe/Hay1tsme/artemis/src/branch/develop/titles/wacca/schema/score.py
  - handlers: https://gitea.tendokyu.moe/Hay1tsme/artemis/src/branch/develop/titles/wacca/base.py
  - constants (option IDs, grade/difficulty enums): https://gitea.tendokyu.moe/Hay1tsme/artemis/src/branch/develop/titles/wacca/const.py
- rg-stats WACCA rate (it matches the game table, see §1.4): https://app.unpkg.com/rg-stats@1.2.1/files/src/algorithms/wacca-rate.ts
- Tachi WACCA docs (lamps FAILED/CLEAR/MISSLESS/FULL COMBO/ALL MARVELOUS): https://docs.tachi.ac/game-support/games/wacca-Single
- Missless = 1–5 misses: TV Tropes https://tvtropes.org/pmwiki/pmwiki.php/VideoGame/WACCA and namu wiki https://namu.moe/w/WACCA.
  The game table agrees: `RhythmGameRuleTable.FlawlesslyMissCount = 5`.
- Moegirl (15 new + 35 old rate split): https://zh.moegirl.org.cn/WACCA. The game table agrees (§1.4).
- AquaDX and other WACCA servers were not read. ARTEMiS was enough. **UNVERIFIED**: other servers may store more fields.
- Game data:
  - `assets/Table/*`, parsed in `docs/research/tables/`: ClearRateTable, GameScoreTable,
    UserRateCoefficientTable, UserRateTargetTable, UserRateStepTable, MusicParameterTable,
    UnlockInfernoTable, UnlockMusicTable, JudgeLineSettingTable, RhythmGameRuleTable, and others.
  - English strings: `C:\Games\WACCA\WindowsNoEditor\Mercury\Content\Message\MusicSearchMessage.uexp`,
    `MusicSelectOptionMenuMessage.uexp`, `ResultMessage.uexp`.

---

## 1. What the real game tracks

### 1.1 Per-chart best record (ARTEMiS `wacca_score_best`) [artemis]

One row per (user, song_id, chart_id). chart_id is 1=NORMAL, 2=HARD, 3=EXPERT, 4=INFERNO.

| field | merge rule on a new play (`handle_user_music_update_request`) |
|---|---|
| `score` | `max(old, new)` |
| `best_combo` | `max(old, new)` |
| `lowest_miss_ct` | `min(old, new)` |
| `play_ct` | +1 |
| `clear_ct`, `missless_ct`, `fullcombo_ct`, `allmarv_ct` | +1 each when the play's flag is set |
| `grade_{d,c,b,a,aa,aaa,s,ss,sss,master,sp,ssp,sssp}_ct` | +1 for the play's grade (13 counters) |
| `rating` | `max(old, calc(new))`. ARTEMiS returns it but never writes it (bug); the client's later `user/rating/update` stores the value. |

- Clear status is not stored as a single value. It is derived from the counters: best lamp =
  highest lamp whose counter is > 0. So **best score and best lamp are merged independently**.
  A 1,000,000 can come from one play and a full combo from another.
- Each play is also logged (`wacca_score_playlog`): score, clear (sum of the 4 flags), grade,
  max_combo, marv/great/good/miss counts, fast/late counts, season, timestamp.
- Per-play flags the client sends: `flagCleared`, `flagMissless`, `flagFullcombo`, `flagAllMarvelous`,
  plus `grade`, `score`, `maxCombo`, `judgements{marvCt,greatCt,goodCt,missCt}`, `fastCt`, `slowCt`.

### 1.2 Grades [table: ClearRateTable]

ARTEMiS enum: D=1, C=2, B=3, A=4, AA=5, AAA=6, S=7, SS=8, SSS=9, MASTER=10, S+=11, SS+=12, SSS+=13.
These are storage IDs. Display order is by threshold.

| grade | RequiredScore | RequiredHitCount |
|---|---|---|
| MASTER | 1,000,000 | 0 |
| SSS+ | 990,000 | 0 |
| SSS | 980,000 | 0 |
| SS+ | 970,000 | 0 |
| SS | 950,000 | 0 |
| S+ | 930,000 | 0 |
| S | 900,000 | 0 |
| AAA | 850,000 | 0 |
| AA | 800,000 | 0 |
| A | 700,000 | 0 |
| B | 0 | 10 |
| C | 0 | 1 |
| D | 0 | 0 |

**UNVERIFIED** semantics of B/C/D: below 700k the grade seems to depend on a hit count
(≥10 hits → B, ≥1 → C, else D) rather than on score. Hit count probably means notes judged
better than MISS. Implement as written and keep it data-driven.

### 1.3 Score and clear [table: GameScoreTable, MusicParameterTable, RhythmGameRuleTable]

- MaxScore = 1,000,000. Judgement weights: Marvelous 1.0, Great 0.7, Good 0.5, Miss 0.
  All clear/achievement bonus rates are 0, so score = `floor(1e6 * Σweight / noteCount)`.
  **UNVERIFIED**: the rounding and whether hold/chain sub-judgements count as notes; see the chart/judge research.
- Clear = the gauge reaches the chart's `ClearNormaRate*` by the end. Per difficulty
  (e.g. Normal 0.45, Hard 0.55, Expert 0.8, Inferno 0.8). Values come from MusicParameterTable per song.
  **UNVERIFIED**: the gauge fill mechanics.
- Lamps, lowest to highest: FAILED < CLEAR < MISSLESS (1–5 misses; `FlawlesslyMissCount=5`)
  < FULL COMBO (0 misses) < ALL MARVELOUS (all Marvelous).
  **UNVERIFIED**: whether Missless/FC also require the clear gauge. Recommendation: a play that
  reaches the end with 0 misses always counts as cleared.
- Give Up (early quit) is a failed play. It still counts toward play count.

### 1.4 Rating (player "Rate") [table: UserRateCoefficientTable, UserRateTargetTable, UserRateStepTable]

- Chart rate = `chart_level_constant × coefficient(score)`. Use the decimal level from
  MusicParameterTable, e.g. 13.2. Coefficient table:

  | score ≥ | coef | score ≥ | coef |
  |---|---|---|---|
  | 990,000 | 4.00 | 800,000 | 1.00 |
  | 980,000 | 3.75 | 700,000 | 0.80 |
  | 970,000 | 3.50 | 600,000 | 0.70 |
  | 960,000 | 3.25 | 500,000 | 0.60 |
  | 950,000 | 3.00 | 400,000 | 0.50 |
  | 940,000 | 2.75 | 300,000 | 0.40 |
  | 920,000 | 2.50 | 200,000 | 0.30 |
  | 900,000 | 2.00 | 100,000 | 0.20 |
  | 850,000 | 1.50 | 1 | 0.10 |
  |  |  | 0 | 0.00 |

  This matches rg-stats. **ARTEMiS's `util_calc_song_rating` uses different bands** (e.g. 900k→1.0,
  930k→2.5) **and is wrong**. Use the table.
- Player Rate = Σ top **15** chart rates among "new" songs + Σ top **35** among "old" songs.
  Both have `CalcRateCoefficient = 1.0`. "New" means `RequiredDate ≥ 2021081007`
  (YYYYMMDDHH, the Reverse launch).
  - **UNVERIFIED**: which song date is compared with RequiredDate. Recommendation: treat
    `VersionNo == 5` (Reverse; 88 songs) as new, and keep it configurable.
  - **UNVERIFIED**: whether a song counts once (best chart) or each chart counts separately.
    Recommendation: each chart separately, as in CHUNITHM-style "best N".
  - Store chart rates as integer ×10, like ARTEMiS's `floor(level×coef×10)`, or as an f64 rounded
    to 3 dp, like rg-stats. **UNVERIFIED** which one the game displays. Recompute; never persist as truth.
- Rate color tiers (UserRateStepTable RequiredRate): 0, 300, 600, 1000, 1300, 1600, 1900, 2200, 2500
  (materials `Rating/uM_rate_000…008`). These values are on the same scale as the sum.

### 1.5 Options (ARTEMiS option IDs, const.py) [artemis + Message strings]

These IDs are the server's persistence keys. In-game menu labels are from
`MusicSelectOptionMenuMessage`.

| id | key | in-game label / values |
|---|---|---|
| 1 | note_speed | Speed Adjustment. ARTEMiS comment says 1.0–6.0. **UNVERIFIED**: step 0.5 and the real max |
| 2 | field_mask | Mask Strength 0–4 |
| 3 | note_sound | Note Touch SE (SESetTable id) |
| 4 | note_color | My Color (ColorTable id) |
| 5 | bgm_volume | BGM Volume 0–100 step 10 (VolumeStepListTable) |
| 7 | bg_video | Background Video: Confirm Each Time / Off / On |
| 101 | mirror | None / L-R (table has 2 rows; strings also list U/D and both, as "-" placeholders, probably disabled) |
| 102 | judge_display_pos | Judgement Display Position: Center / Bottom / Top (ARTEMiS also lists off) |
| 103 | judge_detail_display | Judgement Detail (FAST/LATE) on/off |
| 105 | measure_guidelines | Bar Line Display on/off |
| 106 | guideline_mask | Guide Line Mask 0–5 |
| 108 | judge_line_timing_adjust | Judgement Adjustment −10.0 … +10.0 step 0.1 (JudgeLineSettingTable, 201 rows) |
| 110 | note_design | Note Width 1–5 (NoteDesignTable) |
| 114 | bonus_effect | Bonus Effect on/off |
| 115 | chara_voice | Character Voice on/off (no navigator in scope, keep but ignore) |
| 116 | score_display_method | Score Display Mode: Plus / Minus |
| 117 | give_up | Give Up: Off / No Touch / S Border / SS Border / SSS Border / Personal Best Border |
| 118 | guideline_spacing | Guide Line Interval: None / Type A–G |
| 119 | center_display | Center Display: None / COMBO / SCORE+ / SCORE− / S / SS / SSS / PB border (8 rows) |
| 121 | stage_up_icon_display | out of scope |
| 122 | rating_display | Rate Display on/off |
| 123 | player_level_display | out of scope (no XP) |
| 124 | touch_effect | Touch Effect (Pop) on/off; Shoot and R-Note Effect are separate strings, no ARTEMiS id |
| 125–131 | guide / touch / hold / slide / snap / chain / bonus volumes | 0–100 step 10 |
| 133 | key_beam_display | Key Beam on/off |
| 201–204 | left / right / forward / back slide note color | |
| — | touch/chain/hold note colors, Invert Slide Color, Info Display Translucency (WidgetTranslucencyTable 6 steps), R-Note volume | in the menu strings; ARTEMiS id unknown |
| 1001 | master_vol | |
| 120, 132, 1002–1005 | rank display, gate skip, title/icon/nav/plate | out of scope |

Menu groups (MusicSelectOptionTop): Game Settings, Info Display Settings, Design Settings,
Sound Settings. Presets (MusicSelectOptionBeginner): Beginner / Intermediate / Advanced / Custom.

---

## 2. Our save file

### 2.1 Location and write safety

- Path: `%APPDATA%\wacca-slim\save.json` (Rust: `dirs::config_dir()` / `directories::ProjectDirs`).
  Keep `settings` in the same file. One file is easier to back up.
- Atomic write: serialize → write `save.json.tmp` in the same dir → `File::sync_all` →
  rename the current `save.json` to `save.json.bak` (keep 1, optionally a rotating `save.YYYYMMDD.json`)
  → rename tmp to `save.json`. On Windows use `MoveFileExW(MOVEFILE_REPLACE_EXISTING|MOVEFILE_WRITE_THROUGH)`
  or the `tempfile::NamedTempFile::persist` crate path. Write after every result screen and on
  options exit, never during play.
- Load order: `save.json` → if missing or it fails to parse, `save.json.bak` → else a fresh save.
  Never overwrite an unparseable file. Rename it to `save.corrupt-<ts>.json` first.
- `version` integer at the top. Migrations run `vN → vN+1` in code. Unknown fields are kept
  (`#[serde(flatten)] extra: Map`) so downgrades don't lose data.
- Key charts by `"<songId>:<diff>"`, with diff ∈ `normal|hard|expert|inferno`. songId = MusicParameterTable
  `UniqueID`. Song IDs are stable, which is safer than array indexes.

### 2.2 Schema (v1)

```jsonc
{
  "version": 1,
  "createdAt": "2026-10-12T10:00:00Z",
  "player": { "name": "PLAYER" },             // display only
  "settings": {                                // see §1.5; store by readable key, not ARTEMiS id
    "noteSpeed": 4.0, "judgeOffset": 0.0, "fieldMask": 0, "mirror": "none",
    "giveUp": "off", "bgVideo": "on", "bonusEffect": true,
    "judgeDisplayPos": "center", "judgeDetail": true, "guideLineInterval": "none",
    "guideLineMask": 0, "barLine": true, "centerDisplay": "combo",
    "scoreDisplay": "plus", "rateDisplay": true, "widgetTranslucency": 0,
    "myColor": 0, "noteWidth": 3,
    "noteColors": { "touch": 0, "chain": 0, "hold": 0, "slideLeft": "red", "slideRight": "green",
                    "snapUp": "red", "snapDown": "blue", "invertSlide": false },
    "touchEffectPop": true, "touchEffectShoot": true, "rNoteEffect": true, "keyBeam": true,
    "volume": { "master": 100, "bgm": 100, "guide": 0, "touch": 80, "hold": 80, "slide": 80,
                "snap": 80, "chain": 80, "bonus": 80, "rNote": 80 },
    "touchSe": 0,
    "audioOffsetMs": 0, "inputOffsetMs": 0     // device latency, separate from in-game judgeOffset
  },
  "songSelect": {                              // UI state that should survive restarts
    "lastSongId": 3023, "lastDifficulty": "expert",
    "sort": "default", "filter": "all", "favorites": [3023, 1056]
  },
  "records": {
    "3023:expert": {
      "bestScore": 987654,
      "bestScoreAt": "2026-10-12T10:20:00Z",
      "bestScoreJudgements": { "marvelous": 812, "great": 30, "good": 2, "miss": 1, "fast": 20, "late": 12 },
      "bestLamp": "missless",                  // failed|clear|missless|fullcombo|allmarvelous
      "bestGrade": "SSS",                      // derived, but cached for fast list display
      "bestCombo": 640,
      "lowestMiss": 1,
      "playCount": 7, "clearCount": 6, "misslessCount": 2, "fullComboCount": 0, "allMarvelousCount": 0,
      "gradeCounts": { "SSS": 2, "SS+": 3, "SS": 1, "A": 1 },   // optional, mirrors ARTEMiS
      "lastPlayedAt": "2026-10-12T10:20:00Z",
      "chartHash": "sha1:…"                    // hash of the .mer at record time; flags chart edits
    }
  },
  "history": [                                 // optional ring buffer (e.g. last 200 plays)
    { "at": "2026-10-12T10:20:00Z", "chart": "3023:expert", "score": 987654, "lamp": "missless",
      "grade": "SSS", "maxCombo": 640, "marvelous": 812, "great": 30, "good": 2, "miss": 1,
      "fast": 20, "late": 12, "gaveUp": false, "mirror": "none", "noteSpeed": 4.0 }
  ]
}
```

Merge on result:
`bestScore = max`. When it improves, also replace `bestScoreJudgements` and `bestScoreAt`.
`bestLamp = max(lamp order)`, `bestCombo = max`, `lowestMiss = min` (only for plays that reached the end),
counters +1. `bestGrade` = grade(bestScore).

Not persisted (derived on load from records + MusicParameterTable): chart rates, player Rate,
new/old split, totals (sum of best scores, lamp counts per level). Recompute so table or formula
fixes apply retroactively.

Don't store: unlock state (everything is unlocked), XP/level, items, titles. Exception: the RP total
(`reversePoints`, §4.1) is a running sum and cannot be derived.

Optional extra: per-chart overrides (`noteSpeed` per chart) **only if wanted**. The real game has
global options only (**UNVERIFIED**).

---

## 3. Song select spec

### 3.1 Data needed per song (from MusicParameterTable, 395 rows) [table]

`UniqueID`, `MusicMessage` (title), `ArtistMessage`, `Rubi` (reading; used for title sort),
`VersionNo` (1–5; ARTEMiS names: 1 WACCA, 2 S, 3 Lily, 4 Lily R, 5 Reverse; 4 rows have 0),
`ScoreGenre` (0–6, −1), `Bpm` (string, e.g. "178" or a range), `bRecommend`,
`JacketAssetName`, `AssetDirectory` (chart/audio folder),
`MovieAssetName*` (BG video per difficulty, optional), `PreviewBeginTime` / `PreviewSeconds`,
`NotesDesigner{Normal,Hard,Expert,Inferno}`, `Difficulty{Normal,Hard,Extreme,Inferno}Lv`
(decimal; `0` = chart absent; 26 songs have INFERNO), `ClearNormaRate*`, `CopyrightMessage`,
`bValidCulture_Offline` (all true). Plus the record summary from the save per chart.

- Genre names for `ScoreGenre` (**UNVERIFIED**, inferred from counts and the official genre order):
  0 Anime/POP (81), 1 VOCALOID (55), 2 Touhou Arrange (32), 3 2.5D (7), 4 Variety (64),
  5 Original (27), 6 HARDCORE TANO*C (125), −1 = hidden/test. Confirm from the UI genre textures.
- Difficulty names/colors: NORMAL (blue/cyan), HARD (yellow), EXPERT (red/magenta),
  INFERNO (purple/black). The table field is "Extreme", the UI label is EXPERT.
  **UNVERIFIED** exact RGB; take it from the UI assets.
- Level display: integer part, plus "+" when the decimal ≥ .7 (e.g. 12.7 → "12+").
  **UNVERIFIED**. Also show the exact decimal in a details panel.

### 3.2 Sort / filter

The game's "Music Search" list (`MusicSearchMessage`, English strings verbatim) mixes sorts and filters:

- Unlock Order (Last 2 Weeks) — drop (no unlocks); replace with "Version (newest first)".
- Difficulty Order (Highest) / (EXPERT) / (HARD) / (NORMAL)
- TOP 30 Ranking — drop (server data).
- Song Name (by `Rubi`)
- Uncleared / Uncleared (INFERNO|EXPERT|HARD|NORMAL)
- Unlockable / Locked — drop.
- Unplayed
- Full Combo Not Cleared (All Difficulties / INFERNO / EXPERT)
- SS / SSS / MASTER Not Cleared (All Difficulties / INFERNO / EXPERT)
- Cleared with 1~2 MISSes (INFERNO / EXPERT)

Recommended minimal set:

- Folders: All, Genre (0–6), Level (1…15, with + split), Version (WACCA … Reverse), Favorites,
  Recommended (`bRecommend`), INFERNO charts.
- Sort: Default (genre then ID), Song Name (Rubi), Artist, Level (selected difficulty), Version, Best Score.
- Filters: the game's "not cleared" family above, generalized to (lamp|grade) × (difficulty|all), plus Unplayed.
- Wheel wraps around and remembers the last song, difficulty, sort and folder (`songSelect` in the save).

### 3.3 Preview audio

When the cursor rests on a song for ~300 ms (**UNVERIFIED**; a debounce so scrolling isn't noisy),
play song audio from `PreviewBeginTime` for `PreviewSeconds`. Fade in ~0.5 s and fade out ~1 s
at the end, then loop. Stop/crossfade on cursor move. The menu BGM ducks while the preview plays.
Use the BGM volume setting. Audio decode/stream comes from the AWB/ACB work (separate research).

### 3.4 Difficulty selection

Difficulty selection is a 4-slot selector (NORMAL/HARD/EXPERT/INFERNO). Hide the INFERNO slot when
the level is 0. Changing songs keeps the selected difficulty. If that chart doesn't exist on the
new song, fall back to the highest existing one. Show for each slot: level, decimal constant,
best score, grade, lamp icon, and notes designer. Show chart rate and potential rate gain.

### 3.5 Unlock policy

Everything is unlocked. Ignore UnlockMusicTable (447 rows), UnlockInfernoTable
(26 songs; in-game cost `RequiredInfernoOpenWaccaPoint`, e.g. 1000 WP), `WaccaPointCost` and
the `bValidCulture_*` region flags. Hide only `ScoreGenre == -1` / `VersionNo == 0` rows,
and only if they lack charts or audio. **UNVERIFIED**: whether those are test entries.

### 3.6 Options entry

From song select, open the four option groups (§1.5). Settings are global and saved on exit.

---

## 4. Results screen spec

Shown after each play (including give-up):

- Jacket, title, artist, difficulty and level.
- Score (0–1,000,000), with a "NEW RECORD" flag and the difference from the previous best.
- Grade (§1.2) and lamp: CLEAR / MISSLESS / FULL COMBO / ALL MARVELOUS, or FAILED.
  Show the clear gauge final value against the norma.
- Judgement counts: Marvelous, Great, Good, Miss. FAST / LATE totals (the game shows these on the
  detail view; **UNVERIFIED** layout).
- Max combo / total notes.
- Rate: chart rate for this play and the player Rate before → after. Only show it when it changes.
  The game's ResultMessage contains "Rate" and "R-Notes" strings.
- Best-record comparison for this chart (best score, best lamp).
- Out of scope: items, bingo, stage-up, navigator lines, "Track N" counter
  (`<Number>曲目`; you could keep a session counter).
- Actions: Retry (same chart and options), Back to song select. The save is written before the
  screen accepts input.

### 4.1 Reverse Points, player level, countdowns (implemented)

- **Reverse Points (RP)** ("Obtained Reverse Points" in the rating strip) = `WaccaPointTable`
  [grade][`Level01`..`Level15`], keyed by the chart's whole level (clamped 1–15). RP is the game's
  WACCA Point (WP) under its Reverse name; the attract screen advertises "Earn 3x RP". **Verified** on
  the E3 real results: B on NORMAL 1 and NORMAL 3 → 210 (also when FAILED), AA on NORMAL 3 → 660.
  Boost items / campaign multipliers are left out. The save keeps the running total
  (`reversePoints`), shown as "RP" on the user plate (the real plate shows the guest's 210 after one
  play). ARTEMiS only stores `wp`/`wp_total` that the client sends; it has no formula.
- **Player level** ("Lv." on the plate): the client sends EXP as an item, ARTEMiS just adds it, so
  the amount per play is unknown. We use `LevelUpTable` (50 EXP per level from level 1, 100 from 3,
  200 from 5 … 1000 from 200) with a fixed **UNVERIFIED** 10 EXP per play (total play count over all
  charts). Title line: `settings.playerTitle` (default empty; the guest's is ゲスト).
- **Countdowns** (`LimitationTimeTable`): mode select 30 s, song select (incl. the genre wheel) 99 s,
  final check (our options screen) 25 s, results 15 s; two pink digits above GO, and at 0 the screen
  moves on as if GO was tapped. Off by default (`settings.menuTimers = false`): a home build has no
  time pressure.
- **RETRY tab**: the real results screen has a FAVORITE tab (pale-blue rim wedge, white disc icon,
  small turned label) right of GO. We keep RETRY, drawn as that tab in its place.

Remaining visible menu differences: see doc 14 (menu parity audit). Fixed there: the guest icon
ring (real icon texture), options screen (real final-check layout), results ring art (bands),
countdown font (SHINBI); still open: title key-art materials, mode art, per-genre backdrops; no
CREDIT/COIN header (arcade only).

---

## 5. Open questions / dependencies

1. Score rounding, and how holds, chains and slides contribute to the note count (needs chart/judge research).
2. Clear gauge mechanics, so ClearNormaRate can be applied.
3. B/C/D hit-count meaning; "+" level display cutoff; which date defines "new" songs for Rate.
4. Genre ID → name/color, and difficulty colors: confirm from UI assets
   (`Content/UI/Select/...`, `uMPC_MusicSortColor*`).
5. Note speed range/step: no speed table was found among the parsed tables. Check `RhythmGameRuleTable.NoteSpeedRate = 2.0` (base?).

Helper scripts (read-only, `python -I`): `tools/research/peek_table.py`,
`tools/research/peek_props.py`, `tools/research/dump_strings.py`, `tools/research/dump_fstrings.py`.
