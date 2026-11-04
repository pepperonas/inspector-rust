// Click ripple: every click sends out one soft ring that bends the picture as it passes and
// fades within `duration` seconds. Up to four recent clicks ripple at once (`ctx.clicks`).
fn ripple_at(uv: vec2<f32>, click: vec4<f32>) -> vec3<f32> {
    // w: the slot is used; z: seconds since the click.
    if (click.w < 0.5 || click.z >= params.duration) {
        return vec3<f32>(0.0);
    }
    let t = click.z / params.duration;
    let scale = vec2<f32>(ctx.aspect, 1.0);
    let d = (uv - click.xy) * scale;
    let r = length(d);
    let front = params.radius * (1.0 - (1.0 - t) * (1.0 - t));
    let x = (r - front) / fx_px(26.0);
    let fade = (1.0 - t) * (1.0 - t);
    let ring = exp(-x * x) * fade;
    let push = sin(x * 3.14159265) * ring * params.strength * fx_px(9.0);
    let n = select(vec2<f32>(0.0), d / r, r > 1e-5);
    return vec3<f32>(n * push / scale, ring);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    var acc = vec3<f32>(0.0);
    for (var i = 0; i < 4; i++) {
        acc += ripple_at(uv, ctx.clicks[i]);
    }
    let c = frame(uv - acc.xy);
    let lit = c.rgb + vec3<f32>(min(acc.z, 1.0) * params.shine * 0.12);
    return vec4<f32>(min(lit, vec3<f32>(1.0)), c.a);
}
