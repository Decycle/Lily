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

// The capture as the empty play view shows it, for UI images such as the options screen's monitor
// (the real one is a preview capture of the play view): under the guide-lane cone's base colour
// B_Color_5 with its no-movie depth fade (alpha 1 − min((w/600)³, 1), stage.wgsl), the cone point
// seen at NDC radius ρ being Y = (190ρ − 175)/(ρ + 0.29167); then the main view's pow(1/2.2).
@fragment
fn fs_display(in: VsOut) -> @location(0) vec4<f32> {
    let bg = textureSample(capture, samp, in.uv).rgb;
    let rho = length(in.uv * 2.0 - 1.0);
    let w = 190.0 - (190.0 * rho - 175.0) / (rho + 0.29167);
    let lanes = 1.0 - min(pow(w / 600.0, 3.0), 1.0);
    let c = mix(bg, vec3<f32>(0.0122, 0.0133, 0.0313), lanes);
    return vec4<f32>(pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2)), 1.0);
}
