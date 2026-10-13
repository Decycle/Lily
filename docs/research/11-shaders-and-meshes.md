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

(in progress)

## 3. Port spec

(in progress)
