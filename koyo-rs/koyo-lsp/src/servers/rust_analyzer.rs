//! rust-analyzer, set up for a machine where many worktrees, and the agents in them, build
//! at the same time.

use serde_json::{Value, json};

use super::ServerSpec;
use super::release::{Archive, Release};

pub static RUST_ANALYZER: ServerSpec = ServerSpec {
    name: "rust-analyzer",
    on_path: true,
    project: |_| None,
    release: Release {
        version: "2026-10-26",
        url: "https://github.com/rust-lang/rust-analyzer/releases/download/{version}/rust-analyzer-{target}.gz",
        assets: &[
            (
                ("aarch64", "macos"),
                "aarch64-apple-darwin",
                "8fb90cb58f5910d8d8b24ac1a93cca49f053365cbf0eb3c682144ad0a0027dcf",
            ),
            (
                ("x86_64", "macos"),
                "x86_64-apple-darwin",
                "b3e4904877bcf7013a4a342c449cff1bf0ea0b91d5a1a30ac8df3d32142519c0",
            ),
            (
                ("aarch64", "linux"),
                "aarch64-unknown-linux-gnu",
                "1b225c4c9ae329ca96e8a75a5bbcb5562a7f1d25c57c8b55d3e02299a3dbe836",
            ),
            (
                ("x86_64", "linux"),
                "x86_64-unknown-linux-gnu",
                "4c83773525f93b636ed79761389568b4d2a89c05fa7ecc440245032d49ad80ce",
            ),
        ],
        archive: Archive::Gzip,
        binary: "rust-analyzer",
    },
    args: &[],
    settings,
    initialization_options: Some(defaults),
};

fn settings() -> Value {
    json!({ "rust-analyzer": defaults() })
}

fn defaults() -> Value {
    json!({
        "cargo": { "targetDir": true },
        "checkOnSave": true,
        "cachePriming": { "enable": true, "numThreads": 2 },
    })
}
