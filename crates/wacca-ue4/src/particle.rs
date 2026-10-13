//! Cascade `ParticleSystem` packages (UE 4.19) as plain data: emitters (LOD 0) with their
//! required/spawn settings and the modules the game's hit effects use. Distributions are the
//! cooked lookup tables (`FRawDistributionFloat/Vector::Table`); [`Distribution::eval`] follows
//! `FDistributionLookupTable::GetValue`. Simulation lives in `wacca-render`.
//! Spec: `docs/research/06-visuals.md` §3, `docs/research/12-hit-effects.md`.

use serde::{Deserialize, Serialize};

use crate::Ue4Error;
use crate::object::{Object, Objects, read_objects};
use crate::property::Value;

/// A baked distribution: `entry_count` entries of `entry_stride` floats over time
/// `(t - time_bias) * time_scale`; `op` 1 = plain value, 2 = uniform random between the two
/// sub-entries, 3 = one of the two extremes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Distribution {
    pub op: u8,
    pub entry_count: u8,
    pub entry_stride: u8,
    pub sub_entry_stride: u8,
    pub time_scale: f32,
    pub time_bias: f32,
    pub values: Vec<f32>,
    /// Vector lock (`EDistributionVectorLockFlags`): 0 none, 1 XY, 2 XZ, 3 YZ, 4 XYZ share one
    /// random number.
    pub lock_flag: u8,
}

const OP_RANDOM: u8 = 2;
const OP_EXTREME: u8 = 3;

impl Distribution {
    /// A constant (any number of components).
    pub fn constant(v: &[f32]) -> Self {
        Self {
            op: 1,
            entry_count: 1,
            entry_stride: v.len() as u8,
            values: v.to_vec(),
            ..Self::default()
        }
    }

    /// Components per value.
    pub fn dims(&self) -> usize {
        let stride = usize::from(self.entry_stride.max(1));
        if matches!(self.op, OP_RANDOM | OP_EXTREME) {
            (stride / 2).max(1)
        } else {
            stride
        }
    }

    /// Value at time `t` (up to 3 components; missing ones are 0). `rand` are uniform numbers in
    /// `[0, 1)` per component, used by random distributions (locked components share one).
    pub fn eval(&self, t: f32, rand: [f32; 3]) -> [f32; 3] {
        let mut out = [0.0; 3];
        let n = usize::from(self.entry_count);
        let stride = usize::from(self.entry_stride);
        if n == 0 || stride == 0 || self.values.len() < n * stride {
            return out;
        }
        let alpha = ((t - self.time_bias) * self.time_scale).max(0.0);
        let i1 = (alpha.floor() as usize).min(n - 1);
        let i2 = (i1 + 1).min(n - 1);
        let f = alpha - alpha.floor();
        let at = |k: usize| {
            let (a, b) = (self.values[i1 * stride + k], self.values[i2 * stride + k]);
            a + (b - a) * f
        };
        let dims = self.dims().min(3);
        let mut r = rand;
        match self.lock_flag {
            1 => r[1] = r[0],
            2 => r[2] = r[0],
            3 => r[2] = r[1],
            4 => (r[1], r[2]) = (r[0], r[0]),
            _ => {}
        }
        let sub = usize::from(self.sub_entry_stride.max(1));
        for (k, o) in out.iter_mut().enumerate().take(dims) {
            *o = match self.op {
                OP_RANDOM => {
                    let (lo, hi) = (at(k), at(sub + k));
                    lo + (hi - lo) * r[k]
                }
                OP_EXTREME => {
                    if r[k] > 0.5 {
                        at(sub + k)
                    } else {
                        at(k)
                    }
                }
                _ => at(k),
            };
        }
        out
    }

    /// First component of [`Distribution::eval`].
    pub fn eval1(&self, t: f32, rand: f32) -> f32 {
        self.eval(t, [rand; 3])[0]
    }

    /// Whether the value differs from zero anywhere.
    pub fn is_nonzero(&self) -> bool {
        self.values.iter().any(|v| *v != 0.0)
    }
}

/// One burst: `count` particles (random down to `count_low` when ≥ 0) at emitter-time fraction
/// `time`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Burst {
    pub count: i32,
    pub count_low: i32,
    pub time: f32,
}

