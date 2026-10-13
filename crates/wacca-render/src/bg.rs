//! The background scene of songs without a movie (pure; GPU side in `bg_gpu`). Spec:
//! `docs/research/13-parity-audit.md` §5 (RenderDoc KING frames 11389 / 13214, `RhythmGame.umap`).
//!
//! The level holds one `BackGroundEffectActor` per state, each a looping Cascade system: Normal
//! = `vb_BGS300_00_lp` (blue grid, rings, dots), Good = `vb_BGS200_00_lp` (red/blue cube walls),
//! both at (0, −10000, 10000) turned 180° about Z, with an 8 s warm-up. A `SceneCapture2D` at
//! (0, 190, 10000) looking down −Y with a 45° FOV renders them into a 1080² target
//! (`cTRT_BackgroundEffect`); its tonemapper permutation only clamps (`saturate(colour)` into
//! an 8-bit UNORM target: no film curve, no bloom, FXAA after), and the play view draws that
//! texture as the emissive far quad (±811 cm at Y = −621, the movie's place). Which state shows
//! follows the clear gauge ([`BgState::of_gauge`]).
//!
//! Systems run on [`crate::particles`] with [`Placement::fixed`]; the materials (`MASTERS`) are
//! packed here for `bg.wgsl`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use wacca_ue4::MaterialParams;

use crate::particles::{EffectLibrary, FxError, FxVertex, ParticleWorld, Placement, TouchEffect};

/// Master materials of the background scene (ported in `bg.wgsl`).
pub const MASTERS: [&str; 6] = [
    "vMX_3_BGN03_000",
    "vMX_3_BGN03_002",
    "vMX_3_BGBoss_004",
    "vMX_3_02BGS_002",
    "vMX_3_02BGS_000",
    "vM_1_SAUV_000",
];

/// World position of the background actors' particle components (`RhythmGame.umap`).
pub const ACTOR_ORIGIN: [f32; 3] = [0.0, -10000.0, 10000.0];
/// Their `RelativeRotation` yaw (degrees about +Z): component +Y points away from the camera.
pub const ACTOR_YAW: f32 = 180.0;
/// The capture camera sits at (0, `CAMERA_Y`, `CAPTURE_Z`) looking down −Y (like the play
/// camera, raised by this much).
pub const CAPTURE_Z: f32 = 10000.0;
/// `1 / tan(FOV / 2)` of the capture camera (`FOVAngle` 45°; captured `ViewToClip` 2.41421).
pub const CAPTURE_FOCAL: f32 = 2.414_213_5;
/// Side of the capture target (px).
pub const CAPTURE_SIZE: u32 = 1080;

/// Which background shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BgState {
    #[default]
    Normal,
    /// The clear gauge is at or above the norma line.
    Good,
}

impl BgState {
    /// The state for clear gauge `gauge` against the chart's `norma` (both 0..=1).
    /// MEASURED: KING (`runs/e2/20261013-043842`, perfect play) switches to Good at 65.06 s and
    /// song 31 (E3 `20261013-035555`) at 91.9 s, each at the first judgement after our gauge
    /// model passes the norma (0.4534 / 0.4537 ≥ 0.45; the real gauge is slightly lower), and
    /// stays there. UNVERIFIED: going back to Normal when the gauge drops below the line (the
    /// level has a `BackGroundEffectDown` actor for it).
    pub fn of_gauge(gauge: f32, norma: f32) -> Self {
        if gauge >= norma {
            Self::Good
        } else {
            Self::Normal
        }
    }

    /// The state's particle system (path below `Vfx/Particle`).
    pub fn system(self) -> &'static str {
        match self {
            Self::Normal => "vb_BGS300_00_lp",
            Self::Good => "vb_BGS200_00_lp",
        }
    }

    const ALL: [Self; 2] = [Self::Normal, Self::Good];
}

/// Shader of a background material (entry points of `bg.wgsl`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BgShader {
    /// `vMX_3_BGN03_000`: the Normal backdrop sprite (radial glow, triangle pattern, grid and
    /// line layers; opaque).
    Poly,
    /// `vMX_3_BGN03_002`: the Normal ring sprites (masked, particle colour).
    Rings,
    /// `vMX_3_BGBoss_004`: the Normal dotted lines (mesh particles, masked).
    Dots,
    /// `vMX_3_02BGS_002`: the Good tunnel walls (lit, two-sided).
    Wall,
    /// `vMX_3_02BGS_000`: the Good cube clusters (lit, pulsing with `WorldPositionOffset`).
    Cubes,
    /// `vM_1_SAUV_000`: the Good triangle sprites (masked, screen-aligned colour texture).
    Triangles,
}

