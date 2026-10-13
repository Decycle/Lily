// In the play view's stage pass: the background capture (bg.wgsl) as the emissive far quad at
// Y = −621 (±811 cm: fills the square view; BgCaptureTextureRenderTarget2D_Mat, KING frame 11389
// eid 2783: emissive = capture colour, opaque).

@group(0) @binding(0) var capture: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

const DEPTH: f32 = 10.0 / 811.0;

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
    return vec4<f32>(textureSample(capture, samp, in.uv).rgb, 1.0);
}
