//! `ir-ocr`: a long-lived helper that recognises text in PNG images.
//!
//! Inspector Rust starts it once and sends one frame per image on stdin; the
//! answer comes back as one frame on stdout (see `ir_ocr_helper::frame`).
//! End of stdin ends the process. Why a separate process: see the crate docs.

fn main() {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    ir_ocr_helper::serve(&mut input, &mut output);
}
