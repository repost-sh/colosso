# vendor

gpui-kit's code editor, forked so it can be made fast enough for large files (measured by `crates/bench`).

| Directory | Crate | Version | Upstream |
| --- | --- | --- | --- |
| `gpui-base/` | `gpui-base` | 0.8.1 | [longbridge/gpui-kit](https://github.com/longbridge/gpui-kit) `crates/base` at `99576db58ec8103417c41a092e891b4022d24e7e` |
| `gpui-component/` | `gpui-component` | 0.8.1 | [longbridge/gpui-kit](https://github.com/longbridge/gpui-kit) `crates/component` at the same commit |

Both started as the crates.io packages unpacked as they are, `Cargo.toml` (as normalized by crates.io), `Cargo.toml.orig`, `Cargo.lock`, `.cargo_vcs_info.json` and `LICENSE-APACHE` included; each `.crate` matched the checksum in the workspace's `Cargo.lock` before it was unpacked. `git log -- vendor/` shows what has changed since.

The root `Cargo.toml` puts them in place of the registry crates with `[patch.crates-io]`. The `gpui-kit` facade still comes from crates.io and re-exports both, so the build holds one copy of each: `cargo tree -i gpui-base` names `vendor/gpui-base`.

Neither crate is a workspace member. They keep upstream's code style and their own `[lints]` table; the workspace lint policy and `script/hygiene` cover `crates/` only. Being path dependencies, their warnings aren't capped as a registry crate's are: `script/lint` holds them to rustc's and Clippy's defaults with every warning an error.

Both are licensed Apache-2.0, as is this repository; their `LICENSE-APACHE` files stay with them.
