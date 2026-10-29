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

The scan scenarios run `colosso-project` off the main thread, with no window, on `repo-300k`: a git checkout of 150,001 files in 3,000 folders of nested packages, and 150,000 more in `node_modules/` and `target/`, which its `.gitignore` ignores. It's made once, with empty files, in `.superpowers/fixtures/repo-300k/`.

| Scenario | Each sample |
| --- | --- |
| `scan` | open the checkout until every folder git doesn't ignore is listed (3 times); notes the counts and resident memory after it |
| `scan-touch` | a file made or removed in a tracked folder, until the listing a subscriber receives shows it (20) |
| `burst-ignored` | 50,000 files written into `node_modules/`, then removed: the time the project spent taking them up; notes the reports, those dropped at the watcher, updates to consumers and folders read |
| `burst-tracked` | 2,000 files written into 20 tracked folders, then removed (3 times each), until every listing shows it; notes batches and folders read |

The rows scenarios time the Files panel's rows model (`colosso_project::Rows`) on `tree-500k`, a tree of 510,003 files held in memory: `big/` with 10,000 files, `deep/` 24 folders deep, and `src/` with 100 packages of 50 modules of 100 files. Every folder of `src/` is open, which shows 505,105 rows, before the samples are taken. Nothing is read from disk and no window opens.

| Scenario | Each sample |
| --- | --- |
| `rows-expand` | `big/` opened, its 10,000 rows put in among the others (60) |
| `rows-collapse` | `big/` closed again (60) |
| `rows-update` | a listing of `src/pkg-000/mod-00` with a file added or taken away, applied as an update brings it, before every other open row (60) |
| `rows-reveal` | the file at the bottom of `deep/` revealed with every folder above it closed (60) |

The finder scenarios time Go to File's search (`colosso_project::Finder`) over the same tree's files, handed over by folder as `Project::files_by_folder` hands them out, on as many threads as the machine runs. Each sample ends with the 500 best matches ranked and their matched characters found, which is what the palette draws.

| Scenario | Each sample |
| --- | --- |
| `finder-query` | one keystroke of `modfil` typed a letter at a time, then erased to nothing, from a finder just opened each time (240); every prefix matches all 500,000 files of `src/` |
| `finder-update` | a folder of 1,000 files the query matches added while `modfil` is typed, to the results with them; it's taken away again between samples, untimed (60) |

## Budgets

| What | Budget | Held against |
| --- | --- | --- |
| Keystroke to paint | < 4 ms | p95 |
| Scroll frame | < 8.3 ms (120 Hz) | p95 |
| Resize | never blocks > 5 ms | max |
| Open a 1 MiB file, first paint | < 50 ms | max (files over 1 MiB have no budget) |
| First scan of `repo-300k` | < 3 s | max |
| One file written to its listing updated | < 100 ms | p95 |
| A folder of the file tree opened or closed, a listing applied, a path revealed | < 2 ms | max |
| Go to File's results after a keystroke or a folder added, off the main thread | < 16 ms | p95 |

## What the numbers leave out

- Time stands still during a scenario, so nothing debounced fires: the highlighter's background parse (150 ms after the last edit) lands only between setup steps, as for a typist who never pauses. Work an event spawns runs between samples, untimed.
- Presenting the frame to Metal isn't timed; GPUI only presents from the platform's frame callback.
- `resize` changes the editor's width rather than the window's, which is what the editor sees of a resize.
- The scan scenarios read a warm disk: the fixture was just made or read, so the file system's caches hold it.
- `burst-ignored`'s removal (and now and then its writes) makes macOS drop reports ("user dropped", for the whole checkout): the project then reads every tracked folder again once the checkout has been quiet for 40 ms, or at most once a second while the burst goes on, and tells consumers anything may have changed.
- `search-query` sets the query as the panel does on each change: typed into the panel itself, each keystroke replaces the whole query (the run notes it if that is still so).
