# 14 – Menu visual parity (title, mode, genre, song select, final check, results)

Audit of the menus against the real game (E3 guest sessions, attract recording), with every
visible difference either fixed or listed. The play area is audited separately (doc 13).

## 1. Method

- **Ours:** `tools/research/menu_capture.ps1 -Screen <title|mode|category|select|options|results>`
  starts the release build fullscreen (1920 × 1200) on that screen and grabs its window with
  `PrintWindow` (no synthetic input). It takes the window titled `WACCA_slim` *of the process it
  started* (round 1 matched the title only and could grab another running instance). Results
  need `-Wait 13` since round 2 (GO appears 11 s in). Temp save `runs/parity-save.json`, set up like the real guest:
  `{"version":1,"settings":{"noteSpeed":1.5,"menuTimers":true,"playerName":"GUEST","playerTitle":"ゲスト"}}`.
- **Real:** E3 window captures (675 × 1200): `runs/e3/20261013-023731/nav-08-mode-select.png`,
  `nav-10-category-select.png`, `nav-18-fragile-settled.png`, `nav-20-options.png`,
  `result-3-0130.41.png`, `runs/e3/20261013-030310/nav-11-category-vocaloid.png`; title: frame at 20 s of
  `runs/reference/20261012-223942/video.mkv`.
- **Alignment:** our menu circle sits on the play ring, so the 1200 px play square (x = 360) maps
  onto the game's 675 px square at y = 228 (doc 11 §2.2). `tools/research/menu_compare.sh` crops
  both, writes real | ours | |diff| and prints the mean absolute difference (0–255) over the square
  and per 3 × 3 cell. Elements were then measured on 2–3× crops and pixel samples.
- **Asset checks:** fonts and textures from the cooked widgets (`ue4_widget_dump.py`, import
  tables), UI materials from their compiled shaders (`runs/shader-research/shaders/ui`,
  `ue4_material.py --uniforms`).

Mean |diff| over the circle square, before → after (state differs: our select lists all 395
songs and our genre wheel has no FOR BEGINNERS / favourites, so those screens keep a floor of
content differences):

| screen | before | after | biggest remaining cell |
|---|---|---|---|
| title (attract, 20 s) | 35.4 | 29.2 | key-art materials (§3) |
| mode select | 29.3 | 24.6 | MC 49.8: mode art (`uM_AD`) |
| genre select | 38.5 | 36.8 | MC: other category selected; per-genre backdrop |
| song select | 37.3 | 37.2 | MC/MR: other songs on the wheel |
| final check (options) | 45.0 | 25.3 | MR 40.5: rows show our options |
| results | 30.8 | 28.4 | MR: other song / grade animation phase |

## 2. Findings that apply to every menu

- **UI font.** The widgets' `DF-KaGei-W6_Font` is a *composite*: its default typeface is
  `TT_RODINNTLG-EB` (Rodin NTLG EB, Latin + Japanese); `DF-KaGei-Kerning` only covers ！ ゜ ¶,
  `FTTK-YDHeadB` Hangul, `NotoSansSC-Medium` is the fallback. (Import index = −value − 2 in the
  cooked `FontData`.) We drew everything in DFKaGei; now Rodin first, then KaGei, Noto SC, Kakugo,
  Droid. Text is centred on Rodin's capitals (ascent 0.88, cap ≈ 0.73 → line top 0.52 px above the
  centre; was 0.62 for KaGei).
- **Player name** (`BP_Circle_user` `TextBlock_5`) uses `Kakugo_kana` → `TT_UDKakugoC60-B`, size
  20, letter-spaced, linear (0.0065, 0.007, 0.026). **Countdown** (`BP_Time02`) is `SHINBI_Font`
  size 25, pink with a 1-unit violet outline (linear (0.227, 0, 0.352)). Both are new `Face`s.
- **Backdrop.** `BP_Select01`, `BP_Select_mode`, `BP_Welcome` use the animated UI material
  `uM_WCbg000`; ported 1:1 from its compiled pixel shader (`crates/wacca-render/src/menu_bg.wgsl`):
  `uT_WCbg00` base + three `uT_WCbg01` masks (dot blocks with a 4 rad/s radial sweep through
  `uT_CM19`, triangles / dot lines turning at 0.25 rad/s inside r 0.3, a tiled glow pulsing at
  0.5 Hz, lines with a diagonal wave running down at 0.5 uv/s), View.GameTime = app time.
- **User plate.** The guest icon is the texture `USERICON/S01/uT_UICN_S01_00_001` (not a flat ring);
  label/value sizes and colours from the widget (labels size 10 grey 0x8f8f8f, values size 15
  0x4b4b4b).

## 3. Element table