/// Baked curves of a GPU emitter (`FGPUSpriteResourceData`), sampled uniformly over relative
/// time 0..1: colour (`QuantizedColorSamples · ColorScale + ColorBias`) and the size factor
/// applied to the initial size (`(QuantizedMiscSamples.rg · MiscScale + MiscBias) · InvMaxSize`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GpuCurves {
    pub color: Vec<[f32; 4]>,
    pub size: Vec<[f32; 2]>,
}

impl GpuCurves {
    fn sample<const N: usize>(v: &[[f32; N]], t: f32) -> Option<[f32; N]> {
        let n = v.len();
        let x = t.clamp(0.0, 1.0) * (n.checked_sub(1)?) as f32;
        let i = (x.floor() as usize).min(n - 1);
        let j = (i + 1).min(n - 1);
        let f = x - i as f32;
        Some(std::array::from_fn(|k| v[i][k] + (v[j][k] - v[i][k]) * f))
    }

    /// Colour at relative time `t`.
    pub fn color_at(&self, t: f32) -> Option<[f32; 4]> {
        Self::sample(&self.color, t)
    }

    /// Size factor at relative time `t`.
    pub fn size_at(&self, t: f32) -> Option<[f32; 2]> {
        Self::sample(&self.size, t)
    }
}

fn gpu_curves(t: &Object) -> GpuCurves {
    let data = t.get("ResourceData");
    let vec4 = |name: &str| -> [f32; 4] {
        let mut out = [0.0; 4];
        if let Some(Value::Array(v)) = data.and_then(|d| d.field(name)) {
            for (o, x) in out.iter_mut().zip(v) {
                *o = x.as_f32().unwrap_or(0.0);
            }
        }
        out
    };
    let samples = |name: &str| -> Vec<[f32; 4]> {
        match data.and_then(|d| d.field(name)) {
            Some(Value::Array(v)) => v
                .iter()
                .map(|c| {
                    ["R", "G", "B", "A"]
                        .map(|k| c.field(k).and_then(Value::as_i64).unwrap_or(0) as f32 / 255.0)
                })
                .collect(),
            _ => Vec::new(),
        }
    };
    let inv_max = match t.get("EmitterInfo").and_then(|e| e.field("InvMaxSize")) {
        Some(Value::Array(v)) => [0, 1].map(|i| v.get(i).and_then(Value::as_f32).unwrap_or(1.0)),
        _ => [1.0; 2],
    };
    let (cs, cb) = (vec4("ColorScale"), vec4("ColorBias"));
    let (ms, mb) = (vec4("MiscScale"), vec4("MiscBias"));
    GpuCurves {
        color: samples("QuantizedColorSamples")
            .into_iter()
            .map(|c| std::array::from_fn(|k| c[k] * cs[k] + cb[k]))
            .collect(),
        size: samples("QuantizedMiscSamples")
            .into_iter()
            .map(|m| [0, 1].map(|k| (m[k] * ms[k] + mb[k]) * inv_max[k]))
            .collect(),
    }
}

/// The particle modules the hit effects use, in LOD-0 module order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Module {
    Lifetime(Distribution),
    /// Initial size (vector).
    Size(Distribution),
    /// Size = initial size × curve(relative time) on the flagged axes.
    SizeMultiplyLife {
        curve: Distribution,
        axes: [bool; 3],
    },
    /// Size = initial size × scale(relative time).
    SizeScale(Distribution),
    /// Initial colour and alpha.
    Color {
        color: Distribution,
        alpha: Distribution,
        clamp_alpha: bool,
    },
    /// Colour and alpha set from curves over relative time.
    ColorOverLife {
        color: Distribution,
        alpha: Distribution,
        clamp_alpha: bool,
    },
    /// Initial position offset (vector).
    Location(Distribution),
    /// Initial position on a cylinder (`ParticleModuleLocationPrimitiveCylinder`).
    Cylinder {
        start_location: Distribution,
        radius: Distribution,
        height: Distribution,
        /// 0 = X, 1 = Y, 2 = Z.
        height_axis: u8,
        surface_only: bool,
        velocity: bool,
        velocity_scale: Distribution,
    },
    /// Initial velocity (vector) plus `radial` along the direction from the emitter origin.
    Velocity {
        velocity: Distribution,
        radial: Distribution,
        world_space: bool,
    },
    /// Velocity × curve (or = curve when `absolute`) over relative time.
    VelocityOverLife {
        curve: Distribution,
        absolute: bool,
    },
    /// Initial rotation in turns.
    Rotation(Distribution),
    /// Rotation rate in turns per second.
    RotationRate(Distribution),
    /// Sprite pivot offset (added to the default −0.5, −0.5).
    PivotOffset([f32; 2]),
    /// Sub-image index over relative time.
    SubImage(Distribution),
    /// A module class not simulated; kept so missing behaviour is visible.
    Unsupported(String),
}

