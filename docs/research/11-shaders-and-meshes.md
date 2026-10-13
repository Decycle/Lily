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

(continued below as the remaining captures are analysed)

## 3. Port spec

(in progress)
