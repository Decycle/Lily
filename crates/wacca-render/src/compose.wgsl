// The game's main-view tonemap (doc 11 §2.6, frame 12365 eid 4483): the permutation that draws
// the play view only reads scene colour, `out = pow(scene · exposure, 1/2.2)` with exposure 1,
// into a UNORM target (clamped). No film curve, LUT, bloom or vignette. Drawn over the
// playfield viewport, reading the resolved stage target pixel for pixel.
//
// Then the HUD rim (UMG, blended in display values like the game's UI): the time gauge darkens
// the part of the song still to play and everything beyond the round screen is black
// (style::HUD_RAISE_PX .. GAUGE_SHADE; lengths here are fractions of the view's width).

// 1.0 when the target is sRGB: it re-encodes what we write, so write the value whose sRGB
// encoding is the game's output.
override SRGB_TARGET: f32 = 0.0;
override HUD_RAISE: f32 = 0.0;
override HUD_EDGE: f32 = 0.49;
override GAUGE_RADIUS: f32 = 0.5;
override GAUGE_SHADE: f32 = 0.45;

struct View {
    // Viewport origin (px) and 1 / size (px).
    viewport: vec4<f32>,
    params: vec4<f32>,
    // x: played fraction (< 0: no time gauge).
    hud: vec4<f32>,
};

@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var<uniform> view: View;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32(i & 1u), f32(i >> 1u));
    return vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
}

fn srgb_to_linear(e: vec3<f32>) -> vec3<f32> {
    return select(pow((e + 0.055) / 1.055, vec3<f32>(2.4)), e / 12.92, e <= vec3<f32>(0.04045));
}

const TAU: f32 = 6.283185;

// Display-value factor of the HUD rim at framebuffer pixel `pos`.
fn hud_rim(pos: vec2<f32>) -> f32 {
    let w = 1.0 / view.viewport.z;
    let h = 1.0 / view.viewport.w;
    let centre = view.viewport.xy + vec2<f32>(0.5 * w, 0.5 * h - HUD_RAISE * w);
    // x right, y up, in view widths.
    let p = vec2<f32>(pos.x - centre.x, centre.y - pos.y) / w;
    let r = length(p);
    // Black beyond the round screen, 1 px soft edge.
    let px = r * 675.0;
    var k = 1.0 - clamp(px - (HUD_EDGE * 675.0 - 0.5), 0.0, 1.0);
    // uM_RGguage000's band on the part still to play (from 12 o'clock counter-clockwise).
    let played = view.hud.x;
    if played >= 0.0 {
        let ccw = fract((atan2(p.y, p.x) / TAU) - 0.25 + 1.0);
        let band = clamp(10.0 * (1.0 - 100.0 * abs(r / GAUGE_RADIUS - 0.96)), 0.0, 1.0);
        if ccw >= played {
            k = k * (1.0 - GAUGE_SHADE * band);
        }
    }
    return k;
}

// The game's view runs TAA; a still converged TAA image is the scene seen through a ~0.55 px
// Gaussian (fitted on RenderDoc frame 12365: edge pixels' mean |diff| 20.4 → 15.8 levels,
// minimum at σ 0.45–0.6). Taps: centre, 4 edges, 4 corners of σ = 0.55.
const TAA_W: vec3<f32> = vec3<f32>(0.522, 0.100, 0.0193);

fn scene_at(p: vec2<i32>) -> vec3<f32> {
    let size = vec2<i32>(textureDimensions(scene));
    return textureLoad(scene, clamp(p, vec2<i32>(0), size - 1), 0).rgb;
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let q = vec2<i32>(pos.xy);
    var c = scene_at(q) * TAA_W.x;
    c += (scene_at(q + vec2<i32>(1, 0)) + scene_at(q - vec2<i32>(1, 0)) + scene_at(q + vec2<i32>(0, 1))
        + scene_at(q - vec2<i32>(0, 1))) * TAA_W.y;
    c += (scene_at(q + vec2<i32>(1, 1)) + scene_at(q - vec2<i32>(1, 1)) + scene_at(q + vec2<i32>(1, -1))
        + scene_at(q + vec2<i32>(-1, 1))) * TAA_W.z;
    let e = pow(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(1.0 / 2.2)) * hud_rim(pos.xy);
    return vec4<f32>(select(e, srgb_to_linear(e), SRGB_TARGET > 0.5), 1.0);
}
