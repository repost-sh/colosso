# Driving Koyo

Koyo is the app the person works in: projects in a sidebar, each with its git worktrees,
each worktree with its tabs, each tab split into panes that show a terminal, a browser
page, an editor, the changes or a chat. These tools see and drive the running window as
the person would, and what you change appears there as if the person had done it. Drive
Koyo only through them: never edit its settings, data or other files.

## Finding your way

- `app_status` says what the window shows (the workspace or a settings page), its layout
  and panels, and the selected project, worktree and pane. Start here.
- `project_list` lists the projects in sidebar order, each with its worktrees: path,
  branch, whether it's the main checkout or selected, and how many tabs and terminals it
  has.
- `tab_list` lists a worktree's tabs (every worktree's with `all: true`), each with its
  split layout and its panes: the pane id, kind, title, address, file, unsaved edits,
  terminal handle and focus.
- `browser_list` lists the browser panes, with who controls each page.

The same reads are resources: `koyo://app`, `koyo://projects`, `koyo://tabs` and
`koyo://settings`. Each can be subscribed to; an update says only that it changed, so
read it again.

## Saying what to act on

- `project`: the main checkout's path, or the project's name when no other has it.
- `worktree`: a checkout path, a branch name (unique across projects, or within `project`),
  or `"current"`, the worktree you run in. Left out, it means yours, else the one selected
  in the window.
- `pane`: the pane id from `tab_list`, a number that stays the same while the app runs.
- A browser tool without `pane` acts on the browser pane you last acted on, else the shown
  tab's browser pane; with neither, it fails `not_found` and lists the candidates.

An `ambiguous` failure lists what the input could mean in `details.candidates`: pick one
and call again.

## Projects and worktrees

- `project_add` with a `path` adds a git repository (or the repository the folder is in)
  and selects it; `project_select`, `project_move` (`to`, an index) and `project_remove`
  (out of the sidebar; its files and worktrees stay on disk) take a `project`.
- Before creating a worktree, `worktree_branches` gives the branches, the default base, the
  branch prefix and the folder new worktrees go in.
- `worktree_create` with a `name` makes a branch `<prefix>/<slug>` from `base` (or works on
  `base` itself with `existing: true`) in the project's worktrees folder, as the New
  Worktree dialog does, and switches to it unless `select: false`.
- `worktree_select` switches to a worktree.
- `worktree_delete` deletes a checkout with its tabs and terminals. It never deletes the
  main checkout, and the person confirms every call in the window.

## Tabs and panes

- `tab_open` opens a tab as the menu would. `kind` is `terminal`, `browser`, `editor`,
  `changes` or `chat`; a terminal takes a `name` and a `command`, or reveals an existing
  daemon terminal by its `terminal` handle; a browser page takes a `url`; an editor a
  `file`, `line` and `column`. `place: "beside"` opens it next to the current tab. It
  answers the new pane's id, its tab and its worktree.
- `tab_select` (by `pane`, or by `index` in a worktree), `tab_move` (`to`, an index),
  `tab_close` and `tab_reopen` (the tab closed last).
