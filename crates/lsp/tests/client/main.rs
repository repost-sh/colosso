//! The client against servers: a scripted fake (`src/bin/fake-lsp.rs`) for every message
//! and failure the protocol allows, and a real rust-analyzer for what an editor asks.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test fails loudly at the step that broke"
)]

mod common;
mod lifecycle;
mod messages;
mod rust_analyzer;
mod server_requests;
mod status;