| screen | element | before | after | status |
|---|---|---|---|---|
| all | UI font | DFKaGei-W6 | Rodin NTLG EB (composite default) | fixed |
| all | text vertical centre | 0.62 px (KaGei) | 0.52 px (Rodin caps) | fixed |
| all | guest icon | flat pink ring + "C" | `uT_UICN_S01_00_001` | fixed |
| all | player name | Rodin 22, navy | Kakugo C60 size 20, spaced | fixed |
| all | Title / Lv. / RP labels | size 6–7 | widget size 10 / 9 | fixed |
| all | countdown digits | DFPKaGei + white rim | SHINBI + violet outline | fixed |
| all | GO label | size 22 | 27 (measured) | fixed |
| all | CREDIT / COIN header, USE ITEM tab, 1PLAY FREE tag | — | — | omitted (arcade only) |
| all | name "PLAYER" | save default | real shows the guest name | data (setting `playerName`) |
| mode / genre / select | background | flat blue grid / uT_WCbg00 still | `uM_WCbg000` shader port | fixed |
| mode | selected disc | `uT_STmode003` (pink glow) | `uT_STmode001` (lavender, grey ring) | fixed |
| mode | mode frame | pink rim, dark band | + pulsing magenta `uM_CM_active01` glow | approx (glow shape/strength measured; 2.2 Hz = material default) |
| mode | pill fill / text | white, 0xc8106a, y 300 | 0xe7e7e7, 0xa5127a, y 280 | fixed |
| mode | arrow buttons | 52 u discs | 40 u | fixed |
| mode | mode art ring | `uT_RGclear03` | `uM_CMcircle001` swirl port (round 2) | approx: drawn as a 620 u disc (slot 496 u with a ring mask; the "in" animation's scale not evaluated) |
| genre | background per genre | — | backdrop at 1404 u (round 2) | fixed: not per genre — the blue tiles are `uM_WCbg000`'s moving dot sweep (FOR BEGINNERS shows them too in run 034614); BP_Select01 draws the backdrop 1.3 × larger |
| genre | FOR BEGINNERS / Favorite / New Songs folders | — | — | open (content, not in our catalog) |
| select | tab band | purple 64 u band over the plates | purple −305..−269, dark strip −268..−213, frame under it | fixed |
| select | selected frame tint | peach 0xffe7d5 | 0xffe7ff | fixed |
| select | counter box | square | real is a capsule | open (minor) |
| select | Notes Designer line | — | — | open: not in our catalog |
| final check | layout | own 8-row list, no plate | real layout: `uT_Chara002_00` navigator, `uT_STmenu_obj000/006` panel, monitor, 4 `obj013` rows per page (OPTIONS tab turns the page), jacket, song plate, DIFFICULTY / BACK / OPTIONS / GO | fixed (rows hold our options) |
| final check | preview monitor | — | dark disc with violet centre | live for charts without a movie: the background scene (doc 13 §5) under the empty guide lanes; movie charts keep the static stand-in |
| results | ring art | none | dark / teal / rim bands between the pink stripes | approx: colours of a B grade |
| results | ring letters | — | the play's CLEAR end stamp (round 2) | approx: the "R" is the stamp (`BP_Result_clear02/Clear02`, `uT_FTclear_900..906`) left behind the panels, x0.88 / −11.3° from an overlay; only the CLEAR stamp (other lamps have their own) |
| results | entry timings | E2 estimates | 120 fps E3 (round 2) | fixed (§5) |
| results | FAVORITE tab | — | RETRY in its place | intentional (doc 05 §4.1) |
| title | message plate | 1000 × 230 at y 344 | 860 × 198 at y 320, text 16 | fixed |
| title | logo | 1060 × 265 at y 6 | 993 × 248 at y −5 | fixed |
| title | START | size 26 | 32, icon/text measured | fixed |
| title | key art | layers + RGB-split glitch | ghost = `uM_Title012` port, `uM_Chara_2` glows, darker backdrop, steady RGB fringes on the guide (round 2) | approx: see §5 |

## 4. Open items (round 1, superseded by §5.3)

1. Title key-art materials. 2. Mode art behind the dial. 3. Per-genre backdrops. 4. Results ring
letters. 5. Final-check monitor live preview. 6. Animation timings.

## 5. Round 2

### 5.1 Tools

- **UI materials in the overlay** (`wacca-render::text::material`): a ported UMG material is an
  `Item::Material` drawn in draw-list order with its own pipeline, per-draw uniforms (linear tint +
  15 float4) and up to four exported textures. Ported: `uM_Title012` (`title_glitch.wgsl`),
  `uM_Chara_2` (`chara_glow.wgsl`), `uM_CMcircle001` (`circle_swirl.wgsl`). Texture slots in the
  disassembly are identified by their sampling (sampler slot / coordinates), not by uniform order.
- **`tools/research/umg_layout.py`** lays out a cooked widget tree (canvas slots with the cooked
  defaults, overlay / border slots, render transforms with pivot, opacity, nested user widgets) and
  lists every image / text quad in design units. It showed which BP each "open" element lives in.
- **`ue4_material.py --uniforms`** now parses TrigMath (X, Y, Op); `uM_Title012`'s set reads.
- Low-pass metric: mean |diff| after a 15 px box average (colour / brightness without detail).

### 5.2 Before / after (mean |diff| over the circle square; low-pass in brackets)

| screen | round 1 | round 2 | what changed |
|---|---|---|---|
| title | 28.9 (18.2) | 28.4 (18.7) | key art (below) |
| mode | 24.6 (14.2) | 24.1 (14.0) | swirl art ring |
| genre | 36.8 (22.4) | 35.5 (19.4) | backdrop size |
| song select | 37.2 (21.2) | 37.1 (20.4) | backdrop size |
| final check | 25.3 (13.6) | 25.0 (13.7) | backdrop size |
| results | 28.4 (16.8) | 27.4 (15.8) | CLEAR stamp |

Round-1 numbers re-captured with this round's tool on the round-1 build (title 28.9 vs 29.2 in
§1: the glitch bursts vary).