- `pane_split` splits `pane` on an `edge` (`left`, `right`, `top`, `bottom`) and puts there
  either an existing pane (`move`, taken from its tab) or a new one (`kind` with
  `tab_open`'s fields).
- `pane_focus` selects a pane's tab and gives it the keyboard; `pane_resize` gives a pane a
  `ratio` of its split; `pane_detach` moves it into a tab of its own; `pane_close` closes
  it.

`tab_close` and `pane_close` refuse with `unsaved_changes` when an editor there has unsaved
edits, listing the files in `details.files`. Save them with `file_save`, or pass
`discard: true`, which needs the destructive scope and the person's confirmation. Closing a
terminal's tab leaves the terminal running in the daemon.

The terminal tools (`terminal_*`) drive terminals whatever tab shows them; the
`terminal_guide` prompt covers them.

## Files

- `file_open` opens a `path` in an editor at a `line` and `column`, in a split with
  `split`, or as a preview tab with `preview: true`.
- `file_save` writes one editor's edits (`pane`) or every editor's (`all: true`).
- `changes_open` shows a worktree's changes.

## Browser pages

Open a page with `tab_open` (`kind: "browser"`, `url`) or `pane_split`, and find one with
`browser_list`.

Reading needs nothing more: `browser_read` gives the page as text, Markdown or HTML, whole
or one element's (`selector`); `browser_snapshot` gives its accessibility tree with
elements marked `[ref=eN]`; `browser_screenshot` gives an image; `browser_wait` waits for
the page to load, or for an element, text or address to appear (`met: false` when time ran
out is an answer, not an error). Prefer the snapshot and the read to screenshots.

Acting needs the page's lease, which your first act on a free page claims:
`browser_navigate`, `browser_go` (`back`, `forward`, `reload`), `browser_click`,
`browser_type` (with `clear` and `submit`), `browser_press_key`, `browser_scroll` and
`browser_eval`. Point at elements with a snapshot's `ref` rather than coordinates, and
snapshot again after the page navigates, since refs don't outlive it. `browser_claim` and
`browser_release` take and give back the lease outright.

`person_has_control` means the person holds the page, and its message says which way. A
page they're only using (they clicked or typed in it) comes back on its own 5 s after
their last act: wait, then retry. A page they took with Take control is theirs until they
give it back: do not wait for it, do not retry, do not claim it with takeover; open
another page for your own work.

`browser_viewport` shows a page as a phone or tablet (`device`: `iphone-se`, `iphone-15`,
`iphone-15-pro-max`, `pixel-8`, `galaxy-s24`, `ipad-mini`, `ipad-pro-11`, with its user
agent, touch and mobile layout) or at a free `width` × `height`, scaled down to fit the
pane, under the page's lease like any act. It answers once the page has laid out at the new
size (`settled: false` after 5 seconds means it hadn't), and `screenshot: true` adds an
image of it. To check a layout, set the size, screenshot, then `fill: true`; the person
sees what you do, and their own change wins. `browser_list` shows each page's `viewport`.

## The window and settings

- `app_focus` brings the window forward, opening it when it's closed.
- `layout_set` opens or closes the sidebar, the right panel (`changes`, `files` or
  `none`) and the bottom panel, and switches the layout (`orca` or `arc`).
- `settings_open` shows a settings page and `settings_close` leaves it; neither changes a
  setting.
- `settings_get` reads the settings agents may see, and `settings_set` changes them:
  fields left out stay as they are, and `null` resets `worktreesDir` and
  `mostLanguageServers`. It needs the settings scope. Projects and the MCP server's own
  policy can't be changed this way.

## Repeats, scopes and confirmations

Every changing tool accepts a `requestId`. A repeat with the same id and the same input
returns the first result instead of acting again; the same id with other input fails
`request_conflict`. Give one to anything you might send twice, such as a retry after a
timeout or a lost connection.

What a client may do comes in scopes: `read`, `navigate`, `manage`, `browser`, `settings`
and `destructive`. An agent in a Koyo terminal holds all but `destructive`. Any other
client is first allowed by the person, once, and holds `read`, `navigate`, `manage` and
`browser`; the person switches on `settings` and `destructive` in Settings → MCP server.
`scope_denied` names the missing scope in `details.scope`.

Deleting a worktree, or discarding unsaved edits, asks the person in the window, whoever
calls. While one confirmation of yours waits, another fails `request_conflict` (retryable).
A refusal, or no answer within two minutes, fails `user_declined`, and nothing changed.

## Errors

A failure has a `code`, a `message` and `retryable`; retry only what is retryable.

| Code | What to do |
| --- | --- |
| `invalid_input` | Fix the arguments the message names. |
| `not_found` | The target is gone or never was: list again. |
| `ambiguous` | Pick one of `details.candidates`. |
| `scope_denied` | The person hasn't allowed `details.scope`; ask them, or do without. |
| `user_declined` | The person said no, or didn't answer; nothing changed. Don't repeat it unasked. |
| `request_conflict` | The `requestId` was used with other input, or a confirmation of yours is waiting. |
| `control_disabled` | The person has to turn on Settings → MCP server. |
| `app_not_running` | Koyo isn't running: call `app_launch`, then try again. When a connection was lost mid-call, the call may have acted: check, or repeat it with the same `requestId`. |
| `unsaved_changes` | Save `details.files` first, or discard them with the person's confirmation. |
| `person_has_control` | The person holds the page or terminal. A page they're only using comes back 5 s after they stop (`retryable`): wait, then retry. Otherwise use another. |
| `control_required` | Claim the page first. |
| `timeout` | Retry, or wait for something narrower. |
| `browser_unavailable` | The browser engine is restarting: retry shortly. |
| `git_failed` | Git refused; the message says why. |
| `internal` | Something broke in Koyo; the message says what. |
