# Koyo app control: one MCP for the whole app

Specification, written 2026-11-02 on `main`. Author of the vision: Felipe Schulz. Drafted
with Claude. Source of truth for app control; update it when a decision changes.

Ported from monocode's MCP (`/Users/schulzf/github.com/schulzfel/monocode`, branch
`feat/monocode-mcp`, commits `aeb4274c`, `8b0c6a5c`, `65244d8f`; read
`docs/native/mcp.md` there, `crates/monocode-control`, `crates/monocode-mcp`,
`crates/monocode-gpui/src/shell/control/`). Terminals keep their own catalog and daemon
(`docs/terminals.md` 8); this document adds everything else and joins both behind one
MCP server, the "federation" `docs/terminals.md` 8.1 left for later.

Status marks: `[decided]`, `[proposed]` (designed here, confirmed by implementation),
`[not built]`.

---

## 1. What an agent can do [decided]

An agent in a Koyo terminal, or an MCP client on this computer (Claude Code, Codex,
Cursor, VS Code), sees and drives the running window as the person would: projects,
worktrees, tabs, split panes, browser pages, files, terminals, panels, layout and
settings. Changes appear in the window as if the person made them. One server, `koyo`,
lists both the app's tools (this document) and the terminals' tools (`terminal_*`,
`worktree_list`).

## 2. Architecture [decided]

```
 agent ──stdio──▶ koyo mcp ──┬─ app socket (JSON lines) ──▶ app: control pipeline ──▶ Shell
                                └─ termd socket (CBOR)     ──▶ terminal daemon
 MCP client ──HTTP 127.0.0.1:6272/mcp──▶ app (rmcp Streamable HTTP, same pipeline in-process;
                                         terminal tools through its own relay connection)
 shell / scripts ──▶ koyo app <operation> --json '{…}' ──▶ app socket
```

Crates:

- `koyo-rs/koyo-app-control` (`koyo-app-control`), no UI: the `Operation` trait and the
  registry macro making `Command` (monocode `operation.rs`, `command.rs`), `Scope`,
  `ControlError`/`ErrorCode`, frames, the endpoint (socket path), a Unix-socket listener
  and a blocking multiplexing client, `Topic`s, and every operation's typed input/output
  under `ops/`. Adding an operation is its types in `ops/<domain>.rs` and one registry line.
- `koyo-rs/koyo/src/control/`: the app's side. A listener thread per connection, each
  hello and call handed to the UI thread on one channel; the pipeline checks the caller and
  its grant, replays or records receipts, asks for confirmation, runs the executor on the
  `Shell`, logs the call, answers. Executors in `control/exec/<domain>.rs`.
- `koyo-rs/koyo-term-mcp` becomes the unified MCP handler: tools from both catalogs, app calls
  forwarded through a `Backend` (socket client for the stdio relay; the in-process pipeline
  for HTTP), terminal calls through the existing `Executor`.

The control socket is served whenever the app runs. External callers are served only
while Settings → MCP server is on (section 6).

## 3. Conventions [decided]

As monocode and `docs/terminals.md` 8.2:

- Tool names `snake_case` `noun_verb`, `[a-z0-9_]{1,64}`; sentence-case titles; honest
  hints (`read_only`, `destructive`, `idempotent`); `open_world: false`.
- Arguments camelCase; unknown fields rejected (`deny_unknown_fields`).
- Every mutating tool accepts `requestId` (`^[A-Za-z0-9_-]{1,128}$`): a repeat with the
  same input returns the stored result; with other input fails `request_conflict`. Receipts
  are kept in memory per client, the latest 256, for the app's run.
- Results: `structuredContent` plus a one-line summary text (and the JSON as text for
  clients that read only text). Screenshots also as an MCP `image` block.
- Errors: an `isError` result with `{ code, message, retryable, details? }`. Codes:
  `invalid_input`, `not_found`, `ambiguous`, `scope_denied`, `user_declined`,
  `request_conflict`, `control_disabled`, `app_not_running`, `unsaved_changes`,
  `person_has_control`, `control_required`, `timeout`, `browser_unavailable`, `git_failed`,
  `internal`, plus the terminal codes of `docs/terminals.md` 8.2.
