//! The game's Cascade hit effects as data (`wacca-convert --effects`) and a small deterministic
//! CPU particle system that runs them (pure, no GPU). Spec: `docs/research/12-hit-effects.md`.
//!
//! An effect is a [`ParticleSystem`] spawned as one component per lane of the hit note: at
//! radius [`EFFECT_RADIUS`] on the judgement-line plane, centred on the lane, rotated so local +X
//! runs along the ring (counter-clockwise) and local +Z points at the centre, uniformly scaled
//! (hit effects [`HIT_SCALE`], key beams 1) — all read from RenderDoc captures (frames 9009 and
//! 12365). Emitters follow UE 4.19's Cascade semantics for the modules the effects use (bursts,
//! spawn rate, loops, delay; lifetime, size/colour/velocity and their over-life curves,
//! rotation, pivot offset); GPU emitters are simulated the same way. [`ParticleWorld::build`]
//! turns the particles into view-space triangles ([`FxVertex`]): mesh particles with the
//! captured instance transform (component · location · roll/pitch/yaw · size) and sprites
//! camera-facing or velocity-aligned like UE's sprite vertex shaders.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use wacca_ue4::{Emitter, MaterialParams, Module, ParticleSystem};

use crate::camera::{CAMERA_Y, world_to_view};
use crate::layout::lane_angle;
use crate::notes3d::{NoteMesh, parse_obj};

/// Radius (cm) of the effect components (lane centre on the judgement-line plane).
pub const EFFECT_RADIUS: f32 = 170.0;
/// Uniform scale of the hit-effect components (captured LocalToWorld).
pub const HIT_SCALE: f32 = 1.7;

/// Shader family of an effect material (the master material whose pixel shader is ported in
/// `fx.wgsl`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum FxShader {
    /// `vMX_2_Tap_000`: hit flares and key beams.
    Tap,
    /// `vMX_2_Hold_000`: the hold-held glow.
    Hold,
    /// `vM_1_Base_004`: texture × particle colour.
    Base004,
    /// `vM_1_Base_000`: particle colour, texture red as opacity.
    Base000,
    /// `vM_1_Base_005`: one of three colours picked by the particle alpha.
    Base005,
    /// `vM_2_Scroll_003`: particle alpha scrolls the mask.
    Scroll,
    /// `vM_2_Dslv_003`: colour texture with a dissolve mask scrolled by the particle red.
    Dissolve,
    /// `vMX_1_Miss_000`: the Miss shards.
    Miss,
    /// `vM_1_Base_003`: colour texture × particle colour, mask texture green as opacity.
    Base003,
    /// `vMX_3_RankUp_000`: the R-note hit's growing squares.
    RankUp,
    /// Not ported: drawn as texture × particle colour.
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FxBlend {
    Additive,
    Translucent,
    /// Opaque with an opacity mask (clip 1/3), drawn before the translucent effects.
    Masked,
}

/// A material ready for the GPU: shader, blend, packed constants and texture layers.
#[derive(Clone, Debug, PartialEq)]
pub struct FxMaterial {
    pub name: String,
    pub shader: FxShader,
    pub blend: FxBlend,
    pub params: [[f32; 4]; 6],
    /// Texture layers (index into [`EffectLibrary::textures`]).
    pub textures: [Option<u32>; 2],
}

/// A texture used by the effect materials (`data/ui/Vfx/Textures/<name>.png`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FxTexture {
    pub name: String,
    /// The cooked `SRGB` flag (decode to linear when sampling).
    pub srgb: bool,
}

/// The effect paths (below `Vfx/Particle`) of one `TouchEffectTable` row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TouchEffect {
    pub marvelous_pop: Option<String>,
    pub great_pop: Option<String>,
    pub good: Option<String>,
    pub miss: Option<String>,
    pub bonus_marvelous_pop: Option<String>,
    pub bonus_great_pop: Option<String>,
    pub marvelous_shoot: Option<String>,
    pub great_shoot: Option<String>,
    pub hold: Option<String>,
    pub rehold: Option<String>,
    pub hold_input: Option<String>,
}

/// The default touch effect (`TouchEffectTable` row 312001, guest play).
pub const DEFAULT_TOUCH_EFFECT: &str = "312001";
/// R-note hit (`SNoteEffectTable` ON): one system per hit at the screen centre, behind the
/// guide lanes (RenderDoc KING frame 9800: drawn before the lanes, component at (0, −610, 0)).
pub const R_NOTE_EFFECT: &str = "vb_SnoteHit_00";
/// World Y of the R-note effect's component.
pub const BEHIND_Y: f32 = -610.0;

/// Key beam (`KeyBeamEffectTable` ON): one per touched lane. Captured: `vMX_2_Tap_000_TUS_m_00`
/// (`LineLength_Change_ONOFF` 1) at radius 170 + 18, scale 1 (frame 12365).
pub const KEY_BEAM: &str = "vo_ftap000_00";

impl TouchEffect {
    /// A row of `touch_effects.json` (`OFF` = none).
    pub fn from_row(row: &HashMap<String, String>) -> Self {
        let get = |k: &str| row.get(k).filter(|v| *v != "OFF").cloned();
        Self {
            marvelous_pop: get("MarvelousPopEffectName"),
            great_pop: get("GreatPopEffectName"),
            good: get("GoodEffectName"),
            miss: get("MissEffectName"),
            bonus_marvelous_pop: get("BonusMarvelousPopEffectName"),
            bonus_great_pop: get("BonusGreatPopEffectName"),
            marvelous_shoot: get("MarvelousShootEffectName"),
            great_shoot: get("GreatShootEffectName"),
            hold: get("HoldEffectName"),
            rehold: get("ReHoldEffectName"),
            hold_input: get("HoldInputEffectName"),
        }
    }

