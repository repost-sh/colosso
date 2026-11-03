# vendor

gpui-kit's code editor, forked so it can be made fast enough for large files (measured by `koyo-rs/koyo-bench`).

| Directory | Crate | Version | Upstream |
| --- | --- | --- | --- |
| `gpui-base/` | `gpui-base` | 0.8.1 | [longbridge/gpui-kit](https://github.com/longbridge/gpui-kit) `crates/base` at `99576db58ec8103417c41a092e891b4022d24e7e` |
| `gpui-component/` | `gpui-component` | 0.8.1 | [longbridge/gpui-kit](https://github.com/longbridge/gpui-kit) `crates/component` at the same commit |
| `gpui-fast-apple/` | `gpui-fast-apple` | 0.1.12 | the crates.io package, unpacked from the `.crate` whose checksum `Cargo.lock` holds |

Both started as the crates.io packages unpacked as they are, `Cargo.toml` (as normalized by crates.io), `Cargo.toml.orig`, `Cargo.lock`, `.cargo_vcs_info.json` and `LICENSE-APACHE` included; each `.crate` matched the checksum in the workspace's `Cargo.lock` before it was unpacked. `git log -- koyo-rs/vendor/` shows what has changed since.

The workspace's `Cargo.toml` (`koyo-rs/Cargo.toml`) puts them in place of the registry crates with `[patch.crates-io]`. The `gpui-kit` facade still comes from crates.io and re-exports both, so the build holds one copy of each: `cargo tree -i gpui-base` names `vendor/gpui-base`.

Neither crate is a workspace member. They keep upstream's code style and their own `[lints]` table; the workspace lint policy and `script/hygiene` cover the `koyo-rs/koyo*` crates only. Being path dependencies, their warnings aren't capped as a registry crate's are: `script/lint` holds them to rustc's and Clippy's defaults with every warning an error.

All three are licensed Apache-2.0, as is this repository; their license files (`LICENSE-APACHE`, `LICENSE`) stay with them.

## gpui-fast-apple

GPUI's Metal renderer, vendored for one change, marked `Koyo:` in `src/metal_renderer.rs`: `metal_renderer::present_in_transaction(on)` has every renderer present its frames in the main thread's Core Animation transaction, as upstream does only while AppKit waits for a window to draw. The app turns it on while a browser page has a native view (`koyo-rs/koyo/src/browser/frames.rs`), so a pane's new edge, drawn by GPUI, and the page laid out to it, in its own layer, reach the screen in the same frame; presented when the GPU is done, GPUI's frame came a frame after the page's layer, and a fast drag of a split showed a strip of the pane between them. `build.rs` also allows the unused `stage` its shader helper module has when shaders aren't compiled ahead of time, as a path dependency's warnings aren't capped.
