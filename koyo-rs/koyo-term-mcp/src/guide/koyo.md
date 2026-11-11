# Driving Koyo

Koyo is the app the person works in: projects in a sidebar, each with its git worktrees,
each worktree with its tabs, each tab split into panes that show a terminal, a browser
page, an editor, the changes, a chat, the agent desktop or an iPhone or iPad. These tools see and drive the running window as
the person would, and what you change appears there as if the person had done it. Drive
Koyo only through them: never edit its settings, data or other files.

## Finding your way

- `app_status` says what the window shows (the workspace or a settings page), its layout
  and panels, and the selected project, worktree and pane, and whether the window is on
  screen at all (`onScreen`: not minimized or on another desktop). Start here.
- `project_list` lists the projects in sidebar order, each with its worktrees: path,
  branch, whether it's the main checkout or selected, and how many tabs and terminals it
  has.
- `tab_list` lists a worktree's tabs (every worktree's with `all: true`), each with its
  split layout and its panes: the pane id, kind, title, address, file, unsaved edits,
  terminal handle and focus.
- `browser_list` lists the browser panes, with who controls each page.
- `activity_list` lists what clients did lately, newest first: every changing call with
  its summary or error, and the person's answers to approvals and confirmations. An agent
  in a Koyo terminal or chat reads every client's entries; any other client its own.
- `claude_accounts_list` lists the Claude Code accounts Koyo shows in the sidebar: which is
  the Mac's login, each one's 7-day and per-model usage with when it resets, and what keeps
  a reading from being taken. Switching accounts stays with the person.

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

## What the person sees of your work

The person decides whether the window switches to what an agent makes or opens.
`project_add`, `worktree_create`, `tab_open`, `pane_split`, `file_open`, `changes_open` and
`chat_create` take `select`:

- Left out, the person's choice in Settings → MCP server decides. By default the window
  stays where they are: what you made is listed in the sidebar and the tab strip, and
  nothing takes their keyboard or leaves the settings page they're on.
- `select: true` switches the window to it. Pass it only when the person asked to see it.
- `select: false` keeps it out of sight whatever they chose.

What you open out of sight works as if shown: a terminal runs, a browser page loads and
the browser tools drive it. `worktree_select`, `project_select`, `tab_select`,
`pane_focus`, `chat_open`, `settings_open` and `app_focus` exist to show something and
always do, as does `tab_reopen`, which puts back the tab closed last as ⇧⌘T does.

## Projects and worktrees

- `project_add` with a `path` adds a git repository (or the repository the folder is in)
  to the sidebar; `project_select`, `project_move` (`to`, an index) and `project_remove`
  (out of the sidebar; its files and worktrees stay on disk) take a `project`.
- Before creating a worktree, `worktree_branches` gives the branches, the default base, the
  branch prefix and the folder new worktrees go in.
- `worktree_create` with a `name` makes a branch `<prefix>/<slug>` from `base` (or works on
  `base` itself with `existing: true`) in the project's worktrees folder, as the New
  Worktree dialog does, and lists it in the sidebar; with `select: true` the window
  switches to it, with a chat open on it.
- `worktree_select` switches to a worktree.
- `worktree_delete` deletes a checkout with its tabs and terminals. It never deletes the
  main checkout, and the person confirms every call in the window.

## Tabs and panes

- `tab_open` opens a tab as the menu would. `kind` is `terminal`, `browser`, `editor`,
  `changes`, `chat`, `desktop` (the agent desktop's one tab) or `device` (a device tab on its
  picker: `device_open` opens one on a device); a terminal takes a `name` and a `command`, or reveals an existing
  daemon terminal by its `terminal` handle; a browser page takes a `url` and a
  `viewport`; an editor a `file`, `line` and `column`. `place: "beside"` opens it next to the current tab. It
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

## Chats

