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

GPUI's Metal renderer, vendored for one change, marked `Koyo:` in `src/metal_renderer.rs`, `src/shaders.metal` and `src/fast/rounded_clip_shader.rs`: it draws a BGRA surface, a browser page's frame in the surface the browser host shares with the app (`koyo-rs/koyo/src/browser/frames.rs`), in the window's own frame. Upstream draws only biplanar video surfaces. `metal_renderer::set_surface_picture` says how much of the shared surface, from its top left, is the page; that part goes over the surface's bounds, and the rest of its content mask is filled by the picture's edge pixels reaching out, as while a page is laid out a frame behind a pane that grew. Rounded clips apply to it as to GPUI's other primitives. A page surface's Metal texture is made over its `IOSurface` directly, not through Core Video's texture cache, which keeps a pixel buffer until the cache is flushed; each frame keeps the pixel buffers and textures of the page surfaces it samples until the GPU has finished it (a pixel buffer marks its surface in use while it lives, which the browser host reads before painting into a surface), and no longer. Drawn with everything round it, the page can't reach the screen a frame apart from the pane's edge, which a page in a layer of its own did unless GPUI presented in Core Animation's transaction, at the cost of the window's frame rate. `build.rs` also allows the unused `stage` its shader helper module has when shaders aren't compiled ahead of time, as a path dependency's warnings aren't capped.
