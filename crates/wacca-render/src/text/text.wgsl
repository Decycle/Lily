// 2D overlay: textured, tinted quads in clip space. The glyph atlas is white with coverage in
// alpha; images are plain RGBA. Colours (vertex and texture) are treated as sRGB.

// 1.0 when the target format is sRGB: colours must be linearised.
override LINEARIZE: f32 = 0.0;

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32>) -> VsOut {
    var out: VsOut;
    out.pos = vec4<f32>(pos, 0.0, 1.0);
    out.uv = uv;
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let c = in.color * textureSample(tex, samp, in.uv);
    let rgb = select(c.rgb, pow(c.rgb, vec3<f32>(2.2)), LINEARIZE > 0.5);
    return vec4<f32>(rgb, c.a);
}
