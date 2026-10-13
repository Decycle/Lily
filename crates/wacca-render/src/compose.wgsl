// The game's main-view tonemap (doc 11 §2.6, frame 12365 eid 4483): the permutation that draws
// the play view only reads scene colour, `out = pow(scene · exposure, 1/2.2)` with exposure 1,
// into a UNORM target (clamped). No film curve, LUT, bloom or vignette. Drawn over the
// playfield viewport, reading the resolved stage target pixel for pixel.

// 1.0 when the target is sRGB: it re-encodes what we write, so write the value whose sRGB
// encoding is the game's output.
override SRGB_TARGET: f32 = 0.0;

@group(0) @binding(0) var scene: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32(i & 1u), f32(i >> 1u));
    return vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
}

fn srgb_to_linear(e: vec3<f32>) -> vec3<f32> {
    return select(pow((e + 0.055) / 1.055, vec3<f32>(2.4)), e / 12.92, e <= vec3<f32>(0.04045));
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let c = textureLoad(scene, vec2<i32>(pos.xy), 0).rgb;
    let e = pow(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(1.0 / 2.2));
    return vec4<f32>(select(e, srgb_to_linear(e), SRGB_TARGET > 0.5), 1.0);
}