- Selectors. `project`: main checkout path, or project name when unique. `worktree`:
  checkout path, branch name (unique across projects, or within `project`), or `"current"`
  (the caller's worktree: `KOYO_WORKTREE`, else its cwd's checkout); when no branch has the
  exact name, the name it was made with (`mcp check` or `mcp-check` for the folder `mcp-check`
  on `schulzfel/mcp-check`), matched as git slugs it. Absent `worktree`
  means the caller's worktree, else the selected one in the window. `pane`: the pane id
  (`u64`, stable while the app runs, listed by `tab_list`). Absent `pane` on a browser tool
  means the browser pane the caller last acted on, else the shown tab's browser pane, else
  `not_found` listing candidates.

## 4. Callers, identity and scopes [decided]

| Caller | How it says hello | Approval | Grant |
| --- | --- | --- | --- |
| An agent in a Koyo terminal | relay with `KOYO_TERMINAL` set (`terminal: term_…`, `worktree`) and the terminal exists in the daemon | none | `read navigate manage browser settings` |
| An external MCP client (stdio relay without a terminal, or HTTP) | `clientInfo.name` (HTTP: `initialize`, else `User-Agent` product, else `MCP client`) | first time per name: “Allow “Claude Code” to control Koyo?” Allow / Deny, 2 min timeout, Deny remembered 10 min | `read navigate manage browser`; `settings` and `destructive` off until switched on in Settings |
| `koyo app` CLI | as an external client named `koyo app` unless `KOYO_TERMINAL` is set | as above | as above |

The socket's trust boundary is the user id (any process of the user can reach it), so
approval is consent, not security; names are self-reported. External names may not start
with `terminal:`.

Scopes:

| Scope | Allows |
| --- | --- |
| `read` | lists, reads, status, screenshots, resources |
| `navigate` | select projects and worktrees, open/select/move/close tabs, split/focus/resize panes, panels, layout, open settings pages, open files |
| `manage` | add/remove/reorder projects, create worktrees, save files |
| `browser` | drive pages: navigate, eval, click, type, keys, scroll, claim/release |
| `settings` | `settings_set` (the allowlisted fields) |
| `destructive` | delete worktrees, close tabs discarding unsaved edits; each call also confirmed |

Confirmation: every `destructive` operation asks the person in the window, for every
caller: “Claude Code” wants to delete the worktree “feat-x” (2 unpushed commits, 3
changed files). [Delete] [Cancel]. Cancel or 2 min without an answer fails
`user_declined`; a declined call keeps no receipt. One open prompt per client; a second
fails `request_conflict` (retryable). Deviation from monocode, where sessions skip
confirmations: Koyo's terminal agents have no provider layer deciding MCP calls for
them, so the window asks.

The control policy (on/off, port, approved clients with scopes and last use) lives in
`settings.json` under `externalControl`; no operation can read or change it.
`settings_set` cannot reach it. The activity log keeps every changing call (refused or
not) and every approval/confirmation decision, the latest 200, in memory; reads aren't
logged.

## 5. Tool catalog [decided; schemas proposed]

Hints: R read-only, I idempotent change, C change, D destructive. Scope in brackets.

### App and window

```
app_status [read] R    {} → { version, windowOpen, focused, route: "workspace"|"settings:<page>",
                         layout, sidebarOpen, rightPanel: null|"changes"|"files", bottomPanelOpen,
                         selected: { project?, worktree?, pane? }, mcp: { http, url? }, dataDir }
app_focus  [navigate] I {} → brings the window forward, opening it when closed
app_launch (stdio relay only, answered by the relay) → starts Koyo when it isn't running
           and waits up to 30 s for its socket
layout_set [navigate] I { sidebar?: bool, rightPanel?: "changes"|"files"|"none",
                         bottomPanel?: bool, layout?: "orca"|"arc" } → app_status subset
settings_open [navigate] I { page?: "general"|"chat"|"appearance"|"editor"|"languages"|
                         "browser"|"keyboard"|"mcp"|"about" } ; settings_close [navigate] I {}
```

`sidebar: false` (or ⌘B) is compact mode: the rails fold with the sidebar until every edge is as
thin as the gutter right of the card, and the traffic lights are hidden. The status bar is
tall only while the sidebar is open and the selected checkout has language servers to show.

### Projects and worktrees

```
project_list   [read] R  {} → { projects: [{ path, name, selected, error?, worktrees: [{ path,
                          branch?, detached?, main, selected, tabs, terminals }] }] }
project_add    [manage] I { path } → project; the folder must be a git repository (or inside
                          one: its main checkout is added); selects it
project_select [navigate] I { project }
project_remove [manage] D-hint, no confirm { project } → takes it out of the sidebar; files stay
project_move   [manage] I { project, to: index }
worktree_branches [read] R { project? } → { branches: [{ name, remote, current }], defaultBase,
                          prefix, worktreesDir }
worktree_create [manage] C { project?, name, base?: branch, existing?: bool (work on `base`
                          itself), select?: bool (default true) } → { worktree: { path, branch } }
                          Same naming as the New Worktree dialog: `<prefix>/<slug>` under
                          `settings.worktrees_dir(project)`.
worktree_select [navigate] I { worktree, project? } → selects the checkout (switches to it)
worktree_delete [destructive] D, confirmed { worktree, force?: bool } → refuses the main
                          checkout; the prompt names lost commits, changes, terminals
```