**Title.** The cooked `BP_Title_1` draws `uM_Title011` (dark swirl), `uM_Chara_2` (dark red smoke),
`uM_Title012` (the ghost character `uT_Chara000_EV001`) and additive `uM_Title004` glows; the guide
character `Image_9` (`uT_Chara000_RT000`) and `uM_Chara_3_1` have opacity 0 in every animation of
the package, so the visible guide comes from Blueprint logic. `uM_Title012`: three glitch
functions (two horizontal, one vertical; simplex turbulence of the game time gates them) shift the
lookups feeding B (+ opacity), R and G; emissive = color_1·B + color_2·R + color_4·G. With the
material's default colours (lavender / pink / violet) the ghost is brown-lavender, but the attract
frame shows teal crescents and grey-blue hair, so the colours are set at run time: the port uses
the channels as themselves under a measured blue tint (UNVERIFIED). Her placement was measured by
edge correlation (centre (124, 62), 1220 u; round 1: (76, 95), 1300). The CM19 glows became
`uM_Chara_2` smoke glows (additive), the backdrop 0x310c9a (real far field (36, 10, 126) sRGB),
and the guide keeps steady magenta / cyan fringes (the recording shows them all the time, wider in
bursts). Net: detail diff −0.5, low-pass +0.5 — the real art's purple halo around the guide's head
and its brightness are still not reproduced.

**Results timings** (120 fps `runs/e3/20261013-023731/video.mkv`, user plate drops at 161.8 s =
t 0): info slides in 0.9 s (~0.1 s), list 1.37 s (~0.2 s), score counts 1.83–2.83 s, rank band
4.8 s, letters stamp 5.1–5.4 s, footer 8.4 s, GO + RETRY 11.0 s; the 15 s countdown starts with GO.
(Round 1: 0.4 / 0.9 / 1.25–2.6 / 4.6 / 4.7–5.1 / 5.0 / 5.0, from E2.)

### 5.3 Still open

1. Title: the guide's purple halo / brightness and the ghost's real colours (run-time material
   parameters; a capture of the material instance — RenderDoc — would settle them).
2. Cursor / wheel easing and the menu→menu wipe: the 120 fps recordings start at the final check,
   so no menu navigation is on video; `WHEEL_TAU` 0.07 s and the 0.35 / 0.35 s wipe stay
   UNVERIFIED. The final check → play transition is a zoom tunnel (31.6–33.0 s of that video), not
   our wipe.
3. Final-check monitor: live for charts without a movie (`options::Monitor`,
   `Renderer::render_background_display`: the background scene seen through the empty guide-lane
   cone, as the play view shows it). The real `uM_STmoniter00` samples
   `BgCaptureTextureRenderTarget2D`, which the level's `PreviewSceneCapture` (at the play camera's
   place, `RhythmGame.umap`) fills with the play view itself — lanes, judgement ring and a demo
   note at the chosen speed (E3 `nav-20`). Left: ring, lane separators, the demo note, movie charts
   (static stand-in).
4. Results: stamps for Missless / Full Combo / All Marvelous / Failed; ring colours per grade.
5. Mode: the rings `uM_Ring00_x` / `uM_CMcircle000` of `BP_Select_level` (their sizes come from
   material-parameter animations: `Gauge_Width` tracks).
6. Content gaps unchanged (FOR BEGINNERS / Favorite folders, Notes Designer).
