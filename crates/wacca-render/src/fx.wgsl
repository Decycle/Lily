// Hit effects (particles.rs), ported from the DXBC of the effect materials' base-pass pixel
// shaders (doc 12; annotated captures runs/renderdoc/cap3/d12365/annot/3555 key beam, 3623 tap
// flare, 3642 MarvPt sprite, 3675 HitMarv; d11184/annot/2455 Miss; the others from
// runs/shaders/vfx/<material>/*_ps_TBasePassPSFNoLightMapPolicy*_F{MeshParticle,GPUSprite}*.asm).
// Unlit: the emissive colour is the linear scene colour. Material constants are packed by
// `particles::pack` (names in the comments); NoteHeight = 0.5 (MPC default).

struct Fx {
    // View.GameTime (s).
    time: vec4<f32>,
};

struct Material {
    params: array<vec4<f32>, 6>,
    // Shader, texture layer + 1 of the two textures (0 = none), sRGB bits.
    info: vec4<u32>,
};

@group(0) @binding(0) var<uniform> fx: Fx;
@group(0) @binding(1) var<storage, read> materials: array<Material>;
@group(0) @binding(2) var textures: texture_2d_array<f32>;
@group(0) @binding(3) var samp: sampler;

const TAU: f32 = 6.283185;
const CLIP: f32 = 0.3333;

const SHADER_TAP: u32 = 0u;
const SHADER_HOLD: u32 = 1u;
const SHADER_BASE004: u32 = 2u;
const SHADER_BASE000: u32 = 3u;
const SHADER_BASE005: u32 = 4u;
const SHADER_SCROLL: u32 = 5u;
const SHADER_DISSOLVE: u32 = 6u;
const SHADER_MISS: u32 = 7u;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) extra: vec4<f32>,
    @location(3) @interpolate(flat) material: u32,
};

// View-space `[X, Z, w]` (camera.rs), as the stage: NDC = (X, Z) / w, reversed-Z depth near / w.
@vertex
fn vs_main(
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) extra: vec4<f32>,
    @location(4) material: u32,
) -> VsOut {
    var out: VsOut;
    out.pos = vec4<f32>(pos.x, pos.y, 10.0, pos.z);
    out.uv = uv;
    out.color = color;
    out.extra = extra;
    out.material = material;
    return out;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

// Texture `slot` (0, 1) of material `m` at `uv` (wrapped when `wrap`), linear colour.
fn tex(m: Material, slot: u32, uv: vec2<f32>, wrap: bool) -> vec4<f32> {
    let layer = select(m.info.y, m.info.z, slot == 1u);
    if layer == 0u {
        return vec4<f32>(1.0);
    }
    let st = select(clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), fract(uv), wrap);
    let c = textureSampleLevel(textures, samp, st, i32(layer - 1u), 0.0);
    let srgb = (m.info.w >> slot & 1u) == 1u;
    return vec4<f32>(select(c.rgb, srgb_to_linear(c.rgb), srgb), c.a);
}

