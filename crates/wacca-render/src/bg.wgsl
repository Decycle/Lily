// The no-movie background scene (bg.rs), ported from the base-pass pixel shaders of its
// materials with the captured constants (RenderDoc KING frame 11389: eid 677
// vMX_3_BGN03_000 backdrop, 692 vMX_3_BGN03_002 rings, 711 vMX_3_BGBoss_004 dots; annotated
// disassembly in runs/bg/d11389/annot; KING frame 13214: eid 613 vMX_3_02BGS_002 walls, 647/654
// vMX_3_02BGS_000 cubes, 676 vM_1_SAUV_000 triangles, runs/bg/d13214/annot). Emissive = scene
// colour; the Good walls and cubes are also lit (base colour = emissive) by the level's
// directional light as RenderDoc's pixel debugger showed (frame 13214 eid 647: no indirect light,
// no reflections). Written clamped into the 8-bit capture target like the capture's tonemapper
// (saturate, no curve); bg_quad.wgsl draws the capture as the play view's emissive far quad.

struct Frame {
    // View.GameTime (s), MPC_BpmSignal Bar, Beat, 0.
    clock: vec4<f32>,
};

struct Material {
    p: array<vec4<f32>, 10>,
};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(1) @binding(0) var<uniform> mat: Material;
@group(1) @binding(1) var t0: texture_2d<f32>;
@group(1) @binding(2) var t1: texture_2d<f32>;
@group(1) @binding(3) var t2: texture_2d<f32>;
@group(1) @binding(4) var t3: texture_2d<f32>;
@group(1) @binding(5) var wrap: sampler;

const TAU: f32 = 6.283185;
const PI: f32 = 3.141593;
const CLIP: f32 = 0.3333;
// Capture camera: 45° FOV, raised 10000 cm above the play camera (bg.rs), at world
// (0, 190, 10000); capture target side.
const FOCAL: f32 = 2.4142135;
const CAPTURE_Z: f32 = 10000.0;
const CAMERA: vec3<f32> = vec3<f32>(0.0, 190.0, 10000.0);
const SIZE: f32 = 1080.0;
// The level's directional light (forward light data, frame 13214): towards +Y, intensity 1.1.
const LIGHT_DIR: vec3<f32> = vec3<f32>(0.0, 1.0, 0.0);
const LIGHT: f32 = 1.1;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    // World position (cm, UE axes).
    @location(2) world: vec3<f32>,
};

// View-space `[X, Z, w]` of the play camera (camera.rs) -> world.
fn to_world(v: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(v.x, 190.0 - v.z, v.y);
}

// The capture camera looks the same way as the play camera from CAPTURE_Z higher with a narrower
// view. Reversed-Z depth near / w (near 10).
fn project(world: vec3<f32>) -> vec4<f32> {
    return vec4<f32>(FOCAL * world.x, FOCAL * (world.z - CAPTURE_Z), 10.0, 190.0 - world.y);
}

@vertex
fn vs_main(
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
) -> VsOut {
    var out: VsOut;
    out.world = to_world(pos);
    out.pos = project(out.world);
    out.uv = uv;
    out.color = color;
    return out;
}

// UE material `LinearSine` "rounded": a 0→1→0 triangle wave over one period.
fn tri(x: f32) -> f32 {
    let s = fract(x);
    return select(2.0 * s, 2.0 - 2.0 * s, s >= 0.5);
}

