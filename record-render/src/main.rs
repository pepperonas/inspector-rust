//! `ir-record-render` — Inspector Rust's render helper for the `record` editor.
//!
//! Owns the vendored OpenScreen compositor (Metal + VideoToolbox through LGPL ffmpeg) in its
//! own process, so a crash in the GPU/codec stack can never take the app down. Speaks the
//! `record-render-protocol` frames on stdin/stdout; everything the compositor logs goes to
//! stderr (the app logs that line by line).
//!
//! Only macOS on Apple Silicon has the compositor. Everywhere else this binary reports
//! "unsupported" (as a `Fatal` event and on stderr) and exits 1.

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod session;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() {
    std::process::exit(session::run());
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() {
    let msg = "ir-record-render: unsupported platform (needs macOS on Apple Silicon)";
    eprintln!("{msg}");
    let _ = record_render_protocol::write_event(
        &mut std::io::stdout(),
        &record_render_protocol::Event::Fatal { msg: msg.to_string() },
    );
    std::process::exit(1);
}
