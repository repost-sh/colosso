//! The client against servers: a scripted fake (`src/bin/fake-lsp.rs`) for every message
//! and failure the protocol allows, and a real rust-analyzer for what an editor asks.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a test fails loudly at the step that broke"
)]
#![expect(
    clippy::print_stderr,
    reason = "the tests against real servers say how long each answer took"
)]

mod common;
mod lifecycle;
mod messages;
mod native_servers;
mod node_servers;
mod rust_analyzer;
mod server_requests;
mod status;
