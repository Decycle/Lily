# 12 — Hit effects (Cascade particle systems)

The play area's judgement effects, key beams and hold-held glow are UE 4.19 Cascade `ParticleSystem`s in
`Vfx/Particle`. They are now read from the cooked assets and run by our own particle system
(`wacca-render::particles`), drawn in the stage pass with ported materials (`fx.wgsl`). Confidence as in doc 11:
**CONFIRMED** = read from files or captures, **UNVERIFIED** = inference or measurement only.

## 1. Data

`wacca-convert --effects` (`crates/wacca-convert/src/effects.rs`) writes `data/effects/`:

* `systems/<path below Vfx/Particle>.json`: every system, as `wacca_ue4::ParticleSystem` (LOD 0 of each enabled
  emitter: material, mesh + `RollPitchYawRange`, screen alignment, local space, duration/delay/loops, sub-images,
  spawn rate and bursts, modules in order). Distributions are the cooked lookup tables
  (`FRawDistribution*::Table`: `Op` 1 value / 2 random / 3 extreme, `EntryCount`, `EntryStride`, `TimeScale`,
  `TimeBias`, `LockFlag`); the few unbaked ones (`Distribution*ConstantCurve`, e.g. in `vo_miss000_00`) are
  sampled to 32 entries from their `FInterpCurve` keys. GPU emitters also keep their baked curves
  (`FGPUSpriteResourceData`): colour = `QuantizedColorSamples · ColorScale + ColorBias`, size factor =
  `(QuantizedMiscSamples.rg · MiscScale + MiscBias) · InvMaxSize`. **These differ from the modules** (e.g.
  `HoldPt000`'s ColorOverLife is (2, 0.3, 1) magenta, the baked curve yellow-white (30, 30, 5)); the GPU
  simulation uses the baked ones, and so do we (the KING hold sparks are yellow).
* `materials/<name>.json`: material instance values over the parent's defaults (`ScalarParameterValues`, …,
  `BasePropertyOverrides.BlendMode`) plus the master material name.
* `textures.json`: the cooked `SRGB` flag of each texture they use (PNG pixels come from `--export-ui Vfx`).
* `touch_effects.json`: `TouchEffectTable` rows; particle meshes go to `data/meshes/<name>.obj`.

A copy of the dump is in `runs/effects-dump/`. Only the LOD level's `bEnabled` disables an emitter: `vo_miss000_00`
has its required module flagged off, yet the Miss sprites draw (RenderDoc frame 11184 eid 2455).

### Which effect when (TouchEffectTable 312001 = default/guest, CONFIRMED)

| event | system(s) |
|---|---|
| Marvelous | pop `tap_0/vo_tap000_03` + shoot `vo_tap000_04` (bonus notes: pop `vo_btap000_03`) |
| Great | pop `vo_tap000_05` + shoot `vo_tap000_06` (bonus: `vo_btap000_05`) |
| Good | `vo_tap000_00` |
| Miss | `vo_miss000_00` |
| hold held / re-held / input | `vo_hold000_00_lp` / `_01_lp` / `_02_lp` |
| key beam (`KeyBeamEffectTable` ON) | `vo_ftap000_00` (captured `vMX_2_Tap_000_TUS_m_00`) |
| R note (`SNoteEffectTable` ON) | `vb_SnoteHit_00` (UNVERIFIED; not drawn yet, §4) |

Shoots are on with `TouchEffectShootTable` ON (guest default). Slides and snaps use the same tap effects (KING
frames). 312002 replaces the pops with `tap_1/vo_tap001_03/05`.

## 2. Placement (CONFIRMED, frames 9009 / 12365)

