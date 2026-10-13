// `uM_WCbg000`, the animated backdrop of the select / mode / welcome menus: a port of the
// material's compiled pixel shader (runs/shader-research/shaders/ui/uM_WCbg000, base pass PS 011;
// doc 14 §3). t0 = uT_WCbg00 (sRGB), t1 = uT_WCbg01 (linear masks), t2 = uT_CM19 (radial alpha).
// Output is linear (the sRGB target encodes it, as Slate's gamma step does).

struct U {
    // Quad corners in clip space: x0, y0 (top left), x1, y1 (bottom right).
    rect: vec4<f32>,
    // x = View.GameTime (seconds).
    time: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var t0: texture_2d<f32>;
@group(0) @binding(2) var t1: texture_2d<f32>;
@group(0) @binding(3) var t2: texture_2d<f32>;
@group(0) @binding(4) var s: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    let corner = vec2<f32>(f32(i & 1u), f32((i >> 1u) & 1u));
    var o: VsOut;
    o.pos = vec4<f32>(mix(u.rect.xy, u.rect.zw, corner), 0.0, 1.0);
    o.uv = corner;
    return o;
}

// Material uniforms (cb3), from the material's FUniformExpressionSet.
const DOT_IN = vec3<f32>(1.2, 0.0062499763, 0.8587272);
const DOT_OUT = vec3<f32>(0.39156383, 0.0, 0.9010417);
const DOT_LINE = vec3<f32>(0.7835378, 0.24620132, 0.4507858);
const TRIANGLE = vec3<f32>(1.0, 0.009721217, 0.18116425);
const GLOW = vec3<f32>(0.116971, 0.001821, 0.097587);
const LINE_SUB = vec3<f32>(0.03433981, 0.084376216, 0.8148468);
const LINE = vec3<f32>(0.5260417, 0.0010188967, 0.1400715);
const PI = 3.141593;

fn rot(d: vec2<f32>, a: f32) -> vec2<f32> {
    let c = cos(a);
    let n = sin(a);
    return vec2<f32>(c * d.x - n * d.y, n * d.x + c * d.y);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let t = u.time.x;
    let d = uv - 0.5;
    let r = length(d);
    let mask = textureSample(t1, s, uv);
    var col = textureSample(t0, s, uv).rgb;
    // Dot blocks: inner → outer colour with the radius, a rotating radial sweep lights 80 %.
    let dot_col = mix(DOT_OUT, DOT_IN, saturate((1.0 - 2.0 * r) * 1.010101));
    let sweep = textureSample(t2, s, rot(d, 4.0 * t) + uv + 0.5).a;
    col += dot_col * mask.r * (0.2 + 0.8 * sweep);
    // Slowly turning triangles / dot lines: triangle colour inside r 0.3.
    let inside = saturate((1.0 - 3.333333 * r) * 100000.0);
    let tri = mix(DOT_LINE, TRIANGLE, inside);
    col += tri * textureSample(t1, s, rot(d, 0.25 * t) + 0.5).g;
    // Tiled glow pattern pulsing between the mask and its inverse.
    let q = textureSample(t1, s, uv * 4.0).a;
    let pulse = (sin(PI * t) + 1.0) * 0.5;
    col += (q + pulse * (1.0 - 2.0 * q)) * GLOW * (0.3 + 0.7 * inside);
    // Lines with a diagonal wave running down; a 0.009-offset copy as the shadow.
    let shadow = textureSample(t1, s, uv - 0.009).b;
    let p = vec2<f32>(uv.x, uv.y - 0.5 * t) - 0.5;
    let wave = max(sin((dot(p, vec2<f32>(0.707107, -0.707107)) + 0.5) * 6.283185), 0.0);
    let sub = shadow * mix(LINE_SUB, LINE_SUB * 2.0, wave);
    col += mix(sub, mix(LINE, LINE * 3.0, wave), mask.b);
    return vec4<f32>(max(col, vec3<f32>(0.0)), 1.0);
}
