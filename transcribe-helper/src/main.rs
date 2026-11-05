//! `ir-transcribe`: the live transcription helper of Inspector Rust.
//! Protocol: `transcribe-protocol`, one JSON object per line.
//!
//! The protocol runs on a duplicate of the original stdout; fd 1 itself is
//! pointed at stderr, so nothing whisper.cpp or a library prints can corrupt
//! a protocol line.

mod serve;

#[cfg(unix)]
fn protocol_out() -> std::fs::File {
    use std::os::fd::FromRawFd;
    // SAFETY: plain fd juggling on our own process before any other thread runs.
    unsafe {
        let fd = libc::dup(1);
        libc::dup2(2, 1);
        std::fs::File::from_raw_fd(fd)
    }
}

fn main() {
    #[cfg(unix)]
    let out = protocol_out();
    #[cfg(not(unix))]
    let out = std::io::stdout();
    let (emit, writer) = serve::event_writer(out);
    serve::serve(std::io::BufReader::new(std::io::stdin()), emit.clone());
    drop(emit);
    let _ = writer.join();
}
