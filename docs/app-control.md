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
  `person_has_control`, `control_required`, `timeout`, `browser_unavailable`,
  `desktop_unavailable`, `git_failed`, `internal`, plus the terminal codes of
  `docs/terminals.md` 8.2.
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
| An agent in a Koyo terminal | relay with `KOYO_TERMINAL` set (`terminal: term_…`, `worktree`) and the terminal exists in the daemon | none | `read navigate manage browser desktop settings` |
| An agent in a Koyo chat | the relay a chat's session starts (`docs/chat.md` O1), with `KOYO_CHAT` set (`chat: <thread id>`); the window knows the chat and Settings → Chat → Koyo tools is on | none; Settings → MCP server needn't be on | as a terminal agent; with Koyo tools off, `control_disabled` for its hello and its calls |
| An external MCP client (stdio relay without a terminal, or HTTP) | `clientInfo.name` (HTTP: `initialize`, else `User-Agent` product, else `MCP client`) | first time per name: “Allow “Claude Code” to control Koyo?” Allow / Deny, 2 min timeout, Deny remembered 10 min | `read navigate manage browser`; `desktop`, `settings` and `destructive` off until switched on in Settings |
| `koyo app` CLI | as an external client named `koyo app` unless `KOYO_TERMINAL` or `KOYO_CHAT` is set | as above | as above |

The socket's trust boundary is the user id (any process of the user can reach it), so
approval is consent, not security; names are self-reported. External names may not start
with `terminal:` or `chat:`. Receipts, prompts, page leases and the activity log know a
caller by `terminal:<handle>`, `chat:<id>` or its name.

Scopes:

| Scope | Allows |
| --- | --- |
| `read` | lists, reads, status, screenshots, resources |
| `navigate` | select projects and worktrees, open/select/move/close tabs, split/focus/resize panes, panels, layout, open settings pages, open files |
| `manage` | add/remove/reorder projects, create worktrees, save files |
| `browser` | drive pages: navigate, eval, click, type, keys, scroll, claim/release |
| `desktop` | drive apps on the agent desktop: open, click, move, drag, scroll, type, keys, focus, close |
| `settings` | `settings_set` (the allowlisted fields) |
| `destructive` | delete worktrees, close tabs discarding unsaved edits; each call also confirmed |

Confirmation: every `destructive` operation asks the person in the window, for every
caller: “Claude Code” wants to delete the worktree “feat-x” (2 unpushed commits, 3
changed files). [Delete] [Cancel]. Cancel or 2 min without an answer fails
`user_declined`; a declined call keeps no receipt. One open prompt per client; a second
fails `request_conflict` (retryable). Deviation from monocode, where sessions skip
confirmations: Koyo's terminal agents have no provider layer deciding MCP calls for
them, so the window asks.

The control policy (on/off, port, approved clients with scopes and last use, whether the
window switches to what agents open, `showWhatAgentsOpen`, and the size a page an agent
opens starts at, `pagesOpenAt`, 5) lives in `settings.json` under `externalControl`; no
operation can read or change it, so no agent can switch it back on for itself. The
executors only follow it. `settings_set` cannot reach it. The activity log keeps every
changing call (refused or not) and every approval/confirmation decision, the latest 200, in
memory; reads aren't logged. Agents read it with `activity_list` (5, Activity): an agent in
a Koyo terminal or chat reads every client's entries, an external client only its own, so
it can't learn which other clients the person allowed (a name is all it would need to pass
for one).

## 5. Tool catalog [decided; schemas proposed]

Hints: R read-only, I idempotent change, C change, D destructive. Scope in brackets.

### Shown or out of sight [decided 2026-11-06]

The person decides whether the window switches to what an agent makes or opens, not the
agent. `project_add`, `worktree_create`, `tab_open`, `pane_split`, `file_open`,
`changes_open` and `chat_create` take `select?: bool`:

- `select` left out follows Settings → MCP server → Switch to what agents open
  (`externalControl.showWhatAgentsOpen`), off by default. Off, what the agent made is listed
  in the sidebar and the tab strip, and the window stays where it was: the selected
  checkout, the tab each checkout shows, the settings page the person is on, and what has
  the keyboard. On, the window switches to it as the person's own menus would.
- `select: true` or `select: false` gets what it says, whatever the setting. Agents are told
  to pass `true` only when the person asked to see the thing.
- What opens out of sight works as if shown: a terminal starts, a file's editor is ready,
  a browser page opens in the engine at once (11) so the browser tools drive it. While an
  agent uses a page no pane draws, never shown or shown and left, the engine has it on
  screen: its animation frames run, clicks and keys land at once, screenshots answer. 90 s
  after the agent's last command it's out of sight for the engine too, and the next command
  brings it back (11, 2026-11-12).
- For every caller: agents in Koyo's terminals, external clients and `koyo app`. The
  person's own menus and dialogs are unaffected, and so are terminals an agent opens with
  `terminal_open`, which already come as tabs that aren't shown.

### Pages agents open [decided 2026-11-08]