fn smooth01(x: f32) -> f32 {
    let t = clamp(x, 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

fn powp(x: f32, e: f32) -> f32 {
    return select(0.0, pow(x, e), x > 0.0);
}

// --- vMX_2_Tap_000 (MIs 00/01 key beams, 05–08 hit flares) ---
// p0 = NoteLength_0.5, ColorTex_pos, ColorTexture_ONOFF, btm_Mask_Contrast
// p1 = Btmmask_OnOFF, GlowSub_Gamma, GlowCore_Gamma, LinePos; p2.x = LineLength_Change_ONOFF
fn tap(m: Material, uv: vec2<f32>, vc: vec4<f32>) -> vec4<f32> {
    let p0 = m.params[0];
    let p1 = m.params[1];
    let h = p0.x;
    let ramp = tex(m, 0u, uv + vec2<f32>(0.0, h + p0.y), false).rgb;
    let col = mix(vc.rgb, ramp * vc.rgb, p0.z);
    let c = p0.w;
    let cm = clamp((1.0 - (h + uv.y)) * (1.0 + 2.0 * c) - c, 0.0, 1.0);
    let w = mix(uv.y, uv.y * cm, p1.x);
    let a = (powp(w * 0.5, p1.y) + powp(w, p1.z)) * vc.a;
    let s = sin(TAU * (uv.y * 0.5 + p1.w));
    let k = min(round(smooth01((s - (h - 0.1)) / 0.1)), 1.0);
    let alpha = clamp(a - m.params[2].x * a * k, 0.0, 1.0);
    return vec4<f32>(max(col, vec3<f32>(0.0)), alpha);
}

// --- vMX_2_Hold_000 (MIs 02/03/04): the hold-held glow (no particle colour) ---
// p0 = CoreGlowPos (NoteHeight lerp), CoreGlow_Min, CoreGlow_Max, CoreGlowMskPosV (lerp)
// p1 = CoreGlowRange_Min, _Max, CoreGlow_Strength, ColorBalance
// p2 = LineGlow_Pos, LineGlow_Min, LineGlow_Max, LineGow_Strength
// p3 = U_pos, Rotate, FlickCycle_Speed, FlickStrength_Max; p4 = CoreGlow_Color, FlickStrength_Min
// p5 = SideGlow_Color
fn hold(m: Material, uv: vec2<f32>) -> vec4<f32> {
    let p = m.params;
    let s1 = smooth01((p[0].x + uv.y - p[0].y) / (p[0].z - p[0].y));
    let s2 = 1.0 - smooth01((p[0].w + uv.y - p[1].x) / (p[1].y - p[1].x));
    let core = s1 * s2 * p[1].z;
    let col = mix(p[5].rgb, p[4].rgb, clamp(core * p[1].w, 0.0, 1.0));
    let lv = uv.y + p[2].x;
    let line = smooth01((2.0 * lv * (1.0 - lv) - p[2].y) / (p[2].z - p[2].y));
    var a = core + line * p[2].w;
    // Side mask: CustomRotator(UV + (U_pos, 0), Rotate) — UNVERIFIED uniform values.
    let r = p[3].y * TAU;
    let d = dot(uv + vec2<f32>(p[3].x, 0.0) - 0.5, vec2<f32>(cos(r), -sin(r))) + 0.5;
    a = a * clamp(floor(2.0 * d), 0.0, 1.0);
    let flick = sin(TAU * fx.time.x * p[3].z) * (p[3].w - p[4].w) + p[4].w;
    return vec4<f32>(max(col, vec3<f32>(0.0)), clamp(a * flick, 0.0, 1.0));
}

// --- vM_1_Base_005 (Bhitpt): Color1/2/3 picked by the particle alpha; p0.w = Ramdom ---
fn base005(m: Material, uv: vec2<f32>, vc: vec4<f32>) -> vec4<f32> {
    var col = mix(m.params[1].rgb, m.params[2].rgb, round(vc.a));
    if vc.a >= m.params[0].w || abs(vc.a - m.params[0].w) <= 1e-5 {
        col = m.params[0].rgb;
    }
    return vec4<f32>(max(col, vec3<f32>(0.0)), clamp(tex(m, 0u, uv, false).r, 0.0, 1.0));
}

// --- vMX_1_Miss_000 (masked): shards cut from a ring by angle sectors (d11184 eid 2455) ---
// p0 = RandomColorA, outside_scale; p1 = RandomColorB, outside_Density;
// p2 = Line_Color, inside_Density; p3 = Base_Color. Particle red = inner radius, green = UV
// inset, alpha = opacity; extra.x = ParticleRandom.
fn miss(m: Material, uv: vec2<f32>, vc: vec4<f32>, rnd: f32) -> vec4<f32> {
    let p = m.params;
    let sel = select(1.0, select(0.0, 1.0, rnd <= 0.5), abs(0.5 - rnd) > 1e-5);
    let t = tex(m, 0u, uv, true);
    let col = mix(p[0].rgb, p[1].rgb, sel) * (t.r * p[2].rgb + t.g * p[3].rgb);
    let q = uv * 2.0 - 1.0;
    let ang = fract(atan2(q.y, q.x) / TAU);
    let cuts = ceil(fract((ang + clamp(vec2<f32>(rnd), vec2<f32>(0.4, 0.55), vec2<f32>(0.45, 0.7))) / 3.0) * 3.0) - 2.0;
    let sector = select(0.0, 1.0, cuts.x < -1e-5 && !(cuts.y < -1e-5));
    let d = length(uv - 0.5);
    let e = 1.0 - d / p[0].w;
    let outer = select(0.0, 1.0 - exp(-(e * p[1].w) * (e * p[1].w)), e >= 0.0 && abs(e) > 1e-5);
    let f = 1.0 - d / vc.r;
    let inner = select(0.0, 1.0 - exp(-(f * p[2].w) * (f * p[2].w)), f >= 0.0 && abs(f) > 1e-5);
    let ring = sector * (outer - inner) * vc.a * t.b;
    let uv2 = (uv - vc.g) / (1.0 - 2.0 * vc.g);
    let edge = clamp((1.0 - 2.0 * abs(uv2.x - 0.5)) * 1e5, 0.0, 1.0)
        * clamp((1.0 - 2.0 * abs(uv2.y - 0.5)) * 1e5, 0.0, 1.0);
    let cut = 1.0 - tex(m, 1u, uv2, true).r * edge;
    return vec4<f32>(max(col, vec3<f32>(0.0)), ring * cut);
}

// (colour, opacity) of a fragment.
fn shade(in: VsOut) -> vec4<f32> {
    let m = materials[in.material];
    let vc = in.color;
    let uv = in.uv;
    switch m.info.x {
        case SHADER_TAP: {
            return tap(m, uv, vc);
        }
        case SHADER_HOLD: {
            return hold(m, uv);
        }
        case SHADER_BASE004: {
            // vM_1_Base_004 (MarvPt, Btap): texture × particle colour.
            let t = tex(m, 0u, uv, false);
            return vec4<f32>(max(t.rgb * vc.rgb, vec3<f32>(0.0)), clamp(t.a * vc.a, 0.0, 1.0));
        }
        case SHADER_BASE000: {
            // vM_1_Base_000 (HoldPt, Snote): particle colour, texture red as opacity
            // (BG_Mask lerp = 1).
            let a = clamp(tex(m, 0u, uv, false).r * vc.a, 0.0, 1.0);
            return vec4<f32>(max(vc.rgb, vec3<f32>(0.0)), a);
        }
        case SHADER_BASE005: {
            return base005(m, uv, vc);
        }
        case SHADER_SCROLL: {
            // vM_2_Scroll_003 (HitMarv): the particle alpha scrolls the mask (wrap).
            let a = clamp(tex(m, 0u, uv + vec2<f32>(1.0, vc.a), true).r, 0.0, 1.0);
            return vec4<f32>(max(vc.rgb, vec3<f32>(0.0)), a);
        }
        case SHADER_DISSOLVE: {
            // vM_2_Dslv_003 (GrateBm): ColorPower × colour; dissolve mask scrolled by the
            // particle red.
            let c = tex(m, 0u, uv, true);
            let d = tex(m, 1u, uv + vec2<f32>(0.0, vc.r), true).r;
            return vec4<f32>(max(c.rgb * m.params[0].x, vec3<f32>(0.0)), clamp(c.a * d * vc.a, 0.0, 1.0));
        }
        case SHADER_MISS: {
            return miss(m, uv, vc, in.extra.x);
        }
        default: {
            // Not ported: drawn invisible.
            return vec4<f32>(0.0);
        }
    }
}

// BLEND_Additive: One / One (alpha: Zero / InvSrcAlpha).
@fragment
fn fs_additive(in: VsOut) -> @location(0) vec4<f32> {
    let c = shade(in);
    return vec4<f32>(c.rgb * c.a, 0.0);
}

// BLEND_Translucent: SrcAlpha / InvSrcAlpha.
@fragment
fn fs_translucent(in: VsOut) -> @location(0) vec4<f32> {
    return shade(in);
}

// BLEND_Masked: opaque where the opacity mask reaches the clip value.
@fragment
fn fs_masked(in: VsOut) -> @location(0) vec4<f32> {
    let c = shade(in);
    if c.a < CLIP {
        discard;
    }
    return vec4<f32>(c.rgb, 1.0);
}