### Tabs and panes

```
tab_list   [read] R { worktree?, all?: bool } → { worktrees: [{ path, tabs: [{ index, selected,
             layout: PaneTree, panes: [PaneInfo] }] }] }
             PaneInfo = { pane, kind: "terminal"|"browser"|"editor"|"changes"|"chat", title,
             url?, file?, unsaved?, terminal?: handle, focused }
             PaneTree = { pane } | { axis: "row"|"column", ratio, first, second }
tab_open   [navigate] C { worktree?, kind, url?, file?, line?, column?, terminal?: handle,
             name?: string (new terminal's), command?: string (new terminal's), place?:
             "end"|"beside" (default end), select?: bool (default true) } → { pane, tab }
             Opens the kind as the menu would; `terminal` reveals an existing daemon terminal.
tab_select [navigate] I { pane } | { worktree?, index }
tab_move   [navigate] I { pane, to: index }
tab_close  [navigate] I { pane } → refuses with `unsaved_changes` (listing files) when an
             editor in it has unsaved edits, unless `discard: true`, which needs
             [destructive] and is confirmed
tab_reopen [navigate] C {} → reopens the latest closed tab
pane_split [navigate] C { pane (target), edge: "left"|"right"|"top"|"bottom",
             move?: pane (an existing pane, taken from its tab) | kind + tab_open's fields
             (a new pane) } → { pane, tab }
pane_focus [navigate] I { pane } → selects its tab and gives it the keyboard
pane_resize [navigate] I { pane, ratio } → the split holding the pane gives that pane `ratio`
pane_detach [navigate] I { pane } → moves the pane out of its split into a tab of its own
pane_close [navigate] I { pane } → takes the pane out (same unsaved rule as tab_close)
```

### Chats

```
chat_list   [read] R { worktree?, all?: bool } → { chats: [{ id, title, worktree, status:
              "idle"|"working"|"failed", needs?: "approval"|"question"|"plan", queued, unread,
              open, updatedAt }] }, latest first; a subagent's chat isn't listed
chat_read   [read] R { chat, turns?: n (default 5) } → { status, messages: [{ role:
              "user"|"assistant", text }] }, oldest first, without the work between the words
chat_open   [navigate] I { chat } → { pane, tab, worktree }: the tab showing it, or a new one
chat_create [manage] C { worktree?, text, select?: bool (default true) } → { chat, place }:
              a new chat, set as Settings says, told `text`
chat_send   [manage] C { chat, text, queue?: bool } → { turn }: starts a turn, or joins the one
              running unless `queue`
chat_stop   [manage] I { chat } → asks what's running to stop
```

Sending to a chat has the agent work in that checkout, as far as the chat's permissions let it,
so it's `manage`. What the agent asks of a person (approvals, questions, plans) can't be answered
from here: `chat_list` says what a chat waits for, and the person answers in the window.

### Files

```
file_open  [navigate] C { path, line?, column?, worktree?, preview?: bool, split?: edge } → { pane }
file_save  [manage] I { pane? | all: true } → { saved: [path] }
changes_open [navigate] C { worktree? } → the Changes tab
```

### Browser (on the app's browser panes, via CDP `AppRequest::Cdp`)

The page lease of `browser/control.rs` applies: acting needs the lease; a free page is
claimed on the first act; `person_has_control` means the person holds the page. Its message
says which way: a page they're only using (they clicked or typed in it) comes back on its
own 5 s after their last act, so wait and retry; a page they took with Take control is
theirs until they give it back: don't retry or wait, open another page.

The person sees an agent at work (`browser/pointer.rs`): a glow inside the page's edges
while an agent holds it, and for `browser_click` the agent's pointer, a navigation arrow
that travels to the point along an arc of an ellipse, easing in and out, its heading the
arc's. The click waits for it to arrive, 0.38 to 0.9 s by distance (none with reduced
motion), and sends a ring out where it lands; the pointer fades 6 s after its last click.

