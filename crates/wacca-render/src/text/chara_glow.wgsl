// `uM_Chara_2`: the smoky glow behind the title characters (additive), ported from its
// compiled pixel shader (runs/shader-research/shaders/ui/uM_Chara_2, base pass PS 017).
// t0 = uT_TTlogo07 (its green channel is a soft blob), t1 = vT_FogNoise00_00_m scrolling up
// at 0.1 uv/s. emissive = max(blob - fog, 0) per channel, opacity = saturate(blob - fog.r);
// the widget's ColorAndOpacity is the tint. p[0].x = time.

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let t = d.p[0].x;
    let blob = textureSampleLevel(t0, wrap, in.uv, 0.0).g;
    let fog = textureSampleLevel(t1, wrap, in.uv + vec2<f32>(0.0, 0.1 * t), 0.0).rgb;
    let e = max(vec3<f32>(blob) - fog, vec3<f32>(0.0));
    return finish(e, saturate(blob - fog.r));
}
