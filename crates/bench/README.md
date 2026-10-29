# colosso-bench

Measures what gpui-kit's code editor costs the main thread on large files: one input event and the full frame drawn after it (update, layout, prepaint, paint), timed on a real macOS window kept off screen, with the real text system and Metal renderer.

```sh
cargo run -p colosso-bench --release -- all                 # fixtures, every scenario, table, JSON
cargo run -p colosso-bench --release -- all --only resize,4k # ids containing "resize" or "4k"
cargo run -p colosso-bench --release -- list                # scenario ids; `run <id>` runs one here
```

`--release` is the reference; a debug build says so in the table. Fixtures are generated once into `.superpowers/fixtures/` (seeded; the Rust ones are this repository's own sources). `rust-4k` and `markdown-5k` stay under 256 KB, where the highlighter parses on the main thread at every keystroke (up to 2 ms) and scans the whole tree for injections; every larger file is parsed in the background. Each run writes `.superpowers/bench/<timestamp>.json` with the machine, versions, fixture hashes and every sample. Each scenario runs in a process of its own and is stopped after `--timeout` seconds (default 60), reporting "timed out at X" with the samples it took.

| Scenario | Each sample |
| --- | --- |
| `open` | `set_value` of the whole file into a fresh editor, then the first frame (5 times) |
| `scroll` | a wheel event of half a page, then the frame (300 frames, down the file) |
| `type`, `type-wrap` | a keystroke at the middle of the file, a letter then Backspace in turn, then the frame (200), soft wrap off and on |
| `resize` | the editor narrowed or widened by about 21 px with soft wrap on, then the frame (60) |
| `search-query` | the open search panel's query changed by one letter, typed or erased, then the frame (200) |
| `search-edit` | the `type` keystrokes while the query `self` is active (200) |

A scenario's id is its kind and its fixture, `type/rust-10k`. The long-line scenarios are `open`, `scroll` (sideways) and `type` on `json-1mb`, one 1 MiB line of minified JSON.

## Budgets

| What | Budget | Held against |
| --- | --- | --- |
| Keystroke to paint | < 4 ms | p95 |
| Scroll frame | < 8.3 ms (120 Hz) | p95 |
| Resize | never blocks > 5 ms | max |
| Open a 1 MiB file, first paint | < 50 ms | max (files over 1 MiB have no budget) |

## What the numbers leave out

- Time stands still during a scenario, so nothing debounced fires: the highlighter's background parse (150 ms after the last edit) lands only between setup steps, as for a typist who never pauses. Work an event spawns runs between samples, untimed.
- Presenting the frame to Metal isn't timed; GPUI only presents from the platform's frame callback.
- `resize` changes the editor's width rather than the window's, which is what the editor sees of a resize.
- `search-query` sets the query as the panel does on each change: typed into the panel itself, each keystroke replaces the whole query (the run notes it if that is still so).
