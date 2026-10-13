// The 3D stage elements of stage.rs, ported from the DXBC of the game's base-pass pixel shaders
// (doc 11 §3.2–3.4; annotated disassembly with captured constants:
// runs/renderdoc/cap3/d12365/annot/3028_ps.asm guide lane, 3538_ps.asm hold body,
// d11184/annot/2367_ps.asm slide chevron; the SlideL / SnapIn / SnapOut variants from
// runs/shaders/vfx/<material>/19_ps_*.asm). Unlit: the emissive colour is the linear scene colour.
// Constant names are the material parameters; values are the captured ones (guest options).

struct View {
    // Viewport origin (px) and 1 / size (px), for screen-space UVs.
    viewport: vec4<f32>,
    // View.GameTime, MPC_BpmSignal pulse, PercentageAlpha, WidthGradation.
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var judge_line: texture_2d<f32>;   // vT_JudgeLine00_00_m (sRGB)
@group(0) @binding(2) var judge_color: texture_2d<f32>;  // vT_JudgeColor01_00_m (sRGB)
@group(0) @binding(3) var hold_tex: texture_2d<f32>;     // vT_HoldGrd00_00_m (sRGB)
@group(0) @binding(4) var slide_tex: texture_2d<f32>;    // vT_SlideNote00_00_m (sRGB)
@group(0) @binding(5) var slide_mask: texture_2d<f32>;   // vT_SlideMsk00_00_m (sRGB)
@group(0) @binding(6) var snap_tex: texture_2d<f32>;     // vT_SnapNote00_00_m (linear)
@group(0) @binding(7) var wrap_samp: sampler;
@group(0) @binding(8) var hold_samp: sampler;            // clamp u, mirror v

const TAU: f32 = 6.283185;
const CLIP: f32 = 0.3333;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) w: f32,
    @location(2) @interpolate(flat) material: u32,
};

// View-space `[X, Z, w]` (camera.rs): NDC = (X, Z) / w, reversed-Z depth near / w (near 10).
@vertex
fn vs_main(
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) material: u32,
) -> VsOut {
    var out: VsOut;
    out.pos = vec4<f32>(pos.x, pos.y, 10.0, pos.z);
    out.uv = uv;
    out.w = pos.z;
    out.material = material;
    return out;
}