/// An enabled emitter of a particle system (LOD 0).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Emitter {
    pub name: String,
    /// Material (object name, e.g. `vMX_2_Tap_000_TUS_m_05`).
    pub material: String,
    /// Mesh particles: the static mesh (object name) and its initial roll/pitch/yaw (degrees).
    pub mesh: Option<String>,
    pub mesh_orientation: Option<Distribution>,
    /// GPU sprites (`ParticleModuleTypeDataGpu`): their baked colour and size curves, which the
    /// GPU simulation uses instead of the colour/size-over-life modules.
    pub gpu: Option<GpuCurves>,
    /// `EParticleScreenAlignment` name (`PSA_Square`, `PSA_Velocity`, `PSA_Rectangle`, ...).
    pub alignment: String,
    pub local_space: bool,
    pub duration: f32,
    pub delay: f32,
    /// 0 = loop forever.
    pub loops: i32,
    pub sub_images: [u32; 2],
    /// `EParticleSubUVInterpMethod` name.
    pub sub_uv_method: String,
    /// Spawn rate (per second, over emitter time) and its scale.
    pub rate: Distribution,
    pub rate_scale: Distribution,
    pub bursts: Vec<Burst>,
    pub burst_scale: Distribution,
    pub modules: Vec<Module>,
}

/// A particle system: its enabled emitters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParticleSystem {
    pub name: String,
    pub emitters: Vec<Emitter>,
}

/// Reads the (last) `ParticleSystem` export of a package.
pub fn read_particle_system(uasset: &[u8], uexp: &[u8]) -> Result<ParticleSystem, Ue4Error> {
    let objs = read_objects(uasset, uexp)?;
    let sys = objs
        .exports
        .iter()
        .rfind(|o| o.class == "ParticleSystem")
        .ok_or(Ue4Error::NotAParticleSystem)?;
    let emitters = objects(&objs, sys.get("Emitters"))
        .filter_map(|e| emitter(&objs, e))
        .collect();
    Ok(ParticleSystem {
        name: sys.name.clone(),
        emitters,
    })
}

/// Exports referenced by an array of object references.
fn objects<'a>(objs: &'a Objects, v: Option<&'a Value>) -> impl Iterator<Item = &'a Object> {
    let items = match v {
        Some(Value::Array(items)) => items.as_slice(),
        _ => &[],
    };
    items.iter().filter_map(|v| match v {
        Value::Object(i) => objs.export(*i),
        _ => None,
    })
}

fn object<'a>(objs: &'a Objects, v: Option<&Value>) -> Option<&'a Object> {
    match v {
        Some(Value::Object(i)) => objs.export(*i),
        _ => None,
    }
}

fn ref_name(objs: &Objects, v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::Object(i)) => objs.name(*i).map(str::to_owned),
        _ => None,
    }
}

fn float(o: &Object, name: &str, default: f32) -> f32 {
    o.get(name).and_then(Value::as_f32).unwrap_or(default)
}

fn int(o: &Object, name: &str, default: i64) -> i64 {
    o.get(name).and_then(Value::as_i64).unwrap_or(default)
}

fn flag(o: &Object, name: &str, default: bool) -> bool {
    match o.get(name) {
        Some(Value::Bool(b)) => *b,
        _ => default,
    }
}

fn enum_name(o: &Object, name: &str, default: &str) -> String {
    o.get(name)
        .and_then(Value::as_str)
        .unwrap_or(default)
        .to_owned()
}

fn f32_field(v: &Value, name: &str) -> f32 {
    v.field(name).and_then(Value::as_f32).unwrap_or(0.0)
}

