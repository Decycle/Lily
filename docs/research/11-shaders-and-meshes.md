# 11 — Real shaders, materials and meshes of the play area

Goal: make `wacca-render` match the real play area exactly. Sources: the cooked game files (static extraction,
§1) and RenderDoc captures of real gameplay frames (§2). §3 is the port spec. Nothing extracted is committed:
shaders, meshes and captures live under the worktree's `runs/` (gitignored); paths below are relative to it.

Confidence: **CONFIRMED** = read from the files/captures; **UNVERIFIED** = inference.

## 1. Static extraction

### 1.1 Renderer configuration (CONFIRMED, `Mercury/Config/DefaultEngine.ini`)
* RHI: Windows default **D3D11, SM5** (`Engine/GlobalShaderCache-PCD3D_SM5.bin` + `-SM4`; no D3D12/Vulkan caches).
* `r.ForwardShading=True`, `r.DefaultFeature.AntiAliasing=2` (temporal AA), `Bloom=True`, `AutoExposure=False`,
  `MotionBlur=False`, `AmbientOcclusion=True` (no effect on unlit materials). Fixed 60 fps.
* `DefaultGame.ini`: `bShareMaterialShaderCode=False` → **no ShaderArchive**: every Material /
  MaterialInstance `.uexp` carries its own inline `FMaterialShaderMap`s, one for `PCD3D_SM5` and one for `PCD3D_SM4`
  (each map is preceded by the debug string `Compiling <mat>: Platform=PCD3D_SM5, Usage=...`).

