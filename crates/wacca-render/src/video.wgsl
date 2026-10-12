// Background movie: three R8 planes (yuv420p) → RGB on a quad filling the playfield viewport.
// Same job as CriWare's ManaYUVToRGB. UNVERIFIED: BT.601 limited range (06-visuals §1, color_space 0).

override LINEARIZE: f32 = 0.0;

@group(0) @binding(0) var y_tex: texture_2d<f32>;
@group(0) @binding(1) var u_tex: texture_2d<f32>;
@group(0) @binding(2) var v_tex: texture_2d<f32>;
@group(0) @binding(3) var samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    let uv = vec2<f32>(f32(i & 1u), f32(i >> 1u));
    var out: VsOut;
    out.pos = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // The cabinet screen is round: only the inscribed circle of the square movie is ever visible.
    if (distance(in.uv, vec2<f32>(0.5)) > 0.5) {
        discard;
    }
    let y = 1.164 * (textureSample(y_tex, samp, in.uv).r - 16.0 / 255.0);
    let u = textureSample(u_tex, samp, in.uv).r - 0.5;
    let v = textureSample(v_tex, samp, in.uv).r - 0.5;
    let rgb = clamp(vec3<f32>(y + 1.596 * v, y - 0.392 * u - 0.813 * v, y + 2.017 * u), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(select(rgb, pow(rgb, vec3<f32>(2.2)), LINEARIZE > 0.5), 1.0);
}