fn smooth01(x: f32) -> f32 {
    let t = clamp(x, 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// vMX_3_02BGS_000 WorldPositionOffset (VS of eid 647): each cluster breathes away from the
// capture axis by the beat (MPC Beat, as compiled for both terms), weighted by the mesh's vertex
// colour (green: bar term, blue: beat term). p1.w Beat_SineSpeed, p2.w/p3.w BeatStep_Min/Max,
// p4 Beat_WorldpositionAnim + Bar_SineSpeed, p5 Bar_WorldpositionAnim + BarStep_Min, p6.x
// BarStep_Max.
@vertex
fn vs_cubes(
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) extra: vec4<f32>,
    @location(4) origin: vec3<f32>,
) -> VsOut {
    let p = mat.p;
    let beat = frame.clock.z;
    let b = tri((beat + 0.25 * p[1].w) / p[1].w);
    let beat_amt = smooth01((b - p[2].w) / (p[3].w - p[2].w)) * extra.w;
    let r = tri((beat + 0.25 * p[4].w) / p[4].w);
    let bar_amt = smooth01((smooth01(r) - p[5].w) / (p[6].x - p[5].w)) * extra.z;
    let rel = to_world(origin) - vec3<f32>(0.0, 0.0, CAPTURE_Z);
    var out: VsOut;
    out.world = to_world(pos) + rel * (bar_amt * p[5].xyz + beat_amt * p[4].xyz);
    out.pos = project(out.world);
    out.uv = uv;
    out.color = color;
    return out;
}

// Base colour = clamped emissive, default-lit (roughness 0.5, specular 0.5, not metallic) by the
// directional light only: emissive + (diffuse/π + GGX specular) · 1.1 · N·L (UE 4.19 forward
// shading, frame 13214 eid 647 pixel debug). `world`: the pixel's world position; the face normal
// comes from its derivatives (flat faces), turned to the camera (two-sided walls).
fn lit(e: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    var n = normalize(cross(dpdy(world), dpdx(world)));
    let v = normalize(CAMERA - world);
    if dot(n, v) < 0.0 {
        n = -n;
    }
    let base = clamp(e, vec3<f32>(0.0), vec3<f32>(1.0));
    let nol_raw = dot(n, LIGHT_DIR);
    let nov_raw = dot(n, v);
    let vol = dot(v, LIGHT_DIR);
    let inv = inverseSqrt(2.0 * vol + 2.0);
    let noh = clamp((nol_raw + nov_raw) * inv, 0.0, 1.0);
    let voh = clamp(inv * vol + inv, 0.0, 1.0);
    let nol = clamp(nol_raw, 0.0, 1.0);
    let nov = min(abs(nov_raw) + 0.00001, 1.0);
    let a = 0.25;
    let a2 = a * a;
    let d = (noh * a2 - noh) * noh + 1.0;
    let dist = a2 / (PI * d * d);
    let vis = 1.0 / (nol * (nov * (1.0 - a) + a) + nov * (nol * (1.0 - a) + a));
    let fc = pow(1.0 - voh, 5.0);
    let f = 0.04 * (1.0 - fc) + fc;
    let spec = f * dist * vis * 0.5;
    return max(e, vec3<f32>(0.0)) + (base / PI + spec) * LIGHT * nol;
}

// Sampled through the texture's view format: sRGB textures decode to linear.
fn s0(uv: vec2<f32>) -> vec4<f32> {
    return textureSample(t0, wrap, uv);
}
fn s1(uv: vec2<f32>) -> vec4<f32> {
    return textureSample(t1, wrap, uv);
}
fn s2(uv: vec2<f32>) -> vec4<f32> {
    return textureSample(t2, wrap, uv);
}
fn s3(uv: vec2<f32>) -> vec4<f32> {
    return textureSample(t3, wrap, uv);
}

// 1 inside the unit square, 0 outside (sharp: saturate((1 − 2|p − 0.5|)·1e5)).
fn inside(p: vec2<f32>) -> f32 {
    let q = clamp((1.0 - 2.0 * abs(p - 0.5)) * 100000.0, vec2<f32>(0.0), vec2<f32>(1.0));
    return q.x * q.y;
}

// vMX_3_BGN03_000: radial glow × flickering triangle pattern, rotating light layer, dark rim,
// tile grid and moving line layer. p0 Radial_Inside + Radial, p1 Radial_Outside +
// RadialDensity, p2 ColorB + BG_Triangle_Ptn_Tile, p3 ColorA + 1/DarkRadial_Range, p4 LightColor
// + 1/(1 − DarkRadial_Hard), p5 DarkRadial + Flicker_Stlength, p6 Tile_line, p7 Tile, p8 MecLine,
// p9 Line; t0 BG_Triangle_Ptn, t1 PT_Triangle, t2 BG_Mec_Ptn.
@fragment
fn fs_poly(in: VsOut) -> @location(0) vec4<f32> {
    let p = mat.p;
    let time = frame.clock.x;
    let bar = frame.clock.y;
    let beat = frame.clock.z;
    let uv = in.uv;
    let c = uv - 0.5;
    let d = length(c);
    // RadialGradientExponential.
    let a = 1.0 - d / p[0].w;
    var g = 0.0;
    if a >= 0.0 && abs(a) > 0.00001 {
        let e = a * p[1].w;
        g = 1.0 - exp(-e * e);
    }
    let radial = p[1].xyz + (p[0].xyz - p[1].xyz) * g;
    // Triangle pattern, bulged towards the centre, flickering with the beat.
    let tri = s0(uv * p[2].w + c * (1.0 - 3.5 * d) - 2.0).xyz;
    let flick = clamp(sin(TAU * beat) + 0.5, 0.0, 1.0);
    let q = clamp(mix(tri.x, tri.y, flick) + 0.35, 0.0, 1.0) * tri.z;
    let lit = q * radial;
    let pt = s1(uv).xy;
    let light = mix(p[4].xyz, mix(p[3].xyz, p[2].xyz, pt.y) + 0.2, pt.x + pt.y);
    let ang = time * 4.0;
    let rot = vec2<f32>(cos(ang) * c.x - sin(ang) * c.y, sin(ang) * c.x + cos(ang) * c.y);
    var col = mix(lit, light, s1(rot + uv + 0.5).z);
    // Dark rim: DarkRadial outside DarkRadial_Range, pulsing with the bar.
    let f = clamp((1.0 - d * p[3].w) * p[4].w, 0.0, 1.0);
    col = mix(col * p[5].xyz, col, f + bar * f * (p[5].w - 1.0));
    // Tile grid (two mirrored halves scrolled by the beat) and its lines.
    let mec = s2(uv).zw;
    let top = s0(uv + vec2<f32>(0.0, beat)).a;
    let bottom = s0(vec2<f32>(uv.x, beat - uv.y)).a;
    let tile = mix(top, bottom, select(0.0, 1.0, uv.y > 0.5));
    col += p[6].xyz * mec.x * 0.35 + p[7].xyz * tile * mec.y;
    // Moving line layers along a 30° axis.
    let hx = dot(c, vec2<f32>(0.866025, 0.5));
    let hy = dot(c, vec2<f32>(-0.5, 0.866025)) + 0.5;
    let l1 = s2(vec2<f32>(hx + 0.487, time * 0.15 + hy - 0.013)).r;
    let l2 = s2(vec2<f32>(hx + 0.5, hy + time * 0.3)).g;
    let lines = p[8].xyz * l1 + l2 * (p[9].xyz - p[8].xyz * l1);
    col = mix(col, lines, clamp(l1 + l2, 0.0, 1.0));
    return vec4<f32>(clamp(col, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}

// vMX_3_BGN03_002: two crops of the ring texture (green at G_MinScale, red at R_MinScale,
// widened by the bar) as the opacity mask; particle colour. p0 = (G_MinScale, R_MinScale).
@fragment
fn fs_rings(in: VsOut) -> @location(0) vec4<f32> {
    let bar = frame.clock.y;
    let a = mix(mat.p[0].x, 1.0, bar);
    let b = mix(mat.p[0].y, 1.0, bar);
    let ua = (in.uv - a) / (1.0 - 2.0 * a);
    let ub = (in.uv - b) / (1.0 - 2.0 * b);
    let mask = s0(ua).g * inside(ua) + s0(ub).r * inside(ub);
    if mask - CLIP < 0.0 {
        discard;
    }
    return vec4<f32>(clamp(in.color.rgb, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}

// vMX_3_BGBoss_004: tiled circle texture, each tile cropped by a scrolling BPM strip; the
// circle's red is the mask. p0 = (1_U_Tiling, 1_V_Tiling, 2_U_scroll), p1 = (CircleMain_Min,
// CircleMain_Max); t0 BPM_scroll, t1 Texture.
@fragment
fn fs_dots(in: VsOut) -> @location(0) vec4<f32> {
    let time = frame.clock.x;
    let p = mat.p;
    let tiled = fract(vec2<f32>(in.uv.x * p[0].x + p[0].z, in.uv.y * p[0].y + fract(time)));
    let strip = s0(vec2<f32>(in.uv.x + 1.0, fract(in.color.a * time) - in.uv.y)).r;
    let s = mix(p[1].x, p[1].y, strip);
    let q = (tiled - s) / (1.0 - 2.0 * s);
    let circle = s1(q).rgb;
    let k = inside(q);
    if circle.r * k - CLIP < 0.0 {
        discard;
    }
    return vec4<f32>(clamp(in.color.rgb * circle * k, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}

fn out_lit(e: vec3<f32>, world: vec3<f32>) -> vec4<f32> {
    return vec4<f32>(clamp(lit(e, world), vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}

// vMX_3_02BGS_002: the tunnel walls. A scrolled, rotated RGB pattern mixes the screen-aligned
// colour texture with ColorLine, an overlay of itself shows through beat-sized dots (sphere mask
// from SphereMsk_Xpos/Ypos), and two rotating screen-space line layers add Line_Color and
// Triangle_Color. p0 (Tilling_V, Scroll_U, Scroll_V), p1 (cos, sin of Rotate, TileAmount,
// SphereMsk_Xpos), p2 ColorLine + SphereMsk_Ypos, p3 Line_Color, p4 Triangle_Color; t0
// RGBTexture, t1 vT_02BGS02 (screen), t2 TetureObject (circle), t3 vT_02BGS03 (lines).
@fragment
fn fs_wall(in: VsOut) -> @location(0) vec4<f32> {
    let p = mat.p;
    let time = frame.clock.x;
    let bar = frame.clock.y;
    let beat = frame.clock.z;
    let screen = in.pos.xy / SIZE;
    let q0 = in.uv * p[0].x + time * p[0].yz - 0.5;
    let q = vec2<f32>(dot(q0, vec2<f32>(p[1].x, -p[1].y)), dot(q0, p[1].yx)) + 0.5;
    let pat = s0(q).rgb;
    let glow = clamp(mix(pat.x, pat.y, bar) * 0.2, 0.0, 1.0);
    let sc = s1(screen).rgb;
    let dim = 1.0 - (1.0 - sc) * (1.0 - 0.5 * pat.y);
    var c = mix(dim, sc + 0.2, pat.x);
    c = mix(c, p[2].xyz, pat.z) + glow;
    let over = select(2.0 * c * c, 1.0 - 2.0 * (1.0 - c) * (1.0 - c), c >= vec3<f32>(0.5));
    // Dots: a sphere mask of radius Beat around (Xpos, Ypos) sizes each tile's circle.
    let g = in.uv * p[1].z;
    let cell = (floor(g) + 0.5) / p[1].z - vec2<f32>(p[1].w, p[2].w);
    let sphere = clamp((1.0 - length(cell) / max(beat, 0.00001)) * 1.008065, 0.0, 1.0);
    let pulse = smooth01((1.0 - beat) * 1.25) * smooth01(beat * 1.538462);
    let size = min(2.0 * tri(sphere + 0.25) * pulse, 1.0) * 0.7 + 0.5;
    let cp = (fract(g) - size) / (1.0 - 2.0 * size);
    let dots = s2(cp).rgb * inside(cp);
    c = mix(c, over, dots);
    let a1 = sin(time * 0.031416) * TAU;
    let a2 = sin(time * -0.062832) * TAU;
    let d1 = screen - vec2<f32>(0.1, 0.5);
    let l1 = s3(vec2<f32>(
        cos(a1) * d1.x - sin(a1) * d1.y + 0.1,
        cos(a1) * d1.y + sin(a1) * d1.x + 0.5,
    )).r;
    c = mix(c, p[3].xyz, l1);
    let d2 = screen - 0.5;
    let l2 = s3(vec2<f32>(
        cos(a2) * d2.x - sin(a2) * d2.y + 0.5,
        cos(a2) * d2.y + sin(a2) * d2.x + 0.5,
    )).g;
    c = mix(c, p[4].xyz, l2);
    return out_lit(c, in.world);
}

// vMX_3_02BGS_000: the cubes. The base texture screened with AddColor_Radial, overlaid with
// AddColor_Light and darkened by AddColor_Dark, as the screen-aligned mask's blue / red / green
// say; the top-right tile corner screens itself by the bar. p0 1 − AddColor_Radial +
// Total_brightness, p1 AddColor_Light, p2 1 − AddColor_Light, p3 AddColor_Dark; t0 BaseTexture,
// t1 vT_02BGS04 (screen).
@fragment
fn fs_cubes(in: VsOut) -> @location(0) vec4<f32> {
    let p = mat.p;
    let bar = frame.clock.y;
    let t = s0(in.uv).rgb;
    let m = s1(in.pos.xy / SIZE).xyz;
    var c = mix(t, 1.0 - (1.0 - t) * p[0].xyz, m.z);
    let hi = 1.0 - 2.0 * (1.0 - c) * p[2].xyz;
    let lo = 2.0 * c * p[1].xyz;
    c = mix(c, select(lo, hi, c >= vec3<f32>(0.5)), m.x);
    c = mix(c, p[3].xyz * t, m.y);
    // ValueStep: both UVs + 0.35 in (1, 2] (mod 3).
    let k = ceil(fract((in.uv + 0.35) / 3.0) * 3.0) - 2.0;
    let w = select(0.0, bar, all(abs(k) <= vec2<f32>(0.00001)));
    c = mix(c, 1.0 - (1.0 - c) * (1.0 - c), w);
    return out_lit(c * p[0].w, in.world);
}

// vM_1_SAUV_000: triangle sprites. The particle colour (lowered by Line_Power off the shape's
// green) times a screen-aligned colour texture rotated by SAUV_Rot (+ ScreenAligh_AddColor);
// ScreenAligh_or_SelectColor blends towards LineColor. Mask = particle alpha · (shape r + g).
// p0 (Line_Power, cos, sin of SAUV_Rot, ScreenAligh_or_SelectColor), p1 ScreenAligh_AddColor,
// p2 LineColor; t0 Shape, t1 Color_texture.
@fragment
fn fs_triangles(in: VsOut) -> @location(0) vec4<f32> {
    let p = mat.p;
    let shape = s0(in.uv).xy;
    let d = in.pos.xy / SIZE - 0.5;
    let st = vec2<f32>(dot(d, vec2<f32>(p[0].y, -p[0].z)), dot(d, p[0].zy)) + 0.5;
    let tint = s1(st).rgb + p[1].xyz;
    if in.color.a * (shape.x + shape.y) - CLIP < 0.0 {
        discard;
    }
    let a = mix(in.color.rgb - p[0].x, in.color.rgb, shape.y) * tint;
    let c = a + p[0].w * (mix(p[2].xyz, a, shape.y) - a);
    return vec4<f32>(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
