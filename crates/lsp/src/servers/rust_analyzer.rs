//! rust-analyzer, set up for a machine where many worktrees, and the agents in them, build
//! at the same time.

use serde_json::{Value, json};

use super::{Release, ServerSpec};

pub static RUST_ANALYZER: ServerSpec = ServerSpec {
    name: "rust-analyzer",
    release: Release {
        version: "2026-10-26",
        url: "https://github.com/rust-lang/rust-analyzer/releases/download/{version}/rust-analyzer-{target}.gz",
        // Computed from the downloads, and the same as the digests GitHub lists for them.
        assets: &[
            (
                "aarch64-apple-darwin",
                "8fb90cb58f5910d8d8b24ac1a93cca49f053365cbf0eb3c682144ad0a0027dcf",
            ),
            (
                "x86_64-apple-darwin",
                "b3e4904877bcf7013a4a342c449cff1bf0ea0b91d5a1a30ac8df3d32142519c0",
            ),
            (
                "aarch64-unknown-linux-gnu",
                "1b225c4c9ae329ca96e8a75a5bbcb5562a7f1d25c57c8b55d3e02299a3dbe836",
            ),
            (
                "x86_64-unknown-linux-gnu",
                "4c83773525f93b636ed79761389568b4d2a89c05fa7ecc440245032d49ad80ce",
            ),
        ],
    },
    section: "rust-analyzer",
    defaults,
    // It reads its settings from `initialize` first, and asks for them only later.
    settings_at_initialize: true,
};

fn defaults() -> Value {
    json!({
        // A target folder of its own (`target/rust-analyzer`): checking on save then never
        // waits for, or holds up, the lock on `target` an agent's `cargo build` needs.
        "cargo": { "targetDir": true },
        "checkOnSave": true,
        // With a server per worktree, each warming its caches on every core would starve
        // the builds running beside it.
        "cachePriming": { "enable": true, "numThreads": 2 },
    })
}
