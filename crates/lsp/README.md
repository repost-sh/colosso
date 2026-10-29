# colosso-lsp

A Language Server Protocol client for Colosso, with no GPUI in it: the server's pipes are
served on the client's own threads, and the app polls futures on any executor.

## What it does

- `Client::start(&Launch)` spawns the server: framed messages on a writer and a reader
  thread, stderr logged line by line, and a watcher that reports how the process ended.
  Restarting is starting again from the same `Launch`.
- The reader only sorts messages; payloads are parsed by whoever polls for them.
- `request::<R>(&params)` returns a future, and dropping it sends `$/cancelRequest`.
  `LspError` tells a server error from `Cancelled`, `Exited` and `Protocol`.
- `notify::<N>`, `subscribe::<N>()` for any notification as a stream of `Arc<Params>`
  (parsed once for all subscribers), and `events()`: changed registrations, and
  `Ended(Exit)` with the last stderr lines.
- `ready()` waits until rust-analyzer says it's quiescent. `request_retrying::<R>` asks
  again, up to three times, after a `ContentModified`.
- The server's requests: `handle::<R>()` answers them through a `Reply` and a `Peer` that
  doesn't keep the client alive. Left alone, `workspace/configuration` is answered from
  `Launch::settings`, and `workspace/applyEdit` and `window/showDocument` answer "not
  applied"/"not shown".
- `initialize(root)` offers the capabilities in `src/capabilities.rs`, and `encoding()` is
  the position encoding the server picked: UTF-8 or UTF-16.
- `shutdown(timeout)` asks, then sends `exit`, then kills.
- `Document` sends LSP edits, and borrows the text for `open` and `replace`. `IndexedText`
  turns byte edits into LSP edits for a caller without a rope.
- `servers::RUST_ANALYZER.binary(data, &Curl)` uses a working `rust-analyzer` on PATH if
  there is one. Otherwise it fetches the pinned release, checks its SHA-256, and moves it
  into `<data>/servers/rust-analyzer/<version>/` with a single rename. `Fetch` is the seam
  for the app's own HTTP client. Its settings give rust-analyzer its own target folder.

## Tests

```sh
cargo test -p colosso-lsp
```

runs everything against `fake-lsp` (`src/bin/fake-lsp.rs`), a server that follows a script
each test gives it. The test against a real rust-analyzer downloads it, so it's opt-in. It
prints each request's latency:

```sh
COLOSSO_LSP_IT=1 COLOSSO_DATA_DIR=$PWD/.superpowers/data \
  cargo test -p colosso-lsp -- --ignored --nocapture
```

## To do

- `vtsls` (TypeScript) and `basedpyright` (Python) specs, installed with npm under the app's Node.
