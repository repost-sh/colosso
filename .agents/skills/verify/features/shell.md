# Shell

The shell is Koyo's one window. On its left is the sidebar: in the Orca layout a list of
projects with their worktrees and tabs; in the Arc layout an icon rail, spaces, pinned tiles
and loose pages. The rest of the window shows the selected tab, which may be split into panes.
The person opens tabs from the launcher (⌘T) or the sidebar, moves between them with ⌥↑/⌥↓ and
the tab switcher (⌃⇥), and closes them with ⌘W.

## Sub-features

- `shell-status`: the window reports what it shows (project, worktree, tab, on screen).
- `shell-project-add`: a folder is added as a project and listed in the sidebar.
- `shell-tab-open`: a tab of each kind opens in a worktree (`terminal`, `browser`, `editor`,
  `changes`, `chat`).
- `shell-tab-select`: a tab is selected by the sidebar, ⌥↑/⌥↓ or app control.
- `shell-tab-close`: ⌘W closes the shown tab. ⌘⇧T reopens it.
- `shell-split`: a pane splits to an edge, and focus moves between panes.
- `shell-layout`: Orca ⇄ Arc, the sidebar toggled with ⌘B, the right panel with ⌘⌥B, the bottom
  panel with ⌘J.
- `shell-switcher`: ⌃⇥ shows the switcher with a picture of each tab. Releasing ⌃ picks one.
- `shell-launcher`: ⌘T opens the launcher. Tab and ⇧Tab change its kind (chat, page, file…).
- `shell-kept`: tabs and panes come back after a relaunch (`window.json`).

## How to get to it (user POV)

- The sidebar's rows (projects, worktrees, tabs), its `+` buttons and the new-tab button.
- ⌘T for the launcher, ⌘W, ⌘⇧T, ⌥↑/⌥↓ between tabs, ⌥←/⌥→ between projects, ⌃⇥ / ⌃⇧⇥ for
  the switcher, ⌘B, ⌘⌥B, ⌘J, ⌘⇧M (sidebar mode), ⌘N (new worktree).
- The menu bar's View and Window menus. These are native menus, and they would show on the
  owner's desktop: don't open them.
- App control: `app_status`, `project_add`, `project_list`, `tab_open`, `tab_list`,
  `tab_select`, `tab_close`, `pane_split`, `pane_focus`, `layout_set`.

## Driving it with koyo-verify

Preconditions: the baseline. `$B app tab_list '{}'` lists no tab yet.

- **Status.** Read what the window shows with `$B app app_status '{}'`. `dataDir` is
  `.superpowers/v/d`, and the fixture repository is the selected project.
- **Open a tab of each kind.** Run `$B app tab_open '{"kind":"terminal","command":["/bin/zsh","-f"],"select":true}'`,
  `$B app tab_open '{"kind":"chat","select":true}'` and
  `$B app tab_open '{"kind":"browser","url":"file://'$PWD'/.superpowers/v/fixture/focus.html","viewport":{"fill":true},"select":true}'`.
  Each answers `{pane, tab, worktree}`. `$B app tab_list '{}'` lists all three, and within about
  half a second `.superpowers/v/d/window.json` lists them under the fixture's checkout.
- **Select with keys.** Make the window key (SKILL.md, Keyboard focus), then press ⌥↑:
  `$B probe 'key 126 "" alt' 'wait 300' state`. `$B app app_status '{}'` shows the tab above as
  the selected one. Take a shot to see the highlighted row.
- **Close and reopen.** `$B probe 'key 13 w cmd' 'wait 300'` closes the shown tab, which
  leaves `tab_list`. `$B probe 'key 17 t cmd shift' 'wait 500'` brings it back.
- **Split.** `$B app pane_split '{"pane":<pane>,"edge":"right","kind":"terminal","command":["/bin/zsh","-f"]}'`.
  A shot shows two panes. `$B app pane_focus '{"pane":<new pane>}'` moves the focus.
- **Layout.** `$B app layout_set '{"layout":"arc"}'` switches to Arc: the shot shows the icon
  rail and tiles. `$B app layout_set '{"sidebar":false}'` hides the sidebar, as ⌘B does
  (`$B probe 'key 11 b cmd'`).
- **Switcher** (a floating panel: SKILL.md, rule 5, so close it at
  once). Hold ⌃ and press ⇥ with `$B probe 'key 48 "\t" ctrl'`, then take
  `$B shot switcher`. The panel lives in a window of its own, so get its number from
  `$B probe windows` and pass it to `shot`. The panel is a glass panel. To check its pictures,
  use the visual check (SKILL.md, Other harnesses, `--visual-check-switcher`).
- **Kept across a relaunch.** Run `$B stop --keep`, ask for the desktop again, run
  `desktop_open` and `$B confirm`, then `$B app tab_list '{}'`. The same tabs come back. A page
  tab comes back with no page until it's shown (`open: false` in `browser_list`).
- **Proof.** Take a shot after each act (`$B shot shell-<step>`), keep the `window.json`
  excerpt in the transcript (`$B note "$(jq -c . .superpowers/v/d/window.json)"`), and record
  the `tab_list` answers.

## Gotchas

- With `select` left out, a tab opens out of sight: it is listed but not shown. Pass
  `"select": true` when the journey needs it shown.
- `tab_close` on a terminal tab leaves the terminal running, as the tab's × does.
- A terminal tab opened without `command` runs the owner's login shell, with their rc files
  and history. Always pass `["/bin/zsh","-f"]`.
- Just after launch, before git has listed the worktrees, a worktree can answer `not_found`
  with `retryable: true`. Wait a second and try again.
- `window.json` is written about 500 ms after a change. Read it after a `wait`.
- Pinned and loose pages aren't in `tab_list` (see [browser-pages.md](./browser-pages.md)).
- The launcher (⌘T) and the switcher (⌃⇥) are floating panels that may show on the owner's
  screen (SKILL.md, rule 5). Prefer `tab_open` for opening tabs, and keep panel journeys short.
- Native menus (the menu bar, a right-click's menu) would show on the owner's desktop. Pick a
  menu item by its key equivalent instead, and close any menu with `probe escape`.
