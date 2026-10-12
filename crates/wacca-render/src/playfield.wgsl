// Playfield triangles in playfield space (rim radius 1), drawn into a square viewport, sampling
// the premultiplied sprite atlas (flat colours sample its white block). See layout::Paint.

// Playfield radius mapped to the viewport edge (style::VIEW_EXTENT).
override VIEW_EXTENT: f32 = 1.0;
// 1.0 when the target format is sRGB: colours are sRGB and must be linearised.
override LINEARIZE: f32 = 0.0;

@group(0) @binding(0) var atlas: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    // x: mask, y: additive.
    @location(2) fx: vec2<f32>,
};

@vertex
fn vs_main(
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) fx: vec2<f32>,
) -> VsOut {
    var out: VsOut;
    out.pos = vec4<f32>(pos / VIEW_EXTENT, 0.0, 1.0);
    out.uv = uv;
    out.color = color;
    out.fx = fx;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let t = textureSample(atlas, samp, in.uv);
    let a = t.a * in.color.a;
    let inv = 1.0 / max(t.a, 1e-4);
    // Image: colour × texel. Mask: colour × shade (g) plus white highlight (r).
    let image = in.color.rgb * t.rgb * inv;
    let mask = in.color.rgb * t.g * inv + vec3<f32>(t.r * inv);
    var rgb = clamp(select(image, mask, in.fx.x > 0.5), vec3<f32>(0.0), vec3<f32>(1.0));
    rgb = select(rgb, pow(rgb, vec3<f32>(2.2)), LINEARIZE > 0.5);
    // Premultiplied output; additive pixels keep the destination (alpha 0).
    return vec4<f32>(rgb * a, a * (1.0 - in.fx.y));
}
