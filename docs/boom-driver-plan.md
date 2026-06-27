# boom — virtual-driver system EQ (plan)

## Why a driver

Research conclusion (see session notes + sources below): **driverless system-wide
EQ via Core-Audio process taps is impossible.** Taps are a *capture* API; muting
the source to avoid doubling silences the shared hardware output regardless of
routing (verified across unmuted / muted / mutedWhenTapped / aggregate-as-default).
**Every real system EQ — eqMac, Boom 3D, SoundSource — uses a virtual audio
driver.** So does this.

Architecture (the eqMac model):

```
apps ──▶ "boom Audio" (virtual output device, user-space AudioServerPlugIn)
                       │  loopback: output stream → input stream
         app reads ◀───┘
         app: DspChain (already built + unit-tested)
         app ──▶ real output device (speakers / headphones)
```

- Apps play to our virtual device (set as the system default output).
- The virtual device is a **loopback**: what's written to its output appears on
  its input — so the app captures the system audio *without muting anything*.
- The app processes it (the existing `boom::DspChain`) and plays to the real
  device. No tap, no mute, no doubling.

## Components

1. **The virtual driver** (`boom-driver/`, C, AudioServerPlugIn).
   - Base: adapt **BlackHole** (MIT) — a single-file, proven, 2-channel loopback
     `AudioServerPlugIn`. Rename device/bundle to "boom Audio"
     (`io.celox.boom.driver`), 2ch (or 2/16ch), 44.1+48 kHz.
   - Build → a `boom-driver.driver` bundle (clang, links CoreAudio +
     CoreFoundation; Info.plist; `_Info.plist` resource).
   - **Must be code-signed** (and notarized for distribution outside our own Mac).
2. **Install/uninstall** (`scripts/boom-driver-install.sh`).
   - Copy the bundle to `/Library/Audio/Plug-Ins/HAL/` (**needs admin** — one
     `osascript -e 'do shell script … with administrator privileges'`).
   - `sudo killall coreaudiod` to load it (audio blips for ~1 s).
   - Uninstall = remove the bundle + restart coreaudiod.
3. **App routing** (`boom/macos.rs`, reuse most of the existing engine).
   - On enable: find the "boom Audio" device; save the current default output;
     set "boom Audio" as default; open an IOProc that reads boom-Audio's input
     (the loopback of all app audio) → `DspChain` → writes to the **real** saved
     device (a normal output IOProc on the real device — not muted, since nothing
     is tapped/muted now).
   - On disable/quit: restore the previous default output; stop the IOProc.
   - Needs a small lock-free ring buffer if capture + playback are separate
     IOProcs (boom-Audio input clock vs real-device output clock).
4. **First-run UX** (`BoomPanel`): "boom needs its audio driver — Install"
   button → runs the installer (admin prompt) → detects the device → enables.
   Clear uninstall in Settings.

## Distribution reality (important)

- The driver is a **real system component**: it must be **code-signed** with a
  Developer ID and **notarized** to run on other Macs without Gatekeeper blocks.
  For a personal/GitHub build on this Mac, an ad-hoc/self-signed driver + a
  one-time `spctl`/right-click-open may suffice, but **distribution needs a paid
  Developer ID + notarization**.
- Install requires **admin once** + a `coreaudiod` restart (brief audio glitch).
- This is **no longer "out of the box"** — it's an installed driver, exactly like
  eqMac/BlackHole.

## Honest note

**eqMac is free + open-source and already does exactly this.** Building our own
driver duplicates a large, signing/notarization-heavy, specialized project. Worth
it only if boom-integrated EQ is specifically wanted over pointing users at eqMac.

## Phasing (each step verified before the next)

- **B1** — vendor + adapt BlackHole → `boom-driver/` (renamed, 2ch); get it to
  **compile** into a `.driver` bundle locally. *(verifiable: it builds)*
- **B2** — install script (admin) + `coreaudiod` restart; confirm "boom Audio"
  appears in System Settings → Sound + `boom_presets`/`list_audio_outputs`.
  *(you verify on hardware)*
- **B3** — app routing: default-output switch + capture-from-boom-Audio + DspChain
  + output-to-real-device + restore. *(you verify: EQ audibly works)*
- **B4** — first-run install UX in `BoomPanel`, uninstall in Settings, robustness
  (device-change, SR-change), signing/notarization for distribution.

## Sources

- eqMac (driver model): https://github.com/bitgapp/eqMac
- BlackHole (MIT loopback driver to adapt): https://github.com/ExistentialAudio/BlackHole
- Apple `NullAudio` AudioServerPlugIn sample (canonical reference)
- AudioCap (tap capture, why taps are capture-only): https://github.com/insidegui/AudioCap
