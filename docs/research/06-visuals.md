# 06 – Visual assets (movies, UI, playfield, textures, fonts, materials)

Source: `C:\Games\WACCA\WindowsNoEditor\Mercury\Content` (read-only). Engine: **UE 4.19**
(`++UE4+Release-4.19` in `Mercury-Win64-Shipping.exe`), cooked, split `.uasset/.uexp(/.ubulk)`,
unversioned packages (FileVersionUE4 = 0, legacy version -7). Fixed 60 fps (`DefaultEngine.ini`
`FixedFrameRate=60`), forward shading, UI scale curve keyed on the shortest side = 1080 px.

Helper scripts (stdlib-only, run with `python -I`), all in `WACCA_slim/tools/research/`:

| script | what it does |
|---|---|
| `usm_probe.py <f.usm>` | parses CRID/@SFV/@SFA header @UTF tables + chunk stats (uses `cri_utf.py`) |
| `ue4_texture.py survey <Content>` / `png <x.uasset> <out.png>` | Texture2D platform-data reader; decodes B8G8R8A8, G8, DXT1, DXT5 to PNG (handles inline and `.ubulk` mips) |
| `ue4_widget_dump.py <BP_x.uasset> [--all]` | dumps UMG widget-tree exports (CanvasPanelSlot LayoutData, RenderTransform, brushes) via tagged props (reuses `ue4_datatable.py`) |

Sample PNGs in `docs/research/samples/` (`uT_FTdecision01` = "Marvelous", `uT_FTcombo00` = "COMBO",
`uT_RGclear02` (ubulk DXT1 gradient), `vT_NoteBase00_00_m` (DXT5 mask atlas), `vT_SlideNote00_00_m`,
`vT_JudgeLine00_00_m`, `vT_GuideLane01_00_m`, `uT_RGback00`, `uT_FTjudgment_00/01`, `uT_FTclear_000`).

---

## 1. Background videos (`Movie\`, 20 GB)

### Inventory
* 90 song movies `Movie\mv_SNN_NNN.usm` (127–562 MB each) + `Movie\SystemMovie\mv_S02_opening_1.usm` (30 MB).
* Each `.usm` has 3 sidecar UE assets: `mv_X.uasset` (CriWare `FileManaMovie`, `EManaMovieType::SofdecPrime`),
  `mv_X_Texture` (`ManaTexture` with Y/U/V component textures) and `mv_X_Texture_Mat` (material using
  `/CriWare/ManaYUVToRGB`, `MD_UI` domain). Only the `.usm` matters for us.
* Two files are not referenced by MusicParameterTable: `mv_S01_266`, `mv_S03_35` (unused/removed songs).

### Stream format (verified with `usm_probe.py` on all 91 files – all identical settings)
| field | value |
|---|---|
| container | CRI Sofdec2 USM (`CRID` + `@SFV` chunks only) |
| codec | `mpeg_codec = 1` → **"Sofdec.Prime" = MPEG-1 video syntax** (`00 00 01 B3` sequence header, GOP `B8`, picture `00`, slices `01..44`, no `B5` MPEG-2 extensions) with **11-bit intra-DC precision** (`mpeg_dcprec = 11`), signalled by a user-data block `00 00 01 B2 … "\0TMPGEXS\0" … "IDCPREC" …` |
| resolution | **1080 × 1080** (square; the cabinet screen is round), 68 slices/frame |
| frame rate | **60 fps** (`framerate_n 60000 / d 1000`), ~7 000–10 000 frames (2–2.8 min) |
| bitrate | ~20–24 Mbit/s (avbps), VBR |
| alpha | none (`alpha_type 0`, no `@ALP`) |
| audio | **none** (no `@SFA` stream; music comes from the ACB/AWB files, see audio research) |
| encryption | **none** – MPEG start codes and slice structure are visible after byte 0x40 of each payload (CRI masking would scramble them) |
| source name | CRID lists the original encode e.g. `pp_fix04.mp4` → `mv_pp_fix04.usm` |