`chat_list` lists a worktree's chats with an agent (`all` for every worktree's), whether a tab
shows them or not, each with its `id`, `status`, what it `needs` of a person, and `runsWith`:
the model, effort and mode it runs on, as its composer shows them.

- `chat_create` starts a chat in a worktree and says `text` to it, in a tab of its own, shown
  only as `select` and the person's setting say. It takes the `model`, `effort` and `mode` the
  person asked for, and `fast` and `plan`; what's left out is as a new chat in the window gets
  it. `chat_choices` lists what a worktree's chats can choose and what a new one gets; what a
  model can't run with is refused, naming what it can. A `mode` is granted as asked,
  `full_access` included, without the person confirming it: ask for one only as they did. An
  agent in a Koyo chat gets none above its own chat's mode, and a new chat it starts with `mode`
  left out runs no higher than its own either. Tell the person what it started from the
  answer's `runsWith`, not from what you asked for.
- `chat_send` says `text` to a chat: it joins the turn running, or with `queue` waits for it.
- `chat_set` changes another chat's `model`, `effort`, `mode`, `fast` or `plan`, as its
  composer's chips do; what's left out stays, and another model keeps only what it can do. It's
  checked and granted as `chat_create`'s are, up to your own chat's mode for an agent in a chat,
  which may lower its own but never raise it; tell the person what it's set to from `runsWith`.
- `chat_read` gives what was said in the latest `turns` (5 when left out). Poll `chat_list` for
  when `status` is back to `idle`, then read.
- `chat_open` shows a chat; `chat_stop` asks what it's running to stop.

Its agent works in that checkout as far as the chat's permissions let it. What it asks of a
person (`needs`: `approval`, `question` or `plan`) only the person can answer, in the window: a
chat stays `working` until they do. Tell them rather than wait on it.

## Files

- `file_open` opens a `path` in an editor at a `line` and `column`, in a split with
  `split`, or as a preview tab with `preview: true`.
- `file_save` writes one editor's edits (`pane`) or every editor's (`all: true`).
- `changes_open` opens a worktree's Changes tab, its diff.

## Browser pages

Open a page with `tab_open` (`kind: "browser"`, `url`) or `pane_split`, and find one with
`browser_list`. `browser_navigate` moves a page that's open; it never opens one. The page
you open last is the one your browser calls mean when they leave out `pane`; name `pane`
whenever more than one page is about, so you never act on the person's.

A page opens in the engine at once, shown or not. `browser_list` says whether the engine
has each one `open` (the tools reach only an open page; a tab brought back at launch has no
page until the person shows it) and `onScreen`. Out of sight, a page you use is on screen
for the engine all the same: its animation frames run, clicks and keys land at once,
screenshots answer. 90 s after your last command it goes out of sight (its timers slow and
`visibilityState` turns `hidden`), and your next command brings it back.

A page you open starts in responsive mode at 1440 × 900, a desktop screen's size, scaled
down to fit its pane, so it lays out and screenshots as on a desktop whatever the
pane's size. That's the person's default for pages agents open, in Settings → MCP server;
they may have chosen another size, or to have pages fill their pane. To open one otherwise,
pass `viewport` as `browser_viewport` takes it: `{ "width": 1280, "height": 720 }`,
`{ "device": "iphone-15" }`, or `{ "fill": true }` for the whole pane. `browser_list` shows
what each page got.

Reading needs nothing more: `browser_read` gives the page as text, Markdown or HTML, whole
or one element's (`selector`); `browser_snapshot` gives its accessibility tree with
elements marked `[ref=eN]`; `browser_screenshot` gives an image; `browser_wait` waits for
the page to load, or for an element, text or address to appear (`met: false` when time ran
out is an answer, not an error; a page that can't open or a selector the page can't read
fails at once). Prefer the snapshot and the read to screenshots. `browser_navigate` and
`browser_go` say whether the page finished loading (`loaded`).