/// A background material: shader, constants (layout per shader in `bg.wgsl`) and up to
/// [`BG_TEXTURES`] textures (`T` = name, or index into [`BgLibrary::textures`]).
#[derive(Clone, Debug, PartialEq)]
pub struct BgMaterial<T = u32> {
    pub shader: BgShader,
    pub params: [[f32; 4]; BG_PARAMS],
    pub textures: [Option<T>; BG_TEXTURES],
}

/// Texture slots per material.
pub const BG_TEXTURES: usize = 4;

/// Constant slots per material.
pub const BG_PARAMS: usize = 10;

fn scalar(m: &MaterialParams, name: &str, default: f32) -> f32 {
    m.scalars.get(name).copied().unwrap_or(default)
}

fn vector(m: &MaterialParams, name: &str) -> [f32; 4] {
    m.vectors.get(name).copied().unwrap_or([0.0; 4])
}

fn rgb_w(v: [f32; 4], w: f32) -> [f32; 4] {
    [v[0], v[1], v[2], w]
}

/// `(cos, sin)` of `turns` full turns (UE `CustomRotator` angles).
fn turn(turns: f32) -> (f32, f32) {
    let (s, c) = (turns * std::f32::consts::TAU).sin_cos();
    (c, s)
}

/// The material of master `master` with the instance values `m` (`None`: not a background
/// master). Defaults are the masters' (`runs/shaders/materials`); the layouts follow the captured
/// `Material` cbuffers (KING frame 11389 eids 677 / 692 / 711, frame 13214 eids 613 / 647 / 676).
pub fn pack(master: &str, m: &MaterialParams) -> Option<BgMaterial<String>> {
    let mut p = [[0.0; 4]; BG_PARAMS];
    let tex = |name: &str, default: &str| {
        Some(
            m.textures
                .get(name)
                .cloned()
                .unwrap_or_else(|| default.to_owned()),
        )
    };
    let s = |n, d| scalar(m, n, d);
    let done = |shader, p, textures: [Option<String>; BG_TEXTURES]| {
        Some(BgMaterial {
            shader,
            params: p,
            textures,
        })
    };
    match master {
        "vMX_3_02BGS_002" => {
            let (c, sn) = turn(s("Rotate", 0.15));
            p[0] = [
                s("Tilling_V", 5.0),
                s("Scroll_U", 0.0),
                s("Scroll_V", -0.2),
                0.0,
            ];
            p[1] = [c, sn, s("TileAmount", 50.0), s("SphereMsk_Xpos", 0.5)];
            p[2] = rgb_w(vector(m, "ColorLine"), s("SphereMsk_Ypos", 0.2));
            p[3] = rgb_w(vector(m, "Line_Color"), 0.0);
            p[4] = rgb_w(vector(m, "Triangle_Color"), 0.0);
            done(
                BgShader::Wall,
                p,
                [
                    tex("RGBTexture", "vT_02BGS05_00_m"),
                    // The master's fixed samples (screen-aligned colour, rotating lines).
                    Some("vT_02BGS02_00_m".into()),
                    tex("TetureObject", "vT_Circle02_00_m"),
                    Some("vT_02BGS03_00_m".into()),
                ],
            )
        }
        "vMX_3_02BGS_000" => {
            let one_minus = |v: [f32; 4]| [1.0 - v[0], 1.0 - v[1], 1.0 - v[2], 0.0];
            let light = vector(m, "AddColor_Light");
            p[0] = rgb_w(
                one_minus(vector(m, "AddColor_Radial")),
                s("Total_brightness", 1.0),
            );
            p[1] = rgb_w(light, s("Beat_SineSpeed", 5.0));
            p[2] = rgb_w(one_minus(light), s("BeatStep_Min", 0.0));
            p[3] = rgb_w(vector(m, "AddColor_Dark"), s("BeatStep_Max", 0.5));
            p[4] = rgb_w(vector(m, "Beat_WorldpositionAnim"), s("Bar_SineSpeed", 0.5));
            p[5] = rgb_w(vector(m, "Bar_WorldpositionAnim"), s("BarStep_Min", 0.0));
            p[6] = [s("BarStep_Max", 0.5), 0.0, 0.0, 0.0];
            done(
                BgShader::Cubes,
                p,
                [
                    tex("BaseTexture", "vT_02BGS00_00_m"),
                    Some("vT_02BGS04_00_m".into()),
                    None,
                    None,
                ],
            )
        }
        "vM_1_SAUV_000" => {
            let (c, sn) = turn(s("SAUV_Rot", 0.0));
            p[0] = [
                s("Line_Power", 0.5),
                c,
                sn,
                s("ScreenAligh_or_SelectColor", 0.0),
            ];
            p[1] = rgb_w(vector(m, "ScreenAligh_AddColor"), 0.0);
            let line = m.vectors.get("LineColor").copied().unwrap_or([1.0; 4]);
            p[2] = rgb_w(line, 0.0);
            done(
                BgShader::Triangles,
                p,
                [
                    tex("Shape", "vT_Triangle01_00_m"),
                    tex("Color_texture", "vT_BgNGrad00_00_m"),
                    None,
                    None,
                ],
            )
        }
        "vMX_3_BGN03_000" => {
            p[0] = rgb_w(vector(m, "Radial_Inside"), s("Radial", 0.45));
            p[1] = rgb_w(vector(m, "Radial_Outside"), s("RadialDensity", 1.45));
            p[2] = rgb_w(vector(m, "ColorB"), s("BG_Triangle_Ptn_Tile", 3.0));
            p[3] = rgb_w(
                vector(m, "ColorA"),
                1.0 / s("DarkRadial_Range", 0.35).max(1e-5),
            );
            p[4] = rgb_w(
                vector(m, "LightColor"),
                1.0 / (1.0 - s("DarkRadial_Hard", 0.35)).max(1e-5),
            );
            p[5] = rgb_w(vector(m, "DarkRadial"), s("Flicker_Stlength", 2.0));
            p[6] = rgb_w(vector(m, "Tile_line"), 0.0);
            p[7] = rgb_w(vector(m, "Tile"), 0.0);
            p[8] = rgb_w(vector(m, "MecLine"), 0.0);
            p[9] = rgb_w(vector(m, "Line"), 0.0);
            done(
                BgShader::Poly,
                p,
                [
                    tex("BG_Triangle_Ptn", "vT_BGN03_00_m"),
                    tex("PT_Triangle", "vT_BGN03_01_m"),
                    tex("BG_Mec_Ptn", "vT_BGN03_02_m"),
                    None,
                ],
            )
        }
        "vMX_3_BGN03_002" => {
            p[0] = [s("G_MinScale", 0.7), s("R_MinScale", 0.7), 0.0, 0.0];
            // The master's fixed texture object.
            done(
                BgShader::Rings,
                p,
                [Some("vT_BGN03_03_m".into()), None, None, None],
            )
        }
        "vMX_3_BGBoss_004" => {
            p[0] = [
                s("1_U_Tiling", 30.0),
                s("1_V_Tiling", 30.0),
                s("2_U_scroll", 0.0),
                0.0,
            ];
            p[1] = [s("CircleMain_Min", 0.5), s("CircleMain_Max", 1.0), 0.0, 0.0];
            done(
                BgShader::Dots,
                p,
                [
                    tex("BPM_scroll", "vT_GuideLane01_00_m"),
                    tex("Texture", "vT_Circle02_00_m"),
                    None,
                    None,
                ],
            )
        }
        _ => None,
    }
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

/// The background systems with their meshes ([`EffectLibrary`]) and materials.
#[derive(Debug)]
pub struct BgLibrary {
    pub fx: EffectLibrary,
    /// Per material of `fx` (same index): the background material, if its master is ported.
    pub materials: Vec<Option<BgMaterial>>,
    /// Texture name and cooked `SRGB` flag (`data/ui/Vfx/Textures/<name>.png`).
    pub textures: Vec<(String, bool)>,
}

impl BgLibrary {
    /// Builds the library from the effect library of the background systems and their
    /// material files (name → params, master) and texture sRGB flags.
    pub fn new(
        fx: EffectLibrary,
        files: &HashMap<String, (MaterialParams, String)>,
        srgb: &HashMap<String, bool>,
    ) -> Self {
        let mut textures: Vec<(String, bool)> = Vec::new();
        let materials = fx
            .materials()
            .iter()
            .map(|fm| {
                let (params, master) = files.get(&fm.name)?;
                let BgMaterial {
                    shader,
                    params,
                    textures: names,
                } = pack(master, params)?;
                let textures = names.map(|n| {
                    let n = n?;
                    let i = textures
                        .iter()
                        .position(|(t, _)| *t == n)
                        .unwrap_or_else(|| {
                            textures.push((n.clone(), srgb.get(&n).copied().unwrap_or(true)));
                            textures.len() - 1
                        });
                    Some(i as u32)
                });
                Some(BgMaterial {
                    shader,
                    params,
                    textures,
                })
            })
            .collect();
        Self {
            fx,
            materials,
            textures,
        }
    }

    /// Loads the systems of every [`BgState`] from `data_dir/effects` (`wacca-convert
    /// --effects`), their meshes and materials.
    pub fn load(data_dir: &Path) -> Result<Self, FxError> {
        let names: Vec<String> = BgState::ALL.map(|s| s.system().to_owned()).into();
        let fx = EffectLibrary::load_systems(data_dir, TouchEffect::default(), &names)?;
        let dir = data_dir.join("effects");
        let read = |path: &Path| {
            std::fs::read(path).map_err(|source| FxError::Io {
                path: path.to_owned(),
                source,
            })
        };
        let mut files = HashMap::new();
        for m in fx.materials() {
            let path = dir.join("materials").join(format!("{}.json", m.name));
            match read(&path).and_then(|b| {
                serde_json::from_slice::<MaterialFile>(&b).map_err(|source| FxError::Json {
                    path: path.clone(),
                    source,
                })
            }) {
                Ok(f) => {
                    files.insert(m.name.clone(), (f.params, f.master));
                }
                Err(e) => log::warn!("background: {e}"),
            }
        }
        let path = dir.join("textures.json");
        let textures: HashMap<String, TextureFile> = serde_json::from_slice(&read(&path)?)
            .map_err(|source| FxError::Json { path, source })?;
        let srgb = textures.into_iter().map(|(k, v)| (k, v.srgb)).collect();
        Ok(Self::new(fx, &files, &srgb))
    }
}

/// The running background: the current state's system and the scene clocks.
#[derive(Clone, Debug)]
pub struct Background {
    lib: Arc<BgLibrary>,
    world: ParticleWorld,
    state: Option<BgState>,
}

impl PartialEq for Background {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.lib, &other.lib) && self.world == other.world && self.state == other.state
    }
}