Important decoder detail: plain MPEG-1 decoders assume 8-bit DC and produce wrong brightness/colour.
**FFmpeg handles it**: `libavcodec/mpeg12dec.c` detects the `\0TMPGEXS\0` user data and sets
`intra_dc_precision = 3; intra_matrix[0] = 1` (verified in current source,
https://github.com/FFmpeg/FFmpeg/blob/master/libavcodec/mpeg12dec.c, search `tmpgexs`). FFmpeg has no
USM demuxer, so the elementary stream must be pulled out of the USM chunks first (trivial).
WannaCRI lists "Prime" as extractable-untested (https://github.com/donmai-me/WannaCRI).
Colour matrix assumed BT.601 limited range (UNVERIFIED; `color_space 0`).

### Chunk layout (for a Rust demuxer, ~150 lines)
Big-endian: `sig[4] size u32 | u8 0, u8 payload_off(0x18), u16 pad | u8 chno, u16, u8 type(0 data,1 header,2 end/comment,3 seek) | u32 frame_time, u32 frame_rate(6000 = 60.00) | 8 bytes`.
Payload = `[8+payload_off, 8+size-pad)`. Concatenating all `@SFV` type-0 payloads gives a valid
MPEG-1 elementary stream (`.m1v`). Headers are CRI `@UTF` tables (same as ACB, see `cri_utf.py`).
Refs: vgmstream (https://github.com/vgmstream/vgmstream), WannaCRI (https://github.com/donmai-me/WannaCRI); layout verified by hand on these files.

### How movies are chosen per song
* `MusicParameterTable` fields: `MovieAssetName` (Normal; `"-"` = no movie), `MovieAssetNameHard`,
  `MovieAssetNameExpert`, `MovieAssetNameInferno` (empty = inherit Normal, `"-"` = none).
  87 songs have a movie; only song 2021 uses a different Inferno movie (`mv_S02_21_2`); a few rows set `"-"` on a single difficulty (e.g. 32 Inferno, 3011 Expert) to disable it there.
* `BackMovieSettingTable` is just the player option (default / OFF / ON) — names only, no mapping.
* `RhythmGameAppearanceTable`: `GuideLaneBrightnessParamAlphaMovie = 290` vs `...AlphaNormal = 600`,
  `GradationMovie 6` vs `Normal 3` → guide lanes are dimmed when a movie plays.
* Without a movie the 3D "BackGroundEffect" scenes play (see §6). In `Map\RhythmGame.umap` the movie is
  drawn on a `MovieBillboard`, effects via `BgEffectSceneCapture` → `Capture\cTRT_BackgroundEffect` render target.

### Recommended playback
* **Runtime (recommended):** own USM demuxer in Rust → feed `.m1v` packets to FFmpeg's `mpeg1video`
  decoder via `ffmpeg-next` (https://crates.io/crates/ffmpeg-next, link LGPL shared DLLs) → upload
  Y/U/V planes as three R8 textures → YUV→RGB in a WGSL shader (exactly what `ManaYUVToRGB` does).
  MPEG-1 1080²@60 decoding is cheap (single core, a few ms/frame). No re-encode, no extra disk.
* Alternative without FFmpeg: port/bind `pl_mpeg` (MIT, single-file C, https://github.com/phoboslab/pl_mpeg)
  and patch DC precision (predictor reset 1024, DC × 1 when TMPGEXS present) – ~1 day, UNVERIFIED but simple.
* Offline pre-convert (one-time `ffmpeg` after demux) to AV1/VP9 (decode with dav1d/libvpx) would cut 20 GB
  to ~3–5 GB at visually similar quality but needs a heavier decoder; to H.264 needs platform/ffmpeg anyway.
  Only worth it if disk size matters.

---

## 2. UI / menus (`UI\`, 2.4 GB)

| folder | contents |
|---|---|
| `UI\Title`, `Mode`, `Select`, `Result`, `RhythmGame`, `Welcome`, `Wipe`, `MyRoom`, `Multi`, `Stageup`, `Bingo`, `Sugoroku`, `Tutorial`, `Shop`, `Seeyou`, `Character` | Widget blueprints `BP_*` (UMG `WidgetBlueprintGeneratedClass`, cooked widget tree + `WidgetAnimation` MovieScene curves) |
| `UI\Common` | shared widgets: circular menu (`BP_Circle_menu*`, `Circle_menu_OBJ0x`), "kazaguruma" (pinwheel) song wheel (`BP_CMkazaguruma_*`, `sample_kazaguruma`), popups, timers, volume, warnings |
| `UI\Material\*` | ~470 UI materials: `Ring` (circular gauge), `Glitch` (glitch transitions), `GENRE`, `BG`, `Btn`, `Active`, `Rating`/`RTrate00` (rate plates), `NoteColor` (option previews), `RhythmGame` (`uM_RGready*` ready countdown), `IconGrade`, `StageUp`, `AudioVisualizer00`, `Chara`, `VIP`, `Bingo`… plus `uMPC_JudgeColor` / `uMPC_SC*` parameter collections |
| `UI\Textures` (2 GB) | `uT_*` textures: root (CM = common, MR = MyRoom, MU = Multi, RG = rhythm game, RT = result, ST = select/mode, SU = stage-up, TT = title, WC = welcome), `FONT\` (pre-rendered word art: judgements, CLEAR/FULL COMBO letters, READY, NEW RECORD, rank, level-up…), `JACKET`, `USERICON`, `USERPLATE`, `CHARACTER`, `TROPHY`, `GENRE`, `LOGO`, `ANNOUNCE`, `EVENT`, `KAZAGURUMA`, `CHANGE`, `CLEARRANK`, `STAGEUP`, `BINGO`, `SUGOROKU`, `TUTORIAL` |
| `L10N\en-US` etc. | localized texture overrides (6 for en-US, 136 for zh-Hans) |

Screen → main widgets (from `Map\*.umap` names and widget refs):
* **Title/attract:** `Title\BP_Title`, `BP_Insertcoin`, `BP_Advertise`, `BP_Starting`; uses `uM_Ring00`, `uM_Title00x`, `uT_TTlogo*`, `uT_CM*`, `LOGO\uT_LG04`, character `chara000`.
* **Login/welcome:** `Welcome\BP_Welcome*`.
* **Mode / level select:** `Mode\BP_Select_mode`, `BP_Select_level`; textures `uT_STmode*`.
* **Song select:** `Select\BP_Select01` + `page01..03`; kazaguruma wheel, `Capture\cM_PreviewUI`, genre materials `uM_GR_*`, glitch materials, `uT_STmenu_obj*`.
* **Options:** `Map\MusicSelectOption` + `uT_STop_menu*`; option preview images `uT_KZ_*` (referenced by NoteDesignTable, KeyBeamEffectTable, etc.).
* **Gameplay HUD:** `RhythmGame\BP_RhythmGame` (+ `BP_Game_Combo/Combo00/01`, `BP_Game_Judge`, `BP_Game_Score`, `BP_Game_Life00`, `BP_Game_norma_guage`, `BP_Game_ready`, `BP_Game_time`, `BP_Game_bonus`, `BP_Game_DataDetail`, `BP_RhythmGameClear01/02`).
* **Results:** `Result\BP_Result` + `BP_Result_score*`, `BP_reesult_rank`, `uM_RTrate*`, `uM_Ring00/01`.
* **Loading/wipes:** `Wipe\BP_Loading0x`, `BP_LOADING01`, `BP_Loading_inferno`.

### Extracting layouts — feasible
Cooked UMG trees are ordinary tagged-property exports. `ue4_widget_dump.py` already prints them, e.g.
`BP_Game_Judge`: CanvasPanelSlot `Anchors (0.5,0.5)`, `Alignment (0.5,0.5)`, `Offsets {Top 72.4, Right 160, Bottom 40}`
(= x,y,w,h in Slate units at 1080 px), Border `RenderTransform {Translation (0,100), Scale 0.7}`, brushes → texture paths.
Note: inside UMG exports `FMargin` is tag-serialized (the DataTable parser's binary Margin path is wrong for this; the dump script overrides it).
Animations (`WidgetAnimation` → `MovieScene2DTransformSection`/`ColorSection` RichCurve keys) are also tagged and extractable, but
the MovieScene evaluation templates are bulky. Recommendation: **extract static layout automatically** (positions, sizes, anchors,
texture/font refs, tint) into JSON, and **hand-author animations** in Rust (tweens) using reference videos; Blueprint logic (graph bytecode
in `.uexp`) is not worth decompiling — re-implement behaviour by hand.

---

## 3. Playfield and notes (`Vfx\`, 95 MB; it is a 3D scene, not 2D sprites)

* **Notes are static meshes**, one per width 1..60 segments: `Vfx\Mesh\Notes\vMsh_DefaultNote00_NN_s`,
  `vMsh_DefaultNoteS_00_NN_s` (S = small/"support"?), `vMsh_HoldStartNote00_NN`, `vMsh_HoldStartNoteS00_NN`,
  `vMsh_HoldEndNote00_NN`, `vMsh_SlideNote00_NN`, `vMsh_SlideNoteS00_NN`, `vMsh_SnapNote00_NN`, `vMsh_SnapNoteS00_NN`,
  `vMsh_GuideSync00_NN` (sync lines between simultaneous notes), `vMsh_BarLine00_60_s`. Hold bodies:
  `Vfx\Mesh\vMsh_Hold01/02_00_m`, `vMsh_holdNote01_0x_m`, `vMsh_HoldNEnd0x`. These are procedurally reproducible
  (annular sectors of N/60 of the ring, extruded) — no need to import meshes, but `StaticMesh` LOD0 vertex buffers are
  extractable with UModel/CUE4Parse to check exact profiles.
* **Note materials** (`Vfx\Material`): master `vMX_2_NTap_000` (masked unlit; params `NoteColor`, `GradationRange_Min/Max`,
  `Camera_Length_`, `SyncNote`), instances `vMX_2_Note_MUS_m_000_{Normal00,Chain00,Hold00,SlideL00,SlideR00,SnpUp00,SnpDwn00,BonusN/L/R00/01}`,
  textures `vT_NoteBase00_00_m`/`01` (channel-packed masks, sample PNG), `vT_SlideNote00_00_m` (chevrons), `vT_SnapNote00_00_m`,
  `vT_HoldNote01_00_m`, `vT_HoldENote00_00_m`, `vT_HoldGrd00_00_m`, `vT_SyncNote00_00_m`, `vT_SNote00_00_m`, `vT_BNotePat0x` (bonus pattern).
  Hold/slide/snap/break masters: `vMX_2_Hold_00x`, `vMX_2_SlideL/R_002`, `vMX_2_SnapIn/Out_001`, `vMX_2_Break_000`, `vMX_2_Sync_000`, `vMX_2_NoteAura_000`.
  Note colours are parameters: `NotesColorTable` (`NormalNote_Color1`, `ChainNote_Color1`, `HoldNote_Color1`, `SnapUp/Dwn`, `SlideR/L`,
  `Support_Color1..6`) → `vMPC_NoteCustomize_00` material parameter collection; default colour values live in that MPC (UNVERIFIED – dump it).
* **Judgement ring / lanes:** `vT_JudgeLine00_00_m`, `vT_JudgeColor01_00_m`, `vMX_2_GuideLane_000` + `vT_GuideLane01_00_m`,
  `vMX_2_BarLine_000`, `vT_RGborder00_00_m`, `UI\Textures\uT_RGback00`; `RhythmGamePosTable` (`NoteAppearPos Y -600`,
  `ComboEffectPos/NormaEffectPos Y -820`), `JudgeLineSettingTable`, `GuideLine*Table`, `RhythmGameAppearanceTable` (`HoldNoteDivisionLength 2.0`, lane width 2).
* **Hit effects:** Cascade `ParticleSystem`s (UE 4.19, not Niagara) in `Vfx\Particle\tap_0|tap_1`: `vo_tap000_0x` (pop/shoot per judgement),
  `vo_btap000_0x` (bonus), `vo_hold000_0x_lp`, `vo_miss000_00`; chosen by `TouchEffectTable`. Modules are simple (Spawn/Burst, Lifetime,
  Size(+MultiplyLife), ColorOverLife, Velocity, Location, mesh TypeData `vMsh_tapMarvA01_00_m` etc.) → reimplement as a small
  particle system with curves dumped from the assets. Textures `vT_HitMarv*`, `vT_HitGrate*`, `vT_HitGoodA00`, `vT_HitBPt00`, `vT_Flare*`, `vT_Glow00`.
* **Background effects:** `vb_BGN/BGS/BGB/BGC*_lp` (normal/S/boss/climax loops), `vp_bgcup/down`, `vp_Clear00x`, `vp_stgame000`,
  `vb_SnoteHit_00`, `vp_slideb*`; the level switches `BackGroundEffect{Normal,Good,Bad,Climax,Up,Down,Opening,FullCombo,AllMarvelous,Missless,InfernoNormal,InfernoGood,MultiNormal}` with post-process volumes (`BGNormal_PP`, `BGGood_PP`, `BGClimax_PP`).
* **Judgement text:** `UI\Textures\FONT\uT_FTdecision01..04` (Marvelous / Great / Good / Miss – 01 verified), `uT_FTfastslow00/01`;
  "COMBO" word `uT_FTcombo00/01`; **combo digits are rendered from the `SHINBI` font** (BP_Game_Combo only references `SHINBI_Font`),
  score uses `DF-KaGei` / `SHINBI`; rank letters `uT_FTrank01..04`, life `uT_FTlife00/01`, norma gauge `uT_RGnorma02..04`, clear banners `uT_FTclear_*` + `uT_RGclear0x`.

---

## 4. Textures

Survey (`ue4_texture.py survey`, 2 461 textures outside Sound/Movie):

| group | formats |
|---|---|
| UI/Textures | **PF_B8G8R8A8 2134**, DXT5 25, DXT1 14 |
| Vfx/Textures | DXT1 58, DXT5 28, B8G8R8A8 6, G8 1 |
| L10N | B8G8R8A8 only |
| Splash / Map / UI/Material | a few DXT1/DXT5 |

* No BC7, no BC4/5, no ASTC. Sizes mostly power-of-two (256² ×1138, 512² ×349, 1024² ×258, 1024×256 …).
* Mip0 payload **inline in `.uexp`** for 2 438 textures; only 23 use `.ubulk` (large mips; offset in ubulk =
  `OffsetInFile + len(uasset) + len(uexp) - 4`). Most UI textures have 1 mip.
* 4.19 platform-data layout documented in the script header (`SkipOffset` is int32 in 4.19).

Tools: **UModel/UE Viewer** (https://github.com/gildor2/UEViewer, set game "Unreal engine 4.19"),
**FModel** (https://github.com/4sval/FModel) on top of **CUE4Parse** (https://github.com/FabianFG/CUE4Parse) – loose
cooked files work if pointed at the folder (UNVERIFIED for FModel with loose files; UModel supports it).
Rust: `texture2ddecoder` (BCn decoding, https://crates.io/crates/texture2ddecoder) / `image_dds`, `ktx2` writer;
no mature Rust UE4.19 package reader (`unreal_asset` https://github.com/AstroTechies/unrealmodding supports 4.19 tagged props, UNVERIFIED for textures).

**Recommendation:** one-time offline converter (Python script above, or a Rust `wacca-assets` tool) → PNG for B8G8R8A8
(lossless, already uncompressed), and either PNG or KTX2/DDS keeping BC1/BC3 blocks for Vfx (upload directly as
`Bc1RgbaUnorm`/`Bc3RgbaUnorm` – wgpu supports BC on desktop). Runtime decoding of `.uexp` is unnecessary.
Total UI texture set is ~2 GB raw BGRA → ~0.6–1 GB as PNG (jackets/icons dominate); lazy-load jackets.

---

## 5. Fonts (`UI\Font`)

`.ufont` files are **raw font files** (no wrapper): `OTTO` = CFF OpenType, `00 01 00 00` = TrueType, one `ttcf` collection:

| file | type | use (UNVERIFIED mapping) |
|---|---|---|
| `DF-KaGei-W6.ufont` | TTC | main UI font (`DF-KaGei-W6_Font` composite) |
| `DF-KaGei-Kerning.ufont` | OTF | Latin w/ kerning, default typeface of the composite |
| `SHINBI.ufont` | OTF | numerals (combo/score) |
| `TT_RODINNTLG-EB`, `TT_UDKakugoC60-B`, `FOT-UDKakugo_SmallPr6-B` | TTF/OTF | Fontworks Rodin / UD Kakugo (JP text, kana) |
| `FTTK-YDHeadB`, `FTTC-ARCuheiGBBold`, `FTTC-ARNewHeiGBUltra`, `NotoSans*`, `DroidSansFallback` | TTF/OTF | CJK fallbacks per culture |

`*_Font.uasset` = `UFont` composite (default typeface + sub-typefaces with `CharacterRanges` and `Cultures`) – dump with the
tagged-prop parser to reproduce fallback order. Extract by plain copy (`.ufont` → `.ttf/.otf/.ttc`), load with
`fontdue`/`ab_glyph`/`cosmic-text` (TTC support needed for KaGei → `ttf-parser` handles collections).
Commercial fonts (DynaFont, Fontworks, Morisawa-style) → personal use only, never redistribute.
Outlines/glow on text are UMG `FontOutlineSettings` (OutlineSize/OutlineColor) – render outline via SDF/MSDF.

---

## 6. Materials / shaders to re-author in WGSL

UE material graphs are compiled to HLSL bytecode in the cooked shader cache — not portable; but the uasset keeps
expression names/parameters, so behaviour can be inferred. Main families:

| visual | UE asset | what to write | effort |
|---|---|---|---|
| Movie YUV→RGB | `/CriWare/ManaYUVToRGB` | 3-plane sample + BT.601 matrix | 0.5 d |
| Notes (all types) | `vMX_2_NTap_000` + MIs, `vMPC_NoteCustomize_00` | unlit masked: mask channels × colour param, gradient along camera distance, glow | 2–3 d |
| Hold body, slide chevrons (scroll), snap arrows | `vMX_2_Hold_00x`, `vMX_2_SlideL/R_002`, `vMX_2_SnapIn/Out_001`, `vMF_CustomPanner000` | UV panning, gradients | 2 d |
| Guide lanes / judge ring / bar lines | `vMX_2_GuideLane_000`, `vMX_2_BarLine_000`, `vT_JudgeLine00` | additive gradients, brightness params from RhythmGameAppearanceTable | 1–2 d |
| Tap/hit effects (Cascade) | `vMX_2_Tap_000` (masks, `LinePos`, `NoteLength_*`, `GlowCore_Gamma`), `vM_1_Base_*`, `vM_2_Scroll_003` | particle system + additive materials | 4–6 d |
| UI ring gauge | `UI\Material\Ring\uM_Ring00` (VectorToRadialValue, `Gauge`, `Gauge_Width`, `Gauge_Bokashi`, On/Off colours) | radial gauge shader | 0.5 d |
| Glitch transitions | `UI\Material\Glitch\uM_Glitch*` + `uT_CM_Glitch_*` | UV offset blocks over time | 1–2 d |
| Background 3D effects (no-movie songs) | `vMX_3_BG*`, `vMX_3_Climax_*`, `vb_*` particles, post-process | approximations; or skip & use a generic animated bg | 5–10 d (optional) |
| Misc UI (rate plates, genre, buttons, gradient BGs) | `uM_RTrate*`, `uM_GR*`, `uM_BTN`, `uM_MRbg*` | mostly texture × colour / panning | 2–4 d |

Bloom is enabled in the project (`r.DefaultFeature.Bloom=True`) — a simple bloom pass matters for the neon look.

---

## Recommended pipeline (summary)

1. **Offline `wacca-assets` tool** (Python prototype exists; port to Rust later): textures → PNG/KTX2 (+ JSON manifest),
   fonts → copied TTF/OTF/TTC, widget layouts → JSON (`ue4_widget_dump.py` logic), DataTables → JSON (already done),
   optional: Cascade curves and mesh dumps via UModel.
2. **Movies stay `.usm`**: Rust USM demuxer + FFmpeg `mpeg1video` (TMPGEXS-aware) → YUV textures → WGSL.
3. **Renderer**: procedural 3D ring/notes (mesh generator for 1..60-segment sectors), ~10 WGSL shaders, small particle system, bloom.
4. **UI**: data-driven layout from extracted JSON + hand-written animations, fonts via cosmic-text/fontdue with SDF outlines.

Effort (one dev, UNVERIFIED estimate): asset converter 3–5 d; video playback 3–4 d; playfield+notes+effects 2–3 wk;
HUD 1 wk; menus (title/select/options/results) 3–5 wk depending on fidelity; background 3D effects optional +1–2 wk.

