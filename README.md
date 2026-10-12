# WACCA_slim

Native Rust reimplementation of WACCA's gameplay (charts, judgement, scoring) for desktop.

## assets/ (not in git — copied from the original game, personal use only)

Copied from `C:\Games\WACCA\WindowsNoEditor\Mercury\Content`:

| Folder | What | Format |
|---|---|---|
| `MusicData/<songid>/<songid>_0N.mer` | Charts, one file per difficulty | Plain text |
| `Sound/Bgm/*.awb` | Song audio banks | CRI AFS2 containing HCA (unencrypted, 48 kHz stereo) |
| `Sound/Bgm/MER_BGM*.uexp` | Cue tables (cue name -> track in .awb) | CRI ACB `@UTF` inside UE4 asset |
| `Sound/Se/` | Hit/UI sound effects | UE4 assets |
| `Table/` | Game data tables (judgement windows, scoring, song list, …) | UE4 DataTable (`.uasset` + `.uexp`) |
| `UI/Textures/JACKET/` | Album art | UE4 textures |
| `Message/` | Text (English pack v4.2) | UE4 assets |
| `Movie/` | Background videos | CRI Sofdec `.usm` |
| `UI/` | Menus, textures, fonts (`UI/Font`), jackets | UE4 assets |
| `Vfx/` | Hit/particle effects | UE4 assets |
| `Splash/`, `L10N/`, `Localization/` | Splash screens, localized (English) graphics/text | UE4 assets |
| `Sound/Voice/` | Character voices | UE4 assets |

Key tables: `JudgeParameterTable` (timing windows per note type), `GameScoreTable` (score rates, clear bonuses),
`MusicParameterTable` (song list/metadata), `ScoreInfoTable`, `RhythmGameRuleTable`.
