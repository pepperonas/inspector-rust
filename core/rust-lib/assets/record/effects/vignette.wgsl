// Vignette: the edges of the frame fade towards a colour (black by default), the centre stays
// as recorded. Distances are measured in a circle around the centre (aspect-corrected), so the
// falloff looks the same at every output size and shape.
fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = frame(uv);
    let scale = vec2<f32>(ctx.aspect, 1.0);
    // 0 at the centre, 1 in a corner.
    let d = length((uv - vec2<f32>(0.5)) * scale) / length(scale * 0.5);
    let edge = smoothstep(params.radius - params.softness, params.radius + params.softness, d);
    let k = edge * params.strength * params.color.a;
    return vec4<f32>(mix(c.rgb, params.color.rgb, k), c.a);
}