One system per lane of the judged note (10 lanes → 10 flares, 10 shoots, sprite emitters), and one key beam per
touched lane. The component sits on the judgement-line plane (Y = 0) at radius 170 cm, centred on the lane
(angle 6°·lane + 3°), rotated so local +X runs counter-clockwise along the ring (6°·lane + 90°) and local +Z points
inwards, uniformly scaled 1.7 (hit effects) or 1.0 (key beam: captured origin at r = 170 + 18 = 188 from its
`Location` (0, 0, −18) unscaled). Mesh particles: `instance = component · T(location) · R(roll, pitch, yaw) ·
S(size)`, `R` = UE `FRotationMatrix(Pitch = Y, Yaw = Z, Roll = X)` of `RollPitchYawRange`. The flare mesh
(`vMsh_tapMarvA01_00_m`, a 10.7 × 41.5 quad) with roll −88° becomes a sheet standing on the ring and going down
the tunnel, so it projects as the sector from the line inwards; the 3° pitch turns the lane-start rotation to the
lane centre. Test `particles::tests::mesh_particle_matches_the_captured_instance` checks the translation and both
axes against frame 9009 eid 3694 (relative time 0.0952, colour 1.833 = the ColorOverLife table).

Sprites follow UE's sprite vertex factories: camera-facing (`PSA_Square` uses the X size for both axes) or
`PSA_Velocity` (up = −velocity, right = velocity × to-camera), rotated by the particle rotation, corners at
`(uv + pivot) · size · scale` with pivot −0.5 + `ParticleModulePivotOffset` (the Marvelous sparks have −10.5:
drawn ten sizes ahead of their particle).

## 3. Materials (ported in `fx.wgsl`, constants from §1)

| master | used by | blend | colour / opacity |
|---|---|---|---|
| `vMX_2_Tap_000` | flares (MIs 05/06/07), key beams (00/01) | additive / translucent | `ramp(uv + (0, NoteLength_0.5 + ColorTex_pos)) × vc` (ramp on with `ColorTexture_ONOFF`); opacity `((w/2)^GlowSub + w^GlowCore) · vc.a`, `w = v · CheapContrast(1 − (h + v), btm_Mask_Contrast)` |
| `vMX_2_Hold_000` | hold-held glow (MIs 02/03/04) | additive / translucent | core/side colours by two smoothsteps of v, line glow, side mask, flicker `sin(2π·t·FlickCycle_Speed)` |
| `vM_1_Base_004` | Marvelous sparks, bonus shards | additive / masked | texture × vc |
| `vM_1_Base_000` | hold sparks | additive | vc, texture red × vc.a |
| `vM_1_Base_005` | bonus stars | additive | Color1/2/3 by vc.a, texture red |
| `vM_2_Scroll_003` | Marvelous shoot | additive | vc, mask scrolled by vc.a |
| `vM_2_Dslv_003` | Great shoot | additive | texture × ColorPower, dissolve scrolled by vc.r |
| `vMX_1_Miss_000` | Miss shards | masked | ring cut into sectors by `ParticleRandom`, colours A/B |

UNVERIFIED: the Hold_000 side-mask uniforms (no shader-map layout; taken as `(U_pos, 0)` and `Rotate`).
Effects draw after the guide lanes, hold ribbons and R aura, depth-tested, no depth write (masked ones with the
opaque elements), into the linear HDR stage target.

## 4. Comparison with the real game (120 fps, E3 run 20261013-023731 with the movie frame behind, KING E2)

`runs/shots.py` (real crop | `playfield` `WACCA_SHOT` render, movie planes from `runs/movieplanes.py`).
Marvelous, Great, Good, Miss, bonus, snap and slide hits match in shape, size, colour and life to about a frame;
the real onset is ~2.5 frames after the tap (game latency), ours at the judgement.
Measured, not from data (UNVERIFIED mechanism): a released key beam fades out over 0.3 s (its particle lives
100 s); the rim over a touched sensor shows a white edge and grey band (2D, held 0.2 s after release).

Remaining differences:
* the real judgement ring line stays magenta over the effects (something draws it after the translucency); ours
  whitens under the key beam;
* `vb_SnoteHit_00` (R notes: large tile pattern `vMX_3_RankUp_000`, ParameterDynamic) is not drawn: placement
  not captured, material not ported;
* hold ribbon colour when held (stage) is yellow in the real game, orange in ours (not an effect);
* hold sparks are slightly longer than the real ones; bonus triangle shards spread a little further inwards.