    fn all(&self) -> impl Iterator<Item = &str> {
        [
            &self.marvelous_pop,
            &self.great_pop,
            &self.good,
            &self.miss,
            &self.bonus_marvelous_pop,
            &self.bonus_great_pop,
            &self.marvelous_shoot,
            &self.great_shoot,
            &self.hold,
            &self.rehold,
            &self.hold_input,
        ]
        .into_iter()
        .flatten()
        .map(String::as_str)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FxError {
    #[error("{}: {source}", path.display())]
    Io {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    #[error("{}: {source}", path.display())]
    Json {
        path: std::path::PathBuf,
        source: serde_json::Error,
    },
    #[error("touch effect row {0} missing")]
    NoTouchRow(String),
}

/// A material JSON (`effects/materials/<name>.json`).
#[derive(Debug, serde::Deserialize)]
struct MaterialFile {
    #[serde(flatten)]
    params: MaterialParams,
    master: String,
}

#[derive(Debug, serde::Deserialize)]
struct TextureFile {
    srgb: bool,
}

/// Everything the effects need: systems, materials, meshes, textures.
#[derive(Debug, Default)]
pub struct EffectLibrary {
    pub touch: TouchEffect,
    systems: HashMap<String, Arc<ParticleSystem>>,
    materials: Vec<FxMaterial>,
    material_index: HashMap<String, u32>,
    meshes: HashMap<String, NoteMesh>,
    textures: Vec<FxTexture>,
}

fn scalar(m: &MaterialParams, name: &str) -> f32 {
    m.scalars.get(name).copied().unwrap_or(0.0)
}

fn vector(m: &MaterialParams, name: &str) -> [f32; 4] {
    m.vectors.get(name).copied().unwrap_or([0.0; 4])
}

/// Shader family, constants and texture parameter names of a material (layout in `fx.wgsl`).
fn pack(master: &str, m: &MaterialParams) -> (FxShader, [[f32; 4]; 6], [Option<&'static str>; 2]) {
    let s = |n| scalar(m, n);
    let mut p = [[0.0; 4]; 6];
    let shader = match master {
        "vMX_2_Tap_000" => {
            // NoteHeight 0.5 (MPC default) selects NoteLength_0.5.
            p[0] = [
                s("NoteLength_0.5"),
                s("ColorTex_pos"),
                s("ColorTexture_ONOFF"),
                s("btm_Mask_Contrast"),
            ];
            p[1] = [
                s("Btmmask_OnOFF"),
                s("GlowSub_Gamma"),
                s("GlowCore_Gamma"),
                s("LinePos"),
            ];
            p[2][0] = s("LineLength_Change_ONOFF");
            return (FxShader::Tap, p, [Some("ColorTexture"), None]);
        }
        "vMX_2_Hold_000" => {
            let lerp = |a: f32, b: f32| a + (b - a) * 0.5;
            p[0] = [
                lerp(s("CoreGlowPos_NoteMin"), s("CoreGlowPos_NoteMax")),
                s("CoreGlow_Min"),
                s("CoreGlow_Max"),
                lerp(s("CoreGlowMskPosV_NoteMin"), s("CoreGlowMskPosV_NoteMax")),
            ];
            p[1] = [
                s("CoreGlowRange_Min"),
                s("CoreGlowRange_Max"),
                s("CoreGlow_Strength"),
                s("ColorBalance"),
            ];
            p[2] = [
                s("LineGlow_Pos"),
                s("LineGlow_Min"),
                s("LineGlow_Max"),
                s("LineGow_Strength"),
            ];
            p[3] = [
                s("U_pos"),
                s("Rotate"),
                s("FlickCycle_Speed"),
                s("FlickStrength_Max"),
            ];
            let core = vector(m, "CoreGlow_Color");
            let side = vector(m, "SideGlow_Color");
            p[4] = [core[0], core[1], core[2], s("FlickStrength_Min")];
            p[5] = [side[0], side[1], side[2], 0.0];
            FxShader::Hold
        }
        "vM_1_Base_004" => return (FxShader::Base004, p, [Some("Color_texture"), None]),
        "vM_1_Base_000" => return (FxShader::Base000, p, [Some("texture"), None]),
        "vM_1_Base_005" => {
            let c = |n| vector(m, n);
            let (c1, c2, c3) = (c("Color1"), c("Color2"), c("Color3"));
            p[0] = [c1[0], c1[1], c1[2], s("Ramdom")];
            p[1] = [c2[0], c2[1], c2[2], 0.0];
            p[2] = [c3[0], c3[1], c3[2], 0.0];
            return (FxShader::Base005, p, [Some("Color_texture"), None]);
        }
        "vM_2_Scroll_003" => return (FxShader::Scroll, p, [Some("Mask_Dslv"), None]),
        "vM_1_Base_003" => {
            let (c, k) = (vector(m, "Color_Tiling"), vector(m, "Mask_Tiling"));
            p[0] = [c[0], c[1], k[0], k[1]];
            p[1][0] = s("BG_Mask_0");
            return (
                FxShader::Base003,
                p,
                [Some("Color_texture"), Some("Mask_texture")],
            );
        }
        "vMX_3_RankUp_000" => {
            let (sin, cos) = s("Rotate").to_radians().sin_cos();
            p[0] = [s("BG_Mask_0"), s("ColorTex_ONOFF"), cos, sin];
            p[1] = [
                s("1_UV_Tiling"),
                s("WaveInside_Density"),
                s("SmoothStep_Min"),
                s("SmoothStep_Max"),
            ];
            p[2] = [s("Shape_Min"), s("Shape_Max"), 0.0, 0.0];
            return (
                FxShader::RankUp,
                p,
                [Some("Color_Texture"), Some("Texture")],
            );
        }
        "vM_2_Dslv_003" => {
            p[0][0] = s("ColorPower");
            return (FxShader::Dissolve, p, [Some("Color"), Some("DslvTex")]);
        }
        "vMX_1_Miss_000" => {
            let c = |n| vector(m, n);
            let (a, b, line, base) = (
                c("RandomColorA"),
                c("RandomColorB"),
                c("Line_Color"),
                c("Base_Color"),
            );
            p[0] = [a[0], a[1], a[2], s("outside_scale")];
            p[1] = [b[0], b[1], b[2], s("outside_Density")];
            p[2] = [line[0], line[1], line[2], s("inside_Density")];
            p[3] = [base[0], base[1], base[2], 0.0];
            return (
                FxShader::Miss,
                p,
                [Some("Color_texture"), Some("TextureObject")],
            );
        }
        other => {
            log::warn!("effect material {} ({other}): shader not ported", m.name);
            let tex = m.textures.keys().next().map(|_| "");
            return (FxShader::Other, p, [tex, None]);
        }
    };
    (shader, p, [None, None])
}

fn read_file(path: &Path) -> Result<Vec<u8>, FxError> {
    std::fs::read(path).map_err(|source| FxError::Io {
        path: path.to_owned(),
        source,
    })
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, FxError> {
    serde_json::from_slice(&read_file(path)?).map_err(|source| FxError::Json {
        path: path.to_owned(),
        source,
    })
}

impl EffectLibrary {
    /// Builds the library from its parts: the touch effect row, systems by path, materials
    /// (params + master name), meshes by name, texture sRGB flags by name.
    pub fn new(
        touch: TouchEffect,
        systems: Vec<(String, ParticleSystem)>,
        materials: Vec<(MaterialParams, String)>,
        meshes: Vec<(String, NoteMesh)>,
        srgb: &HashMap<String, bool>,
    ) -> Self {
        let mut lib = Self {
            touch,
            systems: systems.into_iter().map(|(k, s)| (k, Arc::new(s))).collect(),
            meshes: meshes.into_iter().collect(),
            ..Self::default()
        };
        for (m, master) in materials {
            let (shader, params, names) = pack(&master, &m);
            let textures = names.map(|n| {
                let tex = match n? {
                    "" => m.textures.values().next()?,
                    n => m.textures.get(n)?,
                };
                Some(lib.texture_layer(tex, srgb.get(tex).copied().unwrap_or(true)))
            });
            let blend = match m.blend.as_deref() {
                Some("BLEND_Additive") => FxBlend::Additive,
                Some("BLEND_Masked") => FxBlend::Masked,
                _ => FxBlend::Translucent,
            };
            let id = lib.materials.len() as u32;
            lib.material_index.insert(m.name.clone(), id);
            lib.materials.push(FxMaterial {
                name: m.name,
                shader,
                blend,
                params,
                textures,
            });
        }
        lib
    }

    fn texture_layer(&mut self, name: &str, srgb: bool) -> u32 {
        if let Some(i) = self.textures.iter().position(|t| t.name == name) {
            return i as u32;
        }
        self.textures.push(FxTexture {
            name: name.to_owned(),
            srgb,
        });
        self.textures.len() as u32 - 1
    }

    /// Loads `data_dir/effects` (systems of the default touch effect, the key beam and the R-note
    /// hit, their materials) and the meshes they use from `data_dir/meshes`.
    pub fn load(data_dir: &Path) -> Result<Self, FxError> {
        let dir = data_dir.join("effects");
        let path = dir.join("touch_effects.json");
        let rows: HashMap<String, HashMap<String, String>> = read_json(&path)?;
        let touch = rows
            .get(DEFAULT_TOUCH_EFFECT)
            .map(TouchEffect::from_row)
            .ok_or_else(|| FxError::NoTouchRow(DEFAULT_TOUCH_EFFECT.into()))?;
        let mut systems = Vec::new();
        let names: Vec<String> = touch
            .all()
            .chain([KEY_BEAM, R_NOTE_EFFECT])
            .map(str::to_owned)
            .collect();
        for name in names {
            if systems.iter().any(|(n, _)| *n == name) {
                continue;
            }
            let path = dir.join("systems").join(format!("{name}.json"));
            match read_json::<ParticleSystem>(&path) {
                Ok(s) => systems.push((name, s)),
                Err(e) => log::warn!("effects: {e}"),
            }
        }
        let mut materials = Vec::new();
        let mut meshes = Vec::new();
        for e in systems.iter().flat_map(|(_, s)| &s.emitters) {
            if !materials
                .iter()
                .any(|(m, _): &(MaterialParams, String)| m.name == e.material)
            {
                let path = dir.join("materials").join(format!("{}.json", e.material));
                match read_json::<MaterialFile>(&path) {
                    Ok(f) => materials.push((f.params, f.master)),
                    Err(e) => log::warn!("effects: {e}"),
                }
            }
            if let Some(mesh) = &e.mesh
                && !meshes.iter().any(|(n, _): &(String, NoteMesh)| n == mesh)
            {
                let path = data_dir.join("meshes").join(format!("{mesh}.obj"));
                let text = read_file(&path).map(|b| String::from_utf8_lossy(&b).into_owned());
                match text.map(|t| parse_obj(&t)) {
                    Ok(Ok(m)) => meshes.push((mesh.clone(), m)),
                    Ok(Err(e)) => log::warn!("effects: {}: {e}", path.display()),
                    Err(e) => log::warn!("effects: {e}"),
                }
            }
        }
        let path = dir.join("textures.json");
        let textures: HashMap<String, TextureFile> = read_json(&path)?;
        let srgb = textures.into_iter().map(|(k, v)| (k, v.srgb)).collect();
        Ok(Self::new(touch, systems, materials, meshes, &srgb))
    }

    pub fn system(&self, name: &str) -> Option<&Arc<ParticleSystem>> {
        self.systems.get(name)
    }

    pub fn materials(&self) -> &[FxMaterial] {
        &self.materials
    }

    pub fn textures(&self) -> &[FxTexture] {
        &self.textures
    }

    fn material(&self, name: &str) -> Option<(u32, &FxMaterial)> {
        let id = *self.material_index.get(name)?;
        Some((id, &self.materials[id as usize]))
    }
}

/// Where a system instance sits: lane (its first lane, may be fractional for moving holds),
/// radius and scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub lane: f32,
    pub radius: f32,
    pub scale: f32,
    /// At the screen centre, [`BEHIND_Y`] deep, unrotated and drawn before the guide lanes (the
    /// R-note hit) instead of on the ring.
    pub behind: bool,
}

impl Placement {
    /// A hit effect on `lane` ([`EFFECT_RADIUS`], [`HIT_SCALE`]).
    pub fn hit(lane: f32) -> Self {
        Self {
            lane,
            radius: EFFECT_RADIUS,
            scale: HIT_SCALE,
            behind: false,
        }
    }

    /// The R-note hit: the screen centre behind the lanes, scale 1.
    pub fn behind() -> Self {
        Self {
            lane: 0.0,
            radius: 0.0,
            scale: 1.0,
            behind: true,
        }
    }

    /// Component-local → world (cm, UE axes) and the rotated axes.
    fn frame(&self) -> Frame {
        if self.behind {
            return Frame {
                origin: [0.0, BEHIND_Y, 0.0],
                axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                scale: self.scale,
            };
        }
        let phi = lane_angle(self.lane) + std::f32::consts::FRAC_PI_2;
        let (s, c) = phi.sin_cos();
        let centre = lane_angle(self.lane + 0.5);
        Frame {
            origin: [self.radius * centre.cos(), 0.0, self.radius * centre.sin()],
            axes: [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]],
            scale: self.scale,
        }
    }
}

struct Frame {
    origin: [f32; 3],
    /// World images of local X, Y, Z (unit).
    axes: [[f32; 3]; 3],
    scale: f32,
}

impl Frame {
    fn dir(&self, v: [f32; 3]) -> [f32; 3] {
        let a = &self.axes;
        [0, 1, 2].map(|k| (a[0][k] * v[0] + a[1][k] * v[1] + a[2][k] * v[2]) * self.scale)
    }

    fn point(&self, v: [f32; 3]) -> [f32; 3] {
        let d = self.dir(v);
        [0, 1, 2].map(|k| self.origin[k] + d[k])
    }

    /// [`Frame::point`] without the component scale: where world-space (not `bUseLocalSpace`)
    /// sprite emitters put their particles. MEASURED on the KING bonus hits: the shards
    /// (`vo_btap000_*`, spawn 15–65 cm inwards) stay at 180–290 px as the unscaled offsets give;
    /// scaled by 1.7 they reached the screen centre. UNVERIFIED in UE's source terms.
    fn point_unscaled(&self, v: [f32; 3]) -> [f32; 3] {
        let d = self.dir(v);
        [0, 1, 2].map(|k| self.origin[k] + d[k] / self.scale)
    }
}

/// One particle (component-local space).
#[derive(Clone, Debug, PartialEq)]
struct Particle {
    pos: [f32; 3],
    base_vel: [f32; 3],
    vel: [f32; 3],
    base_size: [f32; 3],
    size: [f32; 3],
    base_color: [f32; 4],
    color: [f32; 4],
    rotation: f32,
    rotation_rate: f32,
    age: f32,
    /// 0 = lives until its system is removed.
    life: f32,
    /// `ParticleRandom` (material input) in `[0, 1)`.
    random: f32,
    sub_image: f32,
    /// Mesh roll/pitch/yaw (degrees).
    orientation: [f32; 3],
    /// Material `DynamicParameter` (`ParticleModuleParameterDynamic`; 1 without the module).
    dynamic: [f32; 4],
}

impl Particle {
    fn relative_time(&self) -> f32 {
        if self.life > 0.0 {
            self.age / self.life
        } else {
            0.0
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct EmitterState {
    /// Time in the current loop (negative while delayed).
    time: f32,
    loops_done: i32,
    /// Bursts fired in the current loop (bit per burst).
    fired: u64,
    spawn_carry: f32,
    done: bool,
    particles: Vec<Particle>,
}

/// A running particle system.
#[derive(Clone, Debug, PartialEq)]
struct Instance {
    system: Arc<ParticleSystem>,
    place: Placement,
    emitters: Vec<EmitterState>,
    active: bool,
    key: Option<u64>,
}

/// The live effects: system instances and their particles.
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleWorld {
    instances: Vec<Instance>,
    seed: u32,
}

impl Default for ParticleWorld {
    fn default() -> Self {
        Self {
            instances: Vec::new(),
            seed: 0x2545_f491,
        }
    }
}

/// Uniform random numbers (xorshift32).
fn next_rand(seed: &mut u32) -> f32 {
    let mut x = *seed;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *seed = x;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

fn rand3(seed: &mut u32) -> [f32; 3] {
    [next_rand(seed), next_rand(seed), next_rand(seed)]
}

impl ParticleWorld {
    /// Live particles.
    pub fn len(&self) -> usize {
        self.instances
            .iter()
            .flat_map(|i| &i.emitters)
            .map(|e| e.particles.len())
            .sum()
    }

    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    /// Starts `system` at `place`; `key` names a persistent instance (holds, key beams) for
    /// [`ParticleWorld::place`] / [`ParticleWorld::stop`].
    pub fn spawn(&mut self, system: &Arc<ParticleSystem>, place: Placement, key: Option<u64>) {
        let emitters = system
            .emitters
            .iter()
            .map(|e| EmitterState {
                time: -e.delay,
                loops_done: 0,
                fired: 0,
                spawn_carry: 0.0,
                done: false,
                particles: Vec::new(),
            })
            .collect();
        self.instances.push(Instance {
            system: Arc::clone(system),
            place,
            emitters,
            active: true,
            key,
        });
        // Bursts at time 0 fire on the next update.
    }

    /// Whether a running instance has `key`.
    pub fn has(&self, key: u64) -> bool {
        self.instances
            .iter()
            .any(|i| i.active && i.key == Some(key))
    }

    /// Moves the instance `key`.
    pub fn place(&mut self, key: u64, place: Placement) {
        for i in self.instances.iter_mut().filter(|i| i.key == Some(key)) {
            i.place = place;
        }
    }

    /// Deactivates instance `key`: it stops spawning; with `kill` its particles go too.
    pub fn stop(&mut self, key: u64, kill: bool) {
        for i in self.instances.iter_mut().filter(|i| i.key == Some(key)) {
            i.active = false;
            i.key = None;
            if kill {
                i.emitters.iter_mut().for_each(|e| e.particles.clear());
            }
        }
    }

    /// Keys of the running persistent instances.
    pub fn keys(&self) -> impl Iterator<Item = u64> + '_ {
        self.instances.iter().filter_map(|i| i.key)
    }

    /// Advances every instance by `dt` seconds (negative = 0).
    pub fn update(&mut self, dt: f32) {
        let dt = dt.max(0.0);
        let seed = &mut self.seed;
        for inst in &mut self.instances {
            for (def, st) in inst.system.emitters.iter().zip(&mut inst.emitters) {
                update_emitter(def, st, dt, inst.active, seed);
            }
        }
        self.instances.retain(|i| {
            i.emitters
                .iter()
                .any(|e| !e.particles.is_empty() || (i.active && !e.done))
        });
    }

    /// Appends the view-space triangles of every particle to `out` and the runs of equal blend
    /// to `runs` (`(blend, first vertex, end)`), in draw order.
    pub fn build(
        &self,
        lib: &EffectLibrary,
        out: &mut Vec<FxVertex>,
        runs: &mut Vec<(FxBlend, u32, u32)>,
    ) {
        self.build_layer(lib, out, runs, false);
    }

    /// [`ParticleWorld::build`] of the instances with `place.behind == behind` only.
    pub fn build_layer(
        &self,
        lib: &EffectLibrary,
        out: &mut Vec<FxVertex>,
        runs: &mut Vec<(FxBlend, u32, u32)>,
        behind: bool,
    ) {
        for inst in self.instances.iter().filter(|i| i.place.behind == behind) {
            let frame = inst.place.frame();
            for (def, st) in inst.system.emitters.iter().zip(&inst.emitters) {
                let Some((material, m)) = lib.material(&def.material) else {
                    continue;
                };
                let start = out.len() as u32;
                let pivot = pivot(def);
                for p in &st.particles {
                    match def.mesh.as_ref().and_then(|n| lib.meshes.get(n)) {
                        Some(mesh) => push_mesh_particle(out, &frame, p, mesh, material),
                        None if def.mesh.is_none() => {
                            push_sprite(out, &frame, def, p, pivot, material);
                        }
                        None => {}
                    }
                }
                let end = out.len() as u32;
                if end == start {
                    continue;
                }
                match runs.last_mut() {
                    Some((b, _, e)) if *b == m.blend && *e == start => *e = end,
                    _ => runs.push((m.blend, start, end)),
                }
            }
        }
    }
}

fn pivot(def: &Emitter) -> [f32; 2] {
    let mut p = [-0.5, -0.5];
    for m in &def.modules {
        if let Module::PivotOffset(o) = m {
            p = [p[0] + o[0], p[1] + o[1]];
        }
    }
    p
}

fn emitter_alive(def: &Emitter, st: &EmitterState) -> bool {
    !st.done && (def.loops == 0 || st.loops_done < def.loops)
}

fn update_emitter(def: &Emitter, st: &mut EmitterState, dt: f32, active: bool, seed: &mut u32) {
    // Age and kill, then the per-frame modules (UE: reset to the base values, then update).
    for p in &mut st.particles {
        p.age += dt;
    }
    st.particles.retain(|p| p.life <= 0.0 || p.age < p.life);
    for p in &mut st.particles {
        apply_life_modules(def, p);
        for k in 0..3 {
            p.pos[k] += p.vel[k] * dt;
        }
        p.rotation += p.rotation_rate * dt;
    }
    if !active {
        st.done = true;
        return;
    }
    if !emitter_alive(def, st) {
        st.done = true;
        return;
    }
    let (t0, t1) = (st.time, st.time + dt);
    st.time = t1;
    if t1 < 0.0 {
        return;
    }
    let duration = def.duration.max(1e-4);
    let from = t0.max(0.0);
    let to = t1.min(duration);
    // Bursts due by now that have not fired in this loop.
    let mut count = 0;
    for (i, b) in def.bursts.iter().enumerate().take(64) {
        if st.fired >> i & 1 == 1 || b.time * duration > to {
            continue;
        }
        st.fired |= 1 << i;
        let n = if b.count_low >= 0 {
            let r = next_rand(seed);
            b.count_low + ((b.count - b.count_low + 1) as f32 * r) as i32
        } else {
            b.count
        };
        count += (n as f32 * def.burst_scale.eval1(from, 0.0)).round() as i32;
    }
    let rate = def.rate.eval1(from, next_rand(seed)) * def.rate_scale.eval1(from, 0.0);
    st.spawn_carry += rate.max(0.0) * (to - from).max(0.0);
    let from_rate = st.spawn_carry.floor();
    st.spawn_carry -= from_rate;
    for _ in 0..count.max(0) + from_rate as i32 {
        let p = spawn_particle(def, from, seed);
        st.particles.push(p);
    }
    if t1 >= duration {
        st.loops_done += 1;
        if def.loops == 0 || st.loops_done < def.loops {
            st.time = t1 - duration;
            st.fired = 0;
        } else {
            st.done = true;
        }
    }
}

fn spawn_particle(def: &Emitter, emitter_time: f32, seed: &mut u32) -> Particle {
    let mut p = Particle {
        pos: [0.0; 3],
        base_vel: [0.0; 3],
        vel: [0.0; 3],
        base_size: [1.0; 3],
        size: [1.0; 3],
        base_color: [1.0; 4],
        color: [1.0; 4],
        rotation: 0.0,
        rotation_rate: 0.0,
        age: 0.0,
        life: 0.0,
        random: next_rand(seed),
        sub_image: 0.0,
        orientation: [0.0; 3],
        dynamic: [1.0; 4],
    };
    let t = emitter_time;
    for m in &def.modules {
        match m {
            Module::Lifetime(d) => p.life = d.eval1(t, next_rand(seed)),
            Module::Size(d) => p.base_size = d.eval(t, rand3(seed)),
            Module::Color { color, alpha, .. } => {
                let c = color.eval(t, rand3(seed));
                p.base_color = [c[0], c[1], c[2], alpha.eval1(t, next_rand(seed))];
            }
            Module::Location(d) => {
                let o = d.eval(t, rand3(seed));
                (0..3).for_each(|k| p.pos[k] += o[k]);
            }
            Module::Cylinder {
                start_location,
                radius,
                height,
                height_axis,
                surface_only,
                velocity,
                velocity_scale,
            } => {
                let o = start_location.eval(t, rand3(seed));
                let r = radius.eval1(t, next_rand(seed));
                let h = height.eval1(t, next_rand(seed));
                let a = next_rand(seed) * std::f32::consts::TAU;
                let rr = if *surface_only {
                    r
                } else {
                    r * next_rand(seed).sqrt()
                };
                let along = (next_rand(seed) - 0.5) * h;
                let (ax, b1, b2) = match height_axis {
                    0 => (0, 1, 2),
                    1 => (1, 0, 2),
                    _ => (2, 0, 1),
                };
                let mut q = [0.0; 3];
                q[b1] = rr * a.cos();
                q[b2] = rr * a.sin();
                q[ax] = along;
                (0..3).for_each(|k| p.pos[k] += o[k] + q[k]);
                if *velocity {
                    let s = velocity_scale.eval1(t, next_rand(seed));
                    let mut radial = q;
                    radial[ax] = 0.0;
                    let n = norm(radial);
                    (0..3).for_each(|k| p.base_vel[k] += n[k] * s);
                }
            }
            Module::Velocity {
                velocity, radial, ..
            } => {
                let v = velocity.eval(t, rand3(seed));
                let r = radial.eval1(t, next_rand(seed));
                let n = norm(p.pos);
                (0..3).for_each(|k| p.base_vel[k] += v[k] + n[k] * r);
            }
            Module::Rotation(d) => {
                p.rotation += d.eval1(t, next_rand(seed)) * std::f32::consts::TAU;
            }
            Module::RotationRate(d) => {
                p.rotation_rate += d.eval1(t, next_rand(seed)) * std::f32::consts::TAU;
            }
            Module::SubImage(d) => p.sub_image = d.eval1(0.0, next_rand(seed)),
            Module::DynamicParam(params) => {
                for (k, d) in params.iter().take(4).enumerate() {
                    let at = if d.emitter_time { t } else { 0.0 };
                    p.dynamic[k] = d.value.eval1(at, next_rand(seed));
                }
            }
            _ => {}
        }
    }
    if let Some(o) = &def.mesh_orientation {
        p.orientation = o.eval(t, rand3(seed));
    }
    apply_life_modules(def, &mut p);
    p
}

/// The over-life modules at the particle's relative time (UE resets size, velocity and colour
/// to their base values every frame, then applies these).
fn apply_life_modules(def: &Emitter, p: &mut Particle) {
    let rt = p.relative_time();
    p.size = p.base_size;
    p.vel = p.base_vel;
    p.color = p.base_color;
    for m in &def.modules {
        match m {
            Module::SizeMultiplyLife { curve, axes } => {
                let s = curve.eval(rt, [0.0; 3]);
                let s = if curve.dims() == 1 { [s[0]; 3] } else { s };
                (0..3).filter(|&k| axes[k]).for_each(|k| p.size[k] *= s[k]);
            }
            Module::SizeScale(d) => {
                let s = d.eval(rt, [0.0; 3]);
                (0..3).for_each(|k| p.size[k] *= s[k]);
            }
            Module::ColorOverLife {
                color,
                alpha,
                clamp_alpha,
            } => {
                let c = color.eval(rt, [0.0; 3]);
                let a = alpha.eval1(rt, 0.0);
                p.color = [
                    c[0],
                    c[1],
                    c[2],
                    if *clamp_alpha { a.clamp(0.0, 1.0) } else { a },
                ];
            }
            Module::Color { clamp_alpha, .. } if *clamp_alpha => {
                p.color[3] = p.color[3].clamp(0.0, 1.0);
            }
            Module::VelocityOverLife { curve, absolute } => {
                let v = curve.eval(rt, [0.0; 3]);
                (0..3).for_each(|k| {
                    p.vel[k] = if *absolute { v[k] } else { p.vel[k] * v[k] };
                });
            }
            // UNVERIFIED: emitter-time parameters are kept at their spawn value (none of the
            // game's systems uses them).
            Module::DynamicParam(params) => {
                for (k, d) in params.iter().take(4).enumerate() {
                    if !d.spawn_only && !d.emitter_time {
                        p.dynamic[k] = d.value.eval1(rt, 0.0);
                    }
                }
            }
            _ => {}
        }
    }
    // GPU emitters: the baked curves replace the colour and size modules.
    if let Some(g) = &def.gpu {
        if let Some(c) = g.color_at(rt) {
            p.color = c;
        }
        if let Some(f) = g.size_at(rt) {
            p.size = [p.base_size[0] * f[0], p.base_size[1] * f[1], p.base_size[2]];
        }
    }
}

fn norm(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 1e-6 { v.map(|x| x / l) } else { [0.0; 3] }
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// One effect vertex: view-space position `[X, Z, w]` (see [`crate::camera`]), UV, particle
/// colour (linear, may exceed 1), extra per-particle data (`ParticleRandom`, relative time) and
/// the material index.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FxVertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
    pub extra: [f32; 4],
    /// Material `DynamicParameter`.
    pub dynamic: [f32; 4],
    pub material: u32,
}

/// UE `FRotationMatrix(Pitch, Yaw, Roll)` from a roll/pitch/yaw vector (degrees): the images of
/// the X, Y, Z axes.
fn rotation_matrix([roll, pitch, yaw]: [f32; 3]) -> [[f32; 3]; 3] {
    let (sp, cp) = pitch.to_radians().sin_cos();
    let (sy, cy) = yaw.to_radians().sin_cos();
    let (sr, cr) = roll.to_radians().sin_cos();
    [
        [cp * cy, cp * sy, sp],
        [sr * sp * cy - cr * sy, sr * sp * sy + cr * cy, -sr * cp],
        [-(cr * sp * cy + sr * sy), cy * sr - cr * sp * sy, cr * cp],
    ]
}

fn vertex(world: [f32; 3], uv: [f32; 2], p: &Particle, material: u32) -> FxVertex {
    FxVertex {
        pos: world_to_view(world),
        uv,
        color: p.color,
        extra: [p.random, p.relative_time(), 0.0, 0.0],
        dynamic: p.dynamic,
        material,
    }
}

/// A mesh particle: `frame · (pos + R(orientation) · (size ⊙ v))` (captured instance rows).
fn push_mesh_particle(
    out: &mut Vec<FxVertex>,
    frame: &Frame,
    p: &Particle,
    mesh: &NoteMesh,
    material: u32,
) {
    let r = rotation_matrix(p.orientation);
    for &(v, uv, _) in &mesh.corners {
        let s = [v[0] * p.size[0], v[1] * p.size[1], v[2] * p.size[2]];
        let local = [0, 1, 2].map(|k| p.pos[k] + r[0][k] * s[0] + r[1][k] * s[1] + r[2][k] * s[2]);
        out.push(vertex(frame.point(local), uv, p, material));
    }
}

/// A sprite quad (two triangles), like UE's sprite vertex factories: camera-facing
/// (`PSA_Square` uses the X size for both axes) or, for `PSA_Velocity`, its up axis against the
/// velocity and its right axis across it, rotated by the particle rotation; corners at
/// `(uv + pivot) · size` (default pivot −0.5).
fn push_sprite(
    out: &mut Vec<FxVertex>,
    frame: &Frame,
    def: &Emitter,
    p: &Particle,
    pivot: [f32; 2],
    material: u32,
) {
    let centre = if def.local_space {
        frame.point(p.pos)
    } else {
        frame.point_unscaled(p.pos)
    };
    let to_cam = norm([-centre[0], CAMERA_Y - centre[1], -centre[2]]);
    let (right, up) = if def.alignment == "PSA_Velocity" {
        let v = frame.dir(p.vel);
        let v = norm([v[0], v[1], v[2] + 1e-4]);
        let v = if v == [0.0; 3] { [0.0, 0.0, 1.0] } else { v };
        (norm(cross(v, to_cam)), v.map(|x| -x))
    } else {
        // The play camera looks down −Y: screen right = +X, screen up = +Z. UE's sprite "up"
        // is the view's down (UV v grows downwards: KING frame 9800 eid 2765, UV (0, 0) at the
        // top-left corner).
        ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0])
    };
    let (s, c) = p.rotation.sin_cos();
    let r = [0, 1, 2].map(|k| right[k] * c + up[k] * s);
    let u = [0, 1, 2].map(|k| up[k] * c - right[k] * s);
    let sx = p.size[0] * frame.scale;
    let sy = if def.alignment == "PSA_Square" {
        sx
    } else {
        p.size[1] * frame.scale
    };
    let [nx, ny] = def.sub_images.map(|n| n.max(1) as f32);
    let cell = p.sub_image.floor().max(0.0);
    let (cu, cv) = (cell % nx, (cell / nx).floor() % ny);
    let corner = |a: f32, b: f32| {
        let w =
            [0, 1, 2].map(|k| centre[k] + (a + pivot[0]) * sx * r[k] + (b + pivot[1]) * sy * u[k]);
        vertex(w, [(cu + a) / nx, (cv + b) / ny], p, material)
    };
    let q = [
        corner(0.0, 0.0),
        corner(1.0, 0.0),
        corner(0.0, 1.0),
        corner(1.0, 1.0),
    ];
    out.extend([q[0], q[2], q[1], q[1], q[2], q[3]]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use wacca_ue4::{Burst, Distribution};

    fn emitter(material: &str, modules: Vec<Module>) -> Emitter {
        Emitter {
            name: "e".into(),
            material: material.into(),
            mesh: None,
            mesh_orientation: None,
            gpu: None,
            alignment: "PSA_Square".into(),
            local_space: true,
            duration: 0.35,
            delay: 0.0,
            loops: 1,
            sub_images: [1, 1],
            sub_uv_method: "PSUVIM_None".into(),
            rate: Distribution::constant(&[0.0]),
            rate_scale: Distribution::constant(&[1.0]),
            bursts: vec![Burst {
                count: 1,
                count_low: -1,
                time: 0.0,
            }],
            burst_scale: Distribution::constant(&[1.0]),
            modules,
        }
    }

    fn curve(values: &[f32], stride: u8) -> Distribution {
        let n = values.len() / usize::from(stride);
        Distribution {
            op: 1,
            entry_count: n as u8,
            entry_stride: stride,
            time_scale: (n - 1) as f32,
            values: values.to_vec(),
            ..Distribution::default()
        }
    }

    fn library(
        systems: Vec<(String, ParticleSystem)>,
        meshes: Vec<(String, NoteMesh)>,
    ) -> EffectLibrary {
        let mut m = MaterialParams {
            name: "M".into(),
            blend: Some("BLEND_Additive".into()),
            ..MaterialParams::default()
        };
        m.textures.insert("Color_texture".into(), "vT_A".into());
        let srgb = HashMap::from([("vT_A".to_owned(), false)]);
        EffectLibrary::new(
            TouchEffect::default(),
            systems,
            vec![(m, "vM_1_Base_004".into())],
            meshes,
            &srgb,
        )
    }

    fn flare() -> ParticleSystem {
        // vo_tap000_03's FlareTall: lifetime 0.35, location (0, 0, -5), colour 2 → 1.
        ParticleSystem {
            name: "flare".into(),
            emitters: vec![Emitter {
                mesh: Some("quad".into()),
                mesh_orientation: Some(Distribution::constant(&[-88.0, 3.0, 0.0])),
                ..emitter(
                    "M",
                    vec![
                        Module::Lifetime(Distribution::constant(&[0.35])),
                        Module::Size(Distribution::constant(&[1.019, 1.0, 1.0])),
                        Module::Location(Distribution::constant(&[0.0, 0.0, -5.0])),
                        Module::ColorOverLife {
                            color: curve(&[2.0, 2.0, 2.0, 1.0, 1.0, 1.0], 3),
                            alpha: curve(&[2.0, 0.0], 1),
                            clamp_alpha: false,
                        },
                    ],
                )
            }],
        }
    }

    fn quad() -> NoteMesh {
        // vMsh_tapMarvA01_00_m: a 10.7 x 41.5 quad in XZ, v = 0 at the far end.
        let c = |p: [f32; 3], uv: [f32; 2]| (p, uv, 0);
        NoteMesh {
            corners: vec![
                c([-5.34, 0.0, 41.03], [0.0, 0.0]),
                c([-5.39, 0.0, -0.47], [0.0, 0.993]),
                c([5.34, 0.0, 41.03], [1.0, 0.0]),
            ],
        }
    }

    #[test]
    fn library_packs_materials_and_textures() {
        let lib = library(vec![], vec![]);
        let (id, m) = lib.material("M").unwrap();
        assert_eq!(
            (id, m.shader, m.blend),
            (0, FxShader::Base004, FxBlend::Additive)
        );
        assert_eq!(m.textures, [Some(0), None]);
        assert_eq!(
            lib.textures()[0],
            FxTexture {
                name: "vT_A".into(),
                srgb: false
            }
        );
        let mut tap = MaterialParams::default();
        tap.scalars.insert("NoteLength_0.5".into(), -0.445);
        tap.scalars.insert("GlowCore_Gamma".into(), 4.0);
        let (s, p, t) = pack("vMX_2_Tap_000", &tap);
        assert_eq!(
            (s, p[0][0], p[1][2], t[0]),
            (FxShader::Tap, -0.445, 4.0, Some("ColorTexture"))
        );
    }

    #[test]
    fn mesh_particle_matches_the_captured_instance() {
        let sys = Arc::new(flare());
        let lib = library(vec![], vec![("quad".into(), quad())]);
        let mut w = ParticleWorld::default();
        w.spawn(&sys, Placement::hit(44.0), None);
        // Frame 9009 eid 3694: relative time 0.0952 (age 1/30 s at 60 fps) on lane 44.
        w.update(0.0);
        w.update(1.0 / 30.0);
        let p = &w.instances[0].emitters[0].particles[0];
        assert!((p.relative_time() - 0.0952).abs() < 1e-3);
        let (mut v, mut runs) = (Vec::new(), Vec::new());
        w.build(&lib, &mut v, &mut runs);
        assert_eq!(runs, [(FxBlend::Additive, 0, 3)]);
        // Captured translation (-9.786, 0, -178.22): mesh origin = particle position.
        let frame = Placement::hit(44.0).frame();
        let o = frame.point(p.pos);
        assert!(
            (o[0] + 9.786).abs() < 0.05 && (o[2] + 178.22).abs() < 0.05,
            "{o:?}"
        );
        // Captured rows: mesh X -> (1.73, 0, -0.092), mesh Z -> (0.003, -1.699, 0.059).
        let r = rotation_matrix(p.orientation);
        let x = frame.dir(r[0].map(|c| c * 1.019));
        let z = frame.dir(r[2]);
        let near = |a: [f32; 3], b: [f32; 3]| (0..3).all(|k| (a[k] - b[k]).abs() < 0.01);
        assert!(near(x, [1.73, 0.0, -0.092]), "{x:?}");
        assert!(near(z, [0.003, -1.699, 0.059]), "{z:?}");
        // Colour from the over-life curve, alpha unclamped.
        assert!((v[0].color[0] - (2.0 - 0.0952)).abs() < 1e-3 && v[0].color[3] > 1.0);
    }

    #[test]
    fn bursts_lifetimes_loops_and_rates() {
        let mut e = emitter("M", vec![Module::Lifetime(Distribution::constant(&[0.1]))]);
        e.duration = 0.2;
        e.loops = 0;
        e.rate = Distribution::constant(&[20.0]);
        let sys = Arc::new(ParticleSystem {
            name: "loop".into(),
            emitters: vec![e],
        });
        let mut w = ParticleWorld::default();
        w.spawn(&sys, Placement::hit(0.0), Some(7));
        w.update(0.0);
        assert_eq!(w.len(), 1, "burst at time 0");
        w.update(0.05);
        assert_eq!(w.len(), 2, "one more from the 20/s rate");
        w.update(0.2);
        // Looped: a new burst, older particles expired.
        assert!(w.has(7) && !w.is_empty());
        w.stop(7, false);
        w.update(0.2);
        assert!(w.is_empty(), "stopped instances end with their particles");
    }

    #[test]
    fn delay_and_single_loop_end() {
        let mut e = emitter("M", vec![Module::Lifetime(Distribution::constant(&[0.1]))]);
        e.delay = 0.05;
        let sys = Arc::new(ParticleSystem {
            name: "d".into(),
            emitters: vec![e],
        });
        let mut w = ParticleWorld::default();
        w.spawn(&sys, Placement::hit(0.0), None);
        w.update(0.04);
        assert_eq!(w.len(), 0);
        w.update(0.02);
        assert_eq!(w.len(), 1);
        w.update(0.5);
        assert!(w.is_empty());
    }

    #[test]
    fn velocity_sprites_point_along_their_motion() {
        let mut e = emitter(
            "M",
            vec![
                Module::Lifetime(Distribution::constant(&[1.0])),
                Module::Size(Distribution::constant(&[2.0, 10.0, 1.0])),
                Module::Velocity {
                    velocity: Distribution::constant(&[0.0, 0.0, 100.0]),
                    radial: Distribution::constant(&[0.0]),
                    world_space: false,
                },
                Module::PivotOffset([0.0, -10.0]),
            ],
        );
        e.alignment = "PSA_Velocity".into();
        let sys = Arc::new(ParticleSystem {
            name: "s".into(),
            emitters: vec![e],
        });
        let lib = library(vec![], vec![]);
        let mut w = ParticleWorld::default();
        // Lane 45: local +Z (inwards) is world +Z (screen up); centre at 273 degrees.
        let place = Placement {
            lane: 45.0,
            radius: 170.0,
            scale: 1.0,
            behind: false,
        };
        let zc = 170.0 * 273f32.to_radians().sin();
        w.spawn(&sys, place, None);
        w.update(0.0);
        let (mut v, mut runs) = (Vec::new(), Vec::new());
        w.build(&lib, &mut v, &mut runs);
        assert_eq!(v.len(), 6);
        // Pivot −10.5: the quad lies 9.5..10.5 sizes ahead of the particle (inwards, +Z).
        let zs: Vec<f32> = v.iter().map(|q| q.pos[1] - zc).collect();
        let (lo, hi) = zs
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), z| (a.min(*z), b.max(*z)));
        assert!(
            (lo - 95.0).abs() < 0.5 && (hi - 105.0).abs() < 0.5,
            "{lo} {hi}"
        );
        let xs: Vec<f32> = v.iter().map(|q| q.pos[0]).collect();
        let width = xs.iter().cloned().fold(f32::MIN, f32::max)
            - xs.iter().cloned().fold(f32::MAX, f32::min);
        assert!((width - 2.0).abs() < 0.01);
    }

    #[test]
    fn deterministic() {
        let mut e = emitter(
            "M",
            vec![Module::Lifetime(Distribution {
                op: 2,
                entry_count: 1,
                entry_stride: 2,
                sub_entry_stride: 1,
                values: vec![0.1, 0.3],
                ..Distribution::default()
            })],
        );
        e.bursts[0].count = 5;
        let sys = Arc::new(ParticleSystem {
            name: "r".into(),
            emitters: vec![e],
        });
        let run = || {
            let mut w = ParticleWorld::default();
            w.spawn(&sys, Placement::hit(3.0), None);
            w.update(0.0);
            w.update(0.15);
            w.len()
        };
        assert_eq!(run(), run());
        assert!(run() > 0 && run() < 5);
    }

    #[test]
    fn placement_matches_the_captured_component() {
        // Frame 12365 eid 3642: lane 33, rows (0.525, 0, -1.617), (0, 1.7, 0), (1.617, 0, 0.525),
        // origin (-158.709, 0, -60.9225).
        let f = Placement::hit(33.0).frame();
        let near = |a: [f32; 3], b: [f32; 3]| (0..3).all(|k| (a[k] - b[k]).abs() < 0.01);
        assert!(near(f.dir([1.0, 0.0, 0.0]), [0.525, 0.0, -1.617]));
        assert!(near(f.dir([0.0, 0.0, 1.0]), [1.617, 0.0, 0.525]));
        assert!(near(f.origin, [-158.709, 0.0, -60.922]));
    }
}
