//! `ir-record` — talks to Inspector Rust's control socket (see
//! `core/rust-lib/src/control.rs`). The app owns every project file; this
//! crate only sends requests.
//!
//! * [`client`] — discovery (`control.json`), connect + ping, launching the
//!   app, request/stream, Ctrl+C.
//! * [`cli`] — argument parsing, commands, human / NDJSON output, exit codes.
//! * [`mcp`] — `ir-record mcp`, the MCP stdio server; its tools in [`tools`].

pub mod cli;
pub mod client;
pub mod mcp;
pub mod tools;
#[cfg(all(test, unix))]
mod test_support;