```
browser_list     [read] R {} → { pages: [{ pane, url, title, loading, worktree, controller?,
                   viewport }] }
browser_navigate [browser] C { pane?, url, wait?: "load"|"none" (default load), timeoutMs? }
browser_go       [browser] C { pane?, action: "back"|"forward"|"reload" }
browser_read     [read] R { pane?, format?: "text"|"markdown"|"html" (default text),
                   selector?, limit? } → { url, title, content, truncated }
browser_snapshot [read] R { pane?, interactive?: bool } → { url, title, tree: text with
                   `[ref=eN]` markers }; refs stay valid until the next snapshot or navigation
browser_screenshot [read] R { pane?, fullPage?: bool, format?: "png"|"jpeg" } → image block
browser_eval     [browser] C { pane?, expression, awaitPromise?: bool } → { value }
browser_click    [browser] C { pane?, ref? | selector? | x?, y?, button?, clickCount? }
browser_type     [browser] C { pane?, ref? | selector?, text, clear?: bool, submit?: bool }
browser_press_key [browser] C { pane?, key: "Enter"|"Tab"|"Escape"|"ArrowDown"|"Meta+A"… }
browser_scroll   [browser] C { pane?, dx?, dy?, ref? | selector? (scroll into view) }
browser_wait     [read] R { pane?, for: "load"|"selector"|"text"|"url", value?, timeoutMs? }
                   → { met, elapsedMs }
browser_claim    [browser] I { pane?, takeover?, reason? } ; browser_release [browser] I { pane? }
browser_viewport [browser] I { pane?, device?, width?, height?, scale?, mobile?, touch?,
                   userAgent?, orientation?, zoom?, fill?: true, screenshot?: bool }
                   → { viewport, settled } (+ image block with `screenshot`); responsive mode,
                   `docs/responsive.md` 4.1
```

### Settings

```
settings_get [read] R {} → the allowlisted settings, camelCase
settings_set [settings] I { appearance?: "system"|"light"|"dark", layout?, previewFiles?,
             saveScope?: "shown"|"all", inlayHints?, semanticColouring?, sidebarOpenOnLaunch?,
             worktreesDir?: path|null, branchPrefix?: { githubUser } | { custom } | { none },
             mostLanguageServers?: int|null, stopIdleLanguageServers?, sidebarWidth?,
             devtools? } → the settings after
```

Not reachable: `projects` (project tools), `externalControl` (the person only), themes'
gradients, fonts and language server tables `[not built]`.

### Terminals

The `terminal_*` tools and `worktree_list` of `docs/terminals.md` 8.5, unchanged, under
their own `terminal.*` scopes. `tab_open {kind: "terminal"}` and `pane_split` place a
terminal in the window; `terminal_open` keeps revealing a tab as before.

## 6. Transports [decided]

- **Socket**: `<data_dir>/control/koyo-control.sock` (`settings::data_dir()`, so
  `KOYO_DATA_DIR` moves it), mode 0600, folder 0700; when the path is over 100 bytes,
  `$TMPDIR/koyo-<uid>-<hash8>/control.sock`. Frames are newline-delimited JSON:
  `{"type":"hello","protocol":1,"client":{name,version},"terminal"?,"worktree"?,"cwd"?,"pid"}`
  → `{"type":"welcome","grant":[scopes]}` or an error; then `{"type":"call","id":n,
  "requestId"?,"operation":…,"input":{…}}` → `{"type":"result","id":n,"output":…}` |
  `{"type":"error","id":n,"error":{…}}`; `{"type":"subscribe","topics":[…]}` →
  `{"type":"event","topic":…}`. Calls on one connection may interleave (client multiplexes).
- **stdio relay**: `koyo mcp` (existing subcommand), now the unified server. Without a
  running app, app tools fail `app_not_running` (with `app_launch` listed); with external
  control off and no `KOYO_TERMINAL`, `control_disabled`.
- **HTTP**: the app serves rmcp Streamable HTTP at `http://127.0.0.1:6272/mcp` while
  Settings → MCP server is on (off by default; port is a setting). `127.0.0.1` only;
  `Host` must be `127.0.0.1:<port>` or `localhost:<port>`; an `Origin` other than those is
  refused `403`; other paths `404`. Protocol revisions as rmcp 3.8 serves them.
- **CLI**: `koyo app help` lists operations; `koyo app <operation> [--json '{…}']
  [--request-id ID]` prints the JSON result, exit 1 with the error envelope on failure.

## 7. Resources and subscriptions [proposed]