fn u8_field(v: &Value, name: &str) -> u8 {
    v.field(name)
        .and_then(Value::as_i64)
        .and_then(|i| u8::try_from(i).ok())
        .unwrap_or(0)
}

/// A `FRawDistributionFloat/Vector` property: its baked table, or (not baked) the constant /
/// uniform distribution object it points at; `default` when absent.
fn distribution(objs: &Objects, o: &Object, name: &str, default: &[f32]) -> Distribution {
    let Some(raw) = o.get(name) else {
        return Distribution::constant(default);
    };
    if let Some(table) = raw.field("Table") {
        let values = match table.field("Values") {
            Some(Value::Array(v)) => v.iter().filter_map(Value::as_f32).collect(),
            _ => Vec::new(),
        };
        if !values.is_empty() {
            return Distribution {
                op: u8_field(table, "Op"),
                entry_count: u8_field(table, "EntryCount"),
                entry_stride: u8_field(table, "EntryStride"),
                sub_entry_stride: u8_field(table, "SubEntryStride"),
                time_scale: f32_field(table, "TimeScale"),
                time_bias: f32_field(table, "TimeBias"),
                values,
                lock_flag: u8_field(table, "LockFlag"),
            };
        }
    }
    match object(objs, raw.field("Distribution")) {
        Some(d) if d.class == "DistributionFloatConstant" => {
            Distribution::constant(&[float(d, "Constant", 0.0)])
        }
        Some(d) if d.class == "DistributionFloatUniform" => Distribution {
            op: OP_RANDOM,
            entry_count: 1,
            entry_stride: 2,
            sub_entry_stride: 1,
            values: vec![float(d, "Min", 0.0), float(d, "Max", 0.0)],
            ..Distribution::default()
        },
        Some(d) if d.class.ends_with("ConstantCurve") => {
            bake_curve(d.get("ConstantCurve")).unwrap_or_else(|| Distribution::constant(default))
        }
        Some(d) => {
            log::warn!("{}.{name}: unbaked {} not supported", o.name, d.class);
            Distribution::constant(default)
        }
        None => Distribution::constant(default),
    }
}

/// Entries of a baked [`FInterpCurve`] sample.
const BAKED_ENTRIES: usize = 32;

/// One key of an `FInterpCurveFloat/Vector`.
struct Key {
    t: f32,
    out: Vec<f32>,
    arrive: Vec<f32>,
    leave: Vec<f32>,
    mode: String,
}

fn floats_of(v: Option<&Value>) -> Vec<f32> {
    match v {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_f32).collect(),
        Some(v) => v.as_f32().into_iter().collect(),
        None => Vec::new(),
    }
}

/// Samples an unbaked `FInterpCurve` (linear / constant / Hermite keys, as `FInterpCurve::Eval`)
/// into a [`BAKED_ENTRIES`]-entry table over its key range.
fn bake_curve(curve: Option<&Value>) -> Option<Distribution> {
    let Some(Value::Array(points)) = curve?.field("Points") else {
        return None;
    };
    let keys: Vec<Key> = points
        .iter()
        .map(|p| Key {
            t: f32_field(p, "InVal"),
            out: floats_of(p.field("OutVal")),
            arrive: floats_of(p.field("ArriveTangent")),
            leave: floats_of(p.field("LeaveTangent")),
            mode: p
                .field("InterpMode")
                .and_then(Value::as_str)
                .unwrap_or("CIM_Linear")
                .to_owned(),
        })
        .collect();
    let first = keys.first()?;
    let dims = first.out.len();
    if dims == 0 || keys.iter().any(|k| k.out.len() != dims) {
        return None;
    }
    let (t0, t1) = (first.t, keys.last()?.t);
    let span = (t1 - t0).max(1e-6);
    let at = |t: f32| -> Vec<f32> {
        let Some(i) = keys.windows(2).position(|w| t < w[1].t) else {
            return keys.last().map(|k| k.out.clone()).unwrap_or_default();
        };
        let (a, b) = (&keys[i], &keys[i + 1]);
        if t <= a.t {
            return a.out.clone();
        }
        let diff = b.t - a.t;
        let x = (t - a.t) / diff;
        (0..dims)
            .map(|k| match a.mode.as_str() {
                "CIM_Constant" => a.out[k],
                "CIM_Linear" => a.out[k] + (b.out[k] - a.out[k]) * x,
                _ => {
                    let (p0, p1) = (a.out[k], b.out[k]);
                    let m0 = a.leave.get(k).copied().unwrap_or(0.0) * diff;
                    let m1 = b.arrive.get(k).copied().unwrap_or(0.0) * diff;
                    let (x2, x3) = (x * x, x * x * x);
                    (2.0 * x3 - 3.0 * x2 + 1.0) * p0
                        + (x3 - 2.0 * x2 + x) * m0
                        + (-2.0 * x3 + 3.0 * x2) * p1
                        + (x3 - x2) * m1
                }
            })
            .collect()
    };
    let values = (0..BAKED_ENTRIES)
        .flat_map(|i| at(t0 + span * i as f32 / (BAKED_ENTRIES - 1) as f32))
        .collect();
    Some(Distribution {
        op: 1,
        entry_count: BAKED_ENTRIES as u8,
        entry_stride: dims as u8,
        time_scale: (BAKED_ENTRIES - 1) as f32 / span,
        time_bias: t0,
        values,
        ..Distribution::default()
    })
}

