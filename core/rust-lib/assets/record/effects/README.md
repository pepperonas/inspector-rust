# Built-in shader effects

Our own WGSL effects for the record editor (upstream ships none). Each file defines
`fn effect(uv: vec2<f32>) -> vec4<f32>` against OpenScreen's contract v1
(`vendor/openscreen-compositor/src/shader_fx.rs`). Parameters are declared once, in
`params.json` (read by core's registry, the record-render compile test and the UI).

- `vignette` — darkens the frame's edges towards a colour. Params: `strength`, `radius`, `softness`, `color`.
- `grain` — fine film grain, strongest in the mid-tones. Params: `amount`, `size` (1080p px), `animated`.
- `chromatic` — red/blue split towards the edges, sharp centre. Params: `strength`, `center_clear`.
- `ripple` — a soft ring bends the picture from each click (pointer data). Params: `strength`, `radius`, `duration` (s), `shine`.
- `spotlight` — dims everything but a circle around the pointer (pointer data). Params: `dim`, `radius`, `softness`.
- `scanlines` — faint horizontal monitor lines, optional rolling band. Params: `intensity`, `spacing` (1080p px), `roll`.