```
koyo://app          app_status                      subscribable (topic app)
koyo://projects     project_list                    subscribable (topic workspace)
koyo://tabs         tab_list {all: true}            subscribable (topic workspace)
koyo://settings     settings_get                    subscribable (topic settings)
```

plus the terminals' resources of `docs/terminals.md` 8.6. The app pushes a topic when the
Shell or settings notify, batched every 100 ms; the relay says
`notifications/resources/updated` without content.

## 8. Server instructions [decided]

The MCP `instructions` say: you are connected to Koyo, the app the person works in;
projects, worktrees, tabs, panes, browser pages, settings and terminals are driven
through these tools, never by editing Koyo's files; start with `app_status` or
`project_list`; pane ids come from `tab_list`; the human rule for terminals and pages
(`person_has_control`: leave it, use another). The `terminal_guide` prompt stays; a
`koyo_guide` prompt covers the app tools at length.

## 9. Settings → MCP server [decided]

A Settings page "MCP server": the switch, the server URL with Copy, a port field when the
port is taken, "Add to" buttons (Claude Code and Codex copy the command, Cursor and VS
Code open their install links, as monocode `settings/pages/mcp_clients.rs`), the stdio
command for agents (`claude mcp add koyo -- <exe> mcp`), approved clients with last use,
per-scope switches and Revoke, clients denied lately with Allow, and Recent activity.

## 10. Testing [decided]

- `app-control`: catalog (names valid and unique against the terminal catalog, schemas are
  objects, changing ops accept `requestId`), frames round-trip, socket hello/call/error,
  multiplexed calls, receipts.
- `koyo`: pipeline tests on a headless `Shell` (gpui test-support): approval, scopes,
  confirmations, receipts, each executor's effect on the workspace.
- MCP e2e: a real MCP client against `koyo mcp` and the HTTP endpoint, against the
  built app. Real-surface QA drives the running app through those tools.
- `script/check` green (clippy pedantic, 500-line files).

## 11. As implemented (2026-11-02)

- Crates: `koyo-rs/koyo-app-control` (45 operations, frames, socket, client, `koyo app`),
  `koyo-rs/koyo/src/control/` (listener, gate, approval, confirmations, receipts, activity,
  topics, in-process client, HTTP server in `control/http.rs`, executors in `control/exec/`,
  browser executors in `control/exec/browser/`), `koyo-rs/koyo-term-mcp` (the unified server:
  `backend.rs`, `socket.rs`, `app.rs`, `tools.rs`, `launch.rs`, `subscriptions.rs`,
  `http.rs`), the Settings page in `settings_view/mcp_server.rs` and `mcp_clients.rs`.
- `koyo mcp` lists 60 tools (14 terminal, 45 app, `app_launch`); HTTP lists 59 (no
  `app_launch`). The server is named `koyo`.
- Browser tools run over CDP through `browser/agent.rs`, its refusals typed (`AgentError`). A
  page the host hasn't opened yet (a tab never shown) answers `browser_unavailable` until it's
  shown once. `browser_read` cuts the page's text in the page, near its limit.
  `browser_screenshot`'s image goes once, as an MCP image block; its structured answer
  carries `data` empty. Keys carry the same key codes the person's own typing does. Snapshot
  refs are `data-koyo-ref` attributes set by the snapshot's script. Editing shortcuts
  (⌘A, ⌘C, ⌘X, ⌘V, ⌘Z, ⇧⌘Z) carry their editing command, as Chromium on macOS needs.
- `browser_viewport` (responsive mode, `docs/responsive.md`; executor in
  `control/exec/browser/viewport.rs`, the mode in `browser/responsive/`) answers once the page
  showed a frame of the new size (a page out of sight paints none, so there only its report
  counts) and reported it as `innerWidth`/`innerHeight` (polled every 100 ms, 5 s at most,
  else `settled: false`). With `screenshot: true` its output carries
  `mimeType` and `data` at the top, which the relay turns into an image block as
  `browser_screenshot`'s.
- Agents in terminals get `read navigate manage browser settings`, never `destructive`
  without the confirmation prompt; every destructive call is confirmed in the window.
- `tab_close` on a terminal tab leaves the terminal running, as the tab's × does.
- `project_add` of a folder inside a repository adds that repository's main checkout.
- Just after launch, before git has listed a project's worktrees, a worktree not found is
  answered `not_found` with `retryable: true`.
- An HTTP session follows each topic once; a client's `lastUsed` is saved at most once a
  minute.
- Not built: the activity and denied lists don't push a change notification to the Settings
  page (it redraws on the next settings change); `app_status.focused` and the App topic don't
  follow window focus.
