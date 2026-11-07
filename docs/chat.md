# Koyo chat: agent host, Claude driver and the chat tab

Specification, written 2026-11-03 on branch `claude-chat-agent-sdk`. Drafted with Claude from a
study of T3 Code (pingdotgg/t3code, Apache-2.0) and of the Claude Agent SDK 0.3.317.

This document is the source of truth for the chat work, in the manner of `docs/terminals.md`:
every decision carries its reasons and the alternatives rejected, every external fact its
version, every piece of prior art the file to read. Update it when a decision changes.

Status of each section is marked `[decided]`, `[proposed]` (designed here, to be confirmed by
implementation) or `[open]` (needs the owner's call; a default is given).

---

## 0. Vision in one paragraph

A chat is a tab, like a terminal or a browser page, and belongs to a worktree: the agent works
in that checkout. Claude runs through Anthropic's own Agent SDK inside a small host program
that Koyo ships, compiled from strict TypeScript into one executable, so nobody needs Node.
The host outlives the window, as the terminal daemon does: quitting or updating Koyo leaves a
running turn running, unless the person stops everything as they quit, and reopening Koyo shows
where it got to, approvals still waiting. The
app is a typed client that renders: a timeline that streams the answer as it is written and
folds the work behind it, and a Liquid Glass composer floating over it with the model, effort,
permission and plan controls. Every behaviour T3 Code learned the hard way about driving
Claude is kept; where T3 is thin (streaming text, approval context, keys, a stop key), Koyo
does better. Claude is the only provider, behind a seam another one (Codex) can plug into.

---

## 1. Context and constraints

### 1.1 The Koyo codebase (facts, 2026-11-03)

- Laid out as repost is: `koyo-rs/` is the Cargo workspace, each crate in a folder named
  for its package (`koyo-rs/koyo` is the app, `koyo-rs/koyo-termd` the terminal daemon), the
  vendored forks in `koyo-rs/vendor/`; `koyo-js/` is the Bun workspace, each package a
  `koyo-js/koyo-*` folder sharing the root's lint policy and compiler setup. Scripts and docs
  stay at the checkout's root, and so do the pinned tools (`.tools/`).
- GPUI through `gpui-kit 0.8.1` (`gpui-fast`), vendored `gpui-base` and `gpui-component`
  (`koyo-rs/vendor/README.md`). Clippy pedantic with `unwrap`/`expect`/`panic` denied, a
  500-line limit per file (`script/hygiene`), `script/lint` in the pre-commit hook.
- Tabs and panes: `koyo-rs/koyo/src/model.rs`, `model/tab.rs`. `PaneKind::Chat` exists as a
  placeholder ("New chat", `IconName::MessageSquare`) rendered by `crate::pane::pane`. Pane
  contents are chosen in `Shell::pane_content` (`terminals.rs`).
- Liquid Glass: `native/liquid.rs::backdrop(id, radius, dark, window, cx)` puts an
  `NSGlassEffectView` in a native surface above GPUI's base layer and below its overlays;
  what is drawn on the glass goes in `deferred(...)` (pattern: `browser/toolbar.rs`,
  `control_bar`). The glass's own bright edge is cut off; `liquid::floating` draws a faint line
  of the palette's `glass_edge` just inside the element's corners instead. Without the glass
  (older macOS, Reduce Transparency) the fallback is the palette's `floating()` fill with a
  hairline and a shadow.
- Theme: `theme::palette(cx)` (the card tinted by the gradient theme, `floating()`,
  `hairline`, `mono`, `accent`, and the washes: `wash`, `faint_wash`, `wash_line`, shades of the
  gradient theme's hue laid thin over the card for what stands apart from text), `CARD_RADIUS =
  10`. `mono` is the code font the person picked
  in Settings → Appearance, JetBrains Mono Nerd Font (embedded in the binary) unless they
  picked another (`theme/fonts.rs`).
- Text size: the timeline's rows and the composer's text are drawn inside `at_chat_size`, whose
  rems are the size on the Appearance page's slider (`settings.fonts.chat_size`, 16 by default),
  so what's in a chat is sized in rems to grow with it. A change measures every row again.
- Markdown: `gpui-component`'s `TextView` streams (`push_str`, and `set_text` that appends
  when the new text extends the old) and fades new blocks in.
- Chat primitives in `gpui-component`: `message_scroller` (a `ListState` that follows the
  tail), `message`, `bubble`, `questionnaire`, `attachment`, `shimmer`; `gpui-base`'s
  `textarea` with `auto_grow(min, max)`, `submit_on_enter` and `InputEvent::PressEnter`.
- Daemon precedent: `koyo-termd` (`docs/terminals.md` 5.1, 5.10): a per-user process under
  `settings::data_dir()`, a socket trusted only while its lock is held, started detached by
  the first client under `start.lock`, a `ready` line on stdout, a version handshake, a log
  that starts over at a megabyte (`koyo-termd/src/logfile.rs`), a version kept beside a
  fingerprint of its sources by `script/daemon-version`.
- Helper binaries: the browser host is found beside the app's executable in a bundle and,
  in a debug build, in the checkout (`koyo-rs/koyo/src/browser/engine.rs`: `host_beside`,
  `packaged_in_checkout`), packaged by `script/browser-engine`, which `script/dev` runs.

### 1.2 Claude Agent SDK (facts, 0.3.317, 2026-11-03)

Full notes: `.superpowers/research/notes/claude-wire-protocol.md` (local, not committed).

- `query({prompt, options})` returns a `Query`: an async generator of `SDKMessage` (40
  variants) with control methods: `interrupt`, `setPermissionMode`, `setModel`,
  `applyFlagSettings({effortLevel, fastMode})`, `initializationResult`, `supportedCommands`,
  `supportedModels` (with `supportedEffortLevels`, `supportsFastMode`, `supportsAutoMode`),
  `accountInfo`, `getContextUsage`, `setMcpServers`, `rewindFiles`, `stopTask`, `close`.
- `prompt` may be an `AsyncIterable<SDKUserMessage>`: one live CLI process per session, fed
  prompt after prompt. A user message may carry `priority: 'now' | 'next' | 'later'` and a
  `uuid`.
- The SDK drives the Claude Code CLI over stream-json. It brings the CLI only as an optional
  platform package (299 MB); with `pathToClaudeCodeExecutable` set, its resolver is skipped
  and nothing but our executable needs to ship. Inside `bun build --compile`,
  `require.resolve` of the platform package fails, so the path must always be given.
- Import the root entry (`sdk.mjs`): it inlines zod and the MCP SDK and imports only Node
  builtins, so it compiles into a single file.
- `env` replaces `process.env`. `systemPrompt` omitted means an *empty* prompt: Claude Code's
  behaviour needs `{type: 'preset', preset: 'claude_code'}`. `settingSources` omitted loads
  user, project and local settings, as the CLI does.
- `permissionMode` unset in 0.3.317 lets the CLI decide from the user's settings
  (`permissions.defaultMode`); the `init` frame reports the mode in force.
- Frames outside the typed union arrive at runtime; they must be tolerated.
- A running `claude` process holds about 275 MB (measured 2026-11-03 over 18 processes,
  598 MB at most).

### 1.3 T3 Code (facts, 2026-11)

Notes, local: `.superpowers/research/notes/t3-claude-adapter.md` (the adapter, 48 pitfalls),
`t3-orchestration.md` (the domain model, 53 pitfalls), `t3-chat-ui.md` (the UI inventory).
T3 runs a Node server (Effect, event-sourced SQLite) and a React client; its Claude adapter
is one 12k-line file behind a provider-neutral contract. It does not stream assistant text.

### 1.4 Non-negotiables

- **Strict TypeScript.** No `any`, no `as`, no `!`, no `@ts-ignore`; untrusted input is
  parsed with zod; every union is matched exhaustively. `tsc` with every strict flag and
  `skipLibCheck` off; ESLint with typescript-eslint's strict type-checked sets, every rule an
  error, `--max-warnings 0` (`koyo-js/eslint.config.ts`), in `script/lint` and so in the
  pre-commit hook.
- **No Node for the user.** The host is one executable from `bun build --compile`.
- **Everything a task creates stays in the checkout** while developing: `script/bun` pins
  Bun 1.4.4 in `.tools/`, with its caches; tests run with `KOYO_DATA_DIR` in the checkout.
- **Native feel.** Liquid Glass for the composer and everything floating; the theme's
  palette everywhere; system conventions for keys (⌘. stops), menus and focus.
- **Performance.** A token delta repaints one message, never the list. History loads lazily.

---

## 2. Decisions

**C1 [decided] The agent runtime is a TypeScript program on the official Claude Agent SDK,
compiled with `bun build --compile`.** Why: the SDK is Anthropic's supported, typed surface
over the CLI's stream-json and control protocol, which is internal and moves with every CLI
release (40 message kinds, dozens of control subtypes, version-dependent defaults such as the
permission mode). Following it from Rust means re-deriving that protocol from minified
JavaScript at each release; the SDK absorbs it, and a version bump is a `package.json`
change checked by `tsc`. Bun compiles the host, the SDK and zod into one executable, so the
user needs neither Node nor `node_modules`. Rejected: Rust speaking stream-json to the CLI
(fragile; the owner's call); Node with shipped `node_modules` (needs Node); Deno compile
(works, but Bun has SQLite and FFI built in, and runs ESLint and the tests in the same
toolchain); the Python SDK (needs Python); Gleam (compiles to Erlang or JavaScript, not to an
executable, and would lose the SDK's types at the boundary).

**C2 [decided] The host is a per-user daemon, like `koyo-termd`, not a child of the
window.** Why: an agent turn runs for minutes; a chat that dies when Koyo quits or updates
is worse than running `claude` in a Koyo terminal, which survives (terminals D2). With the
host outliving the app, approvals stay answerable across an app restart, and the cases T3
must recover from (its server restarting under live turns) shrink to the host's own restart.
It also leaves room for other clients later (a `koyo chat` command, mobile). Rejected: a
stdio child of the app (simpler, but every app quit interrupts every agent); a launchd agent
(later, as for termd; not required).

**C3 [decided] The host owns the chats: their state lives in the host's SQLite, and the app
is a client that renders snapshots and live changes.** Why: the host outlives the app, so it
must hold the truth, or what happens while no window is open is lost. Rejected: the app owns
state and the host is a stateless driver (two sources of truth, and events with nowhere to go
while the app is closed); Claude's transcripts as the store (they are the provider's, carry
none of Koyo's items, requests or queue, and a second provider has its own).

**C4 [decided] State tables with a per-thread change number, not event sourcing.** Tables
`threads`, `runs`, `items`, `requests`, each row the record's JSON; every write stamps the
rows it changes with the thread's next change number, so a client catches up with "rows
changed after N". Why: T3's event log, projections, outbox and receipts pay off with remote
clients and several devices; for one local user they are machinery. Kept from T3: the
rules, not the mechanism (4.5).

**C5 [decided] One serialized actor per thread.** Every decision and write for a thread runs
in its mailbox, one at a time, and each publishes its changes before the next runs. Why: JS
is single-threaded but `await` interleaves; T3's ordering bugs (orchestration pitfalls 1, 3,
6, 7, 10) all come from two paths deciding about one thread at once. With one lane per
thread they can't, by construction.

**C6 [decided] A provider seam inside the host.** The actor talks to a `Provider` (status,
catalog, sessions, titles) through normalized events and items; it never sees SDK types.
The Claude driver is the first provider; a scripted fake provider is the second, for tests;
Codex can be the third. Capabilities are declared, and the actor branches on them, never on a
provider's name (T3 `OV2:260-397`).

**C7 [decided] Claude runs from the user's installed `claude`.** The host finds it in the
login shell's `PATH` and the usual places, or where Koyo's settings point. Why: the SDK's
CLI is a 299 MB platform package; people who use Claude have it installed and signed in, and
their CLI keeps itself current. T3 does the same. A missing or signed-out CLI is a state the
chat shows ("Install Claude Code", "Sign in to Claude"), with the command to run in a
terminal tab. Rejected: shipping the CLI (size; two CLIs to keep in step).

**C8 [decided] The app–host protocol is JSON lines over the unix socket**: requests with ids,
their results or errors, and events. Contract tests: golden JSON fixtures that both zod
(host) and serde (app) parse and re-encode unchanged. Why: no hot binary path exists (a
token delta is a few bytes of text), JSON is debuggable with `nc -U`, and both sides have
exact schema types. Rejected: termd's binary framing (built for PTY bytes); gRPC or HTTP
(machinery); MCP (a tool protocol, not a UI state stream).

**C9 [decided] The host has a version, kept as termd's is.** `HOST_VERSION` in the host's
protocol module, beside a fingerprint of what the executable is built from (the package's
sources, `bun.lock`, the Bun version), in `koyo-rs/daemon-version.txt` next to termd's line;
`script/daemon-version` checks both, and `bump` or `keep` settles a change, so a comment-only
edit doesn't count as a new host. The hello's answer carries the version. Same: use the host.
Different and the host is idle (no live run, no task in the background, no pending request):
stop it and start the app's, without asking. Different and busy: the chat shows "Chats are
running on another build of Koyo. Restart them to use this one; running turns stop." with
"Restart chats", as terminals do (D12). Busy also means another client is connected, so two
builds in use at once leave the choice to the person rather than take turns replacing each
other. The hello is the one exchange every version keeps. Rejected: a hash of the sources as
the build's id (automatic, but every edit, a comment included, would turn a busy host away;
and it would be a second compatibility scheme beside termd's).

**C10 [decided] Assistant text streams.** The driver turns `text_delta` and
`thinking_delta` stream events into appends per content block, reconciled with the
`assistant` snapshot by message id and block index (T3 does this for thinking only). Each
append is sent as it comes, as the new text alone; the app applies the appends that arrived
since the last frame once per frame, to the one message they belong to.

**C11 [decided] Rewind takes the conversation back, and the files only when asked and safe.** The
terminals, file tabs and other agents share the worktree, so restoring files under them is the
hazard T3 itself names (orchestration 6.4, pitfall 41). A person goes back to one of their messages
from its row, once nothing's at work or waiting, and confirms. The host finds the entry before the message in Claude's transcript and
keeps it as `Thread.resumeAt`; the next session resumes there (`resumeSessionAt`), which drops
the turns after it from the same session id, and later resumes follow the new branch. A message
that started the conversation has the next turn start another. A run's prompt carries the run's
id as its uuid, which is how its entry is found. `forkSession` isn't used: it renumbers every
entry. Removed runs stay as rows with the status `removed`, their items and requests deleted, so
a client catching up by `since` hears of it. A message the transcript no longer has (compacted
away, or never reached) is refused.

File restore is the SDK's `enableFileCheckpointing`, on for every session, which keeps its copies
under `~/.claude/file-history`, and `rewindFiles`, asked of a CLI opened on the conversation with
nothing said to it: no model call, about half a second, and the transcript doesn't grow.
`threads.filesBack` is the dry run the dialog opens with; it offers nothing when Claude has no
record (chats from before this) or nothing would change. Probed on the real CLI:

- A restore writes over what's there and its dry run doesn't notice, so the host looks: a file
  whose mtime is later than `Thread.filesSettledAt` (the end of the chat's last run, removed ones
  too, or its last restore) blocks the offer by name, and so does another chat at work in the
  checkout. A file that's gone doesn't: it has nothing to lose. An edit by someone else during the
  chat's own run isn't caught, as the mtime can't tell it from Claude's, and neither is another
  chat starting during the second a restore takes.
- The conversation forgets a turn's checkpoint once it goes on from before it, so the files go
  first; if they fail, the chat stays as it was. While it goes back, nothing new starts on that
  chat. Going back without them gives up restoring those turns later, once a new turn is sent.
- Only what Write, Edit and NotebookEdit did is restored. What commands and subagents changed
  stays, unless the main conversation had edited that file too; the dialog says so.

**C12 [decided] Permissions are Claude's own modes, named for people:** Ask (`default`),
Accept edits (`acceptEdits`), Auto (`auto`, where the model supports it), Full access
(`bypassPermissions`); Plan is a separate toggle (`plan`). A new chat starts in the mode the
user's Claude settings choose (nothing is passed; the `init` frame reports it), unless Koyo's
setting names one. Why: respect the user's Claude configuration, which also applies in their
terminal sessions. T3 defaults to full access; Koyo doesn't.

**C13 [decided] A chat is a tab, in a worktree.** Threads are keyed to the worktree's
checkout path, which is also the agent's `cwd`. The tab face shows the chat's title and
status; the sidebar's worktree row shows what its chats need. Closing the tab doesn't delete
the chat: the new-tab launcher (`docs/new-tab.md`), in its Chat kind, lists the chats no tab
shows. Which
chats have a tab is the thread's own `tabOpen`, kept by the host with the rest of it, so the
tabs come back with the app as terminals' do (5.2); the app keeps no list of its own. A new
tab is not a chat until its first send, so empty tabs leave nothing behind.
Rejected: T3's thread inbox in the sidebar (Koyo's sidebar lists worktrees; a chat is a tab
in one, as a terminal is).

**C14 [decided] What the agent waits on a person for is a request, whatever it is.** An
approval, a question and a proposed plan are the three kinds; the thread needs its person
exactly while one is pending. A request the agent stopped waiting on (a plan, whose tool is
denied at once; a question after a host restart) is answered by the person's next message,
which the actor applies in the step that starts the turn; a live one (an approval, a
question the agent waits on) is answered with `requests.respond`. Rejected: plans as items
with a state of their own (a second source for "needs you", and nothing to say what a
restart does to one).

---

## 3. Architecture overview

```
 Koyo.app (Rust, GPUI)                         koyo-agent-host (Bun executable)
 ┌───────────────────────────┐   unix socket   ┌────────────────────────────────────────┐
 │ chat tab (GPUI views)     │   JSON lines    │ server: connections, hello, routing    │
 │  timeline · composer      │◀───────────────▶│ threads: one actor per thread          │
 │ chat model (entities)     │                 │ store: bun:sqlite (WAL)                │
 │ koyo-agent: protocol +    │                 │ providers: claude (Agent SDK) · fake   │
 │   client (threads, serde) │                 │                    │                   │
 └───────────────────────────┘                 └────────────────────┼───────────────────┘
                                                                    │ stream-json (SDK)
                                                        ┌───────────▼───────────┐
                                                        │ claude (user's CLI),  │
                                                        │ one per live session, │
                                                        │ own process group     │
                                                        └───────────────────────┘
```

- `koyo-js/koyo-agent-host/`: `src/protocol/` (zod schemas, the contract), `src/server/`
  (socket, connections, hello), `src/daemon/` (lock, folder, idle exit, shutdown),
  `src/store/` (SQLite), `src/threads/` (actor, status, queue, requests), `src/providers/`
  (the seam, `fake.ts`, and `claude/` with the login-shell environment and CLI discovery),
  `src/main.ts`; tests in `test/`, fixtures in `test/fixtures/`. `script/agent-host` builds
  the executable into `koyo-js/koyo-agent-host/dist/`.
- `koyo-rs/koyo-agent`: the protocol as serde types, tested against the same fixtures; the
  client (connect or start, hello, requests, an event inbox), on its own threads as
  `koyo-lsp` and `koyo-term-control` are.
- `koyo-rs/koyo/src/chat/`: the app's chat model (a `Chats` entity holding thread entities fed
  by the client) and the views.

---

## 4. The host: `koyo-agent-host`

### 4.1 Process model and lifecycle [proposed]

As termd (`docs/terminals.md` 5.1 and 5.10), in a folder of its own,
`settings::data_dir()/agents/`: the socket `host.sock` (0600, in the 0700 folder), trusted
only while `host.lock` is held; `start.lock`, under which a client checks again and starts
the host; a `ready` line on stdout once it listens; `host.log`, which starts over at a
megabyte keeping `host.log.old` (`src/log.ts`); stderr only in the foreground. What differs:

- The socket is reached however deep the folder is, as termd's is (`docs/terminals.md` 5.1): by
  the way to it from the working directory when its path is too long for a socket address
  (`src/socket.ts`). The host runs in its folder, so it listens on `host.sock` by name.
- The host is its own executable, `koyo-agent-host --dir <agents>`, not a subcommand of the
  app: it's Bun's, not Rust's. Compiled with `--no-compile-autoload-dotenv` and
  `--no-compile-autoload-bunfig`, so no `.env` or `bunfig.toml` near it changes it.
- The app starts it with a cleared environment (`HOME`, `USER`, `LOGNAME`, `SHELL`,
  `TMPDIR`, `LANG`, and `KOYO_DATA_DIR`/`KOYO_LOG` when set) and the agents folder as its
  working directory, so nothing of whatever launched Koyo (a terminal, another agent's
  session, an IDE) reaches the host or the agents it runs (4.2).
- The lock is `flock` on `host.lock` through `bun:ffi`; a host that can't take it exits 0,
  since another serves. Bun's sockets don't expose the peer's credentials, so the 0700 folder
  is what keeps other users out.
- Data: `chats.sqlite` (WAL) and `attachments/`.
- A client that stops reading (a hung app, a debugger) is cut off once 32 MiB wait for it;
  it reconnects and catches up with `sinceSeq` (5.1). On shutdown, each client is let go only
  once what it's owed has gone, the `shutdown` reply among it.
- Idle exit: with no client connected, no live run, no task in the background and no pending
  live request for 10 minutes, the host closes its sessions and exits. A pending approval
  keeps it alive: it waits for a person, who will open Koyo. A task in the background keeps it
  alive too: closing its session would end it.
- Quitting the app with a chat at work (a turn running, a question waiting for the person, a
  task in the background) asks, as with terminals (`docs/terminals.md` 5.1;
  `koyo-rs/koyo/src/quitting.rs`): Keep running leaves the host carrying on, and the app shows
  what it did when it opens again; Stop everything shuts the host down (below). The app asks a
  host that runs what's at work, starting none to ask, and once it stopped the host its own
  client starts none again.
- Live sessions: a thread's CLI process stays alive between turns, which saves a spawn;
  at most 4 are alive at once (the least recently used is closed first) and each closes after
  5 minutes idle; neither rule closes one with a task in the background. The next turn resumes
  the session (4.5.6).
- Shutdown (`shutdown` or SIGTERM): stop accepting, interrupt live runs (5 s at most), settle
  them interrupted by the host, expire live approvals, close sessions (each CLI's process
  group gets SIGTERM, then SIGKILL after 5 s), close the store, remove the socket, exit.
- Startup recovery (after a crash or a kill): runs still `starting`, `running` or `stopping`
  are settled interrupted by the host; live approvals expire; live questions become answered
  by the next message; queues are held. Nothing that touches a process is replayed (T3
  orchestration 11.2).

### 4.2 Environment and CLI discovery [proposed]

- The CLI runs in the user's login environment, not the host's: once at start, bounded at
  5 s, the host runs `$SHELL -l -c 'env -0'` (no `-i`: interactive rc files print, prompt and
  hang) and gives that environment, whole, to every CLI it starts. A Finder launch's minimal
  `PATH` is never what the agent sees. Failure falls back to the host's own environment with
  the usual places on `PATH`. `HOME` is never changed (T3 pitfall 44: login lives in the
  keychain).
- `claude` is resolved in order: Koyo's setting `chat.claude_path`, the login `PATH`,
  `~/.local/bin/claude`, `~/.claude/local/claude`, `/opt/homebrew/bin/claude`,
  `/usr/local/bin/claude`. Its version (`claude --version`) is read once per path.
- Spawn: a custom `spawnClaudeCodeProcess` starts the CLI in its own process group
  (`detached`), so an interrupt or close ends its MCP servers and shells too (T3 pitfall 2);
  closing sends SIGTERM to the group, SIGKILL after 5 s. The host's exit ends every group it
  started.

### 4.3 Wire protocol v1 [proposed]

Framing: one JSON object per line (`\n`), UTF-8, at most 8 MiB a line. Attachments don't
travel over the socket: the app writes them into `attachments/` and names them by file. A
missing value is `null`, never an absent field. A reply the host has at once is written at
once, before any event a later step makes, so the snapshot `threads.open` answers with comes
ahead of the changes that follow it.

```
C→H  {"id": 7, "method": "threads.create", "params": {...}}
H→C  {"id": 7, "result": {...}}                       // or
H→C  {"id": 7, "error": {"code": "thread_not_found", "message": "…"}}
H→C  {"event": "thread.changed", "params": {...}}      // unsolicited
```

#### 4.3.1 Host

```
hello       {client: {kind: "app"|"cli"|"test"}, version} → {version, pid, busy}
            `version` is `HOST_VERSION` (C9); `busy` while a run is live, a task runs in the
            background (4.5.6), a request waits or another client is here, so two builds of
            the app don't take turns stopping it.
            A hello of another version is answered, so the app can see whom it met, and that
            client may then only call `shutdown`; before a hello, nothing else is answered
            but `not_allowed`.
shutdown    {} → {}
config.set  {claudePath, defaultMode, koyoTools} → {}
providers.status {refresh: bool} → [ProviderStatus]
            {provider, state: "ready"|"missing"|"signed_out"|"error", executable, version,
             account: {email, plan}, message}
catalog     {provider: id | null, cwd} → {models: [Model], commands: [Command], defaultMode}
            Model = {id, name, label, runs, efforts, fast, auto} (4.5.9)
```

#### 4.3.2 Threads

```
threads.watch   {} → [ThreadSummary], then `thread.summary` for every thread
threads.create  {cwd, provider: id | null, model, effort, fast, permission, plan} → Thread
threads.open    {threadId, sinceSeq} → ThreadSnapshot, then its `thread.changed` and
                `item.appended` until `threads.close`. A first open holds the latest 30
                runs' items; `sinceSeq` gets the rows changed since, and every streaming item
                whole.
threads.history {threadId, before: runId | null, runs} → {runs, items, requests}
threads.filesBack {threadId, runId} → {filesBack}
threads.rewind  {threadId, runId, restoreFiles} → {prompt}
                the page of runs before `before`, with their rows
threads.close   {threadId} → {}
threads.update  {threadId, title?, model?, effort?, fast?, permission?, plan?, tabOpen?}
                → Thread
threads.read    {threadId} → {}                                the person has seen it
threads.delete  {threadId} → {}                                rows and attachments go
```

#### 4.3.3 Turns, queue, requests

```
turns.send      {threadId, prompt: {messageId, text, attachments}, delivery: "auto"|"queue"}
                → {runId, delivered: "started"|"queued"|"steered"}
                `messageId` is the client's uuid: a repeated send is answered from the run
                it made, never sent twice (T3 pitfall 11).
turns.stop      {threadId, force} → {}
turns.compact   {threadId} → {runId}                           a /compact turn
queue.remove    {threadId, runId} → {}
queue.edit      {threadId, runId, text, attachments} → {}
queue.sendNow   {threadId, runId} → {delivered}
queue.resume    {threadId} → {}                                release a held queue
requests.respond {threadId, requestId, response} → {}
                response = {kind: "approval", decision: "allow"|"allow_session"|"deny"|
                                   "cancel", message}
                         | {kind: "question", answers: {[questionId]: [string]}}   (none at all: skipped)
                         | {kind: "plan", decision: "implement"|"dismiss"}
                its kind is the request's; another kind is `bad_request`, a request no longer
                pending `conflict`.
```

#### 4.3.4 Events

```
thread.summary   ThreadSummary, when something a list shows changed
thread.removed   {threadId}, a thread deleted
thread.changed   {threadId, seq, thread, runs, items, requests}: the rows one actor step
                 changed, whole
item.appended    {threadId, itemId, text}: what a streaming item grew by, in order
host.notice      {level, message}, e.g. the CLI went missing
```

Ordering: one actor step publishes one `thread.changed`; appends follow the step that made
their item, in order, on the one socket. `threads.open` registers the subscriber and takes the
snapshot in the same synchronous step, so nothing falls between snapshot and stream (T3
orchestration pitfall 2).

`AttachmentRef = {file, name, mime}`: `file` is a name inside `attachments/`, checked
there (no `..`, no links out). Images (png, jpeg, gif, webp, at most 5 MB after the app
downsizes them) go to the model as image blocks; other files as their path in the text.

### 4.4 Domain model [proposed]

The zod schemas in `src/protocol/model.ts` are the definition; this is their summary. Ids are
UUIDv7 (they sort by creation time), each kind its own branded type. Times are ISO-8601.

```text
Thread {
  id, cwd, title, titleSource: "seed"|"generated"|"user", provider, model, effort, fast,
  permission: Mode | null (null: the provider's settings decide), plan: bool,
  sessionId,                  // the provider's conversation, once there is one
  queueHeld: bool,
  tabOpen: bool,              // a tab shows it, and will again after a restart (C13)
  parent: {threadId, itemId} | null,   // the row of the thread that started it (4.5.10)
  backgroundTasks: n,         // what the agent has running in the background now (4.7); 0
                              // while no session runs, and as a host that stopped comes back
  createdAt, updatedAt, readAt, seq
}

Run {                         // a turn: one prompt and the work it caused
  id, threadId, status: "queued"|"starting"|"running"|"stopping"|"completed"|
                        "interrupted"|"failed"|"removed" (off the queue, or gone back past),
  prompt: {messageId, text, attachments},   // all a queued run is
  model, effort, permission (in force while it ran), plan,
  requestedAt, startedAt, endedAt,
  stop: {requestedAt, by: "person"|"host", force} | null,
  failure: {kind: "api"|"usage_limit"|"auth"|"overloaded"|"process"|"host", message,
            retryAt} | null,
  usage: {inputTokens, outputTokens, cacheReadTokens, cacheCreationTokens, costUsd,
          contextTokens, contextWindow} | null,
  files: [{path, added, removed}], seq
}

Item {                        // one timeline entry
  id, threadId, runId (every entry is a turn's), ordinal,
  status: "pending"|"running"|"completed"|"failed"|"declined"|"interrupted", kind + payload, seq
}
  user_message   {messageId, text, attachments, delivered: "turn"|"steer"}
  assistant_text {text}                                         streams
  reasoning      {text}                                         streams
  tool           {name, family: "command"|"read"|"edit"|"search"|"web"|"mcp"|"agent"|
                  "todo"|"skill"|"other", label, input (≤ 16 KB), output (≤ 16 KB, the end),
                  exitCode, patch: [Hunk] (≤ 16 KB, whole lines), files, server}
  todos          {todos: [{text, status}]}
  request        {requestId}                   a request, in its place in the timeline
  subagent       {description, agentType, summary, threadId, progress, usage, startedAt,
                  endedAt}   (4.5.10)
  compaction     {trigger: "manual"|"auto", before, after}
  notice         {level: "info"|"warning"|"error", text}

Request {
  id, threadId, runId, status: "pending"|"answered"|"cancelled"|"expired", kind + payload,
  answer: a response (4.3.3) | {kind: "message", messageId}, createdAt, resolvedAt, seq
}
  approval  {tool, family, title, label, detail, change: [string] | null, input, reason,
             allowSession: {label} | null}
  question  {questions: [{id, header, question, multiSelect, options: [{label, description,
             preview}]}], live: bool}
  plan      {markdown}                          live: never; answered by a message, or
                                                implemented or dismissed
  `label` is what a line of work would go by; `detail` is shown as is, at most 16 KB: a
  command, a path, or else the input; `change` is an edit's to the file `detail` names, each
  line starting `+` or `-`.

ThreadSummary = id, cwd, title, provider, model, updatedAt,
  status: "idle"|"working"|"failed", needs: "approval"|"question"|"plan" | null,
  background: bool, queued: count, unread: bool
```

Derived, never stored (T3: status written twice drifts):
- `status`, in one function: working while a run is starting, running or stopping; failed
  when the latest settled run failed; else idle. `needs` from the oldest pending request.
  `unread` when the latest settled run ended after `readAt`. `background` while the thread's
  `backgroundTasks` isn't 0 (4.5.6).
- The stop divider ("You stopped this response") from `Run.stop`; the failure row from
  `Run.failure`; Force stop offered once `stop.requestedAt` is 10 s old and the run is still
  stopping; the user message of a run from its `user_message` item.

An item's `ordinal` is fixed the first time the item is seen, before its text exists, so text
that streams in late never moves below something that came after it (T3 orchestration
pitfall 14).

### 4.5 The thread actor [proposed]

One actor per thread. Each step is a method that runs to its end without waiting: it reads,
and writes what it decided, each `Store.commit` one transaction whose `thread.changed` is
published as it's made (a run settling and the next one starting are two), so the event
loop is the mailbox and no two steps interleave. A provider's reports, a request withdrawn
and a title that came are queued behind the step that's running (`queueMicrotask`), never
run inside it, so a stop and a late tool result are never decided at once; a session dropped
(closed, killed, reopened) reports to nothing, and a deleted thread's actor takes no more
steps. The turn a session works on lives with its run: it starts as the run does and ends
with the commit that settles it. A run changes status only through one function, which
knows the moves allowed:

```
queued → starting → running → stopping → completed | interrupted | failed
  │         │  │       └────────────────→ completed | interrupted | failed
  │         │  └───────────────────────→ stopping
  │         └──────────────────────────→ completed | interrupted | failed
  └────────────────────────────────────→ removed
```

A turn may end before it's seen to start (a quick failure, a `/compact`, a stop as it
starts), so `starting` settles too. Nothing leaves `stopping` except to a settled status,
and a settled run leaves only to `removed`, when a person goes back past it (C11). A queued run
taken off the queue is `removed` too. A removed run's row stays and its items and requests go
with it; other rows are never deleted but with their thread.

The commit that settles a run closes the items it left open, the same for every provider,
which closes none of them itself. After a run that completed, a tool, a subagent or a
compaction left open is `failed`, as no result came to say it ran, and anything else is
`completed`. After a run that was interrupted or failed, all of them are `interrupted`. A
subagent's row is the exception: its subagent says how it ended, as one in the background
outlives its run (4.5.10).

#### 4.5.1 Send

`turns.send` with `delivery`:
- `auto` (the composer's Enter): idle → start; running → steer, unless the provider can't
  steer or compaction runs → queue; stopping → queue (T3 orchestration pitfalls 3, 4, 19).
- `queue` (⌘↩ while running): append to the queue.
The step that starts a turn answers every pending request the agent stopped waiting on
(a plan: refined; a question that isn't live: answered by the message) (C14). A live request
the app answers itself: with an approval shown, composer text sent declines it with the
text as the reason; with a question shown, the text is its "Other" answer. If that request
was settled meanwhile (`conflict`), the text stays in the composer.

Start: write the run `starting` and its `user_message` item, pick the session (4.5.6), give
the provider the prompt with a fresh uuid (T3 pitfall 7), move to `running` on the provider's
`turn.started`; the provider's events update items; `turn.finished` settles the run.

#### 4.5.2 Steering

The message is offered to the live session with `priority: "next"`, so it lands at the
turn's next step without cutting off the text being written; its item is a `user_message`
with `delivered: "steer"` in the running run. Right before offering, the actor checks the run
is still running; if it ended in between, the message starts a run of its own, with the same
`messageId` (T3 orchestration pitfall 3). [proposed: confirm `next` against CLI 2.1.317 in
the driver's probe; T3 uses `now`, which aborts the current segment and needs its `aborted_*`
results swallowed (adapter pitfall 22).]

#### 4.5.3 Queue

Queued runs start in the order they were sent (their ids sort by time), the next once the
current one has settled. The queue holds (`queueHeld`) after a stop, a failure, a usage limit
or a host restart, and only the person releases it: `queue.resume`, or `queue.sendNow` on a
run (T3 orchestration pitfalls 16, 17). A message sent while the queue is held starts now
and leaves the held runs waiting, shown as "2 queued messages are waiting · Send · Clear".

#### 4.5.4 Stop

`turns.stop` moves the run to `stopping` with `stop: {requestedAt, by: "person"}`, holds the
queue, then calls the session's interrupt. The `stopping` status is the barrier: a steer, a
queued start or a late tool finds it, and the transition function refuses every move out of
it but to a settled status. The run settles `interrupted` on the provider's result or the
process's end. `force: true` kills the CLI's process group and settles it at once (T3 adapter
pitfall 46, orchestration 8). A stop and a turn's natural end race for the same run; the first
step wins and the second finds it settled (orchestration pitfall 7).

#### 4.5.5 Requests

A provider asks through the sink; the actor writes the request `pending` and its `request`
item, and holds the provider's answer promise. What the agent stopped at without waiting (a
plan) comes as a `request` event instead, with nothing held: the next message answers it.
`requests.respond` resolves it: the row is written `answered` first, then the answer
delivered; a delivery that fails writes an error notice and doesn't reopen the request (T3
orchestration 11.3.6). An abort from the provider (the turn interrupted, the tool withdrawn)
writes `cancelled`, as does a run settling with requests it still waits on; a run the host
stopped leaves them as a restart does (4.1). The answer is checked against the row inside
the step that writes it, so a turn ending as the person answers can't erase the answer
(orchestration pitfall 10). Implementing a plan turns the plan toggle off and sends
"Implement the plan above.", as the person would.

#### 4.5.6 Sessions

A thread has at most one live provider session. A turn reuses it; a closed one is resumed by
`sessionId`; resuming needs the transcript to exist with real turns, else a fresh session
starts and a notice says the earlier context wasn't available (T3 adapter pitfalls 9, 10).
The session id is minted by the host and stored before the first turn (the SDK's `sessionId`
option), so a crash before the first frame still resumes the right conversation.

Model and effort changes apply to the live session (`setModel`, `applyFlagSettings`) before the
next turn; a permission change with `setPermissionMode`; a change the live session can't take
reopens it with resume (adapter pitfalls 24, 27, 28), which ends a task in the background:
refusing the change while one runs isn't built yet. The session says at once which it is
(`configure` returns `"applied"` or `"reopen"`), so a turn the same step starts, as implementing
a plan does, never runs on the old settings; a change that fails to apply is the session's to
recover from, by reopening before its next turn. An interrupt the session can't deliver ends its
process, and the run with it.

A session opens in the chat's mode, else Koyo's default (`config.set`), else `null`, which
leaves the choice to the provider's settings. A change to the chat's mode reaches a live
session the same way. A change to Koyo's default leaves a live session in the mode it has,
until it next opens.

A session with a task in the background doesn't idle-close or get evicted. A task in the
background is a shell the agent started with `run_in_background`, a Monitor, or a subagent
(4.5.10). The agent ends its turn and waits for it: its end starts a turn of the agent's own,
which only the session's process can start, and the task ends with that process. That turn is
a run with no message of the person's (`turn.adopted`, 4.7), shown like any other. Once no
task runs, the session closes after 5 minutes idle, as after a turn. The provider says how
many tasks run each time that changes (the `background` event, kept as
`Thread.backgroundTasks`), and lists show it as the summary's `background`; a task the
provider says isn't activity, as a housekeeping one, doesn't count. A session that ends anyway
ends its tasks with it: the host is stopped or replaced, or crashes, or a settings change
reopens it. The next turn resumes the conversation, and Claude tells the agent they stopped. A
task that never ends, as a dev server, keeps the session, and the host, until the agent stops
it, the chat is deleted, or Koyo quits with Stop everything.

#### 4.5.7 Titles

After the first turn starts, the provider's `title(prompt)` runs once: a one-shot query to
the smallest model with no tools, no settings and no session kept, asking for a title of at
most six words. The provider returns the reply as it came, and the actor makes the title of
it: its first line that isn't empty, without the quotes and markdown around it, at most six
words. A reply with nothing left writes no title. A title is written only while the title
is still `seed` (the prompt's first line, cut at 60 characters): a rename by the person
always wins (T3 orchestration pitfall 47).

#### 4.5.8 Live text

Streaming text lives in the actor's memory; the store keeps it at most every 500 ms without
a change number or an event (`Store.keepText`), since clients have it from the appends, and
the step that ends the block writes it as a change. A crash loses at most half a second of
text, which the provider's transcript still holds. Tool output is never kept per delta; the
item keeps the last 16 KB when the tool settles (T3 orchestration 11.3.4).

#### 4.5.9 Catalog

Models, efforts and the account come from the CLI's initialize response and depend only on
the CLI and the account: they're kept per executable and version. Slash commands and skills
depend on the folder: each session's `init` refreshes them for its cwd, and a cwd no session
has run in yet gets an initialization-only probe (a query whose prompt never yields, read
for `initializationResult()` and closed; no API call). No hand-kept model list (T3 keeps a
manifest by hand). The default mode the `catalog` method answers is Koyo's default when
`config.set` names one, else the one in the provider's settings.

#### 4.5.10 Subagents

A subagent is a thread of its own, as T3 has it: `Thread.parent` names the row of the thread
that started it, and the row's `threadId` names the thread back. It's an ordinary actor with
no session, so open, history and recovery need nothing new, and it's left out of
`threads.watch`, as it's reached through its row. The provider's `subagent.*` events say what
it does; its timeline comes through `subagent.event` as any turn's items do.

The row is written outside the run's `LiveTurn`, so a run settling doesn't close it. One in the
background outlives its run, and a session with one running doesn't idle-close or get evicted,
as with any task in the background (4.5.6).
Its requests go to the parent's timeline, on the live or the last run, and last until the session
goes. Nothing can be said to a subagent (`turns.send` is a conflict); `turns.stop` on its thread
stops it alone and never kills the session. A host that comes back settles both row and thread.

On Claude, as recorded from the CLI (0.3.317):

- An `Agent` call's frames name it as `parent_tool_use_id`, whatever turn runs by then. There
  are no stream events for a subagent: `forwardSubagentText` gives its text whole.
- A call in the background returns "launched" at once, which is never its summary. How it ended
  comes in `task_notification`, what it's doing in `task_progress`.
- It ends on `SubagentHandback`, whose message is its report and is shown as its last answer.
  That report goes over the notification's summary, which then only says where the report went.
- A subagent waiting on a shell or monitor of its own (`parent_task_id`) gets a `completed`
  notification saying it hasn't reported yet, and goes on when that task ends. It stays running
  until it hands back, or a notification comes with nothing of its own left running. No more is
  said of a task already told of, so stopping one that waits ends it once the CLI takes the stop.
- The CLI's `<task-notification>` words to a subagent aren't shown as said to it.

### 4.6 Provider seam [proposed]

```ts
interface Provider {
  readonly id: ProviderId;
  readonly capabilities: { steer; resume; images; compact; plan };
  setConfig: (config: Config) => void;   // Koyo's settings, each time they change
  status: (refresh: boolean) => Promise<ProviderStatus>;
  catalog: (cwd: string) => Promise<Catalog>;
  open: (spec: SessionSpec, sink: SessionSink) => ProviderSession;
  // The model's reply as it came, which the actor makes the title of (4.5.7).
  title: (prompt: string, cwd: string) => Promise<string | undefined>;
}
interface ProviderSession {              // each returns once its work is asked for
  start: (turn: TurnInput) => void;      // one turn at a time; events follow on the sink
  steer: (message: TurnInput) => boolean;// false when no turn is running anymore
  interrupt: () => Promise<void>;
  configure: (change: SessionChange) => "applied" | "reopen";  // decided at once (4.5.6)
  kill: () => void;                      // ends the process, and a turn that won't stop
  close: () => Promise<void>;
}
interface SessionSink {
  event: (event: DriverEvent) => void;
  ask: (request: RequestPayload, signal: AbortSignal) => Promise<Response>;
}
interface TurnInput {
  promptId: UUID;                        // fresh for every send
  text: string;
  attachments: { path; mime }[];         // files by their path on disk
  compact: boolean;                      // Koyo's `/compact`; a steer never is
}
DriverEvent = session.ready {mode} | turn.started {promptId}
  | item.started {key, parentKey, item: {kind, payload}} | item.appended {key, text}
  | item.updated {key, item, status} | usage {usage} | request {request}
  | notice {level, text}
  | turn.finished {outcome: "completed"|"interrupted"|"failed", failure, usage}
  | background {tasks}                  // how many run in the background, as that changes
  | session.ended {error}
```

`key` is the provider's own stable id for an item (a tool use id, a message id and a block
index); the actor maps it to an `Item.id` and fixes the ordinal on first sight. A turn's
events all come before its `turn.finished`, and the actor closes what they left open (4.5).
A compaction is an item like any other. What each model can do (efforts, fast, auto) is the
catalog's, not a capability.

What the core decides, no provider does. `SessionSpec.permission` is the chat's mode, else
Koyo's default, and `null` only when neither names one, which leaves it to the provider's
settings (4.5.6). `catalog` says the default mode in the provider's settings, and the host
answers Koyo's over it (4.5.9). `TurnInput.compact` says a turn is a `/compact`, so no
provider reads the prompt's text for it.

### 4.7 The Claude driver [proposed]

The behaviours, each with the T3 adapter pitfall it carries (`t3-claude-adapter.md` 9).

- **One live `query()` per session**, its prompt an async queue the driver feeds; the
  driver iterates the `Query` itself, not the raw generator (1). Options:
  `pathToClaudeCodeExecutable`, `cwd`, `env` (4.2), `sessionId` or `resume`,
  `systemPrompt: {type: "preset", preset: "claude_code", append: KOYO_PROMPT}` (the files a
  reply names written as Markdown links to their paths, which the chat opens in a tab when
  they're clicked), `includePartialMessages: true`,
  `canUseTool`, `model`, `effort`, `permissionMode` only when the spec names one (4.5.6),
  `allowDangerouslySkipPermissions` with Full access,
  `toolConfig.askUserQuestion.previewFormat: "markdown"`, `spawnClaudeCodeProcess` (4.2).
  While Koyo tools are on (O1): `mcpServers: {koyo: {type: "stdio", command: <the app's koyo>,
  args: ["mcp"], env: {KOYO_CHAT: <thread id>}}}`, `allowedTools` the server's 18 read-only
  tools (`mcp__koyo__…`), `disallowedTools: ["EnterWorktree", "ExitWorktree",
  "mcp__koyo__worktree_delete", "mcp__koyo__app_launch"]`, and `KOYO_TOOLS_PROMPT` appended
  after `KOYO_PROMPT`. The
  probe, the files query and the title keep registering nothing.
- **Prompt identity.** Each prompt gets a fresh uuid; root frames are credited to a turn
  only once the CLI echoes it (`user_message_uuid(s)`) or reports it in `command_lifecycle`.
  Frames that arrive while no turn is running (the CLI's own wake turns, when a background
  task ends) get a run of their own with no user message: `turn.adopted`. A result that names
  no turn of ours shows what was held for it rather than dropping it (6, 7, 8).
- **Background.** `background_tasks_changed` lists every task the CLI runs in the background
  whenever that list changes, whatever turn runs: the shells and Monitors the agent started
  and the subagents running past their turn, which it goes on from as they end. Those it marks
  `ambient` (watchers it started on its own) aren't the agent's work. Their count is the
  thread's `backgroundTasks`, sent as the `background` driver event when it changes, and none
  once the CLI's process restarts or the session goes, as the list is the process's. A session
  with one running is kept open, as each one's end starts a turn only its process can (4.5.6).
  As recorded (CLI 2.1.319): a shell started with `run_in_background` and a Monitor are both
  `local_bash` tasks, neither `ambient`. Each one's end, and each line a Monitor prints,
  starts a turn whose result's `origin` is `task-notification`; the list loses an ended task
  before that turn's first frame.
- **Text and thinking** stream per content block (C10). Thinking is shown as reasoning, as
  the CLI shows it, and asked for by `--thinking-display summarized`: left alone, Claude Code
  sends thinking blocks with no text, and the SDK's `thinking` option would also turn thinking on
  for someone who has it off. Rejected: T3's rule that the second thinking block before a tool is text
  (29): it can only be decided once the tool use arrives, so an item already on screen would
  change kind, and it guesses at one model generation's habit.
- **Tools.** A tool item starts on the `tool_use` block or the permission request,
  whichever comes first (33), settles on its `tool_result`, using `tool_use_result` only
  when the message has a single result (32); the actor closes one still open when the run
  settles (4.5). Families: Bash → command; Read → read; Edit, Write, MultiEdit, NotebookEdit
  → edit (with the structured patch); Grep, Glob, LS → search; WebSearch, WebFetch → web;
  `mcp__*` → mcp; Agent → a `subagent` item and its thread (4.5.10); TodoWrite → todo (and the `todos`
  item); Skill → skill; anything else → other. Legacy names go through
  `LEGACY_TOOL_NAME_ALIASES`.
- **Approvals** through `canUseTool` in every mode but Full access, and in plan mode always
  (14, 15). The SDK's abort signal goes with the request to the actor, which withdraws the
  request when it fires (4.5.5); a request withdrawn or refused denies the tool (18).
  Decisions: allow → `{behavior: "allow", updatedInput: input}`; allow for this chat → with
  the SDK's suggestions rewritten to `destination: "session"` (17); deny → `{behavior:
  "deny", message}`; cancel → deny with `interrupt: true`.
- **AskUserQuestion** becomes a question; the answer allows the tool with
  `updatedInput: {questions, answers}`, answers keyed by question text, a multi-select
  joined with ", ".
- **ExitPlanMode** becomes a plan request; the tool is denied with "The client captured your
  proposed plan. Stop here and wait for the user's decision.", and the next turn restores the
  thread's permission mode with `setPermissionMode` (11).
- **Results** are classified, never trusting `subtype: "success"`: `is_error`, an API status
  429 or 529, or an error `terminal_reason` fail the run; `aborted_*` interrupts it (21).
  Rate-limit events give `retryAt`; auth failures read "Sign in to Claude again" (41).
- **Usage and context** from `message_start` and `message_delta`: input counts
  `input + cache_creation + cache_read`; the context window from the result's `modelUsage`;
  the auto-compact point is the window less a 33k reserve (38).
- **Compaction** from `status: compacting`, `compact_boundary` and `compact_result`, as one
  `compaction` item; a turn `TurnInput.compact` marks that didn't compact shows its result
  text as a notice (39, 40).
- **Interrupt**: `interrupt()`, then wait for the result; the process is kept when the result
  comes (T3 closes it; keeping it saves a respawn) and closed when it doesn't (7).
- **Errors on start**: the last lines of the CLI's stderr are kept until the first frame, so
  a refusal that appears only there is shown (45).
- **Unknown frames** (outside the typed union, or a `system` subtype the driver doesn't
  know) are logged at debug and dropped; the driver's `switch` handles every known one, the
  default reached through a `{type: string}` view of the frame, without a cast.
- **Titles**: `query()` with a string prompt, a haiku model, `tools: []`,
  `settingSources: []`, `persistSession: false`, `maxTurns: 1`; the reply goes back as it
  came (4.5.7).

Skipped for now (T3 adapter 10.2): MCP elicitation, the resume dialog, task output tails,
ultracode, PDF budgets; `agentProgressSummaries` and the hidden
`forwardNestedSubagentToolCalls`.

---

## 5. The app

### 5.1 `koyo-rs/koyo-agent` [proposed]

No GPUI here: what doesn't draw is tested without compiling the app.

- `protocol/`: every request, result and event as serde types (`rename_all = "camelCase"`),
  item payloads an enum by `kind` with an `Unknown` variant holding the `serde_json::Value`,
  so a newer host's item never breaks the app (it shows as a notice). Tests: every fixture in
  `koyo-js/koyo-agent-host/test/fixtures/protocol/` parses and re-encodes to the same JSON.
- `client.rs`, `Client`: one connection. Connect or start (4.1), `hello`, one reader thread,
  requests with timeouts, events into a channel that disconnects when the connection ends.
- `agent_client.rs`, `AgentClient`: the app's one client, as `TermClient` is for termd. Nothing
  connects until the first request; a request made after the connection ended connects again,
  which starts a host that's gone. Every connection tells the host Koyo's settings first
  (`config.set`, kept with `set_config`, which tells an open connection at once and starts no
  host), so whatever request starts a host, its sessions open as they say. `subscribe()` gives a `futures` stream of
  `Notice::{Event, Disconnected}`, made once per connection by a thread that moves its events
  there. Two ways to ask:
  - `request::<M>` blocks, on whichever thread calls it, for what may take a while and depends
    on nothing else: `catalog`, `providers.status`, `threads.history`, `threads.watch`.
  - `ordered::<M>` takes its place in one queue at the moment it's called, without blocking,
    and gives a future of the answer. One thread works the queue, so the host hears what the
    person did in the order they did it: a model picked and then a message sent, a tab closed
    and brought back. Spawning each onto a pool could swap them. Everything that changes a
    thread goes this way; the host answers those at once (4.5), so none holds up the rest.
- The version (C9), as a connection is made: the same, it's used; another and idle, the host
  is stopped and the app's started, without asking; another and busy, `ClientError::OtherBuild`
  until the person chooses "Restart chats", `AgentClient::replace_host`, which stops it.
- `replica.rs`, `Replica`: one open thread as the host last said it. `new` from a snapshot;
  `catch_up` from the snapshot of a reopen; `apply` a `thread.changed`; `append` an
  `item.appended`; `prepend` a history page. Rows merge by id, whole, the higher `seq` winning,
  with items in ordinal order and runs and requests in id order; `seq` is what a reopen passes
  as its `sinceSeq`. Pure, so every case the socket can produce is a unit test.
- Shared with termd's client, not written twice: the start and stop of a daemon (its folder's
  files, `connect_or_start` under the start lock, `start` waiting for `ready`, `stop` asking
  first and signalling only while the lock is held) are `koyo-rs/koyo-daemon`, which both
  clients call with the folder and the command; each says its own hello over the stream.
  The folder comes from the app (`settings::data_dir()`), not from a third lookup of
  `KOYO_DATA_DIR`. The executable is found as the browser host is (`browser/engine.rs`):
  `KOYO_AGENT_HOST` when set; else beside the app's executable in a bundle
  (`Contents/MacOS/koyo-agent-host`, which `script/bundle` puts there); else, in a debug build,
  in the checkout
  (`koyo-js/koyo-agent-host/dist/koyo-agent-host`).
- `KOYO_AGENT_PROVIDER` is one of the variables the app passes on, so a build with its own
  `KOYO_DATA_DIR` can run on the fake provider end to end. The fake goes by its own name: the app
  names no provider (`provider: null`, the host's first) until a person can choose one.
- A daemon starts with no signals blocked, whichever thread started it (`koyo_daemon::start`):
  a child inherits its thread's mask, and the app's background threads are GCD's, with nearly
  every signal blocked, so a daemon started from one could never be told SIGTERM.

### 5.2 The chat model in the app [proposed]

`koyo-rs/koyo/src/chat/`, in the shape of `crate::terminals`:

- `Chats`, an entity: the client (`None` in tests, which start no host), thread summaries (from
  `threads.watch`), the open threads by id, the connection's state (idle, connecting, connected,
  another build, failed with why), the threads still to bring back, the catalog by cwd and the
  provider status. It knows nothing of the shell; views ask it for what they need, and the shell
  follows its events. A person who has no store yet (`chats.sqlite`) runs no host until their
  first chat tab, which reaches it before making the chat, so the window lists its chats. The
  client keeps Koyo's settings from the start and on every change (5.1).
- `ChatTabs`, in `Shell.chat_tabs`: each chat pane's view by pane id, and the threads the panes
  show.
- `Pane.chat: Option<ThreadId>`: which thread a chat pane shows; `None` while it's a new chat.
  A pane's view is made when it's first drawn (`make_shown_chats`), as terminals' are, so a
  restored tab costs nothing until it's looked at. Its first send makes the thread, says so to
  the shell, and from there it's like any other.
- `ChatThread`, an entity per open chat: a `Replica` once the snapshot came, what arrived
  before it, and one `Entity<TextViewState>` per text item a row has drawn. A `thread.changed`
  notifies the entity, and sets the text of the rows it changed; an `item.appended` grows the
  replica and the item's own `TextViewState`, so a token delta notifies only that one
  (`TextViewState` parses in the background, appending rather than starting over).
- One loop per app (`follow`, in `chat.rs`), a `cx.spawn` over `subscribe()`: take a notice,
  take all that are waiting, apply them, notify each touched entity once, then wait out the
  frame (16 ms). So appends are applied once per frame however fast they come, and a batch that
  changes nothing notifies nothing.
- A snapshot's answer and the events after it arrive by different roads. The host writes the
  answer first and every event of that connection after it (4.3.4), so whatever reaches an
  entity while an open is unanswered, the first or a reopen, came after its snapshot: it's kept
  and applied on top, or let go if the open fails. A run of the loop never sees an entity
  half-way: the snapshot is applied in one update.
- After `Disconnected`: the state says so, a second's wait, then `threads.watch` and
  `threads.open` with its `seq` for each open thread, over the new connection. It fails, and
  waits twice as long, at most 30 s, until it works or the state is "another build", which waits
  for the person. Nothing else starts the host in a loop; a send made meanwhile connects, and
  shows its own error if it can't.
- **Tabs follow one rule**, not a hook at each of the ways a tab can close (the tab's close
  button, a split's, ⌘W, app control, a worktree gone): after each change of the shell, as
  `Terminals::retain` runs, `ChatTabs::retain` compares the threads the panes show with the ones
  they showed. One no pane shows now gets `threads.update {tabOpen: false}` and `threads.close`,
  and its entity goes; a thread the host deleted is past telling. One newly shown (⌘⇧T, recent
  chats) gets `tabOpen: true`, whatever its summary says: a false may still be in the queue. An
  update that changes nothing commits nothing, and a tab's coming and going leaves `updatedAt`
  alone, as it's nothing done in the chat.
- **Restoring**: the threads with `tabOpen` in the host's *first* list of this run of the app, in
  the order they were made, get tabs that don't take the focus, each as soon as its `cwd` is a
  listed checkout (that list, and every read of a project's worktrees, looks again). Nothing later
  brings a tab back: a chat just made has a summary that may arrive before the answer that names
  it, and the summary of a tab closed a moment ago still says it's open. A thread some pane
  already shows, made while the first list was on its way, is counted as brought back.
- The tab's title is the summary's, a thread removed closes its tabs, and a chat that isn't
  shown keeps its summary for the tab face and the sidebar (M6).
- Sending: the composer clears at once and the item appears when the host says so, a round
  trip of a few milliseconds over the socket; there's no optimistic copy to reconcile. A send
  that fails puts the text back with the error beside it, and retrying repeats the same
  `messageId` (4.3.3). A new chat makes its thread first; the composer waits for that.
- Drafts per thread and one per folder for a chat with no thread yet (text, attachments,
  cursor), in the app, debounced 300 ms to `chat-drafts.json` in the host's folder, flushed on
  quit. A draft has one composer at a time, the first to open it, since it owns the files it
  attaches; another tab on the same chat starts empty and keeps nothing. Its files go when
  they're taken off it or its thread is deleted, not when its tab closes, and a sent draft's go
  with the message. The host reads the file as it sweeps the attachments folder (5.3), so a draft
  keeps its files for as long as it's kept.

### 5.3 The chat tab [proposed]

Layout, top to bottom inside the pane's card:

```
┌──────────────────────────────────────────────────────────────┐
│        timeline (gpui list, centred column ≤ 760 / 1080 px)   │
│                               ┌──────────────────────────┐   │
│                               │ user bubble              │   │
│                               └──────────────────────────┘   │
│   Worked for 1m 3s ›                                          │
│   assistant markdown, streaming…                             │
│   ⌁ Running cargo test · 41s                                  │
│                       [ ⌄ Scroll to end ]                     │
│  ╭────────────────────────────────────────────────────────╮  │
│  │ attached drawers: question · plan ready · todos · queue │  │  Liquid Glass,
│  │────────────────────────────────────────────────────────│  │  one surface
│  │ Ask Claude…                                            │  │
│  │ [+]             Opus 5.5 · High ▾   Ask ▾   Plan   ◔   ⏎ │  │
│  ╰────────────────────────────────────────────────────────╯  │
└──────────────────────────────────────────────────────────────┘
```

- **The details card.** A floating glass card at the top right of a chat says where it works:
  its branch and folder, an editor to open that in, a project's scripts, which run in a terminal
  tab and are kept in settings by project, what can be done next with its work (commit,
  publish or push, pull, create a pull request, or the one it has), what's changed there, its
  subagents (those working, then "Previous agents" behind a line), and how full its conversation
  is. What's known of the branch is read as the card's drawn, asking its remote, if that's over a
  minute old, `HEAD` moved, or a person is back from another app. Pull requests are the GitHub CLI's, where it's installed. It's up where the pane has room for it beside
  the timeline, as a person left it, and over the timeline only when asked for where there's none.
  ⌘⇧B or its button toggles it, and a menu beside the button turns each part on or off.
- **Subagents.** A subagent's row is a card: Claude's little guy, in his orange, on a round mark
  with a dot for how it's doing; what it's called; what it's doing or the start of its report;
  its status and how long it took. Subagents one after the other sit under one line saying how
  many and how they're doing, open until a person closes it. A card opens the subagent's chat in
  the same tab, over the one that started it, with a glass bar in the composer's place: its
  status, what it runs with, Stop while it runs, and Open parent (`escape`).
- **Glass.** The composer and its drawers are one `liquid::backdrop` (radius 18) floating
  12 px above the card's bottom edge, its content in `deferred`; the scroll-to-end pill,
  the suggestions and the menus are glass too. On a dark theme it's the clear style with a 45%
  tint of the card's colour, a deep shade of the theme, so the page shows through; on a light one, glass as `AppKit` makes it. Every glass
  element has a faint line just inside its edge, round its own corners, in place of the bright
  one the glass draws, which is cut off: `glass_edge`, 1 pt, the theme's tint (the hue of the
  gradient theme's first colour, as the card's) at 8 % (dark) or 7 % (light), an inset shadow
  that takes no room. Fallback: `floating()` + hairline + shadow.
  The timeline's bottom inset is the composer's measured height plus 24 px, so the last
  row clears the glass.
- **Working edge.** While the chat works, a brighter stretch of the composer's faint edge goes
  slowly round it, clockwise from the send button's corner: 120 pt long, brightest in its middle
  and fading to nothing at its ends, at 110 pt/s, at most 24 steps a second. It fades in over
  0.7 s as work starts and out over 1.1 s as it ends, going on round meanwhile. A chat works
  while its turn runs and isn't waiting on a person, while any of its subagents runs (one can
  after the turn that started it), or while the agent has a shell or a Monitor running in the
  background that it'll go on from (`Thread.backgroundTasks`), a dev server left running among
  them; waiting on a person alone, it doesn't. Settings › Chat › Composer › While it works:
  "Subtle line" (the default), "Clear line" (brighter, 170 pt, 1.5 pt wide), "Still edge" (the
  whole edge a little brighter, holding still) or "Off" (the faint edge only). A line holds still
  as that brighter edge where motion isn't wanted: Reduce motion (read as the app starts), or
  Koyo not in front. Nothing is drawn for it once nothing works. Only the composer has it; a
  subagent's bar and the other glass keep the faint edge. It's laid over the composer by the tab,
  not drawn in it, and draws where the composer last said its glass is.
- **Empty chat.** The composer sits centred with the worktree's name above it ("Chat in
  koyo / claude-chat-agent-sdk") and moves to the bottom on the first send.
- **Width and text size.** The timeline, the composer and a subagent's bar share one column, as
  wide as Settings › Chat › Width says: Narrow (760 px, the default), Wide (1080 px) or Full
  width, the pane's. The details card is beside a column only where the pane has room for both,
  never beside a full-width one, over which it comes up when asked for. A chat's text is 11 to
  18 pt, 14 by default (Settings › Appearance); a size saved before the sizes came down a notch,
  under `chat_size`, is read as the one at the same place on the slider.
- **Right-click menu.** A right-click on the chat, where what's under the pointer has no menu of
  its own, offers Copy when text is selected, then Width, Text Size and Verbosity, each the
  setting of the same name.
- **Selecting text.** What a chat shows can be dragged over and copied, as in a terminal: the
  person's messages, Claude's answers, a step's line, what a tool printed or was given, the code
  of a diff (not its line numbers), thoughts and to-dos. A drag across rows copies them in the
  order they read, a line each; a click that ends a drag opens nothing.
- **Timeline rows** (from T3 UI 2.1), derived by a pure `rows(thread, view) -> Vec<Row>`
  with stable row ids; what a person opened and each text's markdown view are kept by id
  across derivations:
  - user message: a bubble on the right, at most 80 % wide, in the palette's `wash`, the theme's
    hue laid thin over the card (never the system's accent, which isn't the theme);
    images above the text; long messages collapse past 8 lines with "Show more";
    hover: copy, time; a steer marked "↷ Steered".
  - assistant text: full-width markdown (`TextView`, streaming), no bubble; on hover a copy button
    under each, and under the final answer model · effort, time, the model being the provider's
    default, as the footer names it, for a run that chose none. A link to a file that's there
    reads as the file tree names it, its icon and name (`files::file_label`), and opens it on a
    click as any link to a file does; a link to a page stays a link. What markdown sets apart
    is washed, never the kit's solid greys, which sat on a card of any colour as slabs of
    another (`theme/text.rs`, every text view): a code block and a table's body in `faint_wash`,
    a table's head and inline code in `wash`, their edges, rules and a quote's bar in
    `wash_line`; a tool's output and a diff go in `faint_wash` too, so code reads the same
    wherever a chat shows it.
  - reasoning: one muted line in italics with the latest thought ("Thinking…" shimmering while
    live), expandable to the trace, in italics too.
  - work: one row per tool with icon, label and status ("Ran cargo test", "Read
    src/main.rs", "Changed 3 files", "Searched the web"), an edit's with the lines it added and
    removed ("+3 −5"); consecutive tools form a group whose header is a summary sentence ("Read
    3 files and ran 2 commands"), except a step of a kind that shows more than its line, which
    has a row of its own so the work reads in the order it came, as Claude Code's does; the live
    row is one row whose id never changes while the turn works, showing the running tool and its
    elapsed time ("Running cargo test · 41s"), and gone while the answer writes or a request
    asks, which show for themselves; opened, a command shows its output (mono, ANSI-coloured,
    capped height), an edit its diff in the changes tab's lines and colours (the file's syntax,
    the words a replaced line changed, "⋯" between hunks; plain until the colours are worked out
    off the main thread), a file it created all its lines as added, a search its results, a todo
    its steps, an MCP call its arguments and result.
  - how much of the work shows is a person's choice, its verbosity (Settings › Chat ›
    Verbosity), and what was said shows whichever it is, as an essay followed by a line is still
    what was asked for. Each kind of step shows its line, a few lines of what it gave back ("…
    +12 lines" opens the rest; a command's last lines, as they say how it went, any other's
    first), or all of it: code changes (one line or the diff), command output (one line, the
    last lines, all of it), other tools (one line, the first lines, all of it), thinking and
    to-dos (one line or all of it). A click takes a step from what's chosen to all of it, or
    from all of it to its line. Three presets, and Custom once a kind is set by hand:
    - "Normal", the default, as Claude Code shows it: every change's diff, the last lines of
      every command, the to-dos; reads, searches and thinking a line each, together.
    - "Verbose": every step open, with all it gave back and every thought.
    - "Quiet": each step a line, and once a run settles, its work folds into "Worked for 1m 3s
      ›", where it started (Finished turns: One line). Not folded: the live run, failed and
      interrupted runs, requests, compactions. A run with no work has no fold.
    A settings file from before, with "Work to show", reads as the preset that shows as much.
  - approval: inline where it was asked, never folded: what is asked (the command in mono;
    for an edit, its file and diff; for a write, the file and its head as added lines), why,
    if the CLI said, then Deny · Allow and a menu: what the host says "Allow for this chat"
    adds ("Allow `Bash(npm test:*)` for this chat", wrapped and never cut, as it's what a
    person agrees to) and "Stop". Keys, while the composer is empty: ⌘↩ allow, ⌥⌘↩ allow for
    this chat, ⌘⌫ deny; they're bound only then, so they edit and queue as ever once something
    is typed. Requests are answered oldest first: a later one's buttons wait. What's typed and
    sent declines with its words. Answered, it's one muted line ("Allowed cargo test",
    "Denied cargo test: not now", "No longer asked: cargo test"), and "Worked for" leaves out
    what the run waited on a person for.
  - question: a drawer attached above the composer, one question at a time ("2 of 3", with
    a way back), options as rows with digits 1–9, a single choice advancing after 200 ms, a
    multi-select with Next (↩); the composer is "Other", which a digit past the options
    starts; Skip (⌘⌫ with nothing typed) denies the tool with "The user skipped the
    question"; `preview` renders as markdown beside the options. In the timeline it's a muted
    line for each thing asked: "Asked: …", then "Indent: Tabs", "Skipped: …" or "No longer
    asked: …"; one the next message answers says its options there, and has no drawer.
  - plan: a card ("Plan", its first heading as title, collapsed past 20 lines) with Copy and
    Save as…; a "Plan ready" drawer above the composer: Implement (⌘↩) while the composer is
    empty, Refine once it has text, Dismiss. An empty plan goes back to Claude to write,
    rather than ask a person about nothing. Once answered the card says what became of it.
  - compaction: a centred divider "Context compacted · 182k → 31k tokens", "Compacting
    context…" while live.
  - failure: from the run, a red icon and the message; for a usage limit "Usage limit
    reached · resets 14:30".
  - stop: from the run, a divider "You stopped this response".
  - changed files: under the final answer, "3 files changed +42 −7"; a row opens the file
    (a file tab).
  - queued: dashed user bubbles at the end, "Send now · Edit · Remove".
- **Composer.** `textarea` growing from 1 to 12 lines; Enter sends, ⇧↩ adds a line; while
  running, Enter steers and ⌘↩ queues (the button's icon follows ⌘); ↑ on an empty composer
  recalls the last prompt; ⌘., or Esc on an empty composer, stops.
- **`@` and `/`.** A word starting `@` completes to the worktree's files (`koyo-project`'s
  index, the finder Go to File uses), and goes in as `@path`, which the CLI reads in; a first
  word starting `/` to the commands and skills from the catalog, which go in as `/name`, and
  Koyo's own, which are done at once and stand in for the agent's of the same name: `/model`,
  `/effort`, `/plan`, `/compact`, `/clear` (a new chat). Rows over the footer that leave the keyboard in
  what's typed: ↑/↓ and the page keys move, Enter or ⇥ picks, Esc leaves that word alone. What
  matches nothing shows nothing and takes no keys, so Enter still sends.
- **Attachments.** Pasted, dropped anywhere on the tab, or chosen with `[+]`, each shows as a
  tile from the moment it's asked for, while it's kept in the host's `attachments/` off the main
  thread; a message waits for its tiles. A picture is known by what's in it, not what it's
  called, and goes as it is if the model takes it, or else smaller (2048 px at most, 5 MB at
  most): a photo stays a JPEG, anything else is a PNG while that fits and a JPEG after. Any other
  file is copied and the agent is told its path. What can't be attached says why. A picture alone
  can be sent, and leaves the chat's title to its first words; a message with neither words nor
  files is refused. Words that answer what's asked leave the tiles for the next message, and don't
  wait for them. A message that doesn't go brings them back; a tile taken away takes its file, and
  one left when the tab closes stays with its draft (5.2). What's left in the folder anyway, by an
  app that stopped, the host removes as it starts once no message or draft names it a week on. In the timeline a picture shows at a
  fixed size above the message's words, drawn from a small copy of it, and opens where the person
  would.
- **Footer.** `[+]` attach · model ("Opus 5.5", never "Default": the host labels that row with what
  it runs, by name or else by id; ⚡ when fast) · effort ("Extra high"; none for a model with no efforts) · permission
  ("Ask", brighter when not the default; unknown, so not shown, until the catalog says what
  the person's settings choose) · Plan toggle (⇧Tab) · context ring (hover: "42 % · 84k of
  200k"; press: that and "Compact now", on a menu's surface) · send / stop. Labels collapse
  to icons from the end as the pane
  narrows. What shows is what the thread says, so it's what the agent runs with: a change goes
  to the host and shows when the host says so; a new chat holds what it will start with.
- **Model menu** (the chip, ⌃M as ⌘⇧M is the sidebar's, or `/model`): the same dropdown as the
  permission's, in a popup window of its own below the chip, or over it with no room below: a line
  for each model from the catalog, ✓ on the current, and below a separator Fast's switch, as in
  Settings, if the chat's model can; it flips where it is and leaves the menu up. ↑/↓, Enter picks
  or flips, Esc closes; the focus stays in what's typed. A model keeps only what it can do:
  the effort it was on or the nearest below, else the least it has; Fast if it can; Auto if it can
  (else Ask).
- **Effort** (the chip, or `/effort`): the same dropdown holding the system's slider over the
  efforts of the chat's model, as in Settings, with its Liquid Glass thumb. ←/→ move it, Enter or
  Esc closes. A move goes to the host as it's made. A chat shows what it's set to, or else
  Settings › Chat's "Effort" (Extra high unless changed), fitted to its model the same way, and
  takes that with its next message, so what shows is what runs.
- **Scroll.** Follows the end while at it; any scroll up releases; reaching the end again
  re-arms, and so does sending. The pill "⌄ Scroll to end" (with "· 2 queued") shows 150 ms
  after leaving the end. Position per thread is kept while its tab is open; scrolling to the
  top loads older runs (`threads.history`).
- **Status in the shell.** The tab face shows a spinner while working, a dot in the text's colour
  when it needs you, a dot when unread, red when failed. The sidebar's worktree row shows the
  most urgent of its chats, and a chat with no tab counts only while it's working or needs you.
  A chat on show with the window in front has been seen, and says so to the host (`threads.read`),
  so a mark for what's new or failed goes once it's looked at. A macOS notification when a chat
  needs you or finishes while Koyo isn't frontmost, opening the chat when clicked; only a bundled
  app can post one.

### 5.4 Performance budget [proposed]

- A token delta: one text notify, the rows on screen drawn again, nothing else: list rows are
  laid out on their own, so the framework can't draw one without the view holding the list. The
  timeline and the composer are views of their own, so a token draws no composer and a key no
  row; the tab around them is drawn for either (a deferred element's reads aren't told apart) and
  is kept to very little. A test holds this. 60 fps while streaming 200 tokens/s on a 2,000-item
  thread.
- A view holding what's read while it draws is drawn again for each update of it, notified or
  not, and every token updates the thread and `Chats`: the composer holds copies of what it shows
  of them, changed only when they differ.
- Row derivation runs on structure changes only, and reuses unchanged rows.
- A thread opens with its latest 30 runs; older ones load as the person scrolls up.
- A tab's elapsed label ticks once a second, only while a tool is running; the shimmer moves at
  20 fps, whatever the display's rate, so the rows it draws again don't matter.
- The composer's working edge is a view of its own, so its steps draw neither the composer nor
  the timeline (the tab around it is drawn for each, as for anything floating over a chat); at
  most 24 a second while it moves or fades, none while it holds still, and none once nothing
  works.

---

## 6. Testing strategy [proposed]

- **Host unit tests** (`script/bun test`): the store (change numbers, snapshot, since),
  the status function, the run transitions, the actor on the fake provider (send, steer,
  queue and its hold, stop, force stop, the stop barrier against late tools, approvals
  answered, cancelled and raced, questions, plans, restart recovery, titles not overwriting
  renames), the protocol (every fixture parses; every method and event has a fixture).
- **Claude driver**: sessions recorded from the real CLI, rebuilt frame by frame with
  builders typed by the SDK's own types (`test/fixtures/claude/`), as recorded JSON can't be
  read back as those types without a cast, and replayed into the mapping (text and thinking
  deltas, tools, edits with patches, AskUserQuestion, ExitPlanMode, compaction, interrupts,
  errors); a live smoke test against the real CLI when `KOYO_LIVE_CLAUDE=1`.
- **Host end to end**: the executable with the fake provider (`KOYO_AGENT_PROVIDER=fake`),
  whose sessions answer each turn by saying back what was sent, driven over its socket.
- **Rust**: `koyo-agent` parses every fixture; `Replica` and the row derivation as pure
  functions; the client against the compiled host on the fake provider, which `script/check`
  builds first with `script/agent-host` (a test that can't find it says so and fails, rather
  than skip what it was there to prove).
- **Real app**: driven with `orca computer` on a real Claude session in a test worktree:
  send, stream, approve, deny, ask, plan, stop, queue, restart Koyo mid-turn.

---

## 7. Milestones [proposed]

- **M1 Host core.** Protocol and fixtures, daemon, store, actor, fake provider, tests.
- **M2 Claude driver.** The driver of 4.7, recorded fixtures, the live smoke test, the
  compiled executable (`script/agent-host`, run by `script/dev`), its version in
  `script/daemon-version`.
- **M3 App client.** `koyo-agent`, the shared daemon start and stop, the chat model, the tab
  restored from its thread id.
- **M4 Chat tab.** Timeline, streaming markdown, work rows and folds, the glass composer.
- **M5 The agent's asks.** Approvals, questions, plans, the model and effort menus, permission and plan
  controls, `@` and `/`, images, queue, stop.
- **M6 Koyo-wide.** Tab and sidebar status, notifications, the chat settings page, keys,
  recent chats, rewind (C11), app-control operations for chats.

Each milestone ends with `script/check` green, the simplify pass, and its commits.

---

## 8. Open decisions

- **O1 [decided] Koyo's MCP server in chat sessions.** Every live chat session registers
  Koyo's own server, so "create a worktree", "open … in the browser" and "run the dev server"
  go to `worktree_create`, `tab_open` and `browser_*`, and `terminal_open`, `terminal_wait`
  and `terminal_read`, where the person sees them.
  - Registration: `config.set` carries `koyoTools`, the app's own `koyo`
    (`current_exe()`, so a debug build's chats reach its own build), or `null` while Settings
    → Chat → Koyo tools is off. Each session takes it when it opens (`SessionSpec.koyoTools`)
    and adds a `koyo` stdio entry to the SDK's `mcpServers`: `koyo mcp` with `KOYO_CHAT` set
    to the chat's id (4.7). Nothing is written to the person's Claude config, and their own
    servers still load.
  - Trust: the relay's hello names the chat (`chat`, from `KOYO_CHAT`). The app welcomes it as
    it welcomes an agent in a Koyo terminal (`docs/app-control.md` 4): no MCP-server switch, no
    prompt, the agent grant `read navigate manage browser settings`, `destructive` withheld. It
    welcomes it only when the window knows the chat; before the window has heard its chats
    from the host, the refusal is retryable. The activity log names it `chat:<id>`. While Koyo
    tools are off, its hello and its calls are refused (`control_disabled`). Terminal tools go
    to the daemon, so a session that's already open keeps them until it closes.
  - Steering lives in `KOYO_TOOLS_PROMPT`, which only chats get. It covers the three asks:
    `worktree_create`, and `chat_create` for work in the new worktree, as the chat's own folder
    doesn't move; `tab_open` with `kind: "browser"` and `select: true` when the person asked to
    see the page, as what an agent opens otherwise stays out of sight (`docs/app-control.md`);
    and `terminal_open` with a command rather than Bash in the background.
    The server's `INSTRUCTIONS` go to every client and stay neutral: an external client's
    person may be looking at its own browser. Koyo's tools come first unless the person names
    another. Koyo can't remove the person's own tools (VS Code's browser, agent-browser,
    tmux-dev, orca). With its tools on, it removes Claude's `EnterWorktree` and
    `ExitWorktree` in favour of `worktree_create`: they move the session's folder where the
    chat can't follow. It also removes `worktree_delete`, which isn't granted, and
    `app_launch`, which a chat would only call once the person quit Koyo.
  - Approvals: the read-only tools run without asking, as Claude Code's own reads do. A test
    holds the list to the catalogs. Every other Koyo tool asks as the chat's mode says, under
    family `mcp`.
  - Default on, which overturns the earlier "no for v1, until scoped to the chat's worktree".
    The tools aren't scoped to the worktree: they reach the whole window, as a terminal agent's
    do. What they change goes through the chat's own approvals, and nothing destructive is
    granted.
  - Rejected: the external path, which would need Settings → MCP server and an approval prompt
    for a server Koyo started itself. A wrapper script, or an entry in the person's
    `~/.claude.json` or `.mcp.json`, which would reach their sessions outside Koyo too.
    Steering in `INSTRUCTIONS`, for the reason above. `alwaysLoad`, which would put 66 schemas
    in every prompt rather than behind tool search.
  - Not done: refusing `chat_send` and `chat_stop` aimed at the calling chat itself, a label
    with the chat's title rather than its id, and the hello of a chat whose window is closed
    (refused, as the window's chats can't be read).
- **O2 [decided] Installing Claude Code** when it's missing: a chat shows the install command
  to copy and a "Check again" button, over the composer. Opening a terminal tab with the
  command is left for later.
- **O3 [open] Claude sessions started outside Koyo.** Sessions started in Koyo are ordinary
  Claude sessions on disk; the worktree's other sessions (`listSessions`) could be continued
  in a chat. Default: later.

---

## 9. Glossary

- **Host**: `koyo-agent-host`, the daemon of section 4.
- **Thread / chat**: one conversation, in one worktree, shown in one tab.
- **Run / turn**: one prompt and the work it caused.
- **Item**: one timeline entry: a message, a thought, a tool call, a request, a divider.
- **Request**: what the agent waits on a person for: an approval, a question or a plan.
- **Session**: the provider's live conversation process, resumable by its id.
- **Steer**: a message added to a running turn instead of starting a new one.