fn emitter(objs: &Objects, e: &Object) -> Option<Emitter> {
    let lod = objects(objs, e.get("LODLevels")).next()?;
    let required = object(objs, lod.get("RequiredModule"))?;
    // Only the LOD level decides: `vo_miss000_00` has its required module flagged off yet draws
    // (RenderDoc frame 11184).
    if !flag(lod, "bEnabled", true) {
        return None;
    }
    let spawn = object(objs, lod.get("SpawnModule"));
    let type_data = object(objs, lod.get("TypeDataModule"));
    let is_mesh = type_data.is_some_and(|t| t.class == "ParticleModuleTypeDataMesh");
    let bursts = spawn
        .and_then(|s| match s.get("BurstList") {
            Some(Value::Array(b)) => Some(b.as_slice()),
            _ => None,
        })
        .unwrap_or_default()
        .iter()
        .map(|b| Burst {
            count: b.field("Count").and_then(Value::as_i64).unwrap_or(0) as i32,
            count_low: b.field("CountLow").and_then(Value::as_i64).unwrap_or(-1) as i32,
            time: f32_field(b, "Time"),
        })
        .collect();
    let spawn_dist = |name, default: f32| match spawn {
        Some(s) => distribution(objs, s, name, &[default]),
        None => Distribution::constant(&[default]),
    };
    let modules = objects(objs, lod.get("Modules"))
        .filter(|m| flag(m, "bEnabled", true))
        .map(|m| module(objs, m))
        .collect();
    Some(Emitter {
        name: enum_name(e, "EmitterName", ""),
        material: ref_name(objs, required.get("Material")).unwrap_or_default(),
        mesh: type_data
            .filter(|_| is_mesh)
            .and_then(|t| ref_name(objs, t.get("Mesh"))),
        mesh_orientation: type_data
            .filter(|_| is_mesh)
            .map(|t| distribution(objs, t, "RollPitchYawRange", &[0.0; 3])),
        gpu: type_data
            .filter(|t| t.class == "ParticleModuleTypeDataGpu")
            .map(gpu_curves),
        alignment: enum_name(required, "ScreenAlignment", "PSA_Square"),
        local_space: flag(required, "bUseLocalSpace", false),
        duration: float(required, "EmitterDuration", 1.0),
        delay: float(required, "EmitterDelay", 0.0),
        loops: int(required, "EmitterLoops", 0) as i32,
        sub_images: [
            int(required, "SubImages_Horizontal", 1).max(1) as u32,
            int(required, "SubImages_Vertical", 1).max(1) as u32,
        ],
        sub_uv_method: enum_name(required, "InterpolationMethod", "PSUVIM_None"),
        rate: spawn_dist("Rate", 0.0),
        rate_scale: spawn_dist("RateScale", 1.0),
        bursts,
        burst_scale: spawn_dist("BurstScale", 1.0),
        modules,
    })
}

