// Film grain: a fine, even noise over the picture, strongest in the mid-tones (deep blacks and
// highlights stay clean, like real film). Grain cells are sized in 1080p pixels (`fx_px`), so the
// texture is the same at every output resolution; `animated` re-rolls it 24 times a second.
fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = frame(uv);
    let cell = vec2<u32>(uv * vec2<f32>(ctx.aspect, 1.0) / fx_px(params.size));
    let tick = u32(floor(max(ctx.time, 0.0) * 24.0) * params.animated);
    let n = fx_hash(cell + vec2<u32>(tick * 7919u, tick * 104729u)) - 0.5;
    let luma = dot(c.rgb, vec3<f32>(0.299, 0.587, 0.114));
    let mid = 1.0 - abs(luma * 2.0 - 1.0) * 0.6;
    let rgb = c.rgb + vec3<f32>(n * params.amount * 0.3 * mid);
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), c.a);
}
