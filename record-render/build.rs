//! Refuses to build a helper without its compositor.
//!
//! The vendored `openscreen-compositor` compiles to an EMPTY library when no ffmpeg tree is
//! found (so the rest of the workspace stays buildable). A helper linked against that would
//! fail later with a wall of unresolved imports — or worse, if someone gated those away, ship
//! a binary that can do nothing. On macOS/Apple Silicon we therefore check the compositor's
//! own verdict (`DEP_OPENSCREEN_FFMPEG_ENABLED`, emitted by its build script only after
//! bindgen succeeded) and stop with a clear message.

fn main() {
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if os != "macos" || arch != "aarch64" {
        return;
    }
    if std::env::var("DEP_OPENSCREEN_FFMPEG_ENABLED").is_err() {
        panic!(
            "\n\nir-record-render needs the OpenScreen compositor, which was built WITHOUT ffmpeg.\n\
             Build LGPL ffmpeg 8.1.2 first:  scripts/build-ffmpeg-macos.sh\n\
             (or point MAC_FFMPEG_DIR at an ffmpeg tree with include/ and lib/).\n\n"
        );
    }
}