fn module(objs: &Objects, m: &Object) -> Module {
    let d = |name, default: &[f32]| distribution(objs, m, name, default);
    let class = m.class.trim_end_matches("_Seeded");
    match class {
        "ParticleModuleLifetime" => Module::Lifetime(d("Lifetime", &[0.0])),
        "ParticleModuleSize" => Module::Size(d("StartSize", &[1.0; 3])),
        "ParticleModuleSizeMultiplyLife" => Module::SizeMultiplyLife {
            curve: d("LifeMultiplier", &[1.0; 3]),
            axes: ["MultiplyX", "MultiplyY", "MultiplyZ"].map(|a| flag(m, a, true)),
        },
        "ParticleModuleSizeScale" => Module::SizeScale(d("SizeScale", &[1.0; 3])),
        "ParticleModuleColor" => Module::Color {
            color: d("StartColor", &[1.0; 3]),
            alpha: d("StartAlpha", &[1.0]),
            clamp_alpha: flag(m, "bClampAlpha", true),
        },
        "ParticleModuleColorOverLife" => Module::ColorOverLife {
            color: d("ColorOverLife", &[1.0; 3]),
            alpha: d("AlphaOverLife", &[1.0]),
            clamp_alpha: flag(m, "bClampAlpha", true),
        },
        "ParticleModuleLocation" => Module::Location(d("StartLocation", &[0.0; 3])),
        "ParticleModuleLocationPrimitiveCylinder" => Module::Cylinder {
            start_location: d("StartLocation", &[0.0; 3]),
            radius: d("StartRadius", &[50.0]),
            height: d("StartHeight", &[50.0]),
            height_axis: match enum_name(m, "HeightAxis", "PMLPC_HEIGHTAXIS_Z").as_str() {
                "PMLPC_HEIGHTAXIS_X" => 0,
                "PMLPC_HEIGHTAXIS_Y" => 1,
                _ => 2,
            },
            surface_only: flag(m, "SurfaceOnly", false),
            velocity: flag(m, "Velocity", false),
            velocity_scale: d("VelocityScale", &[1.0]),
        },
        "ParticleModuleVelocity" => Module::Velocity {
            velocity: d("StartVelocity", &[0.0; 3]),
            radial: d("StartVelocityRadial", &[0.0]),
            world_space: flag(m, "bInWorldSpace", false),
        },
        "ParticleModuleVelocityOverLifetime" => Module::VelocityOverLife {
            curve: d("VelOverLife", &[1.0; 3]),
            absolute: flag(m, "Absolute", false),
        },
        "ParticleModuleRotation" => Module::Rotation(d("StartRotation", &[0.0])),
        "ParticleModuleRotationRate" => Module::RotationRate(d("StartRotationRate", &[0.0])),
        "ParticleModulePivotOffset" => {
            let p = match m.get("PivotOffset") {
                Some(Value::Array(v)) => [0, 1].map(|i| v.get(i).and_then(Value::as_f32)),
                _ => [None; 2],
            };
            Module::PivotOffset(p.map(|v| v.unwrap_or(0.0)))
        }
        "ParticleModuleSubUV" => Module::SubImage(d("SubImageIndex", &[0.0])),
        other => Module::Unsupported(other.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::Fixture;

    fn table(op: u8, count: u8, stride: u8, scale: f32, values: &[f32]) -> Distribution {
        Distribution {
            op,
            entry_count: count,
            entry_stride: stride,
            sub_entry_stride: if op == OP_RANDOM { stride / 2 } else { 0 },
            time_scale: scale,
            values: values.to_vec(),
            ..Distribution::default()
        }
    }

    #[test]
    fn curves_interpolate_and_clamp() {
        // 3 entries over t 0..1 (time scale 2).
        let d = table(1, 3, 1, 2.0, &[2.0, 1.0, 0.0]);
        assert_eq!(d.eval1(0.0, 0.0), 2.0);
        assert_eq!(d.eval1(0.25, 0.0), 1.5);
        assert_eq!(d.eval1(1.0, 0.0), 0.0);
        assert_eq!(d.eval1(5.0, 0.0), 0.0);
        assert_eq!(d.eval1(-1.0, 0.0), 2.0);
        let biased = Distribution {
            time_bias: 0.5,
            ..d.clone()
        };
        assert_eq!(biased.eval1(0.5, 0.0), 2.0);
        // Captured: vo_tap000_03 FlareTall ColorOverLife at relative time 0.0952 = 1.833.
        let c = table(1, 4, 1, 15.0, &[2.0, 1.912_306_4, 1.726_46, 1.515_842]);
        assert!((c.eval1(0.0952, 0.0) - 1.8327).abs() < 1e-3);
    }

    #[test]
    fn random_vectors_respect_locks() {
        let mut d = table(OP_RANDOM, 1, 6, 0.0, &[0.0, 10.0, 20.0, 1.0, 11.0, 21.0]);
        assert_eq!(d.dims(), 3);
        assert_eq!(d.eval(0.0, [0.0, 0.5, 1.0]), [0.0, 10.5, 21.0]);
        d.lock_flag = 4;
        assert_eq!(d.eval(0.0, [0.5, 0.0, 0.0]), [0.5, 10.5, 20.5]);
        d.lock_flag = 1;
        assert_eq!(d.eval(0.0, [1.0, 0.0, 0.0]), [1.0, 11.0, 20.0]);
        let scalar = table(OP_RANDOM, 1, 2, 0.0, &[0.2, 0.3]);
        assert!((scalar.eval1(0.0, 0.5) - 0.25).abs() < 1e-6);
        let extreme = Distribution {
            op: OP_EXTREME,
            ..scalar
        };
        assert_eq!(extreme.eval1(0.0, 0.9), 0.3);
        assert_eq!(Distribution::default().eval1(0.0, 0.0), 0.0);
    }

    #[test]
    fn gpu_curves_sample_uniformly() {
        let g = GpuCurves {
            color: vec![[0.0; 4], [2.0, 4.0, 6.0, 1.0]],
            size: vec![[1.0, 1.0], [1.0, 3.0], [1.0, 5.0]],
        };
        assert_eq!(g.color_at(0.5), Some([1.0, 2.0, 3.0, 0.5]));
        assert_eq!(g.size_at(0.75), Some([1.0, 4.0]));
        assert_eq!(g.size_at(2.0), Some([1.0, 5.0]));
        assert_eq!(GpuCurves::default().color_at(0.3), None);
    }

    #[test]
    fn unbaked_curves_are_sampled() {
        let key = |t: f32, v: f32, mode: &str| {
            Value::Struct(vec![
                ("InVal".into(), Value::Float(t)),
                ("OutVal".into(), Value::Float(v)),
                ("ArriveTangent".into(), Value::Float(0.0)),
                ("LeaveTangent".into(), Value::Float(0.0)),
                ("InterpMode".into(), Value::Enum(mode.into())),
            ])
        };
        let curve = Value::Struct(vec![(
            "Points".into(),
            Value::Array(vec![
                key(0.2, 1.0, "CIM_Linear"),
                key(1.0, 0.0, "CIM_CurveAuto"),
                key(2.0, 4.0, "CIM_Linear"),
            ]),
        )]);
        let d = bake_curve(Some(&curve)).unwrap();
        assert_eq!(d.eval1(0.0, 0.0), 1.0);
        assert!((d.eval1(0.6, 0.0) - 0.5).abs() < 0.01);
        // Hermite with flat tangents: smoothstep from 0 to 4.
        assert!((d.eval1(1.5, 0.0) - 2.0).abs() < 0.05);
        assert_eq!(d.eval1(3.0, 0.0), 4.0);
    }

    /// Tagged `FRawDistributionFloat` with a baked table.
    fn raw_float(fx: &mut Fixture, b: &mut Vec<u8>, name: &str, op: u8, values: &[f32]) {
        let mut t = Vec::new();
        fx.tag(&mut t, "Op", "ByteProperty", &[op]);
        fx.tag(&mut t, "EntryCount", "ByteProperty", &[1]);
        let stride = values.len() as u8;
        fx.tag(&mut t, "EntryStride", "ByteProperty", &[stride]);
        let mut vals = (values.len() as i32).to_le_bytes().to_vec();
        values.iter().for_each(|v| vals.extend(v.to_le_bytes()));
        fx.array_tag(&mut t, "Values", "FloatProperty", &vals);
        fx.none(&mut t);
        let mut raw = Vec::new();
        fx.struct_tag(&mut raw, "Table", "DistributionLookupTable", &t);
        fx.none(&mut raw);
        fx.struct_tag(b, name, "RawDistributionFloat", &raw);
    }

    #[test]
    fn reads_a_system_graph() {
        let mut fx = Fixture::default();
        let class = |fx: &mut Fixture, n: &str| fx.import("/Script/CoreUObject", "Class", n);
        let c_sys = class(&mut fx, "ParticleSystem");
        let c_em = class(&mut fx, "ParticleSpriteEmitter");
        let c_lod = class(&mut fx, "ParticleLODLevel");
        let c_req = class(&mut fx, "ParticleModuleRequired");
        let c_life = class(&mut fx, "ParticleModuleLifetime");
        let c_odd = class(&mut fx, "ParticleModuleKillBox");
        let mat = fx.import("/Script/CoreUObject", "Material", "vM_Test");
        let obj = |v: i32| v.to_le_bytes();
        // Exports: 1 system, 2 emitter, 3 LOD, 4 required, 5 lifetime, 6 unsupported.
        let mut sys = Vec::new();
        fx.array_tag(
            &mut sys,
            "Emitters",
            "ObjectProperty",
            &[1, 0, 0, 0, 2, 0, 0, 0],
        );
        fx.none(&mut sys);
        let mut em = Vec::new();
        let name = fx.fname("Flare");
        fx.tag(&mut em, "EmitterName", "NameProperty", &name);
        fx.array_tag(
            &mut em,
            "LODLevels",
            "ObjectProperty",
            &[1, 0, 0, 0, 3, 0, 0, 0],
        );
        fx.none(&mut em);
        let mut lod = Vec::new();
        fx.tag(&mut lod, "RequiredModule", "ObjectProperty", &obj(4));
        let mut mods = 2i32.to_le_bytes().to_vec();
        mods.extend(obj(5));
        mods.extend(obj(6));
        fx.array_tag(&mut lod, "Modules", "ObjectProperty", &mods);
        fx.none(&mut lod);
        let mut req = Vec::new();
        fx.tag(&mut req, "Material", "ObjectProperty", &obj(mat));
        fx.tag(
            &mut req,
            "EmitterDuration",
            "FloatProperty",
            &0.35f32.to_le_bytes(),
        );
        fx.bool_tag(&mut req, "bUseLocalSpace", true);
        fx.none(&mut req);
        let mut life = Vec::new();
        raw_float(&mut fx, &mut life, "Lifetime", OP_RANDOM, &[0.2, 0.3]);
        fx.none(&mut life);
        let mut odd = Vec::new();
        fx.none(&mut odd);
        let bodies = [sys, em, lod, req, life, odd];
        let classes = [c_sys, c_em, c_lod, c_req, c_life, c_odd];
        for (i, (b, c)) in bodies.iter().zip(classes).enumerate() {
            fx.export(c, &format!("o{i}"), b.len());
        }
        let ps = read_particle_system(&fx.uasset(), &bodies.concat()).unwrap();
        assert_eq!(ps.name, "o0");
        let [e] = ps.emitters.as_slice() else {
            panic!("{ps:?}")
        };
        assert_eq!((e.name.as_str(), e.material.as_str()), ("Flare", "vM_Test"));
        assert!(e.local_space && (e.duration - 0.35).abs() < 1e-6 && e.loops == 0);
        assert_eq!(e.alignment, "PSA_Square");
        assert_eq!((e.mesh.as_ref(), e.gpu.as_ref()), (None, None));
        let Module::Lifetime(l) = &e.modules[0] else {
            panic!("{:?}", e.modules)
        };
        assert!((l.eval1(0.0, 1.0) - 0.3).abs() < 1e-6);
        assert_eq!(
            e.modules[1],
            Module::Unsupported("ParticleModuleKillBox".into())
        );
        // No spawn module: no bursts, rate 0.
        assert!(e.bursts.is_empty() && !e.rate.is_nonzero());
    }

    #[test]
    fn missing_system_is_an_error() {
        let mut fx = Fixture::default();
        let c = fx.import("/Script/CoreUObject", "Class", "Thing");
        let mut b = Vec::new();
        fx.none(&mut b);
        fx.export(c, "t", b.len());
        assert!(matches!(
            read_particle_system(&fx.uasset(), &b),
            Err(Ue4Error::NotAParticleSystem)
        ));
    }
}