A browser page an agent opens (`tab_open` or `pane_split` with `kind: "browser"`) starts in
responsive mode at 1440 × 900, a desktop screen's size, scaled down to fit its pane
(`docs/responsive.md` 4.1, 9), so it lays out and screenshots as on a desktop whatever the
pane's size. The pages the person opens fill their pane, as before.

- `viewport` left out follows Settings → MCP server → Pages agents open
  (`externalControl.pagesOpenAt`): `{ "width": 1440, "height": 900 }` by default, a size of
  the person's own, or `"fill"`.
- `viewport` given gets what it says, whatever the setting, in `browser_viewport`'s shape
  (`ops/browser/viewport.rs` `OpenViewport`): `{ width?, height? }` (a side left out is the
  default's, or the pane's when the default fills it), `{ device, orientation? }`, or
  `{ fill: true }`. A viewport no page takes is `invalid_input`, refused before anything
  opens.
- For every caller, as 5's switch. `browser_list`'s `setBy` names the agent; the device bar
  doesn't. Nothing is remembered: a page brought back at launch fills its pane
  (`docs/browser.md` 7).
- `browser_navigate` never opens a page: with none open it fails `not_found`, pointing at
  `tab_open`. `tab_reopen` (⇧⌘T's) opens the closed tab's pages again at their addresses,
  filling their panes, and a tab a page opens (a link to a new tab) fills its pane: neither
  is an agent opening a page.

Operations whose point is to show something always do: `project_select`,
`worktree_select`, `tab_select`, `pane_focus`, `chat_open`, `settings_open`,
`settings_close`, `app_focus`; and `tab_reopen`, which puts back the tab closed last as
⇧⌘T does. `tab_move` and `pane_detach` change which tab their own checkout shows, never the
selected checkout.

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
settings_open [navigate] I { page?: "general"|"chat"|"accounts"|"appearance"|"editor"|
                         "languages"|"browser"|"keyboard"|"mcp"|"agent_desktop"|"about" } ;
              settings_close [navigate] I {}
```

`sidebar: false` (or ⌘B) is compact mode: the rails fold with the sidebar until every edge is as
thin as the gutter right of the card, and the traffic lights are hidden. The status bar is
tall only while the sidebar is open and the selected checkout has language servers to show.

### Projects and worktrees

```
project_list   [read] R  {} → { projects: [{ path, name, selected, error?, worktrees: [{ path,
                          branch?, detached?, main, selected, tabs, terminals }] }] }
project_add    [manage] I { path, select?: bool (default: the person's setting) } → project;
                          the folder must be a git repository (or inside one: its main
                          checkout is added); listed in the sidebar
project_select [navigate] I { project }
project_remove [manage] D-hint, no confirm { project } → takes it out of the sidebar; files stay
project_move   [manage] I { project, to: index }
worktree_branches [read] R { project? } → { branches: [{ name, remote, current }], defaultBase,
                          prefix, worktreesDir }
worktree_create [manage] C { project?, name, base?: branch, existing?: bool (work on `base`
                          itself), select?: bool (default: the person's setting) } → { worktree:
                          { path, branch } }
                          Same naming as the New Worktree dialog: `<prefix>/<slug>` under
                          `settings.worktrees_dir(project)`. Shown, it's selected with a chat
                          open on it, as the dialog leaves it.
worktree_select [navigate] I { worktree, project? } → selects the checkout (switches to it)
worktree_delete [destructive] D, confirmed { worktree, force?: bool } → refuses the main
                          checkout; the prompt names lost commits, changes, terminals
```

### Tabs and panes

```
tab_list   [read] R { worktree?, all?: bool } → { worktrees: [{ path, tabs: [{ index, selected,
             layout: PaneTree, panes: [PaneInfo] }] }] }
             PaneInfo = { pane, kind: "terminal"|"browser"|"editor"|"changes"|"chat"|
             "desktop", title,
             url?, file?, unsaved?, terminal?: handle, focused }
             `focused` is the pane the person is in, of the shown tab's: the one with the
             keyboard, or, while the sidebar, a dialog or another window has it, the one
             they pressed in or gave the keyboard last; the tab's first until they've been
             in any. `app_status`'s `selected.pane` is the same pane.
             A tab the window brought back and hasn't shown yet is listed as what it will
             show: a page with its `url`, an editor pane with its `file`, the diffs as
             `changes` (`docs/browser.md` 5).
             PaneTree = { pane } | { axis: "row"|"column", ratio, first, second }
tab_open   [navigate] C { worktree?, kind, url?, viewport?: OpenViewport (a page's, default:
             the person's setting, 1440 × 900), file?, line?, column?, terminal?: handle,
             name?: string (new terminal's), command?: string (new terminal's), place?:
             "end"|"beside" (default end), select?: bool (default: the person's setting) }
             → { pane, tab }
             Opens the kind as the menu would; `terminal` reveals an existing daemon terminal;
             `desktop` is the agent desktop's one pane, the one there is or a new one.
             OpenViewport = { device?, width?, height?, orientation?, fill?: true }
             (`browser_viewport`'s fields, Pages agents open above)
tab_select [navigate] I { pane } | { worktree?, index }
tab_move   [navigate] I { pane, to: index }
tab_close  [navigate] I { pane } → refuses with `unsaved_changes` (listing files) when an
             editor in it has unsaved edits, unless `discard: true`, which needs
             [destructive] and is confirmed
tab_reopen [navigate] C {} → reopens the latest closed tab
pane_split [navigate] C { pane (target), edge: "left"|"right"|"top"|"bottom",
             move?: pane (an existing pane, taken from its tab) | kind + tab_open's fields,
             `viewport` among them (a new pane), select?: bool (default: the person's
             setting) } → { pane, tab }
pane_focus [navigate] I { pane } → selects its tab and gives it the keyboard
pane_resize [navigate] I { pane, ratio } → the split holding the pane gives that pane `ratio`
pane_detach [navigate] I { pane } → moves the pane out of its split into a tab of its own
pane_close [navigate] I { pane } → takes the pane out (same unsaved rule as tab_close)
```

### Chats

```
chat_list   [read] R { worktree?, all?: bool } → { chats: [{ id, title, worktree, status:
              "idle"|"working"|"failed", needs?: "approval"|"question"|"plan", queued, unread,
              open, updatedAt, runsWith }] }, latest first; a subagent's chat isn't listed
chat_choices [read] R { worktree? } → { models: [{ id, name, row?, efforts: [effort], fast,
              auto }], modes: [mode], defaults: runsWith }: what the worktree's chats can
              choose, and what chat_create gives what a call leaves out
chat_read   [read] R { chat, turns?: n (default 5) } → { status, messages: [{ role:
              "user"|"assistant", text }] }, oldest first, without the work between the words
chat_open   [navigate] I { chat } → { pane, tab, worktree }: the tab showing it, or a new one
chat_create [manage] C { worktree?, text, model?, effort?, mode?, fast?: bool, plan?: bool,
              select?: bool (default: the person's setting) } → { chat, place, runsWith }:
              a new chat on what's asked, the rest as a new chat in the window, told `text`
chat_send   [manage] C { chat, text, queue?: bool } → { turn }: starts a turn, or joins the one
              running unless `queue`; the chat takes the effort its composer shows first
chat_set    [manage] I { chat, model?, effort?, mode?, fast?: bool, plan?: bool } → { chat,
              runsWith }: another chat (or the caller's own) set as its composer's chips set
              it; what's left out stays
chat_stop   [manage] I { chat } → asks what's running to stop

runsWith = { model?, modelName?, effort?, mode?, fast, plan }, as the chat's composer shows them
effort   = "low"|"medium"|"high"|"xhigh"|"max"
mode     = "ask"|"accept_edits"|"auto"|"full_access"
```

Sending to a chat has the agent work in that checkout, as far as the chat's permissions let it,
so it's `manage`. What the agent asks of a person (approvals, questions, plans) can't be answered
from here: `chat_list` says what a chat waits for, and the person answers in the window.

What a chat runs with [built 2026-11-11] is told as its composer shows it, which is what runs
(`docs/chat.md` 4.5.9), from the host's catalog for its checkout (`catalog` with `commands:
false`, which starts no agent in a folder none ran in): the model a chat chose, else the one the
agent's own settings run (`defaultModel`: Claude Code's `ANTHROPIC_MODEL`, else its `model`
setting), else the first it lists; the effort it chose, else Koyo's `chat_effort`, as near as
the model runs; the mode it chose, else the person's (`chat_mode`), else Claude Code's. A field
Koyo can't tell (the agent isn't ready, the model isn't listed) is left out of `runsWith`.

`chat_create` checks what it's asked for against the same catalog
(`control/exec/chat/choices.rs`): a `model` by its id, what it runs or its name; an `effort` the
model runs at; `fast` on a model that has it; `auto` on a model that can. A refusal is
`invalid_input` naming what can be chosen, and starts nothing. A `mode` is granted as asked,
`full_access` included, whatever the person's new chats get, with no confirmation (12.6), but
a chat's agent gets none above its own chat's mode (12.6). Left out, the model and the mode stay
unchosen as before, and follow the settings; the effort is set, as the composer sets it with a
chat's first message, so the composer and `runsWith` show what runs. A model that can't run in
the Auto the settings choose gets `ask`, as the composer gives it.

`chat_set` [built 2026-11-11] changes a chat as its composer's chips do, through
`threads.update`, checked as `chat_create` is: another model keeps only what it can do (the
effort it was on or the nearest below, fast if it has it, Auto if it can, else Ask), and an
effort, fast or `auto` it can't is refused. The host hears every field asked about, and what
another model changes with it, whether or not the app's copy of the chat already has it, as that
copy lags the host's by a moment; and nothing else. A chat at work takes the change as its
session can (`docs/chat.md` 4.5.6), by its next turn at the latest. A `mode` is granted as asked,
as `chat_create`'s, up to the calling chat's own (12.6); a chat's agent may set its own chat
too, to its own mode or lower.

### Files

```
file_open  [navigate] C { path, line?, column?, worktree?, preview?: bool, split?: edge,
             select?: bool (default: the person's setting; shown, its editor takes the
             keyboard) } → { pane }
file_save  [manage] I { pane? | all: true } → { saved: [path] }
changes_open [navigate] C { worktree?, select?: bool (default: the person's setting) } → the
             Changes tab
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
                   → an open page's; it never opens one
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

### Agent desktop (`docs/agent-desktop.md`, through its host)

Places are desktop points from the agent desktop's top left. An act lands on an element of
the last snapshot by `ref` (a plain click presses it through accessibility, no event) or at
a point (`x`, `y`, both); `window` names the window meant, raised first when another covers
the point. Acts and `desktop_close` fail `person_has_control` (retryable) while the person
acts in the desktop's tab and for 5 s after. `desktop_open` fails it for the person's own
running app (one that takes no second instance, or any app asked for with `personsOwn`),
with a message that tells the agent to stop, say what it wants and ask them: called again
with `consent`, the exact text of the message in which they agreed, the app is lent to the
agent desktop, and given back, never quit (`docs/agent-desktop.md` 5). `desktop_unavailable` (retryable): the agent desktop is off,
still starting, or its host lacks a grant. The reads are `read`, as the browser's.

```
desktop_open       [desktop] C { app: name|bundle id|path, file?: path|URL, args?: [string],
                     personsOwn?: bool, consent?: the person's words }
                     → { app, pid, windows: [DesktopWindow], lent? }; the desktop's tab
                     opens out of sight
desktop_windows    [read] R {} → { windows: [{ window, pid, app, title, x, y, width,
                     height }] }, front to back
desktop_screenshot [read] R { window?, scale?: 1|2 (default 1: a pixel a point) } → image
                     block + { x, y, width, height, scale }: the picture's place on the desktop
desktop_snapshot   [read] R { window?, interactive?: bool, menus?: bool } → { tree: text
                     with `[ref=eN] @x,y` markers, elements, truncated }; refs hold until
                     the next snapshot
desktop_click      [desktop] C { ref? | x?, y?, window?, button?: "left"|"right"|"middle",
                     count?: 1..3, modifiers?: ["cmd"|"shift"|"alt"|"ctrl"] } → { done }
desktop_move       [desktop] I { ref? | x?, y?, window? } → { done }
desktop_drag       [desktop] C { from: { ref? | x?, y? }, to: { ref? | x?, y? }, window? }
                     → { done }
desktop_scroll     [desktop] C { ref? | x?, y?, window?, dx?, dy? (points, about: up to a
                     quarter further; +dy shows what's further down; one non-zero) }
                     → { done }
desktop_type       [desktop] C { text (\n Return, \t Tab; at most 4000 as keys), ref?,
                     replace?: bool (with ref: set the whole value) } → { done }
desktop_key        [desktop] C { keys: "cmd+s"|"return"|"shift+cmd+["…, window? }
                     → { done, copied? }: a copy's text; the clipboard stays the person's.
                     Keys go to the window that has them (last clicked or focused, or the
                     document, panel or sheet its app opened since), or to `window`, which
                     has them from then on
desktop_focus      [desktop] I { window } → { done }; raised, and the keys' from now on
desktop_take_focus [desktop] C { window?, consent?: the person's words } → { done }; the
                     agent desktop on the person's screen, that window in front; refused
                     (person_has_control, with what to ask them) until `consent` is theirs
desktop_return_focus [desktop] I {} → { done }; the person's own desktop on their screen
                     again; nothing when the focus isn't held
desktop_wait       [read] R { quietMs? (default 500, at most 10000), timeoutMs? (default
                     10000, at most 60000) } → { still, millis }
desktop_close      [desktop] D { window? | pid? (one) } → { done }; a window, or quits an app
                     the agent opened (a lent one is given back as it is)
```

### Activity

```
activity_list [read] R { limit?: n (default 50, at most 200) } → { entries: [{ at, client,
                kind: "call"|"approval"|"confirmation", operation?, summary?, error?: { code,
                message }, question?, decision?: "allowed"|"denied"|"timed_out" }],
                onlyYours }, newest first
```

The activity log of 4, which Settings no longer lists. A read, so `read` like every read and
not logged itself. An agent in a Koyo terminal or chat lists every client's entries; an
external client lists its own (`onlyYours: true`): its calls, its approval, the
confirmations of its calls. `client` is the log's key: a client's name, `terminal:<handle>`
or `chat:<id>`.

### Accounts

```
claude_accounts_list [read] R {} → { managed, accounts: [{ email, organisation?, active,
                       windows: [{ name, percent|null, resetsAt?, aheadOfPace }], readAt?,
                       problem?: { kind, message, until? } }], unreadable?, overriddenBy? }
```

The Claude Code accounts the sidebar's accounts panel shows (`docs/claude-accounts.md`): every
account Koyo keeps, in its order, the Mac's login `active`. `windows` are each one's 7 days and
each model's own weekly window as last read, a weekly window past its reset empty again;
`problem` says what keeps it from being read: `kind` is `rate_limited` (with `until`),
`plan_lapsed`, `certificate_refused`, `sign_in_again`, `token_expired`, `keychain_unavailable`
(still signed in), `foreign_login` (the Mac's login is another account's), `offline` or `other`,
and `message` the panel's sentence with its remedy. `unreadable` says why the accounts couldn't
be read at all, and `overriddenBy` names a shell variable (`ANTHROPIC_API_KEY`, …) that makes
Claude Code ignore the login. Times are milliseconds since the epoch. With Manage Claude accounts off, `managed` is false
and nothing is listed. No operation changes the accounts: switching, adding, removing, holding
one out, auto-switch and Manage Claude accounts are the person's, in the panel and Settings →
Accounts, and `settings_set` doesn't reach them (a test holds the catalog to it).

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
  `{"type":"hello","protocol":1,"client":{name,version},"terminal"?,"chat"?,"worktree"?,"cwd"?,"pid"}`
  → `{"type":"welcome","grant":[scopes]}` or an error; then `{"type":"call","id":n,
  "requestId"?,"operation":…,"input":{…}}` → `{"type":"result","id":n,"output":…}` |
  `{"type":"error","id":n,"error":{…}}`; `{"type":"subscribe","topics":[…]}` →
  `{"type":"event","topic":…}`. Calls on one connection may interleave (client multiplexes).
- **stdio relay**: `koyo mcp` (existing subcommand), now the unified server. Without a
  running app, app tools fail `app_not_running` (with `app_launch` listed); with external
  control off and neither `KOYO_TERMINAL` nor `KOYO_CHAT`, `control_disabled`.
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
(`person_has_control`: leave it, use another); what an agent makes or opens shows as the
person chose when `select` is left out, so pass `select: true` only when the person asked
to see it (5). The `terminal_guide` prompt stays; a `koyo_guide` prompt covers the app
tools at length.

## 9. Settings → MCP server [decided]

A Settings page "MCP server": the switch, the server URL with Copy, a port field when the
port is taken, an "Agents" section (shown whether the server is on or not, since it holds
for agents in Koyo's terminals and chats too) with Switch to what agents open (off) and
Pages agents open (a menu: 1440 × 900, the default; a size of the person's own, once set;
Other size…, a dialog taking a width × height, each 50 to 8192; Fill the pane), "Add to"
buttons (Claude Code and Codex copy the command, Cursor and VS Code open their install
links, as monocode `settings/pages/mcp_clients.rs`), the stdio command for agents (`claude
mcp add koyo -- <exe> mcp`), approved clients with last use, per-scope switches and Revoke,
and clients denied lately with Allow. What clients did isn't listed here (2026-11-08): the
owner found the list of no use, and agents read the log with `activity_list` (5).

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

- Crates: `koyo-rs/koyo-app-control` (70 operations, frames, socket, client, `koyo app`),
  `koyo-rs/koyo/src/control/` (listener, gate, approval, confirmations, receipts, activity,
  topics, in-process client, HTTP server in `control/http.rs`, executors in `control/exec/`,
  browser executors in `control/exec/browser/`, the agent desktop's in
  `control/exec/desktop.rs`), `koyo-rs/koyo-term-mcp` (the unified server:
  `backend.rs`, `socket.rs`, `app.rs`, `tools.rs`, `launch.rs`, `subscriptions.rs`,
  `http.rs`), the Settings page in `settings_view/mcp_server.rs`, `mcp_agents.rs` (the
  Agents section) and `mcp_clients.rs`.
- `koyo mcp` lists 85 tools (14 terminal, 70 app, `app_launch`); HTTP lists 84 (no
  `app_launch`). The server is named `koyo`. A chat's session registers it under that name,
  with `KOYO_CHAT` (`docs/chat.md` O1).
- Browser tools run over CDP through `browser/agent.rs`, its refusals typed (`AgentError`). A
  page opens in the engine once its pane is drawn. One an agent opened out of sight is opened
  at once (`BrowserView::open_out_of_sight`) at its responsive mode's size (1440 × 900 by
  default, 2026-11-08), or, filling its pane, at 1280×800, the size a pane assumes before it's
  drawn; once shown a page filling its pane takes the pane's size. Any other page never
  shown, and a tab brought back at launch, answers `browser_unavailable`, not retryable,
  saying to show its tab (`tab_select`, `pane_focus`, and `app_focus` when the window is
  minimized or on another desktop) or open a new page; a start page says `browser_navigate`
  loads one; an engine still starting says to try again shortly (retryable); one that failed
  says the person restarts it (`control/exec/browser/refusals.rs`). A tab brought back at
  launch has no page until it's shown; `browser_list` lists it with its kept address and
  title and a 0 × 0 viewport (`docs/browser.md` 6).
- Not built (found 2026-11-12): a page an agent opens with `select: true` (or with Switch to
  what agents open on) opens once the window draws its tab, as the person's own pages do;
  while Koyo's window is minimized or on another desktop it draws nothing, so the page stays
  unopened and the tools answer as for a page never shown.
- On screen for the engine (2026-11-12): a page is on screen while its pane draws it with no
  notice over it, frame or not (`browser/page.rs`), and while an agent uses it
  (`browser/view/sight.rs`): opening it out of sight, each command through `agent::cdp`, each
  act that claims it and each look of `browser_wait` keep it on screen for `AWAKE_FOR`, 90 s,
  longer than any one tool call takes. Past that a page no pane draws is hidden (Chromium's `WasHidden`: no frames, timers
  slowed, `visibilityState` `hidden`) until the next command. The host applies what it's
  told before the page's browser exists once the browser arrives
  (`koyo-browser-host/src/host/panes.rs` `browser_created`), so a page it has out of sight
  is one Chromium was told is hidden, and the frame clock begins frames of every other.
- Each protocol command a browser tool sends waits 30 s at most for the page, 90 s for a
  script whose promise is awaited (`browser_eval` with `awaitPromise`), within the client's
  own 100 s (`CALL_TIMEOUT`); past it the tool answers `timeout` saying so, and the app stops
  waiting for it (`Browsers::ask`, its limit given by the caller: `script::evaluate` asks
  longer when it awaits). The host lets go of its own note of the command once Chromium
  answers or the page closes.
- `browser_read` cuts the page's text in the page, near its limit.
  `browser_screenshot`'s image goes once, as an MCP image block; its structured answer
  carries `data` empty. Keys carry the same key codes the person's own typing does. Snapshot
  refs are `data-koyo-ref` attributes set by the snapshot's script. Editing shortcuts
  (⌘A, ⌘C, ⌘X, ⌘V, ⌘Z, ⇧⌘Z) carry their editing command, as Chromium on macOS needs.
- `browser_viewport` (responsive mode, `docs/responsive.md`; executor in
  `control/exec/browser/viewport.rs`, the mode in `browser/responsive/`) answers once the page
  showed a frame of the new size (a page the engine has out of sight paints none, so there
  only its report counts; the agent's own change keeps its page on screen for the engine, so
  that one paints) and reported it as `innerWidth`/`innerHeight` (polled every 100 ms, 5 s at most,
  else `settled: false`). With `screenshot: true` its output carries
  `mimeType` and `data` at the top, which the relay turns into an image block as
  `browser_screenshot`'s.
- Agents in terminals and chats get `read navigate manage browser desktop settings`
  (`Scope::AGENT`), never `destructive`; every destructive call is confirmed in the window.
  A chat's agent is welcomed once the window knows the chat; before the window has heard its
  chats from the host, the refusal is `not_found` with `retryable: true`.
- `tab_close` on a terminal tab leaves the terminal running, as the tab's × does.
- `project_add` of a folder inside a repository adds that repository's main checkout.
- Just after launch, before git has listed a project's worktrees, a worktree not found is
  answered `not_found` with `retryable: true`.
- An HTTP session follows each topic once; a client's `lastUsed` is saved at most once a
  minute.
- Not built: the denied list doesn't push a change notification to the Settings page (it
  redraws on the next settings change); `app_status.focused` and the App topic don't
  follow window focus.
- Shown or out of sight (5, 2026-11-06): `control::policy::shows` resolves a call's `select`
  against `externalControl.showWhatAgentsOpen`. An opening out of sight goes through the
  selected checkout as the menus do (`control/exec/open.rs` `on_checkout`; the right panel
  follows it there only for a file or the Changes, which open through it, so a checkout the
  person never looked at gets no panels), then `control/exec/kept.rs` puts the window back
  in the same change (selection, the checkout's shown tab found by a pane in it, the settings page
  by one step Back, the keyboard), so nothing is drawn in between. A new terminal's tab
  arrives once the daemon answers, not shown and without the keyboard (`open_terminal`'s
  `shown`); `add_project_at` and `add_chat` take `select` themselves.
- `activity_list` (5, 2026-11-08): `ops/activity.rs`, executor `control/exec/activity.rs`
  over `control::policy::activity`, read in place; the log's decisions are the operation's
  `ActivityDecision`; the log's size is `ops::activity::KEPT`, which
  `control/activity.rs` keeps to. Recent activity left Settings → MCP server the same day.
- `claude_accounts_list` (5, 2026-11-11): `ops/accounts.rs`, executor
  `control/exec/accounts.rs` over `accounts::view`, the sidebar panel's own; a problem is
  `ProblemKind` with the panel's sentence as its `message`.
- Pages agents open (5, 2026-11-08): `control/exec/open.rs` `open_on`, the opening both
  `tab_open` and `pane_split` go through, checks the pane's fields and makes the mode
  (`page_mode`) from the call's `viewport` (`control/exec/browser/viewport.rs`
  `opening_change`, `browser_viewport`'s mapping) and `externalControl.pagesOpenAt`
  (`control::pages_open_at`), through `browser/responsive/opening.rs` `opening`, which
  resolves it as `browser_viewport`'s change over the default, before anything opens.
  `Shell::open_browser_tab` takes the mode and sets it on the view it makes
  (`BrowserView::start_in`), before anything is sent, so the page opens at that size, out of
  sight or once drawn. The Agents section is `settings_view/mcp_agents.rs`.

## 12. Defaults the person sets for agents [proposed]

The window switching to what agents open (5) was the first behaviour taken from the agents
and the hard-coded defaults and handed to the person; the size pages agents open start at
(5, Pages agents open, 2026-11-08) the second. These are the others a person could
reasonably want to choose. None is built. Each says what happens today and where, the
setting proposed and its default (today's behaviour unless said), whom it protects, and
where it lives:

- `externalControl`: out of every agent's reach (4). For anything an agent could turn to
  its own advantage against the person: more reach, less asking, more interruption. The
  switches go on Settings → MCP server, Agents.
- Ordinary settings: the person's other preferences, on the page they belong to. Left out
  of `settings_set`'s allowlist unless said.

### 12.1 Bringing Koyo forward

- Today: `app_focus` activates the app and its window (`control/exec/app.rs`, `cx.activate`,
  `activate_window`), from whatever app the person is in. The prompt allowing a new client
  activates the window (`control/auth.rs`); a destructive call's confirmation is a sheet on
  the window and doesn't (`control/confirm.rs`), so it can wait two minutes unseen and fail
  `user_declined`. `app_launch` starts Koyo when it isn't running
  (`koyo-term-mcp/src/launch.rs`).
- Proposed: `externalControl.bringForward: "always" | "toAsk" | "never"`, default `"always"`.
  `"toAsk"`: only a prompt waiting on the person (approval, confirmation) activates the
  window; `app_focus` opens a closed window without activating it and bounces the Dock
  icon. `"never"`: prompts bounce the Dock icon too. And `externalControl.agentsStartKoyo:
  bool`, default on; off, `app_launch` fails `control_disabled`, read by the relay from
  `settings.json` since the app isn't running.
- Protects: the person's focus in another app; the same interruption as switching, across
  apps.
- Home: `externalControl`.

### 12.2 Asking before destructive calls

- Today: every destructive call is confirmed in the window, for every caller
  (`Operation::CONFIRM`, `control/confirm.rs`, two minutes). Agents in Koyo's terminals
  can't make one at all: `Scope::TERMINAL_AGENT` has no `destructive`
  (`koyo-app-control/src/scope.rs`, `control/gate.rs`), so `worktree_delete` fails
  `scope_denied` for the agent that made the worktree.
- Proposed: `externalControl.confirm: "always" | "whenWorkIsLost"`, default `"always"`.
  `"whenWorkIsLost"` skips the prompt when the question finds nothing to lose: a
  worktree with no changes, every commit on a branch, no terminal running
  (`exec/worktree.rs` `losses` already counts them). Discarding unsaved edits always loses
  them, so it always asks. No "never".
- Protects: uncommitted changes, unpushed commits, unsaved edits, running terminals.
- Home: `externalControl`.

### 12.3 What agents in Koyo's terminals may do

- Today: `read navigate manage browser desktop settings`, without asking and not changeable
  (`Scope::TERMINAL_AGENT`, `control/gate.rs`); `settings` lets them change appearance,
  layout and the rest of `settings_set`'s allowlist.
- Proposed: `externalControl.terminalAgents: [scope]`, default today's, with the same
  scopes menu as an approved client's. Adding `destructive` lets them delete worktrees and
  discard edits, each still confirmed (12.2); removing `settings` keeps them off the
  settings.
- Protects: the person's work and setup from agents they started themselves, which act
  without any approval prompt.
- Home: `externalControl`.

### 12.4 What a newly allowed client may do

- Today: Allow grants `Scope::DEFAULT_GRANT`, `read navigate manage browser`
  (`koyo-app-control/src/scope.rs`, `control/policy.rs` `approve`); `browser` drives
  pages signed in with the person's sessions.
- Proposed: `externalControl.newClientScopes: [scope]`, default today's, set with a scopes
  menu above the clients list; the prompt says what it grants.
- Protects: the person's data and signed-in pages from a client allowed in a hurry.
- Home: `externalControl`.

### 12.5 A client the person denied

- Today: Deny is remembered ten minutes, then the client's next hello asks again
  (`control/auth.rs` `DENIAL_MEMORY`); the prompt waits two minutes (`APPROVAL_TIMEOUT`).
- Proposed: `externalControl.denials: "tenMinutes" | "untilQuit" | "always"`, default
  `"tenMinutes"`; `"always"` keeps a Blocked list on the page, each with Unblock.
- Protects: the person from a client asking again and again.
- Home: `externalControl`.

### 12.6 Chats agents start

- Today: `chat_create` starts a chat on the `model`, `effort` and `mode` it's asked for, else
  set as a new chat is in Settings → Chat (`chat_mode`, `chat_effort`; `control/exec/chat.rs`),
  so a client holding only `manage` gets an agent working in the checkout with whatever mode it
  asks for, `FullAccess` included, even above what the person's new chats get, and without
  their confirmation [decided 2026-11-11]: the owner chose that over a confirmation sheet and
  over a cap ("if i ask the agent to do it, it should be able to do it"). `chat_set` sets an
  existing chat's the same way.
- A chat's agent gives no chat a mode above its own chat's [decided 2026-11-11, built]: "a chat
  can never dispatch a permission higher than it's own, either in a new chat or in chat_set".
  Its own is the mode it chose, else what its checkout's chats get by the person's settings
  (`chat_mode`) and Claude Code's (`caller_mode` in `control/exec/chat.rs`); one Koyo can't tell
  counts as `ask`. Asked for above it, `chat_create` and `chat_set` refuse with `invalid_input`
  naming what it may choose, and change nothing; a mode left out that would start a new chat
  above it starts it in the caller's own (`chosen_for` in `control/exec/chat/choices.rs`), and
  `runsWith` says so. A chat setting itself is held to the same: it can lower its mode, never
  raise it. The order is `ask` < `accept_edits` < `auto` < `full_access`; planning first isn't
  part of it. Agents outside chats (in Koyo's terminals, external clients) aren't held to one:
  Koyo doesn't know what they may do.
- Nothing marks the chat as an agent's. It notifies as any chat does
  (`chat/notify.rs`: needs the person, or finished, while Koyo isn't frontmost). Whether
  its tab shows is 5.
- Proposed: `externalControl.agentChats.mode: Mode | "asNewChats"`, default `"asNewChats"`;
  set to `ask` or `acceptEdits` it caps what an agent-started chat may do without asking (not
  chosen 2026-11-11: an agent gets the mode it asks for, above). `agentChats.mark: bool`,
  default on (not built today): the tab and Recent chats say "Started by Claude Code". `agentChats.notify: "asAnyChat" | "whenItNeedsYou" | "never"`,
  default `"asAnyChat"`.
- Protects: `mode`, the person from a client reaching more through a chat than its scopes
  give it; `mark` and `notify`, the person's attention.
- Home: `mode` in `externalControl`; `mark` and `notify` in the ordinary settings, on the
  Chat page.

### 12.7 How long the person's use holds a page or a terminal

- Today: a page the person clicks or types in is theirs until 5 s after their last act
  (`browser/control.rs` `IMPLICIT_HOLD`), then agents may act again. A terminal they type
  in is theirs until they give it back, never released by time (`docs/terminals.md`
  D11, the daemon). Take control holds either until given back.
- Proposed: `externalControl.personHold: { pages: seconds | "untilGivenBack", terminals:
  seconds | "untilGivenBack" }`, defaults `5` and `"untilGivenBack"`; seconds from 2 to 60.
- Protects: the person while they're using a page or a terminal an agent works in.
- Home: `externalControl`: an agent shortening it would take pages from the person sooner.

### 12.8 Where agents' tabs open

- Today: `tab_open` puts a tab after the last unless `place: "beside"` (`ops/tab.rs`
  `Place`); `chat_create` and new terminals go after the last (`model/terminals.rs`
  `add_pane_tab`); `file_open` and `changes_open` open beside the shown tab, as the
  person's own do (`files/open.rs`, `changes/open.rs`).
- Proposed: `externalControl.tabsOpen: "end" | "beside"`, default `"end"`: where a tab
  goes when the call leaves `place` out, for every operation that opens one.
- Protects: the order of the person's tab strip, which with 5's default fills with tabs
  they haven't looked at.
- Home: `externalControl`, with the other defaults for agents' calls; nothing an agent
  gains by changing it, but nothing an agent needs to change either.

### 12.9 Files agents open

- Today: `file_open` keeps each file in a tab of its own unless `preview: true`
  (`exec/file.rs`); the person's own clicks in the tree preview (`previewFiles`).
- Proposed: `externalControl.filesOpenAs: "tab" | "preview"`, default `"tab"`: the default
  of a `preview` left out. `"preview"` keeps an agent's files in the one preview tab the
  next file replaces.
- Protects: the tab strip from piling up with files opened out of sight.
- Home: `externalControl`.

### 12.10 The agent's pointer in pages

- Today: `browser_click` shows the agent's pointer travelling to the point and waits for
  it, 0.38 to 0.9 s by distance (`browser/pointer.rs`), with a glow while an agent holds
  the page; none with reduced motion. It waits on a page out of sight too, where nobody
  sees it.
- Proposed: `externalControl.showAgentPointer: bool`, default on. Off, the glow stays and
  clicks don't wait. Separately, a fix rather than a setting: skip the wait on a page out
  of sight.
- Protects: the person's view of what agents do in pages (on), against agents' speed (off).
- Home: `externalControl`: an agent turning it off would act less visibly.

### Not proposed

- A setting per kind for 5 (pages out of sight, chats shown, say): one switch covers it,
  and `select` covers the exceptions. Revisit if people ask for pages treated apart.
- Prompt timeouts (two minutes) and receipt and activity sizes: nobody's preference.
- Turning confirmations off altogether: 12.2's `"whenWorkIsLost"` is as far as it goes.

### What to build first

1. 12.1, bringing Koyo forward: the same interruption as 5, one app over, and small
   (`exec/app.rs`, `auth.rs`); with `"toAsk"`, a confirmation no longer waits unseen.
2. 12.2 with 12.3: asking only when work would be lost, and letting terminal agents
   delete what they made. The question already counts the losses; together they make
   cleaning up an agent's worktrees possible without a prompt for nothing.
3. 12.6's `mode`: closes the way a `manage`-only client reaches `FullAccess` through a
   chat.
4. 12.4: a scopes menu the page already has.

Then 12.8 and 12.9, which matter more now that agents' tabs open out of sight, and the
rest as people ask.