Acting needs the page's lease, which your first act on a free page claims:
`browser_navigate`, `browser_go` (`back`, `forward`, `reload`), `browser_click`,
`browser_type` (with `clear` and `submit`), `browser_press_key`, `browser_scroll` and
`browser_eval`. Point at elements with a snapshot's `ref` rather than coordinates, and
snapshot again after the page navigates, since refs don't outlive it. `browser_claim` and
`browser_release` take and give back the lease outright.

A page's `alert`, `confirm`, `prompt` or question before it's left holds it up: every
command on it waits behind it. A command under way when the page asks stops at once with
`browser_unavailable` naming the dialog (an act that opened it landed: don't repeat it),
and `browser_list` shows it as the page's `dialog`. Answer it with `browser_dialog`
(`accept`, with `text` for a prompt, or `dismiss`). A sign-in, permission or certificate
question is the person's to answer.

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

## Agent desktop

The agent desktop is a desktop of your own in the person's session, which Koyo shows in one
tab, "Agent desktop". An app you open there is an instance of your own, apart from the
person's; their pointer, keyboard and windows are never touched. The desktop tools need the
`desktop` scope; their reads need only `read`.

The loop:

1. `desktop_open` starts an app by name (`TextEdit`), bundle id or path, with a `file` or
   URL to open, and answers its `pid` and windows. It opens the tab out of sight. The app
   is the person's own, with their files and sign-ins; a document app also opens with their
   unsaved documents, which are theirs: leave them as they are.
   An app the person is running that takes no second instance (Arc, Firefox, System
   Settings), or their own running instance of any app, asked for with `personsOwn: true`,
   is refused with `person_has_control` first: see "The person's own app" below.
2. Look: `desktop_snapshot` gives the accessibility tree, elements marked `[ref=eN]` and
   `@x,y`, their middle; `desktop_screenshot` gives an image; `desktop_windows` lists the
   windows, front to back.
3. Act by `ref`, or by point: `desktop_click`, `desktop_move`, `desktop_drag`,
   `desktop_scroll`, `desktop_type` and `desktop_key` (`cmd+s`, `return`, `shift+cmd+[`).
4. `desktop_wait` until nothing changes (`still: false` when time ran out is an answer,
   not an error), then look again. Refs hold until the next snapshot.

After `desktop_open`, and after anything that changes a window's layout, `desktop_wait`
before reading places: an app still laying itself out gives stale ones. Places are desktop
points from the desktop's top left. A screenshot at scale 1 has one pixel per point; a
window's screenshot says its `x` and `y`, which you add to a point in the picture to get
desktop points. When windows overlap, `window` on an act names the
window you mean, raised first. Prefer refs: a plain click on one presses it through
accessibility, with no event at all. Fall back to points where the tree doesn't reach.
The menu bar isn't in the picture: macOS draws none for a desktop that isn't showing. With
`menus: true` a snapshot lists the apps' menu bars, and a menu item is pressed by its ref
without the menu opening (a menu's title itself isn't pressed). Use that, and a pop-up
button's items by ref, rather than opening a menu where you can: a menu that is up takes
the person's keyboard, so it is closed under you the moment they type, and after a minute.
A scroll goes about as far as asked (up to a quarter
further): look again after it. Hovering does nothing, and a shift-click doesn't extend a
selection: drag to select, or use the keys.

The person's own app. Bringing an app the person is running onto the agent desktop takes
every window of it off their desktops, with what they have open in it, and you then act in
their app with their data and accounts. It needs their explicit consent, each time. When
`desktop_open` refuses with `person_has_control` and says so: stop; tell the person which
app you want to bring over, why, what you will do in it, and that its windows leave their
desktop until it is given back; ask whether they want that. Only if they clearly agree,
call `desktop_open` again with `consent` set to the exact text of their message. An unclear
answer is not consent, nothing they said before the refusal counts, and `consent` is never
your own words. Their yes counts once: after the app went back, it is refused again, and
you ask again. The answer then says `lent: true`. Work in it as in any app, and leave what
they had open as it was unless they asked otherwise. When you are done, `desktop_close`
with its `pid` gives it back to them as it is (it is not quit). They can take it back
themselves at any moment, in the tab: its windows are then gone from the agent desktop,
your refs into it are dropped, and you ask again before bringing it back. Tell them when
that cut short something you were doing.

Taking the focus. What you open on the agent desktop yourself works unseen, a browser or
an Electron app included: its pages are drawn, and are in `desktop_snapshot`. The person's
own browser, lent, is the exception: a Chromium browser (Arc, Chrome) that came from their
desktop draws no page, and gives none to read, while the agent desktop isn't on their
screen. For a web page, Koyo's own browser tools are the first choice. When it has to be
their app, and only then, `desktop_take_focus` puts the agent desktop on the person's screen with your
window in front, in place of what they are doing. It is theirs to allow, each time: the
first call is refused with `person_has_control` and says what to tell them and ask; only
if they clearly agree, call again with `consent` set to the exact text of their message.
Do what needed the screen, and give the focus back with `desktop_return_focus` at once:
their own desktop comes back. They can take it back themselves by going to any desktop of
theirs, and you ask again before taking it again.

Typing goes to the window last clicked, focused or named in a `desktop_key`, and from
there to the one its app moves the keys to by itself: the document it just made, the panel
it opened, the sheet on the window. `desktop_focus` brings a window in front and sends keys
to it. `desktop_type` with a `ref` focuses that field first; with
`replace: true` it sets the field's whole value instead, with no keys. `\n` is Return and
`\t` Tab; more than 4000 characters as keys is refused. To save a new document where you
mean to: `cmd+s`, then `shift+cmd+g` in the sheet, the folder's path, Return, the name,
Return. A copy or a cut (`cmd+c`, `cmd+x`)
with `desktop_key` gives the text back in its answer, `copied`, and the person's clipboard
stays theirs: to paste text of your own, use `desktop_type`. Copy with the keys, not with a
menu's Copy, which would take the person's clipboard. `desktop_close` closes a
`window`, or quits an app you opened (`pid`).

`person_has_control` means the person is acting in the tab, or the app is theirs: don't
retry at once. Their use passes a few seconds after they stop (`retryable`): wait, then
look again before acting; an app of theirs stays theirs. `desktop_unavailable` means the
agent desktop is off, still starting, or lacks a grant: tell the person (Settings → Agent
desktop) rather than retry in a loop.

## Devices

Koyo shows iPhone and iPad simulators of its own, each live in a tab. You drive them as a
finger and the device's keys and buttons would, nothing more. The device tools need the
`devices` scope; their reads need only `read`.

The loop:

1. `device_list` lists the simulators with their state, tab and screen size; `device_open`
   starts one (or makes one from a `model`, such as `iPad Pro 13-inch (M5)`) and opens its
   tab out of sight. It answers within a minute: `ready: true` with the screen, or
   `ready: false` with the `step` it is at (a first boot takes about a minute): call it
   again until it's ready.
2. Look: `device_snapshot` gives what's on the screen, elements marked `[ref=eN]` and
   `@x,y`, their middle. Its `source` says how it was read: `accessibility`, the app's own
   elements with their roles and labels, its first line naming the app; or `text`, words
   read off the picture, where the tree had nothing (just after a first boot, often). A
   `text` ref is where its words are: on the home screen an app's name is under its icon,
   where a tap doesn't open it, so open apps with `device_launch`. `device_screenshot`
   gives an image.
3. Act by `ref`, or by point: `device_tap` (`count`, `holdMs` for a long press,
   `fingers`), `device_swipe`, `device_scroll` (moves the content by `dx`, `dy` without a
   glide), `device_pinch`, `device_type` (`into` a ref taps it first), `device_key`
   (`cmd+a`, `return`) and `device_button` (`home`, `lock`, the volume buttons, and the
   system's gestures by name: `app_switcher`, `control_center`, `notification_center`).
4. Read every act's `change`: `none` means nothing on the screen changed, so it most likely
   didn't take: look again before assuming it worked. `small` is a switch or a cursor;
   `large` a new screen. Refs hold until the next snapshot.

Places are points of the picture as it shows, upright, from its top left: an iPhone 17 Pro
is 402 × 874. A screenshot at scale 1 has a pixel a point. After launching or switching
apps, `device_wait` until the screen is still before you look: an app that hasn't drawn yet
isn't in the snapshot, and the one before it may stand in for it. A system alert (a
permission prompt) is SpringBoard's and is in the snapshot as an app of its own; tap its
buttons by ref. Something below the fold isn't in the snapshot: `device_scroll`, then look
again. `device_type` types ASCII only: other text is refused, naming the characters, and
nothing is typed. `device_rotate` turns a simulator.

Apps: `device_apps` lists what's installed; `device_install` installs an `.app` built for
the simulator (`xcodebuild -sdk iphonesimulator`); `device_launch` and `device_terminate`
take its bundle id (`com.apple.Preferences` is Settings); `device_open_url` opens a link in
what opens it. `device_list` gives a simulator's `set`, so your own
`xcrun simctl --set <set> … <id>` reaches it too, to build and install with; start and stop
simulators with `device_open` and `device_close`, not `simctl boot`, or they outlast Koyo and
count against the person's limit of simulators running at once (two unless they changed it).

`person_has_control` means the person is using the device in its tab (`retryable`: wait a
few seconds, then look again before acting) or took it (not: do something else).
`device_unavailable` means it is starting or its host is restarting: wait and look again.
`device_needs_person` means only the person can go on (Xcode or a simulator system missing,
a locked device): stop and tell them what the message says.

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

What a client may do comes in scopes: `read`, `navigate`, `manage`, `browser`, `desktop`,
`devices`, `settings` and `destructive`. An agent in a Koyo terminal or chat holds all but
`destructive`. Any other client is first allowed by the person, once, and holds `read`,
`navigate`, `manage` and `browser`; the person switches on `desktop`, `devices`, `settings`
and `destructive` in Settings → MCP server.
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
| `control_disabled` | The person has to turn on Settings → MCP server; for a chat, Settings → Chat → Koyo tools. |
| `app_not_running` | Koyo isn't running: call `app_launch`, then try again. When a connection was lost mid-call, the call may have acted: check, or repeat it with the same `requestId`. |
| `unsaved_changes` | Save `details.files` first, or discard them with the person's confirmation. |
| `person_has_control` | The person holds the page, terminal, agent desktop or device. A page, the desktop or a device they're only using comes back 5 s after they stop (`retryable`): wait, then retry. Otherwise use another. |
| `control_required` | Claim the page first. |
| `timeout` | Retry when `retryable`, or wait for something narrower. A browser act that timed out may have acted all the same (`retryable: false`): look at the page before doing it again. |
| `browser_unavailable` | The page can't take the command now, as the message says. Retryable: the engine is starting, or the page is between documents; try again shortly. Not retryable: the page waits on a dialog (answer it with `browser_dialog`), a tab brought back at launch or never shown (ask the person to show it, or open your own page), a start page (`browser_navigate` loads one), a failed engine (the person restarts it), or a closed page. |
| `desktop_unavailable` | The agent desktop is off, starting or lacks a grant, as the message says: tell the person (Settings → Agent desktop); don't retry in a loop. |
| `device_unavailable` | The device is starting, or its host restarting: wait a few seconds and look again. Not retryable when it isn't open: `device_open` it. |
| `device_needs_person` | Only the person can go on (Xcode or a simulator system missing, a locked device): tell them what the message says. |
| `git_failed` | Git refused; the message says why. |
| `internal` | Something broke in Koyo; the message says what. |
