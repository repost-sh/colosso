# Koyo

A check enforces every rule in this file. `script/lint` runs all of them, and git runs
`script/lint` before each commit through `script/hooks/pre-commit`. `script/check` adds the
tests.

A rule that no check enforces does not belong here. When the same mistake is corrected a
second time, add the check first, then the row.

## Rules

| Rule | Enforced by | To fix |
|---|---|---|
| No plain comment (`//`, `/* */`) in Rust or TypeScript source. The one exception is `// SAFETY:` directly above the `unsafe` it argues for. | `koyo-rs/koyo-hygiene`, run by `script/hygiene` | The output says what to write instead. `script/cargo run -q -p koyo-hygiene -- --fix` deletes them. |
| The shipped builds of the koyo crate and of the browser host (`koyo-browser-host`, which runs on the app's main thread) read files, probe paths, start or wait on processes, and wait on an executor or a park only where a `Blocking` (`koyo/src/blocking.rs`, `koyo-browser-host/src/blocking.rs`) is in scope: in a function that takes one, or in the closure `blocking::spawn`, `thread`, `on_quit`, `at_launch` or `in_test` lends one to. Code on the main thread isn't handed one, and `at_launch` is the crate root's alone (`main.rs`, the host's `lib.rs`). `koyo/src/logs/sink.rs` is the one other file that may. | `koyo-rs/koyo-hygiene`, run by `script/hygiene`, with `koyo-rs/blocking-debt.txt` and `koyo-rs/blocking-debt-browser-host.txt` for the files that still do | Take `may: Blocking<'_>` from the caller, or start the work with `blocking::spawn` (in the host, `blocking::thread`). A listed file may lose calls but not gain them, and `script/cargo run -q -p koyo-hygiene -- --prune` lowers its line. |
| No Rust file over 500 lines. | `script/hygiene`, with `koyo-rs/hygiene-allow.txt` for the files already over | Split the file by what it does. A listed file may shrink but not grow. |
| No Rust function over 100 lines. | Clippy `too_many_lines`, set in `koyo-rs/clippy.toml` | Split the function. |
| No new import edge that closes a cycle between the `koyo` crate's top-level modules. `koyo-rs/module-edges.txt` lists the edges on a cycle today, and only shrinks. | `koyo-rs/koyo-hygiene` with `koyo-rs/module-edges.txt`, run by `script/hygiene` | Take what the module needs as an argument or event, or move the item. An edge that no longer closes a cycle comes off with `script/cargo run -q -p koyo-hygiene -- --prune`. |
| A lint is never switched off. An exception is `#[expect(lint, reason = "...")]` on the one expression. | Clippy `allow_attributes` and `allow_attributes_without_reason` | Fix the code, or write the `#[expect]` with its reason. |
| No `unwrap`, `expect`, `panic!`, `println!` or `eprintln!` outside tests, and no `todo!`, `unimplemented!`, `unreachable!` or `dbg!` anywhere. | `[workspace.lints.clippy]` in `koyo-rs/Cargo.toml` | Return the error to the caller. |
| No `std::thread::sleep`, `std::process::exit` or `std::process::abort`, no `CommandExt` call that forks the app, and no `refresh_windows` or `AsyncApp::refresh`, which rebuild every view of every window. | `disallowed-methods` in `koyo-rs/clippy.toml` | The lint prints the replacement. |
| Clippy's pedantic group passes with every warning an error. | `script/lint` | Fix what Clippy names. |
| TypeScript has no `as`, no `any` and no `@ts-` comment. Switches are exhaustive and conditions are booleans. A file has at most 500 lines, a function at most 4 parameters and 4 levels of nesting. | `koyo-js/eslint.config.ts` and `koyo-js/tsconfig.base.json`, run by `script/lint` | Fix what ESLint or tsc names. |
| A daemon's version is settled whenever its sources change. | `script/daemon-version` | Run `script/daemon-version bump` if a running daemon must restart for the change, or `keep` if it need not. |

## Decisions the rules rest on

- An empty arrow function or plain function is allowed in TypeScript. ESLint's
  `no-empty-function` asks for a comment inside one, and the source has no comments.
