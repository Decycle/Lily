// Flat-coloured triangles in playfield space (rim radius 1), drawn into a square viewport.

// Playfield radius mapped to the viewport edge (style::VIEW_EXTENT).
override VIEW_EXTENT: f32 = 1.0;
// 1.0 when the target format is sRGB: vertex colours are sRGB and must be linearised.
override LINEARIZE: f32 = 0.0;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec2<f32>, @location(1) color: vec4<f32>) -> VsOut {
    var out: VsOut;
    out.pos = vec4<f32>(pos / VIEW_EXTENT, 0.0, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let rgb = select(in.color.rgb, pow(in.color.rgb, vec3<f32>(2.2)), LINEARIZE > 0.5);
    return vec4<f32>(rgb, in.color.a);
}
