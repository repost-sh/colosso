# Koyo terminals: multiplexer, in-app view and agent control

Specification, written 2026-10-30 on branch `schulzfel/feat-terminal-multiplexer`.
Author of the vision: Felipe Schulz (schulz@nuvera.global). Drafted with Claude.

This document is the source of truth for the terminal work. It is written to survive
context loss: every decision carries its reasons and the alternatives rejected, every
external fact carries its version and date, and every piece of prior art names the file
to read. Update it when a decision changes; do not let the code drift from it silently.

Status of each section is marked `[decided]`, `[proposed]` (designed here, to be
confirmed by implementation) or `[open]` (needs the owner's call; a default is given).

---

## 0. Vision in one paragraph

Terminals in Koyo are long-lived processes owned by a small daemon, not by the app
window. Closing Koyo leaves them running; reopening it, or a later mobile client,
re-attaches to them. Every terminal belongs to a worktree (a branch checkout), so a
human sees, per branch, what is running there, and an agent working in that branch finds
the same terminals by default. Agents reach terminals through an MCP server hosted by the
daemon, with a structured surface: open, read the screen or the stream, send input, wait
for a condition, claim and release control. Agentic TUIs (Claude Code, Codex, others)
run inside these terminals; humans interact with them in the macOS app, later on mobile;
agents get full, clean, logged access. Performance is a top priority on both sides: raw
PTY bytes flow to the renderer unchanged, and the engine on both sides is Ghostty's
terminal core (libghostty-vt).

---

## 1. Context and constraints

### 1.1 The Koyo codebase (facts, 2026-10-30)

- Rust workspace, `edition = "2024"`, toolchain `1.98.1` (`rust-toolchain.toml`), resolver 3,
  license Apache-2.0 (`LICENSE`). Crates: `crates/koyo` (the app), `crates/project`,
  `crates/git`, `crates/lsp`, `crates/bench`. Vendored forks: `vendor/gpui-base`,
  `vendor/gpui-component` (gpui-kit 0.8.1 at commit `99576db5`), see `vendor/README.md`.
- UI: GPUI through `gpui-kit = "0.8.1"` with feature `gpui-fast` (retained views). The GPUI
  crate underneath is `gpui-pre 0.3.10` (see `Cargo.lock`, and the `hot-reload` feature in
  `crates/koyo/Cargo.toml`).
- macOS first. `crates/koyo/src/native/` hosts AppKit `NSView`s inside GPUI's window
  composition (`native/surface.rs`: `WindowCompositionSurface`, stacked over the page and
  under GPUI's overlays). Non-macOS paths exist in `crates/project` (notify watcher), so
  portability is intended but not exercised.
- Lint policy (`Cargo.toml` `[workspace.lints]`): clippy pedantic as warnings; `unwrap_used`,
  `expect_used`, `panic`, `todo`, `unimplemented`, `unreachable`, `print_stdout`,
  `print_stderr`, `dbg_macro`, `map_err_ignore`, `await_holding_lock`,
  `needless_pass_by_value`, `redundant_clone`, `error_impl_error`,
  `unchecked_time_subtraction` are **denied**. Exceptions use
  `#[expect(lint, reason = "...")]`. Logging goes through the `log` facade into
  `koyo::logs`. New crates inherit `[lints] workspace = true`.
- Existing terminal placeholders: `PaneKind::Terminal` in `crates/koyo/src/model.rs:11`
  (title "New Terminal", default name "zsh", icon `SquareTerminal`), `pane.rs:157 fn terminal`
  renders a stand-in, `bottom.rs:54` "The panel under the workspace; a stand-in for the
  terminal it's to hold", action `NewTerminal` in `shell/actions.rs`. No PTY, no VT code.
- Branch identity: `model.rs:86 pub struct BranchUnit { worktree: Worktree, tabs, active_tab }`.
  `Worktree { path, head: Head::Branch(name) | Head::Detached(commit), is_main }`.
  `Project { path (main checkout), name, branches: Vec<BranchUnit>, active_branch }`.
  `Workspace::project_of(worktree_path)` finds a project by a worktree path.
- Checkout identity: `crates/koyo/src/checkouts.rs` keys open checkouts by
  `std::fs::canonicalize(root)` "so a checkout reached through a link is the same checkout".
  Reuse this rule for terminal scoping.
- App data folder: `settings::data_dir()` (`crates/koyo/src/settings.rs:243`), the app's
  folder under Application Support; language servers are fetched under it. Logs go to
  `~/Library/Logs/Koyo` (`logs/mod.rs:117`).
- Dev loop: `script/dev` runs `dx serve --hot-patch`; it downloads a pinned `dx` into
  `.tools/dx-<version>/` with a sha256 check. The same pattern will pin Zig (section 11).
- Tests: `gpui-kit` feature `test-support` gives headless windows; `crates/koyo` has a
  `visual-check` feature and `test_support` module.
- Repository rules (owner's machine rules): everything the work creates stays inside the
  checkout; scratch under `.superpowers/` (gitignored); `.cargo-home` is the cargo home;
  never create branches or worktrees unless asked.

### 1.2 Relationship to Orca and monocode

- Koyo is the native successor of the Orca app's layout ("Orca layout: projects and their
  branch units in the sidebar", `crates/koyo/src/orca.rs`). The Orca CLI
  (`/Applications/Orca.app/Contents/Resources/bin/orca`) already exposes worktree-scoped
  terminals to agents (`orca terminal list|show|read|send|wait|create|...`). Its vocabulary
  is the one agents on this machine already know; the MCP here keeps it (section 8.9).
- monocode (`/Users/schulzf/github.com/schulzfel/monocode-worktrees/feat-browser-panes`) has
  a full app-control MCP in Rust (`crates/monocode-control`, `crates/monocode-mcp`,
  `crates/monocode-gpui/src/shell/control/`). Its conventions are adopted here (section 8).
  Its browser panes are being ported into Koyo in the worktree
  `/Users/schulzf/orca/workspaces/koyo/feat-new-native-app` (CEF host crates
  `crates/browser-host`, `crates/browser-ipc`, uncommitted as of 2026-10-30); no MCP or
  control code has been ported there yet.
- `agent-terminal` (`/Users/schulzf/github.com/repost-sh/agent-terminal`, Rust, ~35k lines,
  binary at `~/.cargo/bin/agent-terminal`) is the owner's first shared human/agent terminal.
  Its mechanics are borrowed; its session model is replaced (section 15).

### 1.3 Non-negotiables

1. Performance: no server-side re-rendering between the PTY and the renderer; raw bytes
   reach the app's engine; repaint only on dirty state.
2. Terminals survive the app window. Only a daemon stop, logout or machine restart ends them.
3. One engine, one state format on both sides and on mobile: libghostty-vt and its snapshot.
4. Terminals are scoped to worktrees; agents find them by worktree by default.
5. Agent access is structured (MCP), authorized, logged, and never silently collides with a
   human or another agent.
6. Licensing: Apache-2.0 compatible only. Zed's `crates/terminal` is GPL-3.0-or-later and must
   not be copied. `alacritty_terminal` is Apache-2.0, Ghostty and its bindings MIT/Apache-2.0.

---

## 2. Decisions

Each decision: what, why, what was rejected.

**D1 [decided] Build our own daemon; do not base the multiplexer on tmux, Zellij or WezTerm's mux.**
Why: tmux becomes the program's terminal (its terminfo, its feature set), re-emulates every
pane server-side, sends output as octal-escaped text in control mode and has no state
snapshot. Zellij renders the composed screen server-side and sends ANSI; its IPC is internal.
WezTerm's `codec`/`mux` crates are `publish = false`, git-only, server-side emulation with line
deltas. All three put a second terminal between the program and Ghostty's engine. The
persistence-only daemons of 2026 (zmx, boo, shpool, herdr) all converge on the design in D2.

**D2 [decided] Daemon design: PTY + headless libghostty-vt per session, raw bytes streamed to
clients, snapshot replay on attach and on catch-up.** Why: it is the fastest local path (one
unix-socket hop), it preserves every feature end to end (kitty keyboard, kitty graphics,
OSC 133, synchronized output), it answers terminal queries while detached, keeps scrollback
when no window is open, and the snapshot bounds bandwidth for slow links (mobile). Ghostty's
maintainer stated in Aug 2026 that Ghostty itself uses "the binary snapshot protocol" for
reconnectable terminals (discussion #12176).

**D3 [decided] Engine: libghostty-vt through Rust bindings, on both the daemon and the app
side.** Why: Ghostty 1.4.0 (2026-10-27) reports plain-ASCII parsing at 725 MB/s (from 128),
has grapheme clustering, kitty keyboard and graphics, OSC 133 semantic prompts, mode 2026
render hold, scrollback compression, a render-state API designed for a renderer thread,
key/mouse encoders, a formatter (text/VT/HTML) and the snapshot API. Adoption: JetBrains
2026.3 terminals, PaneFlow (a GPUI app that left `alacritty_terminal`), Zeron, gpui-cn.
Rejected: `alacritty_terminal 0.26` (no grapheme clustering, no kitty graphics, no OSC 133,
no snapshot; Zed's view code is GPL), full `libghostty` with Ghostty's Metal renderer in an
`NSView` (its `ghostty.h` says "internal embedder API tailored to the macOS app, not designed
for external use", macOS/iOS only, spawns the shell itself with no custom I/O hook, and
would put keyboard, IME, selection and theming outside GPUI).

**D4 [decided] Rendering: a GPUI element paints from an owned per-frame copy of the engine's
render state.** Cell-pinned glyphs per grapheme cluster, procedural box-drawing/Powerline
sprites, repaint on dirty rows only. Pattern proven by PaneFlow and Zeron.

**D5 [decided] Two engine instances per visible terminal (daemon and app).** The double parse
is cheap at these speeds; the alternative (streaming render deltas) means inventing a wire
format nobody ships publicly. Revisit for mobile only if bandwidth measurements demand it.

**D6 [decided] Terminals are keyed to the worktree (canonical checkout path), with per-worktree
human names.** Why: branch names rename and heads detach; the checkout path is stable for the
checkout's life and is what Orca (`repoId::path`) and monocode (session cwd) already use.
Names unique per worktree keep them readable; agent-terminal removed worktree scoping because
hashed global names were unreadable.

**D7 [decided] Control is a lease held by one controller, with explicit, visible, logged
takeover; the human pre-empts everyone; reads never need control.** Why: agent-terminal had a
human-holds-control rule but no arbitration between agents, which is what got in the owner's
way when a second session in the same branch had to take a terminal over.

**D8 [decided] Terminal status is derived in the daemon from the engine's own signals
(OSC 133, titles, OSC 9/777 notifications, bell, OSC 7501, exit), with a stable-screen
heuristic as fallback.** Exposed to the UI and the MCP as one enum.

**D9 [decided] The MCP server is hosted by the daemon, speaks monocode's conventions, and
offers two transports (loopback Streamable HTTP and a stdio relay).** Why: terminals outlive
the window; mobile arrives later; monocode's catalog pattern generates tools from one
registry; stdio is what most MCP clients want and it carries the caller's cwd. App-level tools
(browser, editor) can later federate into the same server (section 8.1); start with
terminals only. **As implemented (M4):** the server runs in the stdio relay, not in the
daemon, which runs the operations behind it; HTTP is deferred (4, 8.1).

**D10 [decided] Terminal input is exposed to agents.** monocode's MCP lists terminal input
under "never exposed". Koyo reverses that on purpose, mitigated by scopes and grants,
the control model, idempotent requests and the activity log.

**D11 [decided 2026-10-30] A human takes a terminal over either implicitly, by typing in it,
or explicitly, with "Take control". Once a human has taken a terminal over, agents do not
use it again until the human explicitly gives it back; there is no automatic release.**
Why: the owner's call. A human stepping into a terminal means it is theirs now; an agent
that keeps typing into it, or resumes after a timer, is the failure mode. The daemon enforces
the hold; the agent guide tells agents to treat `person_has_control` as "leave this terminal
alone and open another one in the worktree", not as something to wait out.

**D12 [decided] Protocol and snapshot versions are explicit from day one; daemon and app ship
as one unit; an older daemon keeps running when the app updates and the app degrades rather
than killing sessions.** Why: zmx's IPC upgrades kill live sessions; libghostty-vt's snapshot
format v1 "does not yet carry a binary-compatibility guarantee". **As implemented (M5):** an
app finding a daemon of another build leaves it running and says so in the tab, whose
"Restart terminals" stops it and starts its own (5.1).

**D13 [decided 2026-10-31] A terminal lives in a tab, and only in a tab.** The bottom panel
holds no terminal; it stays the stand-in `bottom.rs` had. Why: the owner never asked for a
terminal there. Splits came with the browser work merged on 2026-10-31: a tab is a nested
split tree (`Tab::Split`, rows and columns, a ratio each), and a terminal sits on any side of
it as any pane does (O8).

---

## 3. Research record (as of 2026-10-30)

Kept so nobody re-derives it. Versions from crates.io via `cargo info` on 2026-10-30 unless
noted.

### 3.1 Ghostty and libghostty

- Ghostty releases: 1.2.0 2025-09-15, 1.3.0 2026-03-09, 1.3.1 2026-03-13, **1.4.0 2026-10-27**.
  1.5 planned March 2027. libghostty has its own release cycle; no tagged libghostty release
  yet; 1.4.0 notes say one more C API/ABI break, then "likely a libghostty 1.0 tag very soon".
- 1.4.0 performance notes: ASCII VT throughput 128 → 725 MB/s; 150 MB `cat` 1.5 s → 566 ms on
  macOS; I/O +25–55% by draining the pty on one thread and parsing on another; resize ~6×
  faster with large scrollback; renderer holds the terminal lock 2.7–11× less; scrollback
  compression cuts resident memory 70–90%; default scrollback 10 → 50 MB.
- libghostty-vt builds for macOS, Linux, Windows, FreeBSD, Android, iOS, WebAssembly;
  freestanding (no libc++); CMake, Nix, xcframework, source tarball, pkg-config. 30+ examples
  in Zig, C, C++, Swift, Wasm kept current with `main`.
- C headers: `include/ghostty/vt/*.h`: allocator, build_info, color, color_scheme, device,
  focus, formatter, grid_ref, grid_ref_tracked, io, key(+dir), kitty_graphics, modes,
  mouse(+dir), osc, paste, point, render, screen, search, selection, sgr, size_report,
  **snapshot**, style, sys, terminal, types, unicode, wasm.
- `terminal.h` (verified 2026-10-30): `ghostty_terminal_vt_write()` processes bytes and fires
  callbacks synchronously on the calling thread; callbacks must not re-enter `vt_write` on the
  same terminal. `ghostty_terminal_vt_write_until()` stops at a boundary (ground state, string
  start/end). `ghostty_terminal_resize()`, `_reset()`, `_compress()`, `_paste()`,
  `ghostty_key_encoder_setopt_from_terminal()`. Options: `SCROLLBACK_MAX_BYTES`,
  `SCROLLBACK_MAX_LINES`, `RESIZE_PULL_SCROLLBACK`, `MODE`, `MODE_DEFAULT`. Callbacks:
  `WRITE_PTY`, `BELL`, `TITLE_CHANGED`, `PWD_CHANGED`, `ENQUIRY`, `XTVERSION`, `SIZE`,
  `COLOR_SCHEME`, `DEVICE_ATTRIBUTES`, `CLIPBOARD_WRITE`, `CLIPBOARD_READ`,
  `DESKTOP_NOTIFICATION` (OSC 9/777), `PROGRESS_REPORT` (OSC 9;4), `PROGRAM_STATUS`
  (OSC 7501), `UNKNOWN_SEQUENCE`, `RENDER_HOLD` (mode 2026), `SEMANTIC_PROMPT` (OSC 133),
  `RESET`, `RESIZE_WINDOW`. `ghostty_render_state_update()` captures a frame; inside a
  render-hold callback it captures the frame the program wants shown.
- `snapshot.h` (added 2026-08-03, options through 2026-10-21): encode with
  `ghostty_snapshot_encode(terminal, writer)`, `_encode_buf`, `_encode_alloc`; decode with
  `ghostty_snapshot_decoder_new[_buf]`, `_decode` (one shot), `_ready` (restore the renderable
  prefix first), `_next` (one history page or one kitty image per call). Contents:
  terminal-wide state, active screen rows, scrollback pages newest→oldest per screen,
  unfinished VT/UTF-8 continuation (opt-in), kitty graphics (optional). Format: 10-byte
  envelope `"GHOSTSNP"` + u16 version; records `u16 tag, u32 len, u32 CRC32C, payload`,
  little-endian; decoders skip unknown tags; READY and FINISH marker records. "Snapshot format
  version 1 is a work in progress and does not yet carry a binary-compatibility guarantee."
- Ghostty has no multiplexer; maintainers said special SSH handling is "not on the roadmap"
  (Jan 2026). 1.4.0 added `ghostty +ssh` (terminfo + env forwarding only). Community:
  discussion #11891 (SSH fork with headless `ghostty-daemon`, unanswered), #12176
  (`ghostty-snap` proposal: length-prefixed frames `[type:u8][len:u24][payload]`, raw PTY
  streaming with a switch to VT snapshots when the client falls behind via
  `TCP_NOTSENT_LOWAT`; maintainer reply "We're doing this ourselves using the binary snapshot
  protocol"; closed 2026-08-06).

### 3.2 Rust bindings for libghostty-vt

| crate | version | repo | notes |
| --- | --- | --- | --- |
| `ghostty-vt` + `ghostty-vt-sys` | `0.8.0+ghostty.1.4.0-main.83fb956` | github.com/prabirshrestha/gpui-cn | Apache-2.0. Safe modules include `snapshot`, `search`, `io`, `shell_integration`, `terminfo`, `render`, `key`, `mouse`, `fmt`, `osc`, `selection`, `kitty`. The sys crate's `bindings.rs` has the `ghostty_snapshot_*` symbols. Build: `GHOSTTY_SOURCE_DIR`, or a git submodule, or pkg-config, or download of the pinned tarball in `GHOSTTY.lock` with sha256 check, then `zig build`. Needs Zig 0.16. The repo also has a `gpui-cn` component crate with a `terminal` module (a GPUI terminal on this binding) and `gpui-cn-story`. One maintainer, 1 star, pushed 2026-10-28. |
| `libghostty-vt` + `libghostty-vt-sys` | `0.2.2` (2026-09-28) | github.com/uzaaft/libghostty-rs | MIT. Pins Ghostty commit `a887df42c56f6de86c0fe6da9c4eeca37931e083`; needs Zig 0.17. The published 0.2.2 has **no** snapshot symbols; `master` (pushed 2026-10-30) has `snapshot.rs`, `search.rs`, `io.rs`, `sys.rs`. 401 stars; also maintains `awesome-libghostty`. Rust MSRV 1.90. |
| `mnml-libghostty-vt-sys` | 0.2.3 | chris-mclennan/mnml | MIT/Apache-2.0, another sys crate. |
| `embers-ghostty-sys` | 0.4.0 | Pajn/Embers | GPL-3.0-only: **do not use**. |
| `gpui-libghostty` | 0.3.1 (2026-09-25) | behzade/gpui-libghostty | MIT, alpha; embeds Ghostty into GPUI (vendored Ghostty, Zig 0.16, pinned to a Zed revision and `gpui-pre 0.3.4`; `bind_gpui!` macro). Reference only. |

Both safe bindings mark `Terminal` as `!Send + !Sync`; callbacks run synchronously inside
`vt_write`; `grid_ref` lookups are "not meant for render loops"; `compress` full mode can
stall. Choice: start on `ghostty-vt` (snapshot in a published release, tracks 1.4.0), pin the
exact version, keep it behind one module so switching to `libghostty-vt` is cheap.

### 3.3 Other engines and crates

`alacritty_terminal 0.26.0` (Apache-2.0, MSRV 1.85; Zed uses a fork at
`zed-industries/alacritty` rev `4c129667`); `vte 0.15.0`; `portable-pty 0.9.0` (MIT, WezTerm);
`termwiz 0.23.3`; `wezterm-term` not on crates.io; `vt100 0.18.1` (shpool's engine).

### 3.4 Multiplexers and persistence daemons

| tool | version | language | model | notes |
| --- | --- | --- | --- | --- |
| tmux | 3.6/3.6b, 3.7 exists | C | server-side emulation; control mode `-CC` streams `%output` per pane octal-escaped; flow control `refresh-client -f pause-after`, `%pause`/`%continue`; no snapshot (`capture-pane -e`) | iTerm2 is the main `-CC` client; iOS apps remux, Geistty speak it. CVE-2026-11623 in 3.6a. |
| Zellij | 0.45.1 (MSRV 1.95) | Rust | server renders composed screen as ANSI; web client since 0.43, HTTPS remote attach with tokens since 0.44, mobile web UI | wrong shape for an embedded client |
| WezTerm mux | in-tree | Rust | server-side `wezterm-term`, line deltas, `codec` crate `publish = false` (leb128 + varbincode + zstd) | git-only |
| shpool | 0.11.5 | Rust (Apache-2.0) | persistence only, single client, in-memory `shpool_vt100` render, macOS partial | |
| zmx | 0.8.1 | Zig (MIT) | daemon per session, unix socket, multi-client, libghostty-vt snapshot on attach, daemon not in the input path; IPC upgrades kill sessions | closest to D2 |
| boo (Coder) | young | Zig (MIT) | GNU-screen style on libghostty-vt, framed unix-socket protocol, replay via `TerminalFormatter`, single client steals, one window per session | |
| herdr | 0.4.0 | Rust (Apache-2.0 per repo; crates.io 0.1.0 says AGPL) | background server + TUI client, vendored Ghostty VT, agent statuses working/blocked/idle, 46.9k stars | reference for status derivation |
| rmux | 0.10.0 | Rust (MIT/Apache) | tmux-compatible, SDKs, ships a "private full CLI helper" | partly closed |

Sources: ghostty.org release notes 1.3.0/1.3.1/1.4.0; github.com/ghostty-org/ghostty
`include/ghostty/vt/*.h` and discussions 11891, 12176; mitchellh.com "Libghostty is coming"
(2025-09-22); repos of each tool above; paneflow.dev/blog/libghostty-linux; zeronsh/zeron
PR 1006; dev.to "What it takes to embed libghostty in a Swift app".

---

## 4. Architecture overview

```
                    ┌──────────────────────────── machine ────────────────────────────┐
                    │                                                                 │
  Claude Code ──stdio──▶ koyo mcp (MCP) ────┐                                      │
  (HTTP 127.0.0.1 deferred, 8.1)               │  koyo-termd (one per user)        │
                                               │  ├─ registry (worktrees, terminals)  │
  Koyo.app ◀──unix socket (frames v1)──────▶│  ├─ operations behind the MCP        │
   ├─ crates/term: engine + GPUI element       │  ├─ control leases, status, activity │
   └─ branch units ─ terminal tabs             │  └─ sessions[n]: thread {            │
                                               │        PTY ⇄ child process          │
  mobile (later) ◀─ssh/QUIC─▶ same frames ────▶│        headless ghostty-vt Terminal  │
                                               │        raw byte ring + snapshots     │
                                               │        client fan-out, catch-up }    │
                    └─────────────────────────────────────────────────────────────────┘
```

Data flow for one keystroke: GPUI key event → ghostty key encoder in the app → `Input` frame
→ daemon session thread → PTY write. For output: PTY read → session thread → fed to the
daemon's headless engine (status, snapshot, queries) and fanned out as `Output` frames →
app → app engine `vt_write` → render state → GPUI paint on the next frame if dirty.

**As implemented (M5):** the MCP server is the relay `koyo mcp` (crates/term-mcp), one per
agent, over stdio; it reaches the daemon by the same unix socket and frames as the app, and
the daemon runs the operations (8.1). Loopback HTTP is deferred.

---

## 5. The daemon: `koyo-termd`

### 5.1 Process model and lifecycle [proposed]

- One daemon per user account, built from `crates/termd`, a library with a thin binary.
  The app runs it from its own executable as `koyo termd --dir <folder>` (the subcommand
  is dispatched before anything else in `main`), so the daemon ships inside the app and is
  always the app's build; `koyo-termd` is the same daemon on its own, for the tests and
  the `koyo-term` command line.
- Started on demand by the first client (app or relay) that finds no live socket: spawn
  detached (own process group, stdout piped until it prints `ready`, stderr to the log
  file) under `start.lock`, so only one client starts it. The app's startup only lists the
  terminals of a daemon already running (to restore their tabs); it never starts one until a
  terminal is opened. Also installable as a launchd agent later (`KeepAlive`); not required.
- Single-instance guarantee: `flock` on `<dir>/daemon.lock` holding the pid; the socket path
  is only trusted when the lock is held (pattern from agent-terminal
  `cli/src/native/daemon/claim/mod.rs:11-31`).
- Files, under `settings::data_dir()/term/` (macOS: `~/Library/Application Support/Koyo/term/`):
  - `daemon.sock` (mode 0600), `daemon.lock`, `daemon.version` (protocol + build),
    `daemon.log` (rotated, size-bounded).
  - `registry.json` (atomic replace via temp file + rename), see 5.9.
  - `terminals/<handle>/{activity.jsonl, commands.jsonl, screenshots.jsonl, screenshots/}`
    (5.8) and `terminals/<handle>/snapshot.bin` (optional, 5.9).
  - `mcp.token` (0600): secret for app-launched agents, regenerated at daemon start.
- Shutdown: `Shutdown` frame from an authorized client, or SIGTERM. The daemon sends SIGHUP
  to every child, waits up to 5 s, then SIGKILL. It never stops because a client left.
- Upgrade: a new build does not replace a running daemon that holds terminals. It connects,
  compares `protocol` in `Welcome`; if compatible it keeps using the old daemon; if not, the
  app shows "Terminals are served by an older build; restart them to upgrade" and offers
  `Shutdown` + restart. (Fd-passing handover is a later improvement.)

**As implemented (M5):** "compatible" means the daemon's protocol and also its engine: the
views restore its snapshots, so the app's client (`term/src/client.rs`, `this_build`) turns
away a daemon whose Ghostty commit or snapshot format isn't its own, and the daemon refuses a
hello of another protocol, answering `daemon_version_mismatch` before it hangs up. A tab the
daemon turned away shows the notice ("Terminals are served by another build of Koyo;
restart them to use this one. Their programs end, and the shells you opened start again."),
worded for either direction, since an older app meets a newer daemon too, with a "Restart
terminals" button. The button runs `stop_daemon` (`term-control/src/client.rs`): `Shutdown`
when the daemon still takes the hello, else SIGTERM to the pid in `daemon.lock`, sent only
while that lock is held so a stale pid is never signalled; the tab then opens or attaches
again, which starts this build's daemon, and every other tab turned away tries again. When the
app starts on another build's daemon, the person's shells the registry remembers (read as
JSON, since another build wrote it) get those tabs. Tests: `upgrade::*` in the daemon tests
stop a real daemon both ways and find its terminals in the next; the in-app check puts a
stand-in daemon with another engine under the app, sees the notice, presses the button and
gets the shell back on this build's daemon.

Besides the protocol, the welcome's `daemon_version` must be this build's `DAEMON_VERSION`,
so a build whose daemon changed bumps it, or the old daemon would go on serving the new app.
`script/daemon-version`, run by `script/lint` and so by the pre-commit hook, holds the version
in `daemon-version.txt` beside a fingerprint of what the daemon is built from: the crates
`cargo tree` resolves for `koyo-termd` with their versions, the sources of those in the
checkout (tests left out), `rust-toolchain.toml` and `.cargo/config.toml`. It fails once they
part, until `script/daemon-version bump` raises the version or `script/daemon-version keep`
notes a change a running daemon doesn't need.

### 5.2 Terminal model [decided]

A **terminal** (the daemon's unit; "session" is avoided because it collides with agent
sessions) is:

```
Terminal {
  handle:      "term_" + ULID              // issued by the daemon, never reused
  worktree:    WorktreeRef { path (canonical), repo_main (canonical), head: branch|detached }
  name:        String                       // unique within the worktree, [a-z0-9][a-z0-9-]{0,31}
  title:       Option<String>               // last OSC 0/2 title
  cwd:         PathBuf                      // last OSC 7, else spawn cwd
  command:     Vec<String>                  // argv; empty = login shell
  env:         BTreeMap<String,String>      // extra env at spawn
  size:        { cols, rows, cell_w_px, cell_h_px }
  labels:      BTreeSet<String>             // e.g. "agent", "dev-server", "tests"
  created:     Timestamp, created_by: Actor
  incarnation: u32                          // bumps on every (re)spawn
  pid:         Option<u32>, exit: Option<ExitStatus>
  status:      Status                       // section 5.7
  control:     Control                      // section 5.6
  clients:     Vec<ClientId>                // currently attached
  last_output: Timestamp
}
```

- `WorktreeRef.path` is `canonicalize(nearest ancestor containing .git)` of the spawn cwd, or
  the explicit worktree argument. `repo_main` is the main checkout path (from `git worktree
  list` / `GitDirs` in `crates/project`), so one project groups its branches.
- A terminal whose cwd is outside any git checkout gets `worktree = None` and lives under a
  synthetic "no worktree" group; the UI shows it under the project of the app's current
  selection only when explicitly asked. Agents must pass `worktree` explicitly for these.
- Names: on `open` with a taken name and `ifExists: "attach"` the existing terminal is returned
  with `existed: true`; with `ifExists: "error"` the call fails with `name_taken` and the
  existing handle. Default is `attach` (agent-terminal's "session is taken" friction goes away).

### 5.3 Per-terminal thread, PTY, engine, buffers [proposed]

- One OS thread per terminal owns: the PTY master fd, the child, the headless
  `ghostty_vt::Terminal` (it is `!Send`, so it never leaves this thread), the raw byte ring,
  the status machine, and the list of attached clients' outbound queues.
- PTY: `portable-pty 0.9` (unix `openpty`), spawned with the login shell
  (`$SHELL -l`) or the given argv, `cwd` as spawn directory, env: inherit the daemon's env
  plus `TERM=xterm-ghostty` **if** the bundled terminfo is installed (section 11), else
  `TERM=xterm-256color`; `COLORTERM=truecolor`; `TERM_PROGRAM=Koyo`;
  `TERM_PROGRAM_VERSION`; `KOYO_TERMINAL=<handle>`; `KOYO_WORKTREE=<path>`; and for
  shells, the shell-integration hook (5.7). Never inject `KOYO_MCP_*` secrets into a
  terminal that an agent opened for a non-agent purpose; the app passes agent env explicitly.
- Read loop: `read(2)` into a 64 KiB buffer; each chunk is (a) appended to the raw ring,
  (b) fed to the engine with `vt_write` (callbacks update title/cwd/status and queue
  `WRITE_PTY` replies to the PTY when **no client is attached**; when clients are attached,
  the app's engine answers queries, as boo does, to avoid duplicate replies), (c) enqueued to
  every attached client. No coalescing beyond the read itself.

  **As implemented (M5):** the read runs on a thread of its own that does little but read
  (`session/handoff.rs`). macOS's PTY gives a flood about a kilobyte a read, and the program
  writing waits on each. Any other call on the way between reads (a `poll` to read on, an
  `ioctl(FIONREAD)`, waking the session thread) slows the flood by about a quarter, and so
  does sleeping in a blocking read: the reader sleeps on each empty queue and the program's
  next write has to wake it. So the master is non-blocking, and once a read comes back full
  the reader reads again at once on an empty queue, up to 16 times, before it waits in
  `poll`, as Ghostty's reader does (a bare PTY on an M4 Max: 122 MiB/s read blocking, 183 to
  192 read so; 64, 256 or more tries were no faster, nor was a higher thread priority).
  Writes to the program wait for room in `poll` likewise, the flag being the open PTY's. The
  reader appends what it reads to a buffer the session thread takes, and wakes that thread
  only after it said it sleeps. While output flows the session thread takes what's there
  every 0.5 ms, in frames of up to 64 KiB, and goes to sleep when a take finds nothing;
  anything written to the program has the next output wake it, so a keystroke's echo goes on
  at once, even during a flood. Past 4 MiB not yet taken the reader stops reading, and the
  program waits. A `cat` goes in frames of about 55 KiB (`term-cat` in 7.5: 120 MiB/s with a
  blocking reader, 145 to 163 with this one).
- Raw ring: bounded (default 8 MiB per terminal). Each byte has a monotonically increasing
  **stream offset** (u64). Clients track the offset they have received.
- Snapshot cache: the session thread encodes a snapshot on demand (attach, catch-up, MCP
  `read` in `screen` mode when the app is not attached, periodic persistence). Encoding is
  rate-limited (at most one in flight; coalesce requests).
- Write path: `Input` frames from clients are written to the PTY in arrival order; the
  session thread owns the PTY so there is no lock. The control check (5.6) happens before
  the write; refused input is logged and answered with an error, never dropped silently.
- Resize: the terminal's size is negotiated (5.4); `resize` calls both `ioctl(TIOCSWINSZ)` and
  the engine's `resize`.
- Exit: on child exit, record `exit`, keep the terminal (screen stays readable) until
  `close`, or respawn on `open{..., respawn: true}` bumping `incarnation`.

### 5.4 Wire protocol v1 [proposed]

Transport: unix stream socket locally; the same frames over any reliable byte stream
(SSH channel, QUIC stream) remotely. Framing: `u8 type | u24 length | payload` (little-endian
length; max payload 16 MiB; snapshots larger than that are chunked). Payloads are a compact
binary encoding for the hot frames (`Output`, `Input`) and CBOR (or MessagePack) for control
frames; the exact codec is an implementation choice, but **`Output` and `Input` must be
zero-copy-friendly**: `Output = { terminal_index: u32, offset: u64, bytes }`.

Handshake:

```
C→D  Hello    { protocol: 1, client: { kind: app|relay|mobile|cli, name, version, pid },
                auth: none | token(bytes), capabilities: [snapshot_v1, ...] }
D→C  Welcome  { protocol: 1, daemon_version, snapshot_format: 1, client_id, features }
```

Registry and lifecycle (request/response; every request carries `req: u32`, replies echo it):

```
ListWorktrees {}                                   → [WorktreeRef + counts]
ListTerminals { worktree?, status? }               → [TerminalSummary]
Open   { worktree?, name, argv?, cwd?, env?, size?, labels?, if_exists: attach|error,
         respawn?: bool, actor }                   → { terminal, existed }
Attach { handle, size?, mode: control|observe, since_offset?: u64 }
       → AttachOk { terminal, offset: u64 } then Snapshot{...} then Output{...}* 
Detach { handle }
Resize { handle, cols, rows, cell_w_px, cell_h_px }
Pin    { handle, cells?: (cols, rows) }            → Pinned { size, pinned }   (protocol 3;
         a size every viewer shows, owned by the connection; `docs/responsive.md` 3.1)
Rename { handle, name? , title? , labels? }
Signal { handle, signal }
Close  { handle, force?: bool }                    → requires control or manage scope
Theme  { foreground, background, dark }            → Done   (protocol 4; daemon-wide, see 6)
Shutdown {}
```

Streaming (daemon → client, unsolicited after `Attach`):

```
Snapshot { handle, offset: u64, format: 1, chunk: n/of, bytes }   // state as of `offset`
Output   { handle, offset: u64, bytes }                            // raw PTY bytes from offset
Event    { handle, kind: title|cwd|status|control|size|exit|notification|progress|bell, data }
Lagging  { handle }                                                // catch-up begins
```

Input (client → daemon):

```
Input { handle, bytes, request_id?: u128 }          // already encoded by the client's key encoder
Paste { handle, text }                              // daemon brackets it per the engine's mode
```

**As implemented (M5):** the binary frames (`Output`, `Snapshot`, `Input`) name the
attachment by the index `Attached` returned, not the handle (`term-control/src/framing.rs`),
and `Input` carries no request id: a repeated `requestId` is answered from the relay's
receipts (8.2). `Paste` is a control request, not a binary frame.

Attach algorithm: the session thread records the client's `since_offset` (0 for a fresh
attach). It encodes a snapshot at the current offset `S`, sends `Snapshot(S)`, then streams
`Output` from `S` on. A client that already holds state at offset `O ≥ ring.start` may ask
`since_offset: O` and receive `Output` from `O` without a snapshot (fast reattach after a
brief disconnect).

Catch-up algorithm: each client has a bounded outbound queue (default 4 MiB). When an
`Output` would exceed it, the daemon marks the client `lagging`, sends `Lagging`, drops
queued `Output` for that terminal, and once the queue drains below a low-water mark encodes a
fresh snapshot and resumes streaming from its offset. The client resets its engine from the
snapshot (`decoder_ready` first so the screen is usable before history arrives). This bounds
bandwidth and memory for slow links; locally it should never trigger.

**Tested (M5):** `a_client_that_falls_behind_catches_up_from_a_snapshot` attaches a client
on the bare socket that stops reading while about 15 MB go by; it is told `Lagging`, gets a
fresh snapshot once it reads again, its screen then the live one, and the stream goes on
from the snapshot's offset with nothing missing.

Size negotiation: a terminal has one size. Policy, in order: the size of the client holding
control; else the smallest of the attached `control`-mode clients; observers never resize.
**As implemented (M5):** always the smallest `control`-mode client, whoever holds control
(`session/stream.rs`, `settle_size`); observers never resize.
Changes emit `Event{size}` so other clients can letterbox.

Multi-client: any number of clients per terminal. Every attached client receives the same
`Output`. Only the controller's `Input` is applied (5.6).

Versioning: `protocol` is a single integer; a daemon supports exactly one; `Welcome` is the
place to negotiate features (`features` bit set). Snapshot format follows the engine; both
sides must link the same Ghostty revision (CI check: compare `ghostty-vt-sys` build metadata).

### 5.5 Worktree scoping and defaults [decided]

- Scoping key: canonical worktree path (D6). Display: the branch name from `HEAD`
  (`refs/heads/x` → `x`), the folder name, the project (main checkout) name.
- Selectors accepted wherever a `worktree` is addressed (same words as Orca):
  `current` (the caller's cwd, resolved upward), `path:<path>`, `branch:<name>` (unique within
  the caller's project else error listing matches), `main` (the project's main checkout),
  `name:<folder>`.
- Caller's cwd: the stdio relay sends the agent's `cwd` in `Hello`; Streamable HTTP clients
  provide MCP `roots` (Claude Code sends the project root); the app sends the selected
  branch unit. If nothing resolves, `worktree` is required and the error says so.
- Terminal default (`terminal` omitted): the terminal this caller last acted on in the
  resolved worktree; else, if the worktree has exactly one terminal, that one; else an error
  listing the candidates. Never pick another actor's terminal implicitly.
- Worktree deletion (`crates/koyo/src/delete_worktree.rs`) calls `Close` for each terminal
  of that worktree after confirmation naming running processes. **As implemented (M5):**
  the dialog names each terminal with its status ("Its terminal stops: server (running,
  output)") and closes them in the daemon before git removes the folder, so none of their
  programs is left in it; the in-app check deletes a worktree with a running terminal and
  finds the daemon without it.
- **Not built (M5 audit):** the outside-worktree marker in the UI (the MCP reports
  `outsideWorktree`).
- Drift: when OSC 7 reports a cwd outside the terminal's worktree, `Event{cwd}` carries
  `outside_worktree: true`; the UI shows a subtle marker; MCP `terminal_list` shows it.

### 5.6 Control leases [decided]

Actors: `Actor { kind: human|agent|system, id, label }`. Human = the app or mobile client's
user. Agent id = `<client name>:<connection id>` (e.g. `claude-code:7f3a`), optionally with
the agent's own session id if the relay was launched with `KOYO_AGENT_SESSION`.

State per terminal:

```
Control {
  holder:    None | Human { since, mode: implicit|explicit } | Agent { actor, since, lease_until, reason? },
  observers: [actor...],
  queue:     [actor...]        // agents waiting for control, FIFO, informational
}
```

Rules:

1. Reads, snapshots, waits and `observe` attaches never need control.
2. `Input`, `Paste`, `Resize` by the controller, `Signal`, `Close` need control (or the
   `manage` scope for `Close`).
3. An agent acquires control with `Claim { takeover: false }` when `holder` is `None` or its
   own expired lease. Lease TTL default 120 s, renewed by every acting call and by `Renew`.
   A lease ends when the agent's connection closes (the relay exits when the agent exits).
4. `Claim { takeover: true, reason }` succeeds against another **agent**'s lease: the previous
   holder receives `Event{control: lost, by: actor, reason}`; its next acting call fails with
   `control_taken_by { actor, since, reason }` until it re-claims. Takeover is logged (5.8)
   and shown in the UI ("claude-code:7f3a took over from codex:12b0 · 'rerunning tests'").
5. The human pre-empts any agent, in either of two ways (D11): **implicitly**, the first
   keystroke, paste or mouse input a human sends to the terminal in the app (or on mobile)
   takes control at once; **explicitly**, through "Take control" in the terminal tab.
   Both end the agent's lease immediately and emit `Event{control: human_took, mode:
   implicit|explicit}`. While `Human` holds, every agent acting call fails with
   `person_has_control { since, mode, label }`, and `Claim { takeover: true }` is refused the
   same way: an agent can never take a terminal back from a human. **There is no automatic
   release.** The hold lasts until the human chooses "Give back" (or closes the terminal),
   which sets `holder = None` and emits `Event{control: human_released}`. Only then may an
   agent claim again. `Wait { for: control }` is still available for agents the human has
   asked to stand by, but the guide (8.7) tells agents not to use it on a terminal a human
   took over; they open or pick another terminal in the worktree instead.
   An implicit takeover shows a toast in the app ("You took over `tests` from claude-code:7f3a")
   with a "Give back" action, so an accidental keystroke is one click to undo.
6. `Release` by the holder, or `Close`, clears control.
7. Observers see every `Output`; the UI lists them ("watching: qa-worker:3c1").

Events: `control: claimed|released|lost|taken|human_took{mode}|human_released|expired`.

As implemented [2026-10-31, `termd/src/session/control.rs`, tests in
`termd/tests/daemon/control.rs`]:

- Typing, pasting or signalling in a terminal **no one holds** takes nothing: the person
  just uses it, and an agent may still claim it. Only input while an agent holds is the
  implicit takeover (7.3). "Take control" (a person's `Claim`) holds the terminal whoever
  held it.
- A person's focus reports (`CSI I`, `CSI O`, sent when the view gains or loses focus in a
  program that asked for them) and wheel reports (SGR buttons 64 to 67, in a program
  tracking the mouse) are delivered but take nothing and don't clear `needs_input`: looking
  at or scrolling a terminal isn't typing in it. Arrows the wheel sends in alternate scroll
  mode can't be told from typed ones and do take control.
- The app, mobile and `koyo-term` clients are the person (one actor, `human`); a relay
  is the agent `<client name>:<connection id>`.
- Every agent write (input, paste, resize, signal, close) is checked against its lease and
  renews it; the lease moves in lists at most once a second. A person's resize or close
  takes nothing from an agent; a person's signal is input and does.
- `control_taken_by` answers a displaced agent until it claims again, even after the taker
  let go; `person_has_control` answers every agent while a person holds, `Claim` with
  `takeover` included; `control_required` answers an agent that never claimed, and one whose
  claim met another agent's lease without `takeover`. Each names the holder in `details`.
- `Release` of something the caller doesn't hold changes nothing and answers the control
  state, so it's safe to repeat. `Claim`, `Release` and `Renew` all answer
  `Control { control }`.
- An agent's lease ends when its connection closes, attached to the terminal or not; a
  person's hold survives the app quitting. Neither survives a daemon restart, which ends the
  programs too (the registry clears control on load).
- `observers` lists the clients attached to observe. The `queue` is not kept yet.

Summary of who can take control from whom:

| holder → claimant | agent (plain claim) | agent (takeover) | human |
| --- | --- | --- | --- |
| none | yes | yes | yes |
| agent | no, `control_required` | yes, previous holder notified | yes, implicit or explicit |
| human | no, `person_has_control` | no, `person_has_control` | already holds |
| expired agent lease | yes | yes | yes |

### 5.7 Status derivation [decided]

Enum: `running | idle | needs_input | exited | unknown`.

Signals (engine callbacks on the daemon's headless terminal) → transitions:

| signal | meaning | effect |
| --- | --- | --- |
| OSC 133 `A` (prompt start) / `B` (prompt end, input begins) | shell at prompt | `idle`; `command_running = false` |
| OSC 133 `C` (command executed) | a command started | `running` |
| OSC 133 `D;<exit>` (command finished) | exit code known | `idle`, last_exit = code |
| `DESKTOP_NOTIFICATION` (OSC 9 / 777) | program asks for attention | `needs_input` until next input or next output burst longer than 2 s (configurable); event carries the notification text |
| `BELL` | attention | `needs_input` only if no output for 2 s afterwards (TUIs ring on errors too); always an event |
| `PROGRAM_STATUS` (OSC 7501) | explicit program status | map Ghostty's states directly (verify names against `terminal.h` at the pinned revision) |
| `PROGRESS_REPORT` (OSC 9;4) | progress bar | `running` with `progress: Some(pct|indeterminate)`, cleared on `remove` |
| `TITLE_CHANGED` | title | stored; agentic TUIs encode state in titles (e.g. "✳ Claude Code") so the title is exposed verbatim, never parsed |
| output flowing (> N bytes in the last 500 ms) and no OSC 133 info | TUI busy | `running` |
| screen stable for `stable_ms` (default 1500) and no OSC 133 info | TUI waiting | `idle` (the `tui-idle` heuristic from Orca/agent-terminal) |
| child exit | | `exited { code, signal }` |

The daemon emits `Event{status}` on every change with the reason, so clients and the MCP's
`wait` resolve from events, not polling. The heuristic timers run on the session thread.

Shell integration [implemented 2026-10-31, `termd/src/session/integration.rs`]: the daemon
injects Ghostty's scripts, which `ghostty-vt`'s `shell_integration::resources_dir()` writes out
once, by Ghostty's own mechanism: zsh through a `ZDOTDIR` hop (the person's `ZDOTDIR`, if any,
goes in `GHOSTTY_ZSH_ZDOTDIR` and Ghostty's `.zshenv` restores it before reading their files),
fish through `XDG_DATA_DIRS` (plus `GHOSTTY_SHELL_INTEGRATION_XDG_DIR`, which the script takes
back out). bash is not injected: Ghostty's script needs bash 4.4 and macOS ships 3.2. No
`GHOSTTY_SHELL_FEATURES` are set, so the scripts add marks and OSC 7 only, no title or cursor
changes. A terminal whose environment says `KOYO_SHELL_INTEGRATION=off` is left alone. zsh
reports its folder as `kitty-shell-cwd://host/path` (raw), which the daemon reads beside
`file://` (percent-encoded).

**License note (needs the owner's decision):** Ghostty's zsh and bash scripts say they derive
from Kitty's and are GPLv3 (fish's carries no such header). `ghostty-vt` embeds them in the
binary whether or not they are injected. They run as separate programs in the shell, but a
distributed Koyo carries their text, so their license has to be honoured or the scripts
replaced with Koyo's own (the marks are a few lines per shell; agent-terminal's zsh hook at
`cli/src/native/shell/agent-terminal.zsh:22-47` shows the size).

Status as implemented (`termd/src/session/status.rs`): reasons are `started`, `prompt`,
`command running`, `output`, `quiet`, `bell`, `notification: <title>`, `exited <code>`. With
marks seen the marks decide (between `133;C` and `133;D` it is `running` however quiet), so a
TUI run from an integrated shell reads `command running` and asks for the person only through
a bell or a notification; the output heuristics apply to terminals without marks. A bell at a
shell prompt (the shell answering the person's keys) asks for no one. `last_exit` comes from
`133;D`.

### 5.8 Activity, commands and screenshots logs [decided 2026-10-30]

Modelled on agent-terminal's store, which the owner considers well done
(`cli/src/native/store/mod.rs:42-160,264-340`, `cli/src/protocol/history.rs:25`,
`cli/src/protocol/attach.rs:8-14`, `cli/src/output/action/mod.rs:48`). Human input is stored
in full, as agent-terminal stores it (`HumanInput::Typed(String)`); this resolves O2.

Three append-only JSONL files per terminal under `terminals/<handle>/`, plus a
`screenshots/` folder. Lines are appended one at a time (agent-terminal's `append_line`) and
read by **byte offset** (`activity_since(offset)`): a reader passes the offset it reached and
gets only new complete lines back, a trailing partial line being ignored. The UI and the MCP
both read this way, so an idle log costs nothing.

**1. `activity.jsonl`**: one line per call that acted on or read the terminal, by agents and
humans alike in one time-ordered list, and one per control change. Shape = agent-terminal's
`ActivityEntry` plus the actor:

```
{ "time": "2026-10-30T14:22:05.006Z",                         // RFC 3339, milliseconds
  "actor": { "kind": "agent" | "human" | "system", "id": "claude-code:7f3a", "label": "Claude Code" },
  "command": "terminal_send --text \"pnpm test\" --enter",      // the call as a reproducible line
  "action": "typed \"pnpm test\" and pressed enter",            // what it did, in words
  "duration_ms": 12,
  "exit": 0,                       // 0 ok; agent-terminal's classes otherwise: 2 assertion failed,
                                   // 3 timed out, 4 a human has control, 1 other failure
  "error": "person_has_control",   // only when exit != 0
  "frame": 212,                    // the engine's frame counter when the call ran
  "request_id": "01J…",            // when the caller gave one
  "screenshots": ["screenshots/2026-10-30T14-22-05.006.png"],  // when the call took one
  "output": "…",                   // what the call returned, kept up to 64 KiB, then
                                   // "… N more bytes not kept" (agent-terminal's KEPT_OUTPUT_BYTES)
  "human": { "typed": "pnpm test" } | { "pressed": "enter" } | { "pasted": "…" }
           | { "mouse": { … } } | { "focused": true },         // human lines only
  "control": { "change": "taken", "from": "codex:12b0", "reason": "…" }   // control lines only
}
```

Rules copied from agent-terminal: refused calls are logged with their error so the human
sees what an agent tried (its dashboard's `AgentRefused`); a call still running is kept in
memory as a **pending** call (`time, command, action`) and shown after the finished ones as
"waiting", then replaced by its finished line; a human line reads `human: typed "hi"`, an
agent line `agent: read the screen`; `output` holds what the caller got back (a screen read,
a wait's matched text), so the log alone replays what an agent saw.

Action phrases are a port of `input_action`: `typed "…"`, `pressed ctrl+c`,
`pressed enter 3 times`, `pasted "…"`, `sent 14 raw bytes`, `sent SIGINT`, `clicked "Run"`,
`scrolled down 3`, `reported focus in`; plus `read the screen`, `read 2 KiB of output since
offset N`, `waited for "ready" (met after 1.2 s)`, `waited for idle (timed out after 25 s)`,
`opened zsh in feat-x`, `took control`, `took control from codex:12b0 ("rerunning tests")`,
`gave back`, `resized to 120x40`.

**2. `commands.jsonl`**: every shell command the terminal ran, whoever typed it, recorded
from the OSC 133 marks (agent-terminal only recorded its own `exec` calls; with semantic
prompts every command is captured). Shape = agent-terminal's `CommandRecord` + attribution
+ output:

```
{ "id": 17, "command": "pnpm test", "cwd": "/…/feat-x",
  "started_at_ms": 1793377725006, "ended_at_ms": 1793377733456, "duration_ms": 8450,
  "exit_code": 1, "first_frame": 212, "last_frame": 260,
  "by": { "kind": "agent", "id": "claude-code:7f3a" } | { "kind": "human", … } | null,
  "output": "…" }        // the region between 133 C and 133 D as text, same 64 KiB cap
```

`by` is the actor whose input ended with the Enter that ran it (the last writer before 133 C),
or `null` when unknown. Without shell integration the file stays empty. agent-terminal's
`history [--failed]` and `show <id>` become `terminal_log { kind: commands, failed }`.

**3. `screenshots.jsonl` and `screenshots/`**: `{ time, frame, path, by, width, height,
scale, cols, rows }` (agent-terminal's `ScreenshotEntry` with `Cells` flattened), one line
per PNG/SVG/HTML/text capture written by `terminal_screenshot` or by the app's "Save
screenshot".

Retention [proposed]: agent-terminal rotates nothing because its sessions were short-lived.
Koyo terminals live for weeks, so each JSONL file is capped at 64 MB and rotated once to
`.1` (older dropped), the screenshots folder at 512 MB oldest-first; both are settings.
"Export trace" zips a terminal's folder (agent-terminal's `trace export`) for bug reports
and QA evidence; "Clear log" in the UI only hides rows in the view (see 7.4), the files are
never truncated from the UI.

As implemented [2026-10-31; types and the reader in `term-control/src/log.rs`, the writer in
`termd/src/journal.rs`, the lines in `termd/src/session/{activity,commands,keys}.rs`, tests
in `termd/tests/daemon/logs.rs`]:

- `frame` is `offset` instead: the byte offset in the terminal's output stream when the line
  was written (`first_offset`/`last_offset` for a command's output). The daemon keeps no frame
  counter, and an offset is what a stream read resumes from.
- `human` is `{"typed": "…"}`, `{"pressed": {"key": "enter", "times": 3}}`, `{"pasted":
  "…"}` or `{"raw": 14}`, and only on a person's lines; an agent's input reads the same in
  `action` and `command` without it. Focus and wheel reports (5.6) aren't logged.
- Typing is one line per run of text, ended by a pause of 1 s, another key, another writer,
  or any other line; a key pressed again is counted (`pressed enter 3 times`). Refused writes
  are one line each, `tried to write: …`, with `exit` 4 for `person_has_control`, 1 otherwise.
- Lines are also written for the terminal opening (`opened /bin/zsh -i in feat-x`, by its
  opener) and starting again, the program exiting, signals, closes, an agent's resizes, and
  every control change (`took control from codex:12b0 by typing`, `gave back`, `the lease of
  codex:12b0 ended`).
- A command's text is read off the screen: Ghostty's zsh integration sends a bare `133;C`,
  so the daemon writes output to its engine cut at each mark and, at `133;C`, takes the
  soft-wrapped line above the cursor less the prompt's width at `133;B`. A multi-line command
  keeps its last line; a right prompt is dropped when the line reaches the screen's edge after
  a gap of three blanks or more. `by` is the last writer before `133;C`. `output` is the bytes
  between `133;C` and `133;D` with escapes removed, carriage returns and backspaces applied.
- Each append is announced as `Event::Logged { log, end }`, so a reader re-reads only then.
  A file past 64 MB is renamed to `.1` (the older `.1` dropped); `read_since` reports
  `rotated` when the file is shorter than the reader's offset and starts it over.
- Not yet: pending calls, and `request_id`, `output`, `screenshots` and `duration_ms` (filled
  by MCP calls, M4); `screenshots.jsonl` has its type and reader, and gets lines once something
  captures (the MCP's `terminal_screenshot`, the app's "Save screenshot"); the screenshots
  folder's cap comes with them.

### 5.9 Persistence and restart [proposed]

- `registry.json` holds every terminal's record minus live fields; written on every change.
  After a daemon restart the terminals are listed as `exited { reason: daemon_restart }` with
  their command, cwd and worktree, so the UI and agents can offer "run again" (`Open{respawn}`).
  Processes do not survive a daemon restart; this is true of every tool surveyed.
- Optional `snapshot.bin` per terminal, written on a timer (default every 30 s when dirty)
  and on shutdown, so the last screen and scrollback are readable after a restart. Guarded
  by the snapshot format version; discarded on mismatch.
- The ring and live state are memory only.

**As implemented (M5):** each session keeps `terminals/<handle>/snapshot.bin`
(`termd/src/session/persist.rs`): a format line naming the Ghostty commit, then the engine's
snapshot, written whole through a rename when the screen changed and 30 s went by, when the
program exits, and on `Shutdown` before the program is hung up; a file another engine wrote
is removed, and closing a terminal removes its file. After a restart, `ReadScreen` on a
remembered terminal answers from the file, and a respawn starts its engine from it, the
dead program's modes reset (alternate screen, mouse, bracketed paste, `DECSTR`) and a dim
line `[restored after the terminal daemon restarted]` under the old text. The app, bringing
terminals back as it starts, starts the daemon when the registry remembers terminals
(`TermDir::remembers_terminals`) and runs again the shells the person opened (no command,
created by the person); other ended terminals stay closed. Tests: the restart test reads
the kept screen and the respawned one; the in-app check stops the daemon under the app and
sees the shell back from its screen, idle at its prompt.

### 5.10 Security [decided]

- Socket mode 0600 in a 0700 directory; peer credentials checked (`SO_PEERCRED` /
  `LOCAL_PEERCRED`) to be the same uid.
- MCP over HTTP: loopback only; `Host` and `Origin` validated against DNS rebinding (as
  monocode `http.rs`); bearer token required for clients the app launched; interactive
  allow/deny in the app for unknown clients, grants stored by client name (monocode policy
  `external-control.v1`).
- Scopes (section 8.4) gate destructive operations; `Close` of a terminal with a live process
  asks the human unless the caller holds `destructive`.
- Remote transports (section 10) ride on SSH or an authenticated QUIC channel; the daemon
  never listens on a non-loopback TCP port itself.

---

## 6. Engine binding and use

- Crate: `ghostty-vt = "=0.8.0+ghostty.1.4.0-main.83fb956"` (and its sys crate), pinned
  exactly, in one workspace module `crates/term/src/engine/` that re-exports what the rest
  of the code uses. Nothing outside that module names `ghostty_vt` types directly, so a
  switch to `libghostty-vt` (Uzaaft) is a one-module change. Record the Ghostty commit the
  binding builds (`GHOSTTY.lock`) in this document when it changes.
- Build: the sys crate needs Zig 0.16 at build time (section 11). Prefer `GHOSTTY_SOURCE_DIR`
  pointing at a vendored, checksum-verified tarball under `.tools/` so clean builds do not
  hit the network (PaneFlow does this). Release profile must build the Zig side in
  `ReleaseFast` (Calyx lost a month to a missing flag).
- Threading: `Terminal` is `!Send`. In the daemon it lives on the session thread. In the app it
  lives on a dedicated **VT thread per terminal view** (or one VT thread multiplexing several
  terminals with a channel per terminal; start with one per view, measure). The GPUI side
  never touches the engine; it receives owned `Frame` values (section 7.2).
- API used (names per `ghostty-vt`; verify at the pinned version): `Terminal::new(opts)`,
  `vt_write`, `resize`, `reset`, `set_mode`/`mode`, callbacks `on_pty_write`, `on_bell`,
  `on_title_changed`, `on_pwd_changed`, `on_size`, `on_device_attributes`,
  `on_color_scheme`, `on_clipboard_write`, semantic prompt / notification / progress /
  program status / render hold callbacks (check which the binding exposes; add to the
  binding upstream or via the sys crate if missing), `RenderState` update + row/cell
  iteration with dirty flags, `key` and `mouse` encoders, `fmt` (text/VT/HTML) for MCP reads,
  `snapshot` encode/decode, `search`, `selection`.
- Queries: when the app is attached, the **app's** engine answers DA/DSR/color queries
  through `on_pty_write` → `Input`; the daemon's engine answers only when no client is
  attached. The daemon tracks "attached" per terminal to switch this (boo's rule).
- **As implemented (M5):** the engine is the `crates/term-engine` crate (11), not a module of
  `crates/term`; it pins `ghostty-vt = "=0.8.0"` and reads the Ghostty commit from the
  binding (`GHOSTTY_COMMIT`), which the daemon's hello carries and the app compares (5.1).
  The sys crate needs Zig 0.17.0 (`script/zig`). No `GHOSTTY.lock` or `GHOSTTY_SOURCE_DIR`
  offline source yet: a clean build fetches Ghostty's source. The daemon's engine answers
  every query, attached or not: the app asks for no `answers_queries` capability, since two
  windows on one terminal would answer twice (`term/src/client.rs`).
- **The app's theme:** the app tells the daemon its theme with `Theme`: its terminal colours
  and whether it's light or dark, the app's own setting rather than the system's. The client
  sends it on every connection and again when it changes. The daemon keeps it for terminals
  opened later and gives it to every engine. The engine answers OSC 10 and 11 with the
  colours and CSI ? 996 n with light or dark. It reports a change to a program that turned on
  mode 2031. New shells also get `COLORFGBG` (`0;15` light, `15;0` dark).

---

## 7. The in-app view: `crates/term`

### 7.1 Placement in the app [decided]

- A terminal is a tab in a branch unit (`PaneKind::Terminal`), and only a tab (owner,
  2026-10-31): the bottom panel holds no terminal and stays the stand-in it was.
- The branch unit's tab strip lists the daemon's terminals for that worktree; opening the
  tab attaches. Closing the tab **detaches** (the terminal keeps running); a close button with
  a modifier, or the tab's menu, offers "Stop terminal" (sends `Close`).
- The sidebar branch row shows a status roll-up: a dot per `needs_input` terminal and a count
  ("2 waiting"). Project rows roll up their branches.
- The window's "New Terminal" action opens a shell in the selected branch unit's worktree
  with the next free name (`zsh`, `zsh-2`, ...), per `model.rs`'s current default label.
- **As implemented (M5):** the terminal's bar has Stop (Close once the program ended), and
  ⌥-clicking a terminal tab's × stops its terminals; a busy program asks first, then the
  daemon closes the terminal and its tab goes when the daemon says so, from whichever
  client stopped it. A person may stop a terminal an agent holds. The tab has no menu yet.
  Branch rows show the waiting count; project rows don't roll up yet.

### 7.2 Render pipeline [decided]

1. The app's `TermClient` (one per app, on a background task) holds the daemon socket,
   demultiplexes frames per terminal, and forwards `Snapshot`/`Output` to the terminal's VT
   thread over a channel.
2. The VT thread feeds `vt_write`, updates `RenderState`, and when dirty (or a render hold
   ends, or the cursor blinks) publishes an owned `Frame`:
   `Frame { cols, rows, cells: Vec<Cell> (viewport only), dirty_rows: BitSet, cursor,
   selection, scrollbar, title, modes { alt_screen, mouse, kitty_keyboard, bracketed_paste },
   kitty_images: [...] }`. `Cell { grapheme: SmolStr or atom id, fg, bg, underline{style,color},
   flags: bold|italic|strike|wide|wide_spacer|inverse|blink|hidden, hyperlink: Option<id> }`.
   Frames are published at most once per display refresh; the thread coalesces.
3. `TerminalElement` (a GPUI element implementing `Element`):
   - layout: cell size from the font's advance and line height, grid = floor(size / cell),
     a resize request is sent to the daemon when the grid changes, one at a time (the latest
     once the last is answered), so a dragged pane resizes the program at every frame.
   - prepaint: for dirty rows build runs of equal background → quads; for each cell with a
     grapheme, look up a shaped glyph in a cache keyed `(font_id, grapheme, bold, italic)`
     (shape through GPUI's text system once, store glyph ids and offsets), and queue
     `paint_glyph` at `(col * cell_w, row * cell_h + baseline)`; wide graphemes paint once
     and skip the spacer cell; box drawing (U+2500–257F), blocks (2580–259F), braille
     (2800–28FF), Powerline (E0B0–E0D4) and Legacy Computing (1FB00–1FBFF) are drawn
     procedurally as quads/paths at device-pixel size (port of Ghostty's `src/font/sprite`,
     MIT), never from the font, so they join seamlessly.
   - paint: backgrounds, then selection, then glyphs, then underlines/strikes, then cursor
     (block/bar/underline/hollow when unfocused), then kitty images (later).
   - Rows that are not dirty are **not re-shaped**; `gpui-fast`'s retained views keep the
     element from being rebuilt when nothing changed.
4. Fonts: the app's monospace font setting; fallback chain through GPUI's text system;
   emoji through the system color font; ligatures remain cell-pinned (each cluster is shaped
   alone, so cross-cell ligatures are not formed, matching Zeron's choice; a later option can
   shape runs and split advances).
5. Scrollback: the engine owns it. Scrolling moves the engine's viewport (`scroll_viewport`)
   on the VT thread; the element shows a scrollbar from `scrollbar()`. Mouse wheel in alt
   screen with mouse tracking off sends arrow keys (Ghostty behaviour).
6. Selection and copy: engine selection APIs (`select_word`, `select_line`, drag) on the VT
   thread; copy uses `fmt` to produce text; OSC 52 writes go through `on_clipboard_write` →
   app clipboard policy (ask once per terminal, as monocode's `clipboard_approval`).
7. Hyperlinks (OSC 8) and URL detection: hover underline + Cmd-click, via the engine's
   hyperlink ids and `search`.

### 7.3 Input [decided]

- Keyboard: GPUI `KeyDownEvent` → engine key encoder configured from the terminal's modes
  (`key_encoder_setopt_from_terminal`), producing legacy, modifyOtherKeys or kitty-protocol
  bytes → `Input`. IME/composition through GPUI's `InputHandler` (the editor already does
  this). macOS Option-as-Meta is a setting.
- Mouse: engine mouse encoder when the program tracks the mouse; otherwise selection.
- Paste: bracketed when mode 2004 is on; large pastes are chunked; a paste with control
  characters asks first (Ghostty's unsafe-paste rule).
- Focus and control: the first keystroke, paste or mouse input while an agent holds the
  lease is the implicit takeover of D11. The input is delivered (the human's intent wins),
  the agent's lease ends, and a toast names the displaced agent with a "Give back" action.
  Merely focusing the tab, scrolling or selecting text does **not** take control; only input
  that reaches the PTY does. The explicit path is the controller chip's "Take control".

### 7.4 Controller badge and activity [decided]

- The terminal tab shows: status dot, title, controller chip ("you", "claude-code:7f3a",
  none) and observer count. The chip's menu: Take control (when an agent or nobody holds) /
  Give back (when you hold) / Show activity. A human-held terminal shows the chip as "yours"
  with a distinct tint so it is obvious agents are locked out of it.
- As implemented [2026-10-31, `koyo/src/terminals/{states,watch,header}.rs`]: the app keeps
  every terminal's status and control from one `ListTerminals` and then the daemon's events
  (all clients get every terminal's events), listing again after the daemon comes back. A
  tab with terminals shows a dot for the most urgent of their statuses (needs input, then
  running, idle, exited: amber, green, grey, red). Over a terminal pane a 26 px bar shows the
  dot, the status and its reason, "watching: N" when observers are attached, the holder as a
  chip ("yours" in the accent, an agent's label in amber, "no one controls it" in grey) and
  one button: "Take control" or "Give back", sent to the daemon from the click. The toast of
  an implicit takeover is a notice in that bar, "You took over from claude-code", for 12 s
  or until control changes again; the chip's "Give back" undoes it. The sidebar shows beside
  each branch how many of its terminals need input, in both layouts. A chip menu and "Show
  activity" come with the panel below.
- The log history is a **toggleable side panel** beside the terminal (owner's requirement,
  2026-10-30): a header button and a shortcut (⌘⇧L proposed) show or hide it; the state is
  remembered per terminal; new terminals follow Settings → Terminals → "Show the activity
  panel in new terminals" (default off, so a plain terminal looks like a plain terminal).
  Its width is draggable and remembered (agent-terminal starts at 30%). While hidden it holds
  no subscription and reads nothing.
- The panel's tabs mirror agent-terminal's dashboard (`dashboard/src/lib/side-panel.ts`,
  `components/panels.tsx`): **Activity** (the merged human/agent call list from
  `activity.jsonl`), **Commands** (shell command history from `commands.jsonl` with exit
  codes, a "failed only" filter, and jump-to-output), **Screenshots** (thumbnails, lightbox),
  **Output** (the plain text stream since a mark the user sets), **Info** (handle, name,
  worktree, cwd, pid, size, modes, `TERM`, controller, observers, attached clients).
- Activity list behaviour copied from `dashboard/src/components/activity-panel.tsx` and its
  tests: each row shows time, an `agent:` or `human:` prefix with the actor's label, the
  action, the duration and the result; pending calls come last, marked "waiting"; a row
  expands to its output (and to the command line for agent calls; human rows show only what
  they printed, never a command); each kind of call has its own look (read, input, wait,
  control change, refused); Follow sticks the list to the newest row, scrolling away stops
  following, "Jump to latest" resumes; "Clear" hides the rows so far in this view only and
  says "Activity cleared" until new rows arrive; a screenshot a call took opens in the
  lightbox from the row.
- The same panel exists on mobile later, reading the same files through the same offsets.
- As implemented [2026-10-31, `koyo/src/terminals/{panel,beside}.rs`, `panel/rows.rs`]:
  the bar's "Activity" button and ⌘⇧L (bound in the terminal's key context, so editors keep
  the key) show or hide a terminal's panel at the pane's right, 30% of it and dragged by its
  left edge between 15% and 70%; the width is shared by every panel while the app runs.
  Hidden, the panel is dropped, so nothing reads or listens. Opened, it reads each log's
  newest 2 MB (`log::tail_start`, then `read_since` to the end) and then reads on from its
  offsets whenever the daemon announces a `Logged` append; a tab keeps its newest 2000 rows,
  and a rotated log starts over. Activity, Commands (with "Failed only") and Info are as
  above; rows expand by a click. Follow, "Jump to latest" and Clear work as in the dashboard.
  Output sets its mark with "Set mark" and reads on once a second while the tab is shown,
  through the daemon's `ReadStream` (below), keeping the newest 256 KiB. Screenshots shows
  thumbnails from `screenshots.jsonl`; nothing writes it until the MCP's screenshot tool
  exists. Not yet: the setting for new terminals, remembering the panel and its width across
  restarts, jump-to-output from a command, the lightbox, pending "waiting" rows and a look
  per kind of call.
- `ReadStream { handle, since?, max_bytes? }` answers `Stream { text, next, oldest,
  truncated }`: the ring's bytes from `since` as plain text (escapes stripped, a carriage
  return starting its line over), or its newest `max_bytes` (64 KiB unless asked, 1 MiB at
  most) starting at a line when `since` is absent. A read the limit cuts ends after its last
  whole line, and no read ends inside an escape sequence or a UTF-8 character, so the next
  starts clean. `truncated` says output between `since` and `oldest` is gone. Anyone may
  read; it is the MCP's `terminal_read` stream mode too.

### 7.5 Performance budget [decided]

- Local keystroke to glyph: ≤ 1 frame at 120 Hz plus PTY latency; measure with a
  `crates/bench` scenario (type 1000 chars, measure frame times) and keep it in the
  bench suite like the editor scenarios.
- `cat` of a 150 MB file: the view must stay responsive (frames ≤ 16 ms) while throughput
  is bounded by the engine (Ghostty reports 566 ms for 150 MB on its own stack; ours adds a
  socket hop and a second parse). Target: within 1.3 times the time a bare PTY takes to
  carry the same file, read as fast as macOS allows, measured in the same run. The 1.5 s
  first written here is met on an idle M4 Max (about 1 s) but depends on the machine; the
  relative target doesn't.
- Idle: zero CPU when nothing changes (no timers besides cursor blink, which pauses when
  the window is hidden).
- Memory: engine scrollback limit default 50 MB per terminal (Ghostty's default), compressed
  incrementally during idle (`compress` incremental mode on the VT thread).

**As measured (M5):** `koyo-bench` runs `term-echo` and `term-cat` (`crates/bench/README.md`):
the daemon, a client attached as the view attaches, and its engine; each sample ends in the
client's engine, at the frame the view would take, the paint not timed. Release build, Apple
M4 Max:

| | p50 | p95 | max | budget |
| --- | --- | --- | --- | --- |
| `term-echo`: a key into `/bin/cat`, its echo in the client's engine (1,000) | 0.09 ms | 0.14 ms | 0.23 ms | p95 < 8.3 ms, met |
| `term-cat`: 150 MiB `cat`, until the line after it is in the client's engine (5) | 955 ms | 966 ms | 966 ms | p50 within 1.3 times a bare PTY (800 ms), met |

A bare PTY, `cat` in it and its master read with nothing parsed, as fast as macOS allows, is
what no terminal beats. macOS hands the master a kilobyte a read, whatever the read size, baud
rate or setting (`kern.tty.ptmx_max` only sets how many PTYs there can be); how fast those
reads go depends on the reader never sleeping while the program writes. Read blocking, it
carries 122 MiB/s on an idle M4 Max; non-blocking and reading again on an empty queue before
waiting, as Ghostty does, 183 to 192 (a C program, `cat` of the same file). `term-cat` reads
its bare PTY that second way, before each of its samples, so a busy machine slows both alike,
and holds the median sample to 1.3 times the median bare one. The daemon's path takes 1.14 to
1.25 times it: 145 to 163 MiB/s, where a blocking reader made it 120 (the same runs). The rest
is the PTY itself: the daemon's reader drains it at about 160 MiB/s, its `cat` refilling the
queue more slowly than the bare one's while the daemon and the client parse alongside
(skipping the daemon's parse gives 3 to 4% back). Ghostty 1.4.0's 566 ms for 150 MB, 253
MiB/s, wasn't matched by any reader here. Machine load moves every one of these numbers by
half or more (another app's background work held a bare PTY at 73 MiB/s for days); only
ratios taken in the same run compare. The longest the client's engine takes over one output
frame is under a millisecond, so a view's thread is never held for anything near a frame.
The prompt scanner finds OSC 133 marks by jumping from one ESC to the next (`memchr`); read
byte by byte it cost the session thread as much as the VT parse. Idle CPU isn't measured
yet: each terminal's thread in the daemon wakes four times a second for its timers (keeping
its screen, ending leases), whether or not anything happened.

---

## 8. The MCP: `crates/term-control`

### 8.1 Shape and placement [decided]

- A `term-control` library crate defines the **operation catalog**: for each operation its
  name, title, argument schema (JSON Schema via `schemars`), result type, scope, hints
  (read-only / destructive / idempotent), and an executor trait. Both the daemon and the
  relay depend on it; the app depends on it for its own UI actions over the socket.
  Pattern: monocode `crates/monocode-control/src/{operation.rs,command.rs,scope.rs,topic.rs}`
  and `crates/monocode-mcp/src/tools.rs` (tools generated from the catalog, no per-tool code).
- MCP server implementation: `rmcp` (monocode uses 3.7) inside the daemon, serving protocol
  revisions 2025-11-25 (sessions, SSE) and 2026-07-28 (stateless, `subscriptions/listen`).
- Transports:
  1. **Streamable HTTP** on `127.0.0.1:6272/mcp` (6273 for development data; port is a
     setting; distinct from monocode's 6262/6263 so both can run). Off by default; the app's
     Settings → Terminals turns it on. Host/Origin checks; bearer token.
  2. **stdio relay**: `koyo mcp` (a subcommand of the app binary, or a tiny
     `koyo-term-mcp` binary) that forwards MCP over the daemon socket. It sends the
     caller's cwd, pid and `KOYO_AGENT_*` env in `Hello`. This is what Koyo injects
     into agents it launches (Claude `--mcp-config`, Codex `-c mcp_servers…`, ACP
     `mcpServers`, as monocode does in `monocode-core/src/app_mcp.rs`).
- Federation (later): app-level tools (browser, editor, workspace) register in the same
  catalog with an executor that forwards to the app's control socket; when the app is not
  running they fail with `app_not_running`. Agents then configure one server.
- **As implemented (M4):** the MCP server runs in `koyo mcp`, not inside the daemon.
  The relay carries out the catalog itself (`term-control/src/exec.rs`) over its ordinary
  protocol connection, as a `Relay` client whose actor is the MCP client's name from
  `initialize` (`claude-code:…`), and serves it with `rmcp` 3.8 over stdio
  (`crates/term-mcp`). The daemon keeps one protocol and no MCP code; the relay runs as
  the user, which is the trust the socket already checks (peer uid), so grants are
  enforced there. The connection is made on the first call and made again when the daemon
  went away. Streamable HTTP is deferred: it would serve the same handler from the daemon
  or the app. Tools are listed from `CATALOG`, filtered by the client's grants
  (`--scopes`, default read + write + manage). A result is the summary line, the JSON as
  text for clients that read only text, and `structuredContent`; an error is an `isError`
  result whose structured content is `{ code, message, retryable, details }`.
  `terminal_resize` and `terminal_screenshot` aren't in the catalog yet.

### 8.2 Naming and conventions [decided]

Copied from monocode (`docs/native/mcp.md` there):

- Tool names `snake_case` `noun_verb`, `[a-z0-9_]{1,64}`; sentence-case titles; honest hints;
  `open_world: false`.
- Arguments camelCase; unknown fields rejected.
- Every mutating tool accepts `requestId` (`^[A-Za-z0-9_-]{1,128}$`); a repeated id with the
  same payload against the same terminal incarnation returns the stored receipt, never
  re-sends (Orca's `--retry-request` semantics).
- Results: structured content plus a one-line text summary.
- Errors: `{ code, message, retryable, details? }`. Codes: `worktree_required`,
  `worktree_not_found`, `ambiguous_worktree`, `terminal_not_found`, `ambiguous_terminal`,
  `name_taken`, `person_has_control`, `control_taken_by`, `control_required`, `not_attached`,
  `exited`, `timeout`, `invalid_argument`, `scope_denied`, `confirmation_required`,
  `daemon_version_mismatch`, `app_not_running`.
- Lists page with `cursor`/`limit`.

### 8.3 Caller identity and defaults [decided]

- Identity: `clientInfo.name` + connection id → `Actor`. Relay callers add cwd and pid;
  app-launched agents add `KOYO_AGENT_SESSION`.
- Default `worktree`: section 5.5 (relay cwd, MCP roots, else required).
- Default `terminal`: section 5.5 (last acted on in that worktree, else the only one, else
  error listing candidates).

### 8.4 Scopes and grants [decided]

Scopes: `terminal.read` (list, read, wait, log, screenshot), `terminal.write` (open, send,
claim, release, resize, rename), `terminal.manage` (close exited terminals, signal),
`terminal.destructive` (close a terminal with a live process, close all in a worktree,
shutdown). Defaults for app-launched agents: read + write + manage. External clients: a
first-connection Allow/Deny prompt in the app (name, pid, requested scopes), stored per
client name; destructive always confirms in the app unless granted.

**As implemented (M4):** grants are the relay's `--scopes` (default read + write + manage);
tools outside them aren't listed, and calling one answers `scope_denied`. Closing a terminal
whose program runs answers `confirmation_required` unless `terminal.destructive` was
granted. The app's first-connection Allow/Deny prompt for external clients isn't built:
the relay runs as the user, who could reach the socket directly, so the prompt would be
consent rather than a boundary; it waits on the app's settings for external clients.

### 8.5 Tool catalog [decided; schemas proposed]

```
terminal_list
  { worktree?: selector, project?: path, status?: [running|idle|needs_input|exited],
    labels?: [string], cursor?: string, limit?: int (default 50) }
  → { terminals: [TerminalSummary], nextCursor? }
  TerminalSummary = { terminal: handle, name, title?, worktree: { path, branch?, project },
    cwd, outsideWorktree: bool, command: [string], status, statusReason, lastExit?,
    progress?, controller?: Actor, observers: int, attachedClients: int, lastOutputAt,
    createdAt, createdBy: Actor, labels, pinned: null | { cols, rows, by: Actor },
    preview: string (last non-empty screen line) }

terminal_open
  { worktree?: selector, name: string, command?: [string] | string, cwd?: path,
    env?: {k:v}, size?: {cols, rows}, labels?: [string],
    ifExists?: "attach" | "error" (default attach), respawn?: bool (default false),
    reveal?: "none" | "tab" | "focus" (default "tab": a tab appears without stealing focus),
    claim?: bool (default true: take control if free), requestId? }
  → { terminal: TerminalSummary, existed: bool, control: ControlState,
      pinned?: null | { cols, rows, by }, pinRefused?: { code, message } }
  // `size` pins that size while the relay's connection lasts (`pinned`; `pinned: null` and
  // `pinRefused` when the lease refused it); with `claim: false` it's only the starting size

terminal_read
  { terminal?: handle, mode?: "screen" | "stream" | "scrollback" (default screen),
    format?: "text" | "ansi" | "html" (default text),
    cursor?: int (stream only: byte offset from a previous read), limit?: int (stream: max
    bytes, default 64 KiB; scrollback: max rows), rows?: "a:b" (screen row range),
    foldBlank?: bool, withRefs?: bool (heuristic @ref markers for panels/buttons, like
    agent-terminal `snapshot -i`; off by default) }
  → screen: { source: "screen", header: { name, title, cols, rows, altScreen, mouse,
      kittyKeyboard, cursor: {row, col, visible, shape}, frame: int, status }, lines: [string],
      cursorLine? }
    stream: { source: "stream", text (escape sequences stripped), nextCursor, oldestCursor,
      truncated }
    scrollback: { source: "scrollback", lines, totalRows, scrollbackRows }
  Notes copied from Orca: stream shows repaints as stacked fragments; prefer screen when the
  answer depends on how output looks. `cursor` is only for stream.

terminal_send
  { terminal?: handle, text?: string, keys?: [string] (named keys: "enter", "ctrl+c",
    "escape", "up", "tab", "shift+tab", "f1".."f12", "ctrl+shift+p"…; encoded by the
    engine's key encoder for the terminal's current modes), paste?: string (bracketed),
    enter?: bool (append Enter after text), interrupt?: bool (SIGINT-equivalent ctrl+c),
    then?: { wait?: WaitSpec, read?: ReadSpec }   // act, wait and read in one call
    requestId? }
  → { accepted: true, bytes, control: ControlState, submission?: "observed" | "unobserved",
      wait?: WaitResult, read?: ReadResult }
  `submission` is "observed" when OSC 133 C (or a status change to running) followed within
  the `then.wait` window; otherwise "unobserved" (Orca separates acceptance from submission).

terminal_wait
  { terminal?: handle, for: "idle" | "running" | "prompt" | "exit" | "needs_input" |
    "text" | "gone" | "stable" | "control" | "status_change",
    pattern?: string (regex for text/gone), rows?: "a:b", stableMs?: int (default 1500),
    timeoutMs?: int (default 25000, max 600000), read?: ReadSpec }
  → { met: bool, condition, elapsedMs, status, lastExit?, match?: { row, col, text },
      read?: ReadResult }   // timeout returns met:false, not an error, unless assert:true

terminal_claim     { terminal?: handle, takeover?: bool, reason?: string, requestId? }
                   → { control: ControlState }  // errors: person_has_control, control_taken_by
terminal_release   { terminal?: handle } → { control }
terminal_control   { terminal?: handle } → { control, queue, activityTail: [..5] }
terminal_resize    { terminal?: handle, cols, rows } | { terminal?, fill: true },
                   then?: { wait?: WaitSpec, read?: ReadSpec }, requestId?
                   → { size: { cols, rows }, pinned: null | { cols, rows, by },
                       wait?: WaitResult, read?: ReadResult }
                   // pins the size for every viewer while the relay's connection lasts;
                   // without `then` waits for a still screen (300 ms, 25 s) and reads it
                   // (`docs/responsive.md` 3.1, 4.2)
terminal_rename    { terminal?: handle, name?: string, title?: string, labels?: [string] }
terminal_signal    { terminal?: handle, signal: "int" | "term" | "hup" | "kill" | "tstp" | "cont" }
terminal_close     { terminal?: handle, worktree?: selector, all?: bool, force?: bool, requestId? }
                   → { closed: [handle], confirmed: bool }
terminal_log       { terminal?: handle, kind?: "activity" | "commands" | "screenshots" (default activity),
                     cursor?: int (byte offset returned by a previous call; omit for the tail),
                     limit?: int (entries, default 200), failed?: bool (commands only) }
                   → { entries: [...], pending: [...] (activity only), nextCursor: int, oldestDropped: bool }
terminal_screenshot{ terminal?: handle, format: "png" | "html" | "ansi" }
                   → png only when the app is attached (rendered by the app); html/ansi from
                     the daemon's formatter
worktree_list      { project?: path } → { worktrees: [{ path, branch, project, main,
                     terminals: int, needsInput: int }] }
```

`ControlState = { holder: null | { kind, id, label, since, leaseUntil?, reason? },
observers: [Actor], youHold: bool }`.

**Not built (M5 audit)**, against `term-control/src/catalog/args.rs`: `terminal_list` has no
`project` and `worktree_list` takes no arguments; `terminal_read` has `screen` and `stream`
but no `scrollback` mode, no `format` (text only) and no `withRefs`; `terminal_open` has no
`reveal` (the app reveals every agent's terminal, 9); `terminal_close` closes one terminal,
with no `worktree`, `all` or `force`; `terminal_screenshot` is missing (8.1).
`terminal_resize` is built (2026-11-02, `term-control/src/exec/resize.rs`) as the daemon's
`Pin` (`docs/responsive.md`); `terminal_open`'s `size` pins that size for the agent that
claims it, and is only the starting size with `claim: false`. `requestId` is kept by the relay, not the daemon (5.4's input ids).

### 8.6 Resources and subscriptions [decided]

```
koyo://worktrees                          list, with counts
koyo://worktrees/{path}/terminals         TerminalSummary[]        (template)
koyo://terminals/{handle}                 TerminalSummary + control + size
koyo://terminals/{handle}/screen          text screen (format text)
koyo://terminals/{handle}/activity        last 200 activity entries
```

All but `worktrees` are subscribable; updates are `notifications/resources/updated` without
content (client re-reads). The daemon batches dirty topics every 100 ms into bounded
per-connection queues (monocode `hub.rs`). Screen updates are rate-limited to 4/s per
subscriber. MCP `logging` notifications carry `Event{status|control}` for clients that
prefer a push without a re-read.

**As implemented (M4):** the relay serves these resources (`term-mcp/src/resources.rs`;
`{path}` percent-encoded), lists the worktrees plus each terminal of the caller's worktree
and its screen, and takes subscriptions both as `resources/subscribe` and as
`subscriptions/listen` streams. The daemon's events on the relay's own connection drive
the updates, gathered for 100 ms and said once each (`term-mcp/src/watch.rs`); a screen
has no event, so a watched one is looked at four times a second and said when its frame
number moved since the subscription. Opening or closing a terminal says the resource
list changed. The `logging` push isn't sent: SEP-2577 deprecates logging notifications.

### 8.7 Server instructions and the agent guide [decided]

The MCP `instructions` field and a `terminal_guide` prompt ship **inside the daemon binary**
so they match its version (agent-terminal ships guides in the binary: `skills get core`).
Content: the loop (open → read screen → send with `then.wait` → read), prefer `wait for:
text|prompt|idle` over fixed sleeps, never poll faster than 1/s, claim before writing, how
takeover between agents behaves, how to scope by worktree, how to read stream vs screen,
and the human rule: **`person_has_control` means the person has taken this terminal; do not
wait for it, do not retry, do not claim with takeover; read it if you need its output, and
open or pick another terminal in the worktree for your own work. Return to it only after a
`human_released` event or when the person tells you to.**
A Claude skill in `~/.claude/skills/koyo-terminals` is a two-line stub pointing at the
prompt (so the guide is never stale).

**As implemented (M4):** both ship in the app binary that runs the daemon and the relay
(`term-mcp/src/guide.rs`): the instructions say the loop, waits and the human rule in a
few lines, and `terminal_guide` says it at length (waits, full-screen programs and keys,
control and takeover, logs, errors and `requestId`). The skill stub isn't written: it
lives in the person's home folder. Accepted from the official TypeScript SDK's client
(1.32): tools, a zsh driven through `less` by keys, resources, a subscription's update,
the guide, and the refusals, against `koyo mcp` from the built app.

### 8.8 App-side consumption

The app uses the same operations over the socket (not over MCP) for its own UI, with
`Actor{kind: human}`. The catalog is the single definition of what a terminal can do.

### 8.9 Vocabulary mapping to Orca and agent-terminal

| Orca CLI | Koyo MCP |
| --- | --- |
| `terminal list --worktree <selector>` | `terminal_list { worktree }` |
| handles `term_<uuid>` | handles `term_<ulid>` |
| `terminal read` (stream) / `--screen` / `--cursor` | `terminal_read { mode: stream\|screen, cursor }` |
| `terminal send --text --enter --interrupt --wait-submit --retry-request` | `terminal_send { text, enter, interrupt, then.wait, requestId }` |
| `terminal wait --for exit\|tui-idle` | `terminal_wait { for: exit\|idle }` |
| `terminal create --worktree --title --command --focus` | `terminal_open { worktree, name, command, reveal }` |
| `worktree current` | `worktree: "current"` |

agent-terminal's `wait` conditions (`Text`, `Gone`, `Stable`, `Idle`, `Exit`, `Command`) map
onto `for: text|gone|stable|idle|exit|prompt`; its `snapshot -i` refs onto
`read { withRefs }`; its `wait --control` onto `wait { for: control }`.

---

## 9. Human and agent UX in Koyo

- Opening a branch unit shows its terminals as tabs with status dots; a terminal an agent
  opened appears automatically (reveal `tab`) without stealing focus. **As implemented
  (M5):** an agent's terminal gets a tab in its checkout as the daemon says it opened, if
  the checkout is listed, without the focus; `reveal` is not a parameter of
  `terminal_open` yet, so every agent's terminal is revealed.
- The human can watch an agent drive a terminal (observer), take it over by typing or by
  "Take control" (D11), and later "Give back". A terminal the human took stays theirs: the
  displaced agent is told, stops using it, and works in another terminal of the branch. An
  accidental takeover is undone with the toast's "Give back".
- "Needs input" terminals bubble up: sidebar count, a dock badge later, and an optional
  notification routed from OSC 9/777 with the terminal and branch in its title.
- Terminals are not closed by closing tabs or the window. Quitting the app shows nothing
  special; the daemon keeps everything. A "Stop all terminals of this branch" exists in the
  branch menu, with confirmation listing processes. **As implemented (M5):** "Stop
  Terminals…" in the checkout row's menu, shown while it has terminals, asks with each
  terminal's name and status.
- Settings → Terminals: font, scrollback, MCP HTTP on/off and port, external client grants,
  "Show the activity panel in new terminals", log and screenshot retention caps, "Export
  trace", `TERM` choice.
- **Not built (M5 audit):** the Settings → Terminals page, system notifications from OSC
  9/777, and the dock badge.

---

## 10. Mobile and remote path [proposed]

1. Transport first: SSH port-forward or Tailscale to the daemon's socket (an `ssh -L` to a
   loopback TCP listener the daemon can optionally open for this; token-authenticated).
2. Then QUIC (quinn) with connection migration for roaming, mutual TLS with keys paired
   through the desktop app; the daemon still never listens publicly without pairing.
3. The mobile client (Swift, libghostty-vt via GhosttyKit xcframework; iOS is a Ghostty 1.4 CI
   target) speaks frames v1, decodes the same snapshots, renders natively, and uses the same
   status and control model. The catch-up snapshot rule keeps bandwidth bounded.
4. Zellij's web client and tmux control-mode iOS apps were considered and rejected for the
   same reasons as D1.

---

## 11. Crate layout, build and toolchain [decided]

```
crates/term-engine    libghostty-vt behind one module: Engine, owned Frames, key/mouse/focus
                      encoding, selection, snapshots, the OSC 133 scanner
crates/term-control   wire types (frames v1, CBOR control frames), selectors, errors, the
                      blocking client Connection, the daemon's folder; later the catalog
crates/termd          the daemon, a library (`koyo_termd::main`) and the `koyo-termd`
                      binary: registry, sessions, PTY, headless engine, later the MCP server;
                      also the `koyo-term` command line for testing without the app
crates/term           the app side: TermClient, the VT thread, TerminalElement, TerminalView,
                      key mapping, glyph cache, sprites
crates/koyo        wires TerminalView into tabs (`terminals.rs`); the
                      `koyo termd` subcommand; later `koyo mcp`, the settings page and
                      launching agents with the relay injected
```

- Zig: pin a Zig version per the binding's requirement (0.16.x for `ghostty-vt` today) into
  `.tools/zig-<version>/` with a sha256 check, via `script/zig` modelled on `script/dev`'s
  `dx` download. `script/check`, `script/lint` and `script/dev` export `ZIG` to it. CI does the
  same. Keep the Ghostty source tarball the sys crate builds under `.tools/ghostty-<commit>/`
  (checksum from `GHOSTTY.lock`) and point `GHOSTTY_SOURCE_DIR` at it so builds are offline.
- Terminfo: bundle Ghostty's `xterm-ghostty` terminfo (MIT) into the app resources and have
  the daemon install it under `settings::data_dir()/term/terminfo/` with `TERMINFO_DIRS` set
  for children; set `TERM=xterm-ghostty` only when present. Fallback `xterm-256color`.
- Lints: new crates use `[lints] workspace = true`; no `unwrap`/`expect`/`panic`; errors are
  typed per crate; logging through `log`.
- Dependencies to add (versions to lock at implementation): `ghostty-vt` (pinned), `portable-pty
  0.9`, `rmcp` (monocode: 3.7), `schemars`, `serde`, `serde_json`, `ulid`, an async runtime for
  the daemon (`smol`, already in the lockfile via GPUI; or `tokio` for `rmcp`'s HTTP; monocode
  runs rmcp's HTTP on its own thread with a current-thread tokio runtime), `rustix` for
  `setsid`/`flock`/peer creds (already in the lockfile).

---

## 12. Testing strategy [proposed]

- `crates/termd` integration tests: start a daemon in a temp dir under the checkout
  (`.superpowers/test-home/`), open a terminal running a tiny fixture program (`printf`
  loops, a program that emits OSC 133/9/7501 sequences, a program that reads input), attach
  two clients, assert: output offsets are contiguous, attach replays a snapshot equal to the
  live screen (decode and compare row text), catch-up triggers at the queue bound and the
  resumed state matches, control rules (claim, takeover event, human pre-emption, lease
  expiry on disconnect), status transitions per 5.7's table, registry survives restart.
- Protocol golden tests: frame encode/decode round trips; fuzz the decoder.
- Engine conformance: feed the same VT corpus (Ghostty's and vtebench's) through the daemon
  and app engines and compare formatted text; detect binding drift at upgrade time.
- MCP contract tests: the catalog's JSON schemas vs this document's tables (monocode has
  `crates/monocode-mcp/tests/docs.rs` doing exactly this; copy the idea).
- App: headless GPUI tests for `TerminalElement` layout (grid size from bounds, cell-pinned
  glyph positions, wide-cell handling), visual checks via the existing `visual-check` feature,
  bench scenarios in `crates/bench` (typing latency, `cat` throughput, idle CPU).
- Real-surface QA with `agent-terminal`/`agent-tui` driving Claude Code inside a Koyo
  terminal, and with the MCP from a real Claude Code session in a worktree.

**As implemented (M5):** `crates/termd/tests/daemon/` runs the daemon end to end (attach,
snapshots, catch-up, control, status, logs, persistence, upgrade); `term-control/tests`
round-trips the frames; the app's `--visual-check-terminal` drives the window (typing,
keys, selection, scrollback, full screen, resize, control and takeover, the activity
panel, agents' tabs, Stop, deleting a worktree, restart and upgrade); `crates/bench` times
echo and `cat`. **Not built:** decoder fuzzing, the engine conformance corpus, MCP contract
tests against this document's tables, and the real-surface QA from a Claude Code session
(it writes to `~/.claude`, which needs the owner's approval). The MCP was driven by the
official TypeScript SDK's client instead (8.7).

---

## 13. Milestones and acceptance

M1 **Daemon core** (`term-control` frames v1, `termd`): open/attach/detach/input/resize,
raw streaming, snapshot on attach, registry on disk, one CLI `koyo-term attach <handle>`
for manual testing. Accept: a shell opened via the CLI survives the CLI exiting; re-attach
shows the same screen and scrollback; two attaches see the same output.

M2 **In-app view**: `TerminalView` on `ghostty-vt`, fed by the daemon; tabs per branch unit
(tabs only, no bottom panel); keyboard, mouse, selection, copy/paste, scrollback, resize. Accept: typing
latency within budget; `cat` of a large file keeps frames under 16 ms; closing the window
and reopening re-attaches every terminal of every branch.

M3 **Status, control, activity**: shell integration injection, status derivation, leases,
human pre-emption, activity log, controller badge and sidebar roll-up.

M4 **MCP**: catalog, stdio relay, loopback HTTP, scopes and grants, resources and
subscriptions, instructions/prompt, Claude Code launched by Koyo with the relay injected.
Accept: an agent in a worktree calls `terminal_list` with no arguments and sees only that
branch's terminals; two agents in one branch hand a terminal over with a visible takeover; a
human typing (or pressing "Take control") displaces the agent, every further agent write to
that terminal fails with `person_has_control` for as long as the human holds it, the agent
continues in another terminal of the branch, and "Give back" is the only thing that reopens it
to agents.

**Accepted (M4):** daemon end-to-end tests prove the scoping
(`each_agent_sees_its_own_worktrees_terminals`: two checkouts, each agent lists, names and
finds only its own), the handover between agents and the person's takeover until Give
back (`agents_hold_by_lease_and_a_person_takes_over_until_giving_back`), and
`person_has_control` through the catalog (`an_agent_drives_a_shell_through_the_catalog`);
the in-app check shows the same in the window. The official TypeScript SDK's client drove
`koyo mcp` from the built app. Not done: Claude Code *launched by Koyo* with the
relay injected, since Koyo launches no agents yet; until it does, an agent adds the
server itself (`claude mcp add koyo -- <app>/koyo mcp`). Loopback HTTP is deferred
(8.1).

M5 **Hardening**: catch-up under a throttled client, daemon upgrade path, persistence of
snapshots across daemon restart, bench suite, docs and the skill stub.

**Accepted (M5):** catch-up by `a_client_that_falls_behind_catches_up_from_a_snapshot` (5.4);
the upgrade path by `daemon/upgrade.rs` (a daemon refusing the hello is stopped by its pid,
one taking it is asked to stop) and in the window, where a stand-in daemon of another
engine is replaced from the tab's notice (5.1); persistence by the restart tests and the
window's "restored after the terminal daemon restarted" (5.9). The bench meets the typing
budget (`term-echo` p95 0.14 ms against 8.3 ms) and the `cat` one, restated against a bare
PTY in the same run since what a PTY carries depends on the machine and its load (`term-cat`
within 1.3 times a bare PTY, about 1 s for 150 MiB on an idle M4 Max); idle CPU is not
measured (7.5).
The audit closed the lifecycle gaps it found: agents' terminals appear as tabs (9), Stop
and "Stop Terminals…" (7.1, 9), and deleting a worktree stops its terminals (5.5). Not
done: the skill stub, which lives in the person's home folder (8.7), and the items marked
**Not built (M5 audit)** in 5.5, 6, 7, 8.5, 9 and 12.

M6 **Remote**: loopback TCP listener + SSH forward, then QUIC; mobile client scaffold.

---

## 14. Open decisions

O1 **Human takeover.** Resolved 2026-10-30 by the owner: both implicit (typing) and explicit
("Take control") takeover exist; a terminal a human took over is not used by agents again
until the human explicitly gives it back; no automatic release. Recorded in D11 and 5.6.

O2 **Activity log content.** Resolved 2026-10-30 by the owner: store logs as agent-terminal
does, human input included in full, in the three per-terminal JSONL files of 5.8; the log
history UI is a toggleable side panel (7.4).

O3 **One federated MCP server vs separate servers.** Default: terminals-only server in the
daemon now; federate app tools later behind the same catalog. Confirm when the browser port
needs its MCP.

O4 **Binding choice.** Default: `ghostty-vt` 0.8.0 (snapshot in a release). Revisit when
`libghostty-vt` publishes a release with `snapshot` or when libghostty tags 1.0.

O5 **Lease TTL and renewal.** Default 120 s, renewed by acting calls; connection close ends
it immediately.

O6 **`TERM` value.** Default `xterm-ghostty` with bundled terminfo, else `xterm-256color`.

O7 **Daemon per user vs per app build.** Default per user; the upgrade rule in 5.1 covers
build changes.

O8 **Pane splitting.** Resolved 2026-10-31 by the merge of the browser work: a tab is a
nested split tree (`Tab::Split { axis, first, second, ratio }`), and terminals sit in it like
any pane (`model/terminals.rs`, D13). As first raised: the app's tab model had only
`Tab::Split(Pane, Pane)`, two panes side by side, and terminals don't use it yet. Choices: a
terminal on either side of that split, or a split tree (nested horizontal and vertical
splits, draggable dividers, focus by direction, layout kept per branch), which reshapes the
tab model editors and browsers share. Below the app nothing changes: views are independent,
one terminal may show in several views (the client shares the daemon's one attachment per
connection among them), the daemon negotiates one size between them, and agents address terminals,
not panes.

---

## 15. Prior art to read before implementing (paths)

agent-terminal (`/Users/schulzf/github.com/repost-sh/agent-terminal`):
- daemon start/claim/flock: `cli/src/connection.rs:65-164`, `cli/src/native/daemon/claim/mod.rs:11-31`
- framing (len-prefixed JSON) and request enum: `cli/src/protocol/framing.rs:7-45`, `cli/src/protocol/request.rs:20-55,128-139`
- PTY: `cli/src/native/pty.rs:8,115-132` (portable-pty)
- wait conditions: `cli/src/native/wait/mod.rs:12-24,91-153` (Text, Gone, Stable, Idle, Exit, Cursor, Frame, Elapsed, Command; FirstStable for `open`)
- control (human counter, refusals exit 4, `AgentRefused`): `cli/src/native/control/mod.rs:8-60`, `cli/src/native/daemon/refusals.rs:18-28,63-67`, `cli/src/native/dashboard/control.rs:22-29`
- attach mirror: `cli/src/attach/mod.rs:19-20,250-255`, `cli/src/native/daemon/attach.rs:41-48`
- activity log with human input kinds: `cli/src/native/daemon/mod.rs:207-245` (`type_as_human` builds the entry), `cli/src/protocol/attach.rs:8-14` (`HumanInput`)
- store: `cli/src/native/store/mod.rs:42-160` (`PendingCall`, `ActivityEntry`, `KEPT_OUTPUT_BYTES`, `ScreenshotEntry`), `:264-290` (`activity_since` by offset), `:314-340` (`CommandLog`, `append_line`), `export` (trace zip); `cli/src/protocol/history.rs:25` (`CommandRecord`); `cli/src/output/action/mod.rs:48` (`input_action` phrases)
- dashboard panels: `dashboard/src/lib/side-panel.ts` (`activity | commands | screenshots | output | info`, width 30), `dashboard/src/components/{panels,activity-panel,commands-panel,screenshots-panel,output-panel,info-panel}.tsx`, behaviour pinned by `activity-panel.test.tsx`
- shell integration (OSC 133/7 injection): `cli/src/native/shell/agent-terminal.zsh:22-47`
- removed worktree scoping and why: `git show 457a51c:cli/src/session_id.rs`, commit `bf9548d`, `.superpowers/specs/2026-10-16-one-daemon-named-sessions-and-tabs-design.md:7-13,95-97`
- agent guide: `skill-data/core/SKILL.md` (loop, wait rules, one session per task, close at the end)

monocode (`/Users/schulzf/github.com/schulzfel/monocode-worktrees/feat-browser-panes`):
- catalog and tool generation: `crates/monocode-control/src/{operation.rs:57-59,113-120,163-175,195, command.rs:167-267, scope.rs:11-53, topic.rs:13-33, frame.rs:1-9, endpoint.rs:17-19,140}`, `crates/monocode-mcp/src/{tools.rs:12-36, http.rs:31-70, server.rs:26-29, backend.rs, lib.rs}`
- app-side dispatch on the GPUI thread: `crates/monocode-gpui/src/shell/control/{mod.rs:339,357, exec/mod.rs:39-42,174, defaults.rs:30-74}`, change batching `hub.rs`
- agent launch env and relay: `crates/monocode-core/src/app_mcp.rs:23-30`, `crates/monocode-gpui/src/{main.rs:202,228, mcp_command.rs}`
- browser control model (opener drives, person takes, per-tab log): `crates/monocode-control/src/ops/browser/` (uncommitted)
- the spec itself: `docs/native/mcp.md` (tool tables 336-423, resources 505-529, never-exposed 643), `docs/agent-access.md`, tests `crates/monocode-mcp/tests/docs.rs`

Orca CLI: `orca terminal <cmd> --help`, `orca agent-context --json` (288 commands; the JSON
contains raw control characters, strip them before `jq`), `orca worktree current --json`
(worktree `id = <repoId>::<path>`, `identity.key`, `branch` as a full ref, `displayName`).

Koyo: `crates/koyo/src/{model.rs, checkouts.rs, checkout/mod.rs, native/surface.rs,
bottom.rs, pane.rs, shell/actions.rs, settings.rs, delete_worktree.rs}`, `script/dev`,
`crates/bench`.

External reference implementations: PaneFlow blog (owned frames copied under the lock),
zeronsh/zeron PR 1006 (cell-pinned glyphs, sprite port, kitty keyboard via the encoder),
gpui-cn `crates/gpui-cn/src/terminal` (a GPUI terminal on `ghostty-vt`), coder/boo
(`src/protocol.zig`, query answering only while detached), neurosnap/zmx (daemon per session,
multi-client, snapshot on reattach), Ghostty discussion #12176 (catch-up via snapshots).

---

## 16. Glossary

- **Terminal**: a daemon-owned PTY + process + headless engine + record; the unit agents and
  the UI address. Has a handle and a per-worktree name.
- **Worktree / branch unit**: a git checkout directory; Koyo's `BranchUnit` wraps one.
- **Engine**: libghostty-vt (`ghostty-vt` binding): parser + grid + scrollback + encoders.
- **Snapshot**: libghostty-vt's binary serialization of a terminal's complete state.
- **Attach**: a client subscribing to a terminal's output after receiving a snapshot.
- **Catch-up**: replacing a backlog of output with a fresh snapshot for a slow client.
- **Controller / lease**: the single actor allowed to write; agents hold it as a timed lease.
- **Observer**: a client that reads but does not write.
- **Actor**: human, agent or system identity recorded in the activity log.
- **Status**: `running | idle | needs_input | exited | unknown`, derived in the daemon.
- **Relay**: the stdio MCP process that forwards to the daemon over the socket.
- **Catalog**: the single registry of operations from which MCP tools and the app's own
  calls are generated.
