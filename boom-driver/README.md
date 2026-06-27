# boom Audio — virtual audio driver

The `AudioServerPlugIn` that makes system-wide `boom` EQ possible (the only viable
path — see `../docs/boom-driver-plan.md`). It's a loopback output device: apps
play to **boom Audio**, the app reads it back, runs the (tested) `boom::DspChain`,
and outputs to the real device. No process tap, no muting.

## Source

`BlackHole.c` is **BlackHole** (© Existential Audio, MIT — see
`LICENSE-BlackHole.txt`), vendored **verbatim**. It's rebranded to "boom Audio"
**only via `-D` compile defines** in `build.sh` (own device UID `BoomAudio_UID`,
bundle id `io.celox.boom.driver`, and a fresh CFPlugIn factory UUID in
`Info.plist`) so it never collides with a separately-installed BlackHole.

## Build & install

```bash
bash boom-driver/build.sh                 # → build/boom-driver.driver (universal, ad-hoc signed)
bash scripts/boom-driver-install.sh       # copy to /Library/Audio/Plug-Ins/HAL + restart coreaudiod (admin)
bash scripts/boom-driver-install.sh --remove   # uninstall
```

After install, **boom Audio** appears in System Settings → Sound → Output.

## Status / phasing

- **B1 (done):** builds into a valid signed bundle.
- **B2:** install → confirm "boom Audio" shows up as an audio device.
- **B3:** app routing (default-output switch + capture-from-boom-Audio + DspChain
  + output-to-real-device + restore).
- **B4:** first-run install UX, uninstall, robustness, **Developer-ID signing +
  notarization for distribution** (ad-hoc only loads locally).
