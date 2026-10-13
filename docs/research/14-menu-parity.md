# 14 – Menu visual parity (title, mode, genre, song select, final check, results)

Audit of the menus against the real game (E3 guest sessions, attract recording), with every
visible difference either fixed or listed. The play area is audited separately (doc 13).

## 1. Method

- **Ours:** `tools/research/menu_capture.ps1 -Screen <title|mode|category|select|options|results>`
  starts the release build fullscreen (1920 × 1200) on that screen and grabs its window with
  `PrintWindow` (no synthetic input). Temp save `runs/parity-save.json`, set up like the real guest:
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
| mode | mode art ring | `uT_RGclear03` | — | open: material `uM_AD_6` |
| genre | background per genre | — | — | open: `uM_GR_*_0` pattern materials (e.g. VOCALOID blue tiles `uT_CM16`) |
| genre | FOR BEGINNERS / Favorite / New Songs folders | — | — | open (content, not in our catalog) |
| select | tab band | purple 64 u band over the plates | purple −305..−269, dark strip −268..−213, frame under it | fixed |
| select | selected frame tint | peach 0xffe7d5 | 0xffe7ff | fixed |
| select | counter box | square | real is a capsule | open (minor) |
| select | Notes Designer line | — | — | open: not in our catalog |
| final check | layout | own 8-row list, no plate | real layout: `uT_Chara002_00` navigator, `uT_STmenu_obj000/006` panel, monitor, 4 `obj013` rows per page (OPTIONS tab turns the page), jacket, song plate, DIFFICULTY / BACK / OPTIONS / GO | fixed (rows hold our options) |
| final check | preview monitor | — | dark disc with violet centre | approx: real is a live capture (`uM_STmoniter00` ← `BgCaptureTextureRenderTarget2D`) |
| results | ring art | none | dark / teal / rim bands between the pink stripes | approx: ring letters (grey, pink outline) left out; colours of a B grade |
| results | FAVORITE tab | — | RETRY in its place | intentional (doc 05 §4.1) |
| title | message plate | 1000 × 230 at y 344 | 860 × 198 at y 320, text 16 | fixed |
| title | logo | 1060 × 265 at y 6 | 993 × 248 at y −5 | fixed |
| title | START | size 26 | 32, icon/text measured | fixed |
| title | key art | layers + RGB-split glitch | unchanged | open: `uM_Title011/012`, `uM_Chara`, `uM_Title004/005` (glitch, swirl, glow) not ported |

## 4. Open items (priority order)

1. Title key-art materials (`uM_Title012` 468-instruction PS with `uMF_Horizontal/VerticalGlitch`,
   `uM_Title011` swirl, `uM_Chara` glow): the real art is brighter and more saturated.
2. Mode art (`uM_AD` / `uM_AD_6`) behind the dial.
3. Per-genre backdrops on the genre wheel (`uM_GR_{a..f}_0`, instance colours).
4. Results ring letters (`BP_Circle_Text`) and grade-dependent ring colours.
5. Final-check monitor as a live preview of the play background.
6. Animation timings were not re-measured in this pass (layout/colour/font first); the slide-in
   and countdown timings stay as in doc 05/06.
