// Shared part of the UI material ports (`material.rs`): one quad per draw, per-draw uniforms
// (linear tint + parameters), up to four textures with a wrapping and a clamping sampler.
// Textures are plain RGBA8 (not sRGB views): `decode` linearises colour textures like the
// overlay does; masks are sampled as they are. Output is linear when the target is sRGB.

override LINEARIZE: f32 = 0.0;

const TAU: f32 = 6.2831855;

struct Draw {
    // Linear RGB multiplier and opacity (UMG ColorAndOpacity x RenderOpacity).
    tint: vec4<f32>,
    p: array<vec4<f32>, 15>,
};

@group(0) @binding(0) var<uniform> d: Draw;
@group(1) @binding(0) var t0: texture_2d<f32>;
@group(1) @binding(1) var t1: texture_2d<f32>;
@group(1) @binding(2) var t2: texture_2d<f32>;
@group(1) @binding(3) var t3: texture_2d<f32>;
@group(1) @binding(4) var wrap: sampler;
@group(1) @binding(5) var clamped: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32>) -> VsOut {
    var out: VsOut;
    out.pos = vec4<f32>(pos, 0.0, 1.0);
    out.uv = uv;
    return out;
}

fn decode(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.2));
}

// Emissive (linear) and opacity -> target colour, times the draw's tint.
fn finish(emissive: vec3<f32>, opacity: f32) -> vec4<f32> {
    let c = max(emissive * d.tint.rgb, vec3<f32>(0.0));
    let rgb = select(pow(c, vec3<f32>(1.0 / 2.2)), c, LINEARIZE > 0.5);
    return vec4<f32>(rgb, clamp(opacity * d.tint.a, 0.0, 1.0));
}

// UE4's `Noise` node, Simplex (texture based) flavour: the gradient table (a 128 x 128 random
// texture looked up at (x + 17 z, y + 89 z)) is replaced by a hash of the lattice cell.
// UNVERIFIED: the gradients themselves (the engine texture is not in the game data).
fn lattice_gradient(c: vec3<f32>) -> vec3<f32> {
    var v = vec3<u32>(vec3<i32>(c) + vec3<i32>(4096));
    v = v * 1664525u + 1013904223u;
    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    v = v ^ (v >> vec3<u32>(16u));
    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    return vec3<f32>(v & vec3<u32>(0xffffu)) / 32767.5 - 1.0;
}

fn simplex_corner(p: vec3<f32>, corner: vec3<f32>) -> f32 {
    let off = p - (corner - dot(corner, vec3<f32>(1.0 / 6.0)));
    var w = max(0.6 - dot(off, off), 0.0);
    w = w * w;
    return w * w * dot(lattice_gradient(corner), off);
}

fn simplex(p: vec3<f32>) -> f32 {
    let skewed = p + dot(p, vec3<f32>(1.0 / 3.0));
    let cell = floor(skewed);
    let f = skewed - cell;
    let hi = max(f.z, max(f.y, f.x));
    let lo = min(f.z, min(f.y, f.x));
    let c1 = cell + select(vec3<f32>(0.0), vec3<f32>(1.0), f == vec3<f32>(hi));
    let c2 = cell + select(vec3<f32>(0.0), vec3<f32>(1.0), f != vec3<f32>(lo));
    let n = simplex_corner(p, cell) + simplex_corner(p, cell + 1.0) + simplex_corner(p, c1)
        + simplex_corner(p, c2);
    return n * 32.0;
}

// Six turbulent octaves (|noise| summed, frequency x2 and amplitude x0.5 per level), mapped
// to -1..1 as the material's noise output.
fn turbulence(x: f32) -> f32 {
    var p = vec3<f32>(x);
    var amp = 1.0;
    var sum = 0.0;
    for (var i = 0; i < 6; i++) {
        sum += abs(simplex(p)) * amp;
        p = p + p;
        amp *= 0.5;
    }
    return sum * 2.0 - 1.0;
}
