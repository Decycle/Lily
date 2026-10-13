// `uM_CMcircle001`: the mode select's art ring, ported from its compiled pixel shader
// (runs/shader-research/shaders/ui/uM_CMcircle001, base pass PS 025). The art (t0, `TEX` =
// uT_RGclear03) turns at RotateSpeed turns/s and is swirled about a point 0.2 off centre that
// circles at -0.2 rad/s: swirl angle = 2 pi * saturate((1 - dist / Radius_1) / (1 - Hardness)).
// Opacity = alpha of t1 (`A` = uT_TTIogo04, a ring), or a full disc when p[1].x > 0.5.
// p[0] = (time, RotateSpeed, 1 / Radius_1, 1 / (1 - Hardness)).

fn turn(v: vec2<f32>, a: f32) -> vec2<f32> {
    let s = sin(a);
    let c = cos(a);
    return vec2<f32>(c * v.x - s * v.y, s * v.x + c * v.y);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let t = d.p[0].x;
    let c = in.uv - 0.5;
    let spun = turn(c, t * d.p[0].y * TAU);
    let q = turn(c, -0.2 * t) + vec2<f32>(0.2, 0.0);
    let amount = saturate((1.0 - length(q) * d.p[0].z) * d.p[0].w);
    let uv = turn(spun, amount * TAU) + 0.5;
    let art = decode(textureSampleLevel(t0, wrap, uv, 0.0).rgb);
    var a = saturate(textureSampleLevel(t1, wrap, in.uv, 0.0).a);
    if d.p[1].x > 0.5 {
        a = select(0.0, 1.0, length(c) <= 0.5);
    }
    return finish(art, a);
}
