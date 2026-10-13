// Background movie: three R8 planes (yuv420p) on the game's movie quad, ported from the
// `mv_<song>_Texture_Mat` (CriWare ManaYUVToRGB) base-pass PS (doc 11 §2.6, frame 12365 eid 2754).
// The quad is ±810 cm at Y = −620, 810 cm from the camera: it fills the square view exactly.
// Output: linear scene colour into the stage target (BT.601 RGB, max 0, ^2.2 as emissive).

@group(0) @binding(0) var y_tex: texture_2d<f32>;
@group(0) @binding(1) var u_tex: texture_2d<f32>;
@group(0) @binding(2) var v_tex: texture_2d<f32>;
@group(0) @binding(3) var samp: sampler;

// Reversed-Z depth of the quad: near / w = 10 / 810.
const DEPTH: f32 = 10.0 / 810.0;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    let uv = vec2<f32>(f32(i & 1u), f32(i >> 1u));
    var out: VsOut;
    out.pos = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, DEPTH, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let y = textureSample(y_tex, samp, in.uv).r - 0.0625;
    let u = textureSample(u_tex, samp, in.uv).r - 0.5;
    let v = textureSample(v_tex, samp, in.uv).r - 0.5;
    let rgb = 1.164 * y + vec3<f32>(1.596 * v, -0.813 * v - 0.392 * u, 2.017 * u);
    return vec4<f32>(pow(max(rgb, vec3<f32>(0.0)), vec3<f32>(2.2)), 1.0);
}
