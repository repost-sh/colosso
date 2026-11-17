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
| No Rust file over 500 lines. | `script/hygiene`, with `koyo-rs/hygiene-allow.txt` for the files already over | Split the file by what it does. A listed file may shrink but not grow. |
| No Rust function over 100 lines. | Clippy `too_many_lines`, set in `koyo-rs/clippy.toml` | Split the function. |
| A lint is never switched off. An exception is `#[expect(lint, reason = "...")]` on the one expression. | Clippy `allow_attributes` and `allow_attributes_without_reason` | Fix the code, or write the `#[expect]` with its reason. |
| No `unwrap`, `expect`, `panic!`, `println!` or `eprintln!` outside tests, and no `todo!`, `unimplemented!`, `unreachable!` or `dbg!` anywhere. | `[workspace.lints.clippy]` in `koyo-rs/Cargo.toml` | Return the error to the caller. |
| No `std::thread::sleep`, `std::process::exit` or `std::process::abort`, and no `CommandExt` call that forks the app. | `disallowed-methods` in `koyo-rs/clippy.toml` | The lint prints the replacement. |
| Clippy's pedantic group passes with every warning an error. | `script/lint` | Fix what Clippy names. |
| TypeScript has no `as`, no `any` and no `@ts-` comment. Switches are exhaustive and conditions are booleans. A file has at most 500 lines, a function at most 4 parameters and 4 levels of nesting. | `koyo-js/eslint.config.ts` and `koyo-js/tsconfig.base.json`, run by `script/lint` | Fix what ESLint or tsc names. |
| A daemon's version is settled whenever its sources change. | `script/daemon-version` | Run `script/daemon-version bump` if a running daemon must restart for the change, or `keep` if it need not. |

## Decisions the rules rest on

- An empty arrow function or plain function is allowed in TypeScript. ESLint's
  `no-empty-function` asks for a comment inside one, and the source has no comments.