/// One frame of the background for the GPU: view-space triangles of the capture scene (see
/// [`FxVertex`]; the capture camera is the play camera raised by [`CAPTURE_Z`]), the runs of
/// equal material `(material, first vertex, end)` in draw order, and the material clocks.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BgFrame {
    pub vertices: Vec<FxVertex>,
    pub runs: Vec<(u32, u32, u32)>,
    /// `View.GameTime` (s).
    pub time: f32,
    /// `MPC_BpmSignal` Bar and Beat.
    pub bar: f32,
    pub beat: f32,
}

impl BgFrame {
    pub fn clear(&mut self) {
        self.vertices.clear();
        self.runs.clear();
    }
}

impl Background {
    pub fn new(lib: Arc<BgLibrary>) -> Self {
        Self {
            lib,
            world: ParticleWorld::default(),
            state: None,
        }
    }

    pub fn library(&self) -> &Arc<BgLibrary> {
        &self.lib
    }

    pub fn state(&self) -> Option<BgState> {
        self.state
    }

    /// Shows `state`: a change starts its system (with the system's warm-up) and drops the
    /// previous one at once (MEASURED: KING switches within one 120 fps frame).
    pub fn set_state(&mut self, state: BgState) {
        if self.state == Some(state) {
            return;
        }
        self.state = Some(state);
        self.world = ParticleWorld::default();
        match self.lib.fx.system(state.system()) {
            Some(sys) => {
                let sys = Arc::clone(sys);
                self.world
                    .spawn(&sys, Placement::fixed(ACTOR_ORIGIN, ACTOR_YAW), None);
            }
            None => log::warn!("background: system {} not loaded", state.system()),
        }
    }

