//! `ir-record` — talks to Inspector Rust's control socket (see
//! `core/rust-lib/src/control.rs`). The app owns every project file; this
//! crate only sends requests.
//!
//! * [`client`] — discovery (`control.json`), connect + ping, launching the
//!   app, request/stream, Ctrl+C.
//! * [`cli`] — argument parsing, commands, human / NDJSON output, exit codes.

pub mod cli;
pub mod client;
#[cfg(all(test, unix))]
mod test_support;
