// Scanlines: faint horizontal lines like an old monitor, `spacing` pixels apart at 1080p. With
// `roll` a slightly brighter band drifts slowly down the picture.
fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = frame(uv);
    let phase = uv.y / fx_px(params.spacing);
    let line = 0.5 + 0.5 * cos(phase * 6.28318531);
    var rgb = c.rgb * (1.0 - params.intensity * (1.0 - line));
    let band_y = fract(ctx.time * 0.12);
    let dy = abs(uv.y - band_y);
    let band = exp(-(dy * dy) / 0.0025) * params.roll;
    rgb = rgb + vec3<f32>(band * params.intensity * 0.25);
    return vec4<f32>(min(rgb, vec3<f32>(1.0)), c.a);
}
