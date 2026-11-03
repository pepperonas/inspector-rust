# MediaPipe Selfie Segmentation (landscape) — ONNX model

`selfie_segmentation_landscape.onnx` is the person-segmentation model the record editor's
camera background effects (blur, cut-out, custom background) run on. It is loaded by the
vendored compositor (`vendor/openscreen-compositor/src/segmentation.rs`, patch 16) inside the
`ir-record-render` helper, on the workspace's own statically linked ONNX Runtime (`ort`).

## Provenance

- Upstream model: MediaPipe Selfie Segmentation, `selfie_segmentation_landscape.tflite`
  (249 792 B) — <https://google.github.io/mediapipe/solutions/selfie_segmentation>.
  Copyright The MediaPipe Authors.
- The `.onnx` is a **derived work**: OpenScreen v2.1.2 (commit `bfd5bc32`) converted the
  `.tflite` with its `scripts/convert-selfie-segmentation-to-onnx.py` (tf2onnx plus a repair of
  11 `HardSwish` operators and MediaPipe's custom `TFL_Convolution2DTransposeBias`). Copied
  unchanged from OpenScreen's `public/mediapipe/selfie_segmentation/`; nothing was converted,
  downloaded or executed here.
- File: 457 447 bytes, sha256
  `daa5d38991b6a013367a9be88cb03229bb01abeee5129707a70a3987511c9e94`.
  The stage-5 recon recorded only `daa5d389…9e94`; the full value was computed from the copy
  (both ends match the recon). `scripts/bundle-record-render.sh` refuses to bundle a file with
  another size or hash.

| | |
|---|---|
| input | `input_1` `[1, 144, 256, 3]` float32, NHWC, RGB scaled to 0..1 |
| output | `segment_back` `[1, 144, 256, 1]` float32, already sigmoid-activated (1 = person) |

## License

Apache License 2.0 — full text in [`LICENSE`](LICENSE). The app bundle ships this README and
the licence under `Contents/Resources/THIRD-PARTY/selfie-segmentation/` next to the model in
`Contents/Resources/`.