    /// Advances the particles by `dt` seconds.
    pub fn update(&mut self, dt: f32) {
        self.world.update(dt);
    }

    /// The frame's triangles into `out` (cleared first).
    pub fn build(&self, out: &mut BgFrame) {
        out.clear();
        self.world
            .build_by_material(&self.lib.fx, &mut out.vertices, &mut out.runs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wacca_ue4::{Burst, Distribution, Emitter, Module, ParticleSystem};

    #[test]
    fn state_follows_the_norma_line() {
        assert_eq!(BgState::of_gauge(0.4499, 0.45), BgState::Normal);
        assert_eq!(BgState::of_gauge(0.45, 0.45), BgState::Good);
        assert_eq!(BgState::of_gauge(1.0, 0.8), BgState::Good);
        assert_eq!(BgState::of_gauge(0.0, 0.0), BgState::Good);
        assert_eq!(BgState::Normal.system(), "vb_BGS300_00_lp");
        assert_eq!(BgState::Good.system(), "vb_BGS200_00_lp");
    }

    fn mi(scalars: &[(&str, f32)], vectors: &[(&str, [f32; 4])]) -> MaterialParams {
        MaterialParams {
            name: "mi".into(),
            scalars: scalars.iter().map(|(k, v)| ((*k).into(), *v)).collect(),
            vectors: vectors.iter().map(|(k, v)| ((*k).into(), *v)).collect(),
            ..MaterialParams::default()
        }
    }

    #[test]
    fn backdrop_constants_match_the_captured_cbuffer() {
        // vMX_3_BGN03_000_OUS_m_0 (frame 11389 eid 677: cb4[23].y 3.33333, cb4[24].y 1.11111).
        let m = mi(
            &[
                ("Radial", 0.5),
                ("DarkRadial_Range", 0.3),
                ("DarkRadial_Hard", 0.1),
                ("BG_Triangle_Ptn_Tile", 4.0),
            ],
            &[("Radial_Inside", [0.29, 0.22, 2.5, 0.0])],
        );
        let BgMaterial {
            shader,
            params: p,
            textures: tex,
        } = pack("vMX_3_BGN03_000", &m).unwrap();
        assert_eq!(shader, BgShader::Poly);
        assert_eq!(p[0], [0.29, 0.22, 2.5, 0.5]);
        assert_eq!(p[2][3], 4.0);
        assert!((p[3][3] - 3.333_333).abs() < 1e-4 && (p[4][3] - 1.111_111).abs() < 1e-4);
        // Defaults: RadialDensity 1.45, Flicker_Stlength 2, the master's textures.
        assert_eq!((p[1][3], p[5][3]), (1.45, 2.0));
        assert_eq!(tex[2].as_deref(), Some("vT_BGN03_02_m"));
        assert!(pack("vMX_2_Tap_000", &m).is_none());
    }

    #[test]
    fn cube_and_wall_constants_match_the_captured_cbuffers() {
        // vMX_3_02BGS_000_ODS_m_CubeTile00, frame 13214 eid 647: cb6[3] (-0.380209, 0.239583,
        // -1), cb6[5] (0.382462, 0, 1), cb6[6] (0.617538, 1, 0), cb6[14].x 0.8.
        let m = mi(
            &[("Total_brightness", 0.8)],
            &[
                ("AddColor_Radial", [1.380209, 0.760417, 2.0, 0.0]),
                ("AddColor_Light", [0.382462, 0.0, 1.0, 1.0]),
            ],
        );
        let c = pack("vMX_3_02BGS_000", &m).unwrap();
        assert_eq!(c.shader, BgShader::Cubes);
        let near = |a: [f32; 4], b: [f32; 4]| (0..4).all(|k| (a[k] - b[k]).abs() < 1e-5);
        assert!(
            near(c.params[0], [-0.380209, 0.239583, -1.0, 0.8]),
            "{:?}",
            c.params[0]
        );
        assert!(
            near(c.params[2], [0.617538, 1.0, 0.0, 0.0]),
            "{:?}",
            c.params[2]
        );
        // Wall00, eid 613: rotation rows (0.587785, -0.809017), (0.809017, 0.587785).
        let w = pack("vMX_3_02BGS_002", &mi(&[("Rotate", 0.15)], &[])).unwrap();
        assert!(
            (w.params[1][0] - 0.587_785).abs() < 1e-5 && (w.params[1][1] - 0.809_017).abs() < 1e-5
        );
        assert_eq!(w.textures[3].as_deref(), Some("vT_02BGS03_00_m"));
    }

    fn system(name: &str, material: &str) -> ParticleSystem {
        ParticleSystem {
            name: name.into(),
            warmup: 1.0,
            emitters: vec![Emitter {
                name: String::new(),
                material: material.into(),
                mesh: None,
                mesh_orientation: None,
                gpu: None,
                alignment: "PSA_Square".into(),
                local_space: true,
                origin: [0.0, 1000.0, 0.0],
                duration: 2.0,
                delay: 0.0,
                loops: 0,
                sub_images: [1, 1],
                sub_uv_method: String::new(),
                rate: Distribution::constant(&[0.0]),
                rate_scale: Distribution::constant(&[1.0]),
                bursts: vec![Burst {
                    count: 1,
                    count_low: -1,
                    time: 0.0,
                }],
                burst_scale: Distribution::constant(&[1.0]),
                modules: vec![
                    Module::Lifetime(Distribution::constant(&[2.0])),
                    Module::Size(Distribution::constant(&[8700.0; 3])),
                ],
            }],
        }
    }

    fn library() -> Arc<BgLibrary> {
        let systems = BgState::ALL
            .iter()
            .map(|s| (s.system().to_owned(), system(s.system(), "poly")))
            .collect();
        let params = MaterialParams {
            name: "poly".into(),
            ..MaterialParams::default()
        };
        let fx = EffectLibrary::new(
            TouchEffect::default(),
            systems,
            vec![(params.clone(), "vMX_3_BGN03_000".into())],
            vec![],
            &HashMap::new(),
        );
        let files = HashMap::from([("poly".to_owned(), (params, "vMX_3_BGN03_000".to_owned()))]);
        let srgb = HashMap::from([("vT_BGN03_02_m".to_owned(), false)]);
        Arc::new(BgLibrary::new(fx, &files, &srgb))
    }

    #[test]
    fn library_packs_materials_and_shares_textures() {
        let lib = library();
        let [Some(m)] = lib.materials.as_slice() else {
            panic!("{:?}", lib.materials)
        };
        assert_eq!(m.shader, BgShader::Poly);
        assert_eq!(m.textures, [Some(0), Some(1), Some(2), None]);
        assert_eq!(lib.textures[2], ("vT_BGN03_02_m".to_owned(), false));
        assert!(lib.textures[0].1, "unknown textures default to sRGB");
    }

    #[test]
    fn the_backdrop_fills_the_capture_like_the_captured_sprite() {
        let mut bg = Background::new(library());
        bg.set_state(BgState::Normal);
        let mut frame = BgFrame::default();
        bg.build(&mut frame);
        assert_eq!(frame.runs, [(0, 0, 6)]);
        // Captured (frame 11389 eid 677): clip x ±10501.8 at w 11190 → NDC ±0.9385, UV (0, 0)
        // at the top left.
        for v in &frame.vertices {
            let ndc = [
                CAPTURE_FOCAL * v.pos[0] / v.pos[2],
                CAPTURE_FOCAL * (v.pos[1] - CAPTURE_Z) / v.pos[2],
            ];
            assert!((v.pos[2] - 11190.0).abs() < 1e-2, "{:?}", v.pos);
            assert!((ndc[0].abs() - 0.9385).abs() < 1e-3, "{ndc:?}");
            assert_eq!(ndc[0] > 0.0, v.uv[0] > 0.5);
            assert_eq!(ndc[1] > 0.0, v.uv[1] < 0.5);
        }
        // Same state: nothing restarts; a change swaps the system.
        let before = bg.clone();
        bg.set_state(BgState::Normal);
        assert_eq!(bg, before);
        bg.set_state(BgState::Good);
        assert_eq!(bg.state(), Some(BgState::Good));
    }
}
