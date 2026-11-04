// Chromatic aberration: red and blue drift apart towards the edges of the frame, like a cheap
// lens. The centre (`center_clear`) stays sharp; the offset is at most 8 px of a 1080p output.
fn effect(uv: vec2<f32>) -> vec4<f32> {
    let scale = vec2<f32>(ctx.aspect, 1.0);
    let p = (uv - vec2<f32>(0.5)) * scale;
    let r = length(p) / length(scale * 0.5);
    let reach = smoothstep(params.center_clear * 0.8, 1.0, r);
    let dir = select(vec2<f32>(0.0), p / length(p), length(p) > 1e-5);
    let off = dir * fx_px(8.0) * params.strength * reach / scale;
    let c = frame(uv);
    let red = frame(uv + off).r;
    let blue = frame(uv - off).b;
    return vec4<f32>(red, c.g, blue, c.a);
}
