// Spotlight: everything but a soft circle around the pointer is dimmed. It eases in and out
// over the first and last 0.3 s of the region, and lets go while the pointer is off the frame.
fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = frame(uv);
    let d = length((uv - ctx.cursor.xy) * vec2<f32>(ctx.aspect, 1.0));
    let outside = smoothstep(params.radius, params.radius + params.softness, d);
    // region: y = its length, z = seconds since its start.
    let ease = clamp(ctx.region.z / 0.3, 0.0, 1.0) * clamp((ctx.region.y - ctx.region.z) / 0.3, 0.0, 1.0);
    let k = outside * params.dim * ease * ctx.cursor.z;
    return vec4<f32>(c.rgb * (1.0 - k), c.a);
}