fn smooth01(x: f32) -> f32 {
    let t = clamp(x, 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// --- vMX_2_GuideLane_000 (MI TUS_m_Lane00): lanes, separator lines and the judgement ring ---

const B_COLOR_5: vec3<f32> = vec3<f32>(0.012207, 0.013348, 0.03125);
const B_COLOR_2: vec3<f32> = vec3<f32>(0.044434, 0.0473883, 0.09375);
const L_LINE_POS: f32 = 0.0;
const R_LINE_POS: f32 = 0.51;
const LINE_L_OR_R: f32 = 0.5;
const LINE_ONOFF: f32 = 1.0;
const JUDGE_LINE_POS: f32 = 0.1473;
// NoteLength_0.5, picked by MPC NoteHeight 0.5.
const NOTE_LENGTH: f32 = 4600.0;
const OUTSIDE_GLOW: f32 = 0.12;
const SS_MIN: f32 = 0.6;
const SS_MAX: f32 = 1.0;
const INSIDE_MAX: f32 = 0.3;
const INSIDE_MIN: f32 = 0.25;
const GUIDE_LANE_ALPHA: f32 = 1.0;
const BOSS_ALPHA: f32 = 0.0;

@fragment
fn fs_lane(in: VsOut) -> @location(0) vec4<f32> {
    let u = in.uv.x;
    let v = in.uv.y;
    let time = view.params.x;
    // Faint scrolling noise (judge-line texture b) and the screen-space ring colour's alpha.
    let noise = textureSample(judge_line, wrap_samp, vec2<f32>(2.0 * u, 10.0 * v + 5.0 * time)).b;
    let screen = (in.pos.xy - view.viewport.xy) * view.viewport.zw;
    let ring = textureSample(judge_color, wrap_samp, screen);
    let n = clamp(pow(max(1.0 - v, 0.0), 9.0) * noise, 0.0, 0.003) + clamp(ring.a, 0.0, 0.0015);
    let base = B_COLOR_5 - n;
    // Lane separator lines: judge-line texture g, straight and rotated by 13.68°.
    let l0 = 5.0 * textureSample(judge_line, wrap_samp, vec2<f32>(u, -v) + vec2<f32>(L_LINE_POS, 0.0)).g;
    let p = vec2<f32>(u - 0.5, -v);
    let q = vec2<f32>(
        dot(p, vec2<f32>(0.971632, -0.236499)) + R_LINE_POS + 0.5,
        dot(p, vec2<f32>(0.236499, 0.971632)),
    );
    let l1 = 5.0 * textureSample(judge_line, wrap_samp, q).g;
    let line = mix(l0, l1, LINE_L_OR_R) * LINE_ONOFF;
    let col = mix(base, B_COLOR_2, line);
    // (Combo_ONOFF = 0: no guide-lane scroll texture.)
    // The judgement ring: a sharp peak of sin(2π(v/2 + JudgeLine_Pos)) with a pulsing glow.
    let k = min(pow(max(sin(TAU * (0.5 * v + JUDGE_LINE_POS)), 0.0), NOTE_LENGTH), 1.0);
    let k3 = 3.0 * k;
    let glow = smooth01((1.0 - k3 * OUTSIDE_GLOW - SS_MIN) / (SS_MAX - SS_MIN))
        + (sin(TAU * view.params.y) + 0.5) * (INSIDE_MAX - INSIDE_MIN) + INSIDE_MIN;
    let edge = k3 * OUTSIDE_GLOW + floor(k3);
    let r = clamp(glow * clamp(edge, 0.0, 1.0), 0.0, 1.0);
    let rgb = mix(col, 1.5 * ring.rgb, r);
    // Opacity: fades out with view depth ((w / PercentageAlpha)^WidthGradation), ring opaque.
    let fade = min(pow(max(in.w / view.params.z, 0.0), view.params.w), 1.0);
    let a0 = GUIDE_LANE_ALPHA + fade * (BOSS_ALPHA * (GUIDE_LANE_ALPHA - 0.3) - GUIDE_LANE_ALPHA);
    let q2 = edge * ((cos(TAU * max(fract(k), 0.5)) + 1.0) * 0.5 + ceil(k));
    let alpha = clamp(q2 * (5.0 - a0) + a0, 0.0, 1.0);
    return vec4<f32>(max(rgb, vec3<f32>(0.0)), alpha);
}

// --- vMX_2_Hold_001: hold ribbon ---

// MPC vMPC_NoteCustomize_00.HoldColorNumber; U_Tiling.
const HOLD_COLOR_NUMBER: f32 = 2.0;
const U_TILING: f32 = 13.0;
const HOLD_OPACITY: f32 = 0.6;

fn hold_tile(uv: vec2<f32>, n: f32) -> vec3<f32> {
    let tile = vec2<f32>(n - U_TILING * trunc(n / U_TILING), floor(n / U_TILING));
    return textureSample(hold_tex, hold_samp, (uv + tile) * vec2<f32>(1.0 / U_TILING, 1.0)).rgb;
}

// vMX_2_Hold_004 (held; runs/shaders/vfx/vMX_2_Hold_004/08_ps_*.asm): Alpha_RangePos, Mask_Pos and
// NoteLength_0.5 (picked by MPC NoteHeight 0.5).
const HELD_ALPHA_RANGE: f32 = 175.0;
const HELD_MASK_POS: f32 = 0.886;
const HELD_NOTE_LENGTH: f32 = 222.5;

@fragment
fn fs_hold(in: VsOut) -> @location(0) vec4<f32> {
    let f = fract(HOLD_COLOR_NUMBER);
    let n = HOLD_COLOR_NUMBER - f;
    // CustomRotator by 180° about (0.45, 0.5).
    let uv = vec2<f32>(0.95 - in.uv.x, 1.0 - in.uv.y);
    let rgb = mix(hold_tile(uv, n), hold_tile(uv, n + 1.0), f);
    if in.material == 0u {
        return vec4<f32>(max(rgb, vec3<f32>(0.0)), HOLD_OPACITY);
    }
    // Held: the tile screened with itself, ×1.3·(1.2 − 0.2·(w/430)²); opacity 0.3..0.6 by
    // smoothstep(w/175), cut off below w = 0.886·222.5 (just before the line).
    let s = 1.0 - (1.0 - rgb) * (1.0 - rgb);
    let fw = min(pow(max(in.w * 0.002326, 0.0), 2.0), 1.0);
    let col = s * (1.2 - 0.2 * fw) * 1.3;
    let a = 0.3 + 0.3 * min(smooth01(in.w / HELD_ALPHA_RANGE), 1.0);
    let cut = min(ceil(smooth01((in.w / HELD_NOTE_LENGTH - HELD_MASK_POS) / 0.1)), 1.0);
    return vec4<f32>(max(col, vec3<f32>(0.0)), a * cut);
}

// --- vMX_2_SlideL/R_002 chevrons, vMX_2_SnapIn/Out_001 arrows (masked) ---

const SLIDE_L_COLOR: vec3<f32> = vec3<f32>(1.0, 0.32314324, 0.0);
const SLIDE_R_COLOR: vec3<f32> = vec3<f32>(0.013702083, 0.6514058, 0.008568125);
const SNAP_IN_COLOR: vec3<f32> = vec3<f32>(0.730461, 0.005181516, 0.005181516);
const SNAP_OUT_COLOR: vec3<f32> = vec3<f32>(0.00913406, 0.41788515, 1.0);
// UV_Scale.y + 0.55 as captured on a 12-lane slide (frame 11184); per lane for other widths.
// UNVERIFIED: the game sets UV_Scale at run time; taken as proportional to the width.
const SLIDE_V_SCALE_PER_LANE: f32 = 3.395 / 12.0;

fn screen_blend(c: vec3<f32>, nc: vec3<f32>) -> vec3<f32> {
    return 1.0 - (1.0 - c) * (1.0 - nc);
}

@fragment
fn fs_chevron(in: VsOut) -> @location(0) vec4<f32> {
    let kind = in.material & 0xffu;
    let width = f32(in.material >> 8u);
    let u = in.uv.x;
    let v = in.uv.y;
    // Both material families are evaluated (texture samples need uniform control flow).
    // Slides: SlideL (kind 0) mirrors the gradient, the scroll and the mask along v.
    let left = kind == 0u;
    let snc = select(SLIDE_R_COLOR, SLIDE_L_COLOR, left);
    let sg = select(v, 1.0 - v, left);
    let st = sg * sg;
    let sc1 = mix(0.25 * snc * snc, 0.5 * snc, st);
    let sc2 = 1.0 - (1.0 - st) * (1.0 - 1.5 * snc);
    // The pattern scrolls with view depth: chevrons are fixed on the note.
    let vs = v * SLIDE_V_SCALE_PER_LANE * width * select(1.7, -1.7, left);
    let pat = textureSample(slide_tex, wrap_samp, vec2<f32>(u, vs + in.w * 0.007246));
    let m = textureSample(slide_mask, wrap_samp, vec2<f32>(u, select(-v, v, left)));
    let scol = sc1 + pat.r * m.r * (sc2 - sc1);
    let slide = 2.0 * (scol + 0.1) * screen_blend(scol, snc);
    // Snaps: SnapOut (kind 3) mirrors u and squares the bright colour.
    let into = kind == 2u;
    let nnc = select(SNAP_OUT_COLOR, SNAP_IN_COLOR, into);
    let tex = textureSample(snap_tex, wrap_samp, vec2<f32>(select(-u, u, into), v));
    let ng = select(u, 1.0 - u, into);
    let nt = ng * ng;
    let nc1 = mix(0.25 * nnc * nnc, 0.5 * nnc, nt) * tex.g;
    let nc2 = 1.0 - (1.0 - nt) * (1.0 - 1.5 * nnc);
    let ncol = mix(nc1 * nc1, select(nc2 * nc2, nc2, into), tex.r);
    let snap = 2.0 * (ncol + 0.2) * screen_blend(ncol, nnc);
    let is_slide = kind < 2u;
    let mask = select(tex.b, pat.b * m.b, is_slide);
    if mask < CLIP {
        discard;
    }
    return vec4<f32>(max(select(snap, slide, is_slide), vec3<f32>(0.0)), 1.0);
}
