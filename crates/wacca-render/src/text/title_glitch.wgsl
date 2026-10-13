// `uM_Title012`: the title key art's glitching layer, ported from its compiled pixel shader
// (runs/shader-research/shaders/ui/uM_Title012, base pass PS 017). Three glitch functions
// (`uMF_HorizontalGlitch` x2, `uMF_VerticalGlitch`) displace the texture coordinates of three
// lookups of the art; each lookup feeds one channel, recoloured:
// emissive = color_1 * B(uv_a) + color_2 * R(uv_b) + color_4 * G(uv_c) + add, opacity = A(uv_a).
// t0 = uT_TTlogo07 (wavy distortion), t1 = uT_CM06 (horizontal glitch mask), t2 = the art
// (clamped). The vertical glitch's mask (uT_GlitchMaskV) is bypassed by Ran_3 = 1 and not bound.
// p[0] = (time, Span_1, Duration_1 clamped to the span, move_5)
// p[1] = (Speed_1, 1 / widht_1, offset_1, Ran_1)
// p[2] = (Span_2, Duration_2 clamped, Speed_2, 1 / widht_2)
// p[3] = (offset_2, Ran_2, Span_3, Duration_3 clamped)
// p[4] = (cos, sin of Rotator_3 turns, 1 / widht_3, Speed_3)
// p[5] = (offset_3, Ran_3, -, -)
// p[6..9] = color_1 (B), color_2 (R), color_4 (G), constant added to the emissive.

// The noise while the glitch's window is open: the first `dur` s of every `span` s. A zero
// span divides by zero in the original (NaN compares false), i.e. always open.
fn gate(noise: f32, t: f32, span: f32, dur: f32) -> f32 {
    if span > 0.0 && fract(t / span) * span >= dur {
        return 0.0;
    }
    return noise;
}

fn on(n: f32) -> bool {
    return abs(n) > 1e-5 && n >= 0.0;
}

fn picked(flag: f32) -> bool {
    return abs(flag) > 1e-5 && flag >= 0.0;
}

// round(cos(2 pi cos(x))): -1, 0 or 1 in bands.
fn band_c(x: f32) -> f32 {
    return round(cos(TAU * cos(x)));
}

fn band_s(x: f32) -> f32 {
    return round(sin(TAU * sin(x)));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let t = d.p[0].x;
    let noise = turbulence(t);
    // Wavy distortion of the band coordinate and the mask lookup.
    let wa = textureSampleLevel(t0, wrap, uv * vec2<f32>(0.2, 2.0) + t * vec2<f32>(0.5, 0.1), 0.0).a;
    let wb = textureSampleLevel(t0, wrap, uv * vec2<f32>(0.2, 2.0) + t * vec2<f32>(-0.8, 0.1), 0.0).r;
    let uvd = (1.0 - vec2<f32>(wa, wb)) * d.p[0].w + uv;
    let mask = textureSampleLevel(t1, wrap, uvd, 0.0).r;

    // Horizontal glitch 1 -> B and opacity.
    let n1 = gate(noise, t, d.p[0].y, d.p[0].z);
    let s1 = band_c((d.p[1].x * n1 + uvd.y) * d.p[1].y * TAU) + band_c(uvd.y * d.p[1].y * TAU);
    let g1 = select(uv, uv + vec2<f32>(d.p[1].z * s1, 0.0), on(n1));
    let uv_a = select(mix(uv, g1, mask), g1, picked(d.p[1].w));
    let ba = textureSampleLevel(t2, clamped, uv_a, 0.0);

    // Horizontal glitch 2 -> R.
    let n2 = gate(noise, t, d.p[2].x, d.p[2].y);
    let s2 = band_c((d.p[2].z * n2 + uvd.y) * d.p[2].w * TAU) + band_c(uvd.y * d.p[2].w * TAU);
    let g2 = select(uv, uv + vec2<f32>(d.p[3].x * s2, 0.0), on(n2));
    let uv_b = select(mix(uv, g2, mask), g2, picked(d.p[3].y));
    let r = textureSampleLevel(t2, clamped, uv_b, 0.0).r;

    // Vertical glitch -> G (rotated band coordinate; the original swaps the axes back this way).
    let n3 = gate(noise, t, d.p[3].z, d.p[3].w);
    let c = uv - 0.5;
    let rot = vec2<f32>(dot(c, vec2<f32>(d.p[4].x, -d.p[4].y)), dot(c, d.p[4].yx)) + 0.5;
    let s3 = band_s(rot.x * d.p[4].z * TAU) + band_s((d.p[4].w * n3 + rot.x) * d.p[4].z * TAU);
    let swapped = vec2<f32>(s3 + uv.y, uv.x);
    let g3 = select(uv, d.p[5].x * (swapped - uv) + uv, on(n3));
    // Ran_3 = 1 in the material: the masked mix with uT_GlitchMaskV is never taken.
    let g = textureSampleLevel(t2, clamped, g3, 0.0).g;

    let rgb = decode(vec3<f32>(r, g, ba.b));
    let emissive = d.p[6].rgb * rgb.b + d.p[7].rgb * rgb.r + d.p[8].rgb * rgb.g + d.p[9].rgb;
    return finish(emissive, ba.a);
}
