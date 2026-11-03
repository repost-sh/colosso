//! The protocol the app speaks with the agent host (`docs/chat.md` 4.3), mirrored from the
//! host's schemas in `koyo-js/koyo-agent-host/src/protocol`. The host's fixtures hold both
//! sides to it (`tests/fixtures.rs`).

pub mod events;
pub mod ids;
pub mod methods;
pub mod model;
pub mod requests;
pub mod wire;
