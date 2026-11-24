//! Development tool for `md2pdf`: prints on the main thread without the app.
//! `cargo run -p inspector-rust-core --example md2pdf -- <in.md> <out.pdf> [--smart]`
fn main() {
    #[cfg(target_os = "macos")]
    std::process::exit(inspector_rust_core::md2pdf_dev_main(std::env::args().skip(1).collect()));
}
