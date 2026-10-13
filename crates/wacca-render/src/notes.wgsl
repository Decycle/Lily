// Note bodies (notes3d.rs) with vMX_2_NTap_000, ported from the DXBC of its
// TBasePassPSFNoLightMapPolicy<FLocalVertexFactory> pixel shader (doc 11 §1.2–1.3, §3.1); the
// material instance values (NoteColor, Line, ChainNote, Bornus*) come per vertex from a table.
// BLEND_Masked + MSM_Unlit: emissive colour, discard below OpacityMaskClipValue 0.3333.
// Bar lines (vMX_2_BarLine_000, doc 11 §3.1, captured PS d12365/2493) share the pipeline.
// Constant names in comments are the material parameters (cb4 / cb3 slots in the disassembly).

struct Material {
    color: vec4<f32>,
    // Dark, HighLight, Line, ChainNote
    p0: vec4<f32>,
    // BornusNote, BornusNoteFlip, bar line (1), -
    p1: vec4<f32>,
};

@group(0) @binding(0) var<uniform> materials: array<Material, 11>;
@group(0) @binding(1) var note_base: texture_2d<f32>;   // vT_NoteBase00_00_m (sRGB)
@group(0) @binding(2) var note_mask: texture_2d<f32>;   // vT_NoteBase01_00_m (linear)
@group(0) @binding(3) var clamp_samp: sampler;          // SSM_Clamp_WorldGroupSettings

// vMPC_NoteCustomize_00.NoteHeight default.
const NOTE_HEIGHT: f32 = 0.5;
// vMX_2_NTap_000 defaults (unchanged by every note MI).
const CAMERA_LENGTH: f32 = 30.0;
const GRAD_MIN: f32 = 10.0;
const GRAD_MAX: f32 = 22.0;
const FAR: f32 = 0.0;
const EDGE_MIN: f32 = 1.0;
const EDGE_MAX: f32 = 0.92;
const SYNC_NOTE: f32 = 0.0;
const CLIP: f32 = 0.3333;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) w: f32,
    @location(2) @interpolate(flat) material: u32,
};

@vertex
fn vs_main(
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) material: u32,
) -> VsOut {
    var out: VsOut;
    // 90° FOV: NDC = (X, Z) / w (notes3d::place); reversed-Z depth near / w (stage.wgsl).
    out.pos = vec4<f32>(pos.x, pos.y, 10.0, pos.z);
    out.uv = uv;
    out.w = pos.z;
    out.material = material;
    return out;
}

// UE's SmoothStep material function on an already normalised input.
fn smooth01(x: f32) -> f32 {
    let t = clamp(x, 0.0, 1.0);
    return min(t * t * (3.0 - 2.0 * t), 1.0);
}

// UE `If(A, B)` folded to: A >= B -> 1, else 0, with A == B (within 1e-5) -> 1.
fn at_least(a: f32, b: f32) -> f32 {
    if abs(a - b) <= 0.00001 {
        return 1.0;
    }
    return select(0.0, 1.0, a >= b);
}

// NoteBase shading of one base texel: Dark (r), screen HighLight (g), Line (b).
fn shade(t: vec3<f32>, m: Material) -> vec3<f32> {
    let c = m.color.rgb;
    let dark = (c * m.p0.x) * (c * m.p0.x);
    var r = mix(c, dark, t.x);
    r = r + t.y * (1.0 - r) * (c + m.p0.y);
    return r + t.z * m.p0.z;
}

// vMX_2_NTap_000: rgb emissive, a opacity mask.
fn note_tap(uv: vec2<f32>, w: f32, m: Material) -> vec4<f32> {
    // Distance gradient: NoteHeight near the camera, Far beyond.
    let g = smooth01((w / CAMERA_LENGTH - GRAD_MIN) / (GRAD_MAX - GRAD_MIN));
    let nh = clamp(NOTE_HEIGHT, 0.0, 0.85);
    let h = mix(nh, FAR, g);
    let o = vec2<f32>(0.4 * h, 0.0);
    // Outer half (u > 0.5) samples shifted by +o, inner half by -o: the bands split apart.
    let side = smooth01((2.0 * uv.x - EDGE_MIN) / (EDGE_MAX - EDGE_MIN));
    let mask = mix(
        textureSample(note_mask, clamp_samp, uv + o).x,
        textureSample(note_mask, clamp_samp, uv - o).x,
        side,
    );
    let body = mask * at_least(uv.y, 0.57619);
    let ta = textureSample(note_base, clamp_samp, uv + o).rgb;
    let tb = textureSample(note_base, clamp_samp, uv - o).rgb;
    let col = mix(mix(ta, shade(ta, m), body), mix(tb, shade(tb, m), body), side);
    // ChainNote: base alpha stripes darken by half.
    let stripes = body * textureSample(note_base, clamp_samp, uv).a;
    let chained = col * (1.0 - 0.5 * m.p0.w * stripes);
    // BornusNote: mask texture z/w (BornusNoteFlip) in a box rescaled by the split.
    let q = vec2<f32>((uv.x - 0.4 * h) / (1.0 - 0.8 * h), uv.y);
    let fz = textureSample(note_mask, clamp_samp, q).zw;
    let box2 = clamp((1.0 - 2.0 * abs(q - 0.5)) * 100000.0, vec2<f32>(0.0), vec2<f32>(1.0));
    let flip = mix(fz.x, fz.y, m.p1.y) * box2.x * box2.y;
    let bonus = col + flip * (1.0 - col) * m.color.rgb;
    let rgb = mix(chained, bonus, m.p1.x);
    let lo = at_least(uv.y, 0.4);
    let opacity = mask * lo + SYNC_NOTE * mask * (1.0 - lo);
    return vec4<f32>(max(rgb, vec3<f32>(0.0)), opacity);
}

// vMX_2_BarLine_000: a thin line across the band (u = 0.5) that widens to the full band with
// view depth: mask = floor(a + L·(3 − a)), a = 0.2 − cos(2πu), L = min((w/500)^6, 1)·Line_Width.
fn bar_line(uv: vec2<f32>, w: f32, m: Material) -> vec4<f32> {
    let a = 0.2 - cos(6.283185 * uv.x);
    let x = max(w * 0.002, 0.0);
    let l = 1.0 + 1.0 * (min(x * x * x * x * x * x, 1.0) - 1.0);
    return vec4<f32>(max(m.color.rgb, vec3<f32>(0.0)), floor(l * (3.0 - a) + a));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let m = materials[in.material];
    let tap = note_tap(in.uv, in.w, m);
    let c = select(tap, bar_line(in.uv, in.w, m), m.p1.z > 0.5);
    if c.a < CLIP {
        discard;
    }
    // Linear scene colour into the stage target (stage_gpu composes the final 1/2.2 power).
    return vec4<f32>(c.rgb, 1.0);
}