### 1.2 Shader extraction — `tools/research/ue4_shaders.py`
Each `FShaderResource`'s code is **zlib-compressed** in the package: `u32 uncompressed_size, u32 0, u32
compressed_size, 78 9c ...`. Inflated, it is UE's D3D11 code container (uniform-buffer name table, the `DXBC`
container, packed resource counts). The tool inflates every stream, cuts out the DXBC (its header has the size),
tags it with the nearest preceding shader-type and vertex-factory FName (`TBasePassPSFNoLightMapPolicy`,
`FLocalVertexFactory`, ...) and the platform, and disassembles it with `d3dcompiler_47.dll!D3DDisassemble`
(ctypes). `GlobalShaderCache-PCD3D_SM5.bin` uses the same compression (1258 global shaders: post-process, bloom,
tonemapper, TAA, YUV convert, ...; type names are plain strings there).

```
python -I tools/research/ue4_shaders.py extract runs/shaders/vfx  <Content>/Vfx/Material
python -I tools/research/ue4_shaders.py extract runs/shaders/global <WindowsNoEditor>/Engine/GlobalShaderCache-PCD3D_SM5.bin
```
Output: `runs/shaders/<set>/<package>/<NNN>_<vs|ps|...>_<ShaderType>_<VertexFactory>.{dxbc,asm}` + `index.json`
(sha1 of each DXBC, used to match shaders seen in RenderDoc captures). Vfx/Material: 160 packages, SM5 only.

For an unlit forward material the shaders that draw on screen are **`TBasePassVSFNoLightMapPolicy` +
`TBasePassPSFNoLightMapPolicy`** for the vertex factory used: `FLocalVertexFactory` (static mesh components —
notes, lanes), `FMeshParticleVertexFactory` (Cascade mesh particles), `FParticleSpriteVertexFactory` (sprites).
The others (depth-only, velocity, hit proxy, lightmap density, surfel CS) are not used for the visible colour.

Constant buffers in a BasePass PS (D3D11 slots, verified on `vMX_2_NTap_000`; names from UE 4.19 source):
`cb0` = `Primitive`, `cb1` = `View`, `cb2` = (`ReflectionCapture`/forward-light data; only debug paths use it),
`cb3` = the material's **MaterialParameterCollection** (here `vMPC_NoteCustomize_00`), `cb4` = **`Material`**
uniform buffer (layout below). `t2`/`s1` = the fog/translucency-lighting volume (only used when `cb0[39].x > 0`).

### 1.3 Material parameters — `tools/research/ue4_material.py`
Cooked materials keep the parameter / texture-sample / function-call expression objects (arithmetic nodes are
stripped), so names and defaults are readable; MaterialInstanceConstants keep `ScalarParameterValues`,
`VectorParameterValues`, `TextureParameterValues` and `BasePropertyOverrides`.

`--uniforms` parses the `FUniformExpressionSet` of each shader map — this is the **exact layout of the `Material`
cbuffer** (`cb4` in the disassembly): float4 slot *i* = vector expression *i*, then the scalar expressions packed
4 per slot, then the 2D textures in `t`-register order (`Tex(index=k)` = k-th entry of the material's referenced
texture list). Example (`vMX_2_NTap_000`, the master of every tap-type note):

| cb4 | expression |
|---|---|
| [1] | `NoteColor` |
| [2] | `NoteColor.rgb` |
| [3] | `NoteColor.rgb * Dark` |
| [4] | `(NoteColor.rgb * Dark)^2` |
| [5] | `NoteColor.rgb + HighLight` |
| [6] | `1 - (NoteColor.rgb + HighLight)` |
| [7] | `1 - NoteColor.rgb` |
| [8] | `SelectionColor.rgb` (editor only, 0) |
| [9] | `Camera_Length_` (30), `GradationRange_Min` (10), `GradationRange_Max` (22), `Far` (0) |
| [10] | `Dark` (0.8), `HighLight` (0.35), `Line` (0.08), `min` (1.0) |
| [11] | `max` (0.92), `ChainNote` (0), `BornusNoteFlip` (1), `BornusNote` (0) |
| [12].x | `SyncNote` (0) |
| t0, t1 | `vT_NoteBase01_00_m`, `vT_NoteBase00_00_m` (referenced-texture index 1, 0; order to confirm in §2) |
| cb3[0].x | MPC `NoteHeight` (default 0.5) |

Dumps of all 160 Vfx materials/MIs (graph nodes + Material-cbuffer layout): `runs/shaders/materials/<name>.txt`.
`vMPC_NoteCustomize_00` defaults: `NoteHeight 0.5`, `HoldColorNumber 2`, `NormalNote_Color1 (1, .0356, .863)`,
`ChainNote_Color1 (1, .776, .056)`, `HoldNote_Color1 (.328, .258, 0)`, `SlideLNote_Color1 (1, .323, 0)`,
`SlideRNote_Color1 (.0137, .651, .0086)`, `SnapUpNote_Color1 (.730, .0052, .0052)`, `SnapDwnNote_Color1
(.0091, .418, 1)`, `Support_Color1..6` (linear RGB). Note MIs (`MI/vMX_2_Note_MUS_m_000_*`): all `BLEND_Masked`,
`MSM_Unlit`, `OpacityMaskClipValue 0.3333`; `Normal00` sets `NoteColor (1, .0356, .863)`, `Camera_Length_ 30`,
`GradationRange 10..22`.

Play-area masters (blend mode / key textures, CONFIRMED from the packages):

| material | blend | textures | notable parameters |
|---|---|---|---|
| `vMX_2_NTap_000` (+ MIs Normal/Chain/Hold/SlideL/R/SnpUp/Dwn/Bonus) | Masked, unlit | `vT_NoteBase00/01_00_m` | above |
| `vMX_2_SlideL_002` / `R` | Masked | `vT_SlideNote00_00_m`, `vT_SlideMsk00_00_m` | `NoteColor`, `UV_Scale (1,-0.1)` |
| `vMX_2_SnapIn_001` / `Out` | Masked (mask = tex.B) | `vT_SnapNote00_00_m` | `NoteColor` |
| `vMX_2_Hold_001..004` | Translucent (003: Masked) | `vT_HoldNote01_00_m`, `vT_HoldGrd00_00_m`, `vT_HoldENote00_00_m` | `U_Tiling 13`, `NoteLength_*`, `Alpha_RangePos 175`, `Add_Color (1.2,.4,.2)` |
| `vMX_2_Sync_000` | Translucent | `vT_SyncNote00_00_m` | — |
| `vMX_2_BarLine_000` | Masked | — | `B_Color_2 (.466,.468,.5)`, `Line_Width` |
| `vMX_2_GuideLane_000` (+ MI `TUS_m_Lane00`) | Translucent | `vT_JudgeLine00_00_m`, `vT_JudgeColor01_00_m`, `vT_GuideLane01_00_m` | `JudgeLine_Pos .1473`, `NoteLength_*`, `GuideLane_Alpha`, `PercentageAlpha 420`, glow params |
| `vMX_2_NoteAura_000` | Additive | `vT_SNote00_00_m` | `NewNoteColor` |
| `vMX_2_Tap_000` (+ MIs 00–08, hit effects) | Translucent | — | `LinePos -0.1925`, `NoteLength_*`, `GlowSub_Gamma` |

### 1.4 Meshes — `tools/research/ue4_mesh.py`
Cooked `UStaticMesh` LOD0 (sections, positions, packed tangent basis, half-float UV0/UV1, optional colours,
16/32-bit indices) → OBJ + JSON. Layout notes in the script header. Extracted set: `runs/meshes/`.

Notes are **3D meshes in UE units (cm), one asset per width 1..60 lanes** (`Vfx/Mesh/Notes/vMsh_<Kind>_NN_s`):
* Ring plane = mesh **XZ**, ring axis = mesh **Y** (towards the camera, UNVERIFIED sign until §2), lane 0 starts
  at angle 0 (+X), 6° per lane, counter-clockwise in XZ.
* `DefaultNote00` (tap/chain/hold-start body): a band **tilted along the axis** — outer edge r ≈ 177.9 at
  y ≈ +11.4…+11.65, inner edge r ≈ 161.5 at y ≈ −10.7…−11.0 (a strip of a cone). A width-N note spans
  `[6°·start + 1.5°, 6°·(start+N) − 1.5°]` (1.5° inset at both ends). One **section per lane** (each with its own
  material slot): end sections 8 triangles, inner sections 16; plus a coplanar joint strip at each lane
  boundary (r 164.4–174.1, UV0.v 0.40–0.568).
* UV0: `u` = 0 at the outer edge, 1 at the inner edge; `v` runs along the arc (0 → 0.3374 over 1.5°→6° in an end
  section) — the end caps are the texture's top region, the joint strip uses `v` 0.40–0.57. UV1 is a
  per-mesh unwrap (lightmap-style), unused by the note PS (it reads `TEXCOORD0` only).
* Variants: `DefaultNoteS_00` (S = with "support"/edge frame: extra 8-tri sections, r 139–181, |y| ≤ 15.9),
  `HoldStartNote00(S)`, `HoldEndNote00` (thin: r 152.6–174.1, y −3.2…1.0), `SlideNote00(S)` (+ chevron
  quads to r 111.8), `SnapNote00(S)` (+ arrow quad, y to −24.6), `GuideSync00` (flat y = 0, r 145.8–171.9),
  `BarLine00_60_s` (flat y = 0, r ≈ 172.7, full circle), `vMsh_Hold01/02_00_m` (hold body quads, 12.2 × 29/66),
  `vMsh_circle03_00_m` (r 248.4 disc, 96 sectors).

How these meshes are placed (component transforms: scale along the axis, travel, camera) is not in the
meshes — that comes from the capture (§2).

## 2. RenderDoc capture of real gameplay

### 2.1 How (CONFIRMED working, 2026-10-12)
RenderDoc 1.46 cannot simply launch the game: injected at process start (or `renderdoccmd capture
--opt-hook-children` on `inject.exe`) the segatools and RenderDoc IAT hooks on LoadLibrary*/GetProcAddress recurse
(stack overflow, 0xC00000FD, ~1.5 s in); injected after segatools, RenderDoc bypasses segatools' DLL redirection and
the game stops at **ERROR 1202-1** (LED board `USBIntLED.DLL` not emulated). Working recipe
(`runs/renderdoc/launch_inject.ps1`, `launch_nod.bat`):
1. start the install's `launch.bat` equivalent (amdaemon + `inject -d -k mercuryhook.dll Mercury...`),
2. as soon as `mercuryhook.dll` is in the game process, snapshot its main-module IAT
   (`tools/research/iat_restore.py snapshot`),
3. ~1.1 s after process start (segatools initialised, D3D11 device not created yet — the NVIDIA UMD loads at
   ~1.6 s) `renderdoccmd inject --PID=<pid> -c <dir>\wacca`,
4. `iat_restore.py restore` puts segatools' LoadLibraryA/W/ExA/ExW + GetProcAddress hooks back.
The overlay then shows "Capturing D3D11" and the game runs normally (title, guest play). Captures are triggered
through RenderDoc target control from qrenderdoc's embedded Python (`tools/research/rdc_capture.py`, `cmd`
file "capture" lines) at chart times scheduled from the harness' live audio sync (`runs/renderdoc/sched.py`).
Dump: `qrenderdoc --python tools/research/rdc_dump.py` (env `WACCA_RDC_DUMP=<rdc>;<out>`) → `draws.json`,
cbuffers, shaders (DXBC; sha1-matched to §1.2 by `tools/research/rdc_analyze.py`), textures, RT snapshots.
In RenderDoc 1.46 the D3D11 bindings are read from the descriptor store (`GetDescriptors`): per stage a block of
222 entries (VS 0, HS 222, DS 444, GS 666, PS 888, CS 1110): +0..13 cbuffers, +14..141 SRVs, +142.. samplers.

Captured run: フラジール Normal, guest options (speed 1.5), 16 frames at chart t = 9.30 9.50 12.0 22.8 38.0 46.2
46.42 53.9 66.2 67.5 69.05 70.2 75.6 85.3 93.6 113.5 s → `runs/renderdoc/cap3/wacca_frame{9006,9009,9153,9794,
10699,11184,11192,11634,12365,12436,12521,12583,12901,13476,13967,15154}.rdc` (≈ 28 MB each; dumps in
`cap3/d<frame>/`). Each capture stalls the game ~0.2 s, so notes right after a capture are judged Miss — judgement
art/hit effects after the first capture are not representative of perfect play (the 9.50 frame shows the first
Marvelous).

### 2.2 Frame structure (D3D11, 675×1200 window; frame 9153, CONFIRMED)
1. **Scene capture** of the background-effect scene into a 1080² HDR target (`176692`, R16G16B16A16F): depth
   prepass, base pass (`vMX_3_BGN03_*`, `vMX_3_BGBoss_004` ... — the no-movie 3D background scenes), its own
   bloom + tonemap + FXAA. (Also draws the main-scene actors; only used as a texture.)
2. GPU particle simulation (`TParticleSimulationPS`, 1024² float targets) for GPU sprite emitters.
3. **Main view**: one view rendered into a 1080² R16G16B16A16F scene colour with viewport **(0, 228, 675, 675)**
   — i.e. a square 675×675 view whose centre is (337.5, 565.5) in the window (E2 measured (337.5, 565.3)).
   Depth prepass (reversed Z) → opaque/masked base pass (`depth GreaterEqual`, no depth write) → velocity →
   **separate translucency** RT (`177486`, cleared, depth-tested against the scene depth): `uM_BgMask000`
   (alpha), `vMX_2_GuideLane_000` (alpha, 60 lane sections), hold/hit effects (`vMX_2_Tap_000` MIs, alpha and
   `One/One` additive, mesh particles; `vM_1_Base_004_AUS_m_MarvPt*` GPU sprites additive; `vM_2_Scroll_003_TUD_HitMarv000`
   additive) → composited onto scene colour (`FPostProcessBokehDOFRecombinePS`) → **TAA**
   (`FPostProcessTemporalAAPS`) → bloom: downsample 540/270/135/68/34/17 (R11G11B10F) + `TFilterPS` Gaussian
   up-chain → eye adaptation pass (auto exposure off in ini; pass still runs) → `FLUTBlenderPS` (32³ LUT) →
   `FPostProcessTonemapPS` straight into the back buffer (675×1200 R8G8B8A8, viewport 0,228,675,675).
4. **UMG/Slate HUD** on the back buffer (`TSlateElementPS`, alpha blend; `uM_RGready*`, `uM_RGguage000` UI materials),
   incl. the debug panel below the playfield.

### 2.3 Camera (main view, VS cb1 = View uniform buffer, CONFIRMED)
* World: UE units (cm). Camera at **(0, +190, 0)** looking along **−Y**; up = +Z, right = +X
  (`TranslatedWorldToView` rows: X→right, Z→up, Y→−forward). `PreViewTranslation = (0, −190, 0)`.
* `ViewToClip` = UE reversed-Z infinite perspective, **near 10**, x/y scale **1.0 → 90° FOV, aspect 1**, plus TAA
  sub-pixel jitter (≈1e-4 NDC). So NDC = (X/d, Z/d) with depth d = 190 − Y, mapped to the 675 px square (NDC 1 =
  337.5 px).
* The movie is a quad at Y = −620 (scale 1, rotated 90° about Z; `mv_<song>_Texture_Mat`, opaque), a `LineMaterial`
  quad at Y = −621, `uM_BgMask000` (alpha) at Y = −619.

### 2.4 Notes and bar lines (VS cb2 = Primitive: LocalToWorld rows 0–3, CONFIRMED)
Notes are ordinary **StaticMeshComponents** (`FLocalVertexFactory`, the §1.4 meshes, one draw per mesh section =
per lane), not particles. Each note's LocalToWorld is **uniform scale s, rotation about world Y by the lane angle,
translation (0, Y, 0)**, with

    Y = 600·(s − 1)          (s = 0 at Y = −600 = RhythmGamePosTable NoteAppearPos, s = 1 at Y = 0)

and s linear in time. Two notes in one frame (12.0 s capture, onsets 12.283 s and 13.228 s, exactly 0.9449 s
apart): s = 0.81994 / 0.39474, Y = −108.036 / −363.154 → **ds/dt = 0.4500 /s at speed 1.5**, i.e. the travel
from s = 0 to s = 1 takes **2.2222 s at speed 1.5 = 3.333 s at speed 1.0** (= 10/3 s; E2's 3.276/3.29 s came
from the outer edge with a different projection model). Bar lines (`vMX_2_BarLine_000`, 60 sections × 4 tris, mesh
`vMsh_BarLine00_60_s`) use the same transform as a note at the measure start (s = 0.39474 at 13.228 s = measure 7).
Rotation: **φ = 6°·lane_start** (the width-N mesh covers local angles 0…6N° from +X): notes at lanes 40, 45, 50
(width 10) have φ = 240°, 270°, 300° (frames 9009/9153; lanes as in `e2-fragile-normal.taps.json`). In UE row-vector
form LocalToWorld rows are `s·(cosφ, 0, sinφ)`, `(0, s, 0)`, `s·(−sinφ, 0, cosφ)`, `(0, 600(s−1), 0)` (e.g. lane 45:
row0 = (0, 0, −0.82), row2 = (0.82, 0, 0)). A local vertex (x_l, y_l, z_l) with polar angle α = atan2(z_l, x_l) and
radius ρ goes to

    X_w = s·ρ·cos(α+φ),   Z_w = s·ρ·sin(α+φ),   Y_w = 600(s−1) + s·y_l
    d   = 190 − Y_w = 790 − s·(600 + y_l)
    ndc = (X_w, Z_w) / d        → screen x right, screen y up (world +Z), angle CCW from 3 o'clock

so lane 0 is at 3 o'clock and lanes run counter-clockwise on screen, as E2 found. `rdc_analyze.py --prims`
prints the decoded (s, Y, φ) for every base-pass draw (its `rotY` column is −φ mod 360).

Note draw state: base pass, opaque (masked: `discard` below `OpacityMaskClipValue` 0.3333, MSAA off → no
alpha-to-coverage), depth test GreaterEqual against the prepass depth, back-face culling, `front CCW`. PS
cbuffers: `cb3` = MPC NoteCustomize (row 0 = NoteHeight 0.5, HoldColorNumber 2; rows 1–13 the colours, equal to
the MPC defaults), `cb4` = Material (= §1.3 table with the MI values, e.g. Normal00 NoteColor (1, .0356, .863));
`t0` = `vT_NoteBase00_00_m` (1024×2048 BC3), `t1` = `vT_NoteBase01_00_m` (256×512 BC3), sampler s0
anisotropic clamp.

### 2.5 Other main-view draws (CONFIRMED, frames 10699 / 11184 / 12365)
Draw order inside the main view (opaque pass first, then separate translucency):

| # | element | material | geometry / transform | state |
|---|---|---|---|---|
| 1 | bar lines | `vMX_2_BarLine_000` | `vMsh_BarLine00_60_s`, note transform of the measure start | opaque/masked, 60 draws × 12 idx |
| 2 | notes (touch/chain/hold-start/slide bodies) | `vMX_2_NTap_000` MIs | §1.4 meshes, §2.4 transform | opaque/masked, one draw per lane section |
| 3 | slide chevrons | `vMX_2_SlideL_002` / `SlideR_002` | extra sections of `vMsh_SlideNote00_NN_s`, **same transform as the slide body** | masked (cutoff 0.3333) |
| 4 | hold end cap | `vMX_2_Hold_003` (masked) | `vMsh_HoldEndNote00_NN`, note transform (s = 0 / Y = −600 until it appears) | opaque/masked |
| 5 | (hidden) `LineMaterial` quad | engine | ±335.9 quad at Y = −621 | opaque, fully covered by the movie |
| 6 | **movie** | `mv_<song>_Texture_Mat` (`/CriWare/ManaYUVToRGB`) | quad **±810 × ±810** in world XZ at **Y = −620** → exactly fills the 675² view (810/810 = NDC 1) | opaque, emissive |
| 7 | miss effect | `vMX_1_Miss_000_MUS_m_Miss000` | GPU sprites | opaque/masked |
| 8 | `uM_BgMask000` | UI material | ±809 quad at Y = −619 | alpha blend; opacity = MPC `vMPC_Option.BG_Mask` (0 in guest play → invisible) |
| 9 | **guide lanes + judgement ring** | `vMX_2_GuideLane_000` (MI `TUS_m_Lane00`) | 60 dynamic per-lane meshes (one draw each): verts `P0 = 218.75·(cos a0, ·, sin a0)` at Y = +150, `P1 = (0, −600, 0)`, `P2` = same as P0 at a1 = a0 + 6°, `P3 = P1`; UV0 (half2) = (0,0), (0,1), (0.5,0), (0.5,1); indices (u32) 2,1,0, 3,1,2 → a **cone** from the far apex (r 0 at Y −600) to r 218.75 at Y +150 (r = 0.29167·(Y+600)) | separate translucency, `SrcAlpha/InvSrcAlpha`, depth test GreaterEqual, no write |
| 10 | hold bodies | `vMX_2_Hold_001` | one dynamic mesh for all holds (8100 indices in frame 12365), identity transform | translucency, alpha blend |
| 11 | hit effects | `vMX_2_Tap_000` (alpha) + `vMX_2_Tap_000_TUS_m_05..08` (additive) mesh particles; `vM_1_Base_004_AUS_m_MarvPt00x` GPU sprites (additive, component at r = 170, Y = 0, scale 1.7, rotated to the lane); `vM_2_Scroll_003_TUD_HitMarv000` mesh particle (additive) | Cascade systems `Vfx/Particle/tap_*` | translucency, `One/One` or alpha |

Then separate translucency is composited (`FPostProcessBokehDOFRecombinePS`: scene·alpha + translucency).

### 2.6 Post-processing (CONFIRMED, frame 12365)
* MSAA: off (`View.NumSceneColorMSAASamples` = 1 → masked materials `discard`, no alpha-to-coverage).
* Fog: none (fog factor 1, fog colour 0 in every base-pass VS).
* **TAA** (`FPostProcessTemporalAAPS`) runs on the main view (sub-pixel jitter ~1e-4 NDC in `ViewToClip`).
* Bloom chain (downsample 540→17, `TFilterPS` up-chain) and the LUT blender run, **but the tonemapper permutation that
  draws the main view only reads scene colour**: `out.rgb = pow(sceneColor · exposure, 1/2.2)` with exposure from
  the eye-adaptation texture = **1.0** (read back), `cb0[19].x = 0.454545`. No film curve, no LUT, no bloom, no
  vignette/grain in the main view. Output UNORM (clamps at 1), alpha 0. → render linear HDR, then `pow(x, 1/2.2)`.
* Movie: Y (1080²) + U,V (540²) R8 planes, `rgb = 1.164·(Y − 0.0625) + {1.596·(V−.5), −0.813·(V−.5) −
  0.392·(U−.5), 2.017·(U−.5)}`, then `max(·,0)^2.2` as emissive (linear) — so after the final 1/2.2 the movie shows
  its BT.601 RGB unchanged except where translucency (guide lanes) is blended over it.

### 2.7 Runtime material constants (captured, guest options, movie on)
* Notes: MPC NoteCustomize = asset defaults (§1.3); `NoteHeight` 0.5. Normal00 NoteColor (1, .0356, .8632),
  Hold00 NoteColor (.3278, .2582, 0) with `Line` 0.01, SlideR (.0137, .6514, .0086), SlideL (1, .3231, 0). `Far` 0.
* Guide lane (Material cbuffer = `cb5` in its PS): `PercentageAlpha` **290**, `WidthGradation` **6** (=
  `RhythmGameAppearanceTable` movie values), `OutsideGlowStrength` 0.12, `GuideLane_Alpha` 1, `Boss_Alpha` 0,
  `JudgeLine_Pos` 0.1473, `NoteLength_*` (1600/2500/4600/11000/19000, picked by `NoteHeight`: 0.5 → 4600),
  `B_Color_5` (.0122, .0133, .0313), `B_Color_2`/`B_Color_1` (.0444, .0474, .0938), `Judge_Line_10` (.289, .429, .5),
  `L_line_pos` 0, `R_line_pos` 0.51, `Line_LorR` 0.5, `LineONOFF` 1, `Combo_ONOFF` 0, insideglow 0.25..0.3,
  SmoothStep 0.6..1. Second MPC scalar (`cb4[0].y`) = 0.6249 in that frame (animated pulse; UNVERIFIED source).
  `View.GameTime` (`cb1[132].z`) drives the scrolls.
* Hold body: `HoldColorNumber` 2 (picks tile 2 of the 13-tile `vT_HoldNote01_00_m` strip), `U_Tiling` 13.

Annotated disassembly (every constant operand followed by its captured value, plus SRV/sampler list) for each of
these draws: `runs/renderdoc/cap3/d12365/annot/<eid>_{vs,ps}.asm` — 2493 bar line, 2635 hold-start note, 2680 hold
end, 2754 movie, 3003 BgMask, 3028 guide lane, 3538 hold body, 3555/3623 tap effects, 3642 MarvPt, 3675 HitMarv,
4227/4234 TAA, 4248 downsample, 4316/4434/4446 bloom filter, 4462 LUT blender, 4483 tonemap; and
`runs/renderdoc/cap3/d11184/annot/` 2367 slide chevron, 2258 slide body, 2455 miss, 3359 tap. Generate more with
`python -I tools/research/rdc_analyze.py <dump> runs/shaders --annotate <eid>`.

## 3. Port spec (for wacca-render / WGSL)

### 3.0 Frame
* One perspective camera: eye (0, 190, 0) → −Y, up +Z, 90° vertical FOV, aspect 1, reversed-Z (near 10, infinite).
  Viewport: the largest centred square (real 675×1200 window: (0, 228, 675, 675)). World units = cm.
* Passes: (a) opaque/masked: bar lines, notes, slide chevrons, hold end caps, movie quad (depth test + write);
  (b) translucency over it, depth-tested, no depth write, in this order: guide-lane cone (alpha), hold bodies
  (alpha), hit effects (alpha / additive); (c) output `pow(linear, 1/2.2)` to a UNORM target (no sRGB format, no
  tonemap curve, no bloom). TAA is optional (sub-pixel; skip first).
* Time base: the note scale s for a note with onset t_n at song time t: `s = 1 − (t_n − t)/T`, `T = 3.3333 s /
  speed` (2.2222 s at the guest speed 1.5); drawn while 0 < s ≤ 1 (UNVERIFIED: how far past 1 it is drawn before
  the judgement removes it; the visual is ~0.1 s ahead of audio on the real PC, E2).

### 3.1 Notes (touch, chain, hold start, slide body; bar lines)
Geometry: the cooked mesh for the note kind and width (`ue4_mesh.py` → positions, UV0, indices per section;
`runs/meshes/`), or regenerate it procedurally from the profile in §1.4. Model matrix (column-vector form):

    M = T(0, 600(s−1), 0) · R_y(φ) · S(s),   φ = 6°·lane_start, R_y maps +X → (cosφ, 0, sinφ), +Z → (−sinφ, 0, cosφ)

Vertex: `clip = ViewProj · M · p`; pass UV0 and view depth w (= 190 − Y_world).
Pixel (exact translation of `vMX_2_NTap_000` BasePass PS; `M*` = Material cbuffer of the MI, see §1.3/§2.7;
t0 = `vT_NoteBase00_00_m`, t1 = `vT_NoteBase01_00_m`, clamp, linear/aniso):

    h    = lerp(clamp(NoteHeight,0,0.85), Far, smoothstep01(sat((w/Camera_Length_ − GradMin)/(GradMax − GradMin))))
    off  = 0.4·h
    uvA  = uv + (off, 0);  uvB = uv − (off, 0);  uvC = ((uv.x − off)/(1 − 2·off), uv.y)
    s    = smoothstep01(sat((2·uv.x − min)/(max − min)))        // min 1.0, max 0.92: 1 on the outer half, 0 inner
    m1   = lerp(t1(uvA).r, t1(uvB).r, s)
    stepA = uv.y ≥ 0.57619 ;  stepB = uv.y ≥ 0.4
    shade(T) = lerp(lerp(NC, (NC·Dark)², T.r), screen(lerp(NC,(NC·Dark)²,T.r), NC + HighLight), T.g) + Line·T.b
               with screen(a,b) = 1 − (1−a)(1−b), NC = NoteColor.rgb
    A = t0(uvA).rgb ; B = t0(uvB).rgb
    colA = lerp(A, shade(A), m1·stepA) ; colB = lerp(B, shade(B), m1·stepA)
    col  = lerp(colA, colB, s)
    colC = col·(1 − 0.5·ChainNote·m1·stepA·t0(uv).a)                   // chain stripes
    bon  = lerp(t1(uvC).b, t1(uvC).a, BornusNoteFlip) · inside01(uvC)  // bonus pattern
    col  = lerp(colC, lerp(col, screen(col, NC), bon), BornusNote) + SelectionColor(0)
    mask = m1·(stepB + SyncNote·(1 − stepB));  discard if mask < 0.3333
    out  = max(col, 0)   (opaque; alpha unused)

(`smoothstep01(x) = x²(3−2x)`; `inside01(p)` = 1 inside [0,1]², sharp edge.) Bar lines: `vMX_2_BarLine_000`,
emissive `B_Color_2` (.466, .468, .5), opacity mask from `Floor` of a UV expression with `Line_Width` (see
`d12365/annot/2493_ps.asm`).

### 3.2 Slide chevrons (`vMX_2_SlideL/R_002`)
Same model matrix as the slide body (the chevrons are extra sections of `vMsh_SlideNote00_NN_s`). PS (annotated
`d11184/annot/2367_ps.asm`): pattern t0 (256², wrap) sampled at `(uv.x·UV_Scale.x, uv.y·3.395·1.7 + w·0.007246)` —
**the scroll comes from the view depth w, not time** (chevrons are anchored in world space and slide with the note);
mask t1 (128×256, `vT_SlideMsk00`) at `(uv.x, −uv.y)`; colour = gradient between two MI colours by
`uv.y²`/smoothstep, screen-blended with a third, ×2 (+0.1); mask = t0.g·t1.b, cutoff 0.3333.

### 3.3 Guide lanes, lane mask and judgement ring (`vMX_2_GuideLane_000`)
Geometry per lane i (0..59, a0 = 6°·i, a1 = a0 + 6°): `P0 = (218.75 cos a0, 150, 218.75 sin a0)`, `P1 = P3 = (0,
−600, 0)`, `P2 = (218.75 cos a1, 150, 218.75 sin a1)`, UV0 (0,0), (0,1), (0.5,0), (0.5,1), triangles (2,1,0),
(3,1,2). Lanes masked by the chart are simply not drawn (the frame shows 30/60 lane draws during a half mask) —
the movie is then seen unshaded there. Pixel (`d12365/annot/3028_ps.asm`; constants §2.7; t0 = `vT_JudgeLine00_00_m`
64×1024 BC1 wrap, t1 = 1024² BC3 sampled in **screen space** (`(svpos.xy − viewMin)/viewSize`; the ring colour
gradient, `vT_JudgeColor01_00_m` UNVERIFIED by name), t2 = `vT_GuideLane01_00_m` 32×256):

    n    = clamp(pow(1−v, 9)·t0.Sample((2u, 10v + 5·time)).b, 0, 0.003) + clamp(t1(screen).a, 0, 0.0015)
    base = B_Color_5 − n
    l0   = 5·t0((u, −v) + (L_line_pos, 0)).g
    q    = rot(−13.68°)·(u − 0.5, −v) + (R_line_pos + 0.5, 0)        // (0.971632, −0.236499 / 0.236499, 0.971632)
    line = lerp(l0, 5·t0(q).g, Line_LorR)·LineONOFF
    col  = lerp(base, B_Color_2, line)                                  // lane separator lines
    (Combo_ONOFF = 0 → no guide-lane scroll texture)
    k    = min(pow(max(sin(2π(0.5·v + JudgeLine_Pos)), 0), NoteLength), 1)   // NoteLength 4600 at NoteHeight 0.5
    g    = smoothstep01(sat((1 − 3k·OutsideGlowStrength − SS_min)/(SS_max − SS_min)))
           + (sin(2π·P) + 0.5)·(inside_max − inside_min) + inside_min       // P = MPC pulse (cb4[0].y)
    r    = sat(g·sat(3k·OutsideGlowStrength + floor(3k)))
    col  = lerp(col, 1.5·t1(screen).rgb, r)                             // the magenta judgement ring
    fade = min(pow(max(w/PercentageAlpha, 0), WidthGradation), 1)        // 290, 6 with a movie
    a0   = lerp(GuideLane_Alpha, Boss_Alpha·(GuideLane_Alpha − 0.3), fade)
    q2   = (3k·OutsideGlowStrength + floor(3k))·((cos(2π·max(frac(k), 0.5)) + 1)/2 + ceil(k))
    alpha = sat(q2·(5 − a0) + a0)
    out  = (max(col, 0), alpha)       blend SrcAlpha / InvSrcAlpha

The ring peak is where 0.5·v + 0.1473 = 0.25 → v = 0.2054 → Y = −4.05, r = 173.8, d = 194.05 → NDC 0.896 (302 px
of 337.5; E2 measured the band at 292–315 px).

### 3.4 Holds
Body: one dynamic triangle mesh per frame in world space (identity transform), material `vMX_2_Hold_001`: colour =
tile `floor(HoldColorNumber)` (lerp to the next tile by `frac`) of the 13-tile strip `vT_HoldNote01_00_m` (512²)
sampled at `((rot180(uv − (0.45, 0.5)) + 0.5 + (tileX, tileY))·(1/13, 1))`, constant opacity, alpha blend.
Strip geometry (CONFIRMED, frame 12365, `cap3/d12365b`, hold at lanes 40–49): rows of **16 vertices** at fixed world
Y, rows every **ΔY = 4.5 cm (Δs = 0.0075)** from the hold head (here s = 0.6737, Y = −195.77) back to the tail (or to
Y = −600 / s = 0 if the tail has not appeared: 91 rows). A row at scale s lies on the circle **r = 172.5·s** at
Y = 600(s−1) and spans the hold's lane range at that time **inset 4° at each side** (lanes 40–49 → 244°…296°), the
16 vertices evenly spaced in angle (u = 0…0.5 across); the angle range follows the hold's lane interpolation per
row (the strip bends with moving holds). UV0.v grows by 0.0059 per row (≈ 1.31e-3 per cm of Y). Indices per quad:
(2,1,0), (3,1,2) pattern (u32).
Start cap = `vMsh_HoldStartNote00(S)` with the Hold00 MI (§3.1); end cap = `vMsh_HoldEndNote00` + `vMX_2_Hold_003`.

### 3.5 Hit effects
Cascade particle systems (`Vfx/Particle/tap_0|tap_1`): mesh particles with `vMX_2_Tap_000` and its MIs 05–08
(additive), GPU sprites `vM_1_Base_004_AUS_m_MarvPt00x` (additive, emitter at r = 170, Y = 0, scale 1.7, rotated
to the lane: one per lane of the hit note), `vM_2_Scroll_003_TUD_HitMarv000` (additive mesh particle). Shaders and
constants: `d12365/annot/3555, 3623, 3642, 3675`. Particle curves are in the ParticleSystem assets (not yet
dumped).

### 3.6 Movie
Quad ±810 at Y = −620 (fills the view square), YUV→RGB as §2.6, `^2.2` → linear, drawn opaque before the
translucency; final `^(1/2.2)`.

### 3.7 Current wacca-render approximations that are wrong
* Projection: 2D `r = (1−k)x/(1−kx)` with k 0.75 and `APPROACH_SECS` 3.29 → real is a 3D scene: note scale
  s ∝ time, Y = 600(s−1), camera 190 in front, 90° FOV; travel 3.333 s at speed 1 (s from 0 to 1).
* Notes are flat textured sectors (`push_note`, `NOTE_THICKNESS`, caps) → real notes are tilted 3D bands (outer edge
  nearer the camera) with per-lane sections, 1.5° end insets, joint strips, and the NoteBase00/01 two-texture
  parallax shading of §3.1 (gradation by view depth, chain stripes, bonus pattern).
* Guide lanes: flat dark sectors / tint (`GUIDE_LANE*`, `LANE_LINE*`) → real is a 60-lane cone, dark blue
  (.0122, .0133, .0313) with **depth-faded alpha** `1 − (w/290)^6` (movie), separator lines from the JudgeLine texture.
* Judgement ring: drawn as bands with fixed colours (`JUDGE_RING_*`, `JUDGE_BAND_*`, stripes, glow) → real ring is
  the guide-lane shader's peak at v = 0.2054 coloured by a screen-space texture ×1.5.
* Rim / score row (`RIM*`, `PROGRESS*`, `SCORE_*`) are UMG/Slate HUD drawn after the 3D view (not part of the 3D
  scene) — keep as 2D but match the widget layout (doc 06 §2).
* Slide arrows: animated by time (`SLIDE_ARROW_SPEED`) → real chevrons are fixed in world space (scroll = depth).
* Colour pipeline: sprite colours treated as sRGB and linearised with 2.2 → the real output is linear HDR then
  `pow(1/2.2)`; no tonemap, no bloom in the main view.
* Hit effects (`effects.rs` pop/shoot/sparks constants) are hand-tuned → real are Cascade systems with the
  materials above at r 170, scale 1.7.
