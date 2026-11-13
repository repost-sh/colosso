# Koyo's Arc sidebar: spaces, project tiles and worktree folders

Written 2026-11-08 on branch `schulzfel/arc-like-sidebar`. Drafted with Claude.

The Arc layout's sidebar is rebuilt to look and work like Arc's own: browser controls and an
address bar at the top, the projects of a **space** as tiles, the space's name, and each
worktree of the selected project as a folder with its tabs under it. Spaces are switched
along the sidebar's foot. This note is the spec the work followed; keep it in step with the
code in `koyo-rs/koyo/src/arc/` and `koyo-rs/koyo/src/spaces.rs`. The visual check draws it,
and then does what a person does in it (a space's mark saying a chat in it works and waits, a
tab's row and its tile saying the same, a tile dragged to another row and back, the address
bar following the pane pressed in, ⌘L on a page), saving a picture of each. It shows a chat, so it wants a data folder and an agent host
of its own, and says so if it isn't given them:

```sh
Q=$PWD/.superpowers/qa/arc && mkdir -p $Q/tmp &&
  KOYO_AGENT_PROVIDER=fake KOYO_DATA_DIR=$Q/data TMPDIR=$Q/tmp \
  KOYO_CONFIG_DIR=$Q/config KOYO_LOG_DIR=$Q/logs \
  script/cargo run -p koyo --features visual-check -- --visual-check-arc
```

The Orca layout still lists every project. In both layouts the icon rail down the window's left
is gone, a gutter in its place: **Settings…** is in the app's Koyo menu, ⌘, opens it, and the
settings sidebar has a ← back to the workspace (Escape and ⌘W go there too). Switching layout
is ⌘⇧M or the rails' right-click menu, and the appearance is in that menu and in Settings.

---

## 1. Top row: window and browser controls

- The traffic lights, then the sidebar toggle, on the left of the sidebar's top row.
- Back, forward and reload on the right of the same row, in Arc's style. They act on the
  page of the pane the person is in (section 2), and in a pane of another kind on the tab's
  first page, so a page beside a chat can be reloaded from the chat. They're greyed out on a
  tab with no page and on a settings page.
- A page has no toolbar of its own in this layout: the sidebar has its controls and its
  address. Responsive mode and the `DevTools` are in the page's right-click menu (Responsive
  mode, Inspect, Show or Hide DevTools), in either layout. The chip that said who drives a page
  is gone from the toolbar in both; the bar over a page an agent uses still offers to take it.

## 2. Address bar

- An Arc-style rounded, translucent field under the top row.
- It says what's shown:
  - **Browser tab:** the page's address, host style (`example.com`).
  - **Terminal:** the program running in it, as its title says.
  - **Chat:** the chat's name.
  - **File tab:** "File".
  - **Settings page:** "Settings".
- In a split it says the pane the person is in: the one they last pressed in or gave the
  keyboard. Pressing the page of a chat and a page shows the page's host, pressing the chat
  the chat's name, and with two pages each has its turn. The pane is remembered while the
  sidebar, a dialog or another window has the keyboard (`Pane::entered_at`, `Tab::current`).
  A page's `DevTools` stand for their page.
- **Clicking the bar, or ⌘L from anywhere in the window, opens the launcher on the page**
  (`docs/new-tab.md` 1), as Arc's ⌘L does: the page's whole address in the field, all of it
  selected, what the history has for it underneath. An address or a search picked there loads
  in that page. The page is the one the controls act on (section 1). There's one way to
  change an address, so the bar is never a field for it.
- With no page in the tab shown there's no address to change: a click on the bar does
  nothing, and ⌘L opens nothing. A new tab is ⌘T's.
- The exception is a new browser tab opened empty: its start page has no address to show, so
  the bar is the page's own address field, the keyboard in it, until an address is entered.
- A page takes the keyboard when its tab is picked in the sidebar, as a terminal and a chat
  do, so its own keys (⌘R, ⌘F, ⌘G, ⌘[, ⌘], zoom, ⌘⌥I) work at once. In a split, the pane
  the person was in takes it, a file's editor too; a file they've only looked at, as the
  tree previews one, leaves the keyboard to the tab's first terminal, chat or page
  (`Shell::give_shown_tab_the_keyboard`).

## 3. Project tiles

Arc's favourites grid, holding the projects of the space shown.

- One rounded tile per project, its icon centred. Tiles share the row's width and wrap
  onto new rows.
- The icon comes from the repository's git host: the `origin` remote says which host
  (GitHub, GitLab, Bitbucket or another) and owner, and the owner's or repository's avatar
  is fetched once and kept in the app's data folder. With no remote, or no avatar to fetch,
  the project's initials stand in.
- The selected project's tile is highlighted. Clicking a tile selects that project.
- Dragging a tile moves its project among the space's (`arc/tile_grid.rs`): the tile follows
  the pointer, the tiles it passes step into the slot it left, along their row and round onto
  the next, and let go it slides into the slot it's over. The room a short last row leaves
  counts as the last slot. Taken well away from the grid it goes back, and letting go there
  changes nothing. The order is the sidebar's (`settings.projects`), saved at once: the
  projects of other spaces keep their places in it, the Orca list shows the same order, and
  the project selected stays selected.
- A small indigo dot in the tile's top-right corner while any tab of the project needs
  attention (section 5). Indigo is what waits for the person (`Palette::waiting`), on a tile,
  a tab's row and a space's mark; amber is what's at work, and red is kept for what went wrong.
- While something in the project works (a chat running), the spinner in the same corner
  (`chat/spinner.rs`): a small ring in amber with a gap in it, its line bold for its size,
  going round. It's drawn at every frame the display shows, so it turns evenly rather than in
  steps, and all of them turn together. Wherever a chat is said to be working, a tab's row,
  a space's mark, the Orca layout's tabs and rows, it's this spinner.
- Hovering a tile shows a card with the project's name and stats: its worktrees, and its
  activity (chats running, terminals, tabs open).
- Right-clicking a tile opens its menu:
  - **Move to ›**: the spaces, the current one checked, a divider, then **New Space…**,
    which makes a space and moves the project into it.
  - **Rename…**: a name kept only in Koyo; the folder isn't renamed.
  - **Change Icon…**: an image picked from disk stands in for the git host's avatar;
    **Reset Icon** goes back to it.
  - **New Worktree…**, **Reveal in Finder** and **Copy Path**, as before.
  - **Remove Project**: takes it out of Koyo, as before. Its files stay.

## 4. Project name

- The selected project's name, after the space's dot in its colour. Nothing sits between it and
  the first worktree's header. The space's own name shows on glass over its mark at the foot
  while the pointer's on it (section 7).

## 5. Worktrees and their tabs

- Each worktree is a folder, as in Arc's sidebar: a header, and its tabs under it
  (`arc/header.rs`, `arc/worktrees.rs`).
- The header is a row as tall as a tab's, with its corners: the worktree's name alone, from
  where the tabs' icons start, a label over them: smaller and lighter than their titles
  (12 pt, medium), in the muted grey of the sidebar's labels, cut short with an ellipsis
  before the row's end. No chevron, no other icon, and no badges: a collapsed worktree shows
  by its tabs being gone.
- The name is the branch in words, without the prefix new worktrees' branches get (Settings →
  General: the person's GitHub user unless they set another): `schulzfel/some-nasty-name`
  shows as "Some nasty name", `main` as "Main". A detached checkout keeps "detached at …".
  The Clear dialog calls it so too; the Orca layout lists the branch.
- The worktrees stand apart by space: 12 pt between one worktree and the next, 2 pt between a
  header and its tabs, so each header sits on its own tabs.
- Under the pointer a pill fades in across the header, as a tab's hover tint, with a muted `…`
  and a **+** at its end; leaving, they fade out (150 ms, as the sidebar's other hovers do).
  The `…`, the **+** and a tab's ×, or a split's pane's, are Arc's size, bigger than a tab
  bar's: 24 pt squares, their glyphs 18 pt.
- **Clicking the header collapses the worktree, or opens it**, as Arc's folder does. Its tabs
  slide closed or open, in the time a tab takes to slide into place (240 ms, easing out); at
  once with reduced motion.
- Collapsed, all its tabs are hidden, the tab shown among them too: the panes still show it.
  Only its header opens it again, so a tab moved onto it, or shown some other way, leaves it
  collapsed.
- The header's **+** selects the worktree and opens the ⌘T launcher on it, opening the
  worktree first if it's collapsed, so the tab it opens shows. Clicking the rest of the header
  doesn't select the worktree: its **+** and its tabs do.
- The `…`, or a right-click on the header, opens the worktree's menu:
  - **Clear Tabs…**: asks first, then closes every tab of the worktree, stopping its terminals
    and chats. Greyed out while it has no tabs.
  - **Delete Worktree…**: the usual Delete Worktree dialog, which says what deleting would lose
    before anything goes. Greyed out on a main checkout, which git can't delete. Its switch, and
    the New Worktree dialog's, are the settings' own, macOS's `NSSwitch`.
  - **Reveal in Finder** and **Copy Path**, after a separator.
  - Stopping all of a worktree's terminals is left to the Orca layout's menu.
- **Dragging a header moves its worktree**, header and rows together, among the project's, as a
  tab moves in its list (`shell/reorder.rs`): it follows the pointer, the worktrees it passes
  slide into the place it left once it's past their middle, and let go it slides into the place
  it's over. It joins none, and stays on the list wherever the pointer goes. The order is the
  project's (`Project::branches`), so the Orca layout lists the worktrees in it too, and it's
  kept for the next launch (section 9); a worktree it doesn't name, as a new one, comes last.
- Tab rows as Arc's, 36 pt high with 10 pt corners: a 16 pt icon, the title cut short, the
  selected tab a pill of white laid thin over the colours, so it takes their hue, with no
  shadow; a hover tint, and a close button at its end on hover, and on the tab shown. While
  nothing's at its end, the title and what was said under it run on to the row's end; they
  make room once the × or how the tab's doing shows there, as a split's panes do.
- A chat's icon is Claude's logo in its colour, claude.ai's own, as a page shows its own
  (`Pane::favicon`), there and wherever a tab shows its icon.
- A chat's row, but for the tab shown's, says under its title the start of what was said last
  in it, the person's message or the agent's reply, smaller and in the labels' grey, the two
  lines in the row's height, as Arc's say what's recent in a tab. The tab shown shows the chat
  beside the sidebar, so its row has the title alone. Pages' rows don't.
- A split tab's row shows its panes side by side as Arc does: each pane's icon and title,
  sharing the row, a hairline between them and no fill of their own, the row's tint or pill
  being theirs. Each pane shows its × while the pointer's on it, making room for it then; one
  with unsaved edits shows its dot always.
- A tab's row says at its end how the tab's doing, as its project's tile does in its corner
  (`Shell::tab_activity`), so it's plain which tab the tile and the space's mark speak of:
  - the spinner while a chat in it is at work;
  - an indigo dot once something in it waits for the person: a chat that finished and hasn't
    been seen, failed or asks, a terminal waiting for input. It's where the spinner was, and
    it wins over the spinner, as on the tile.
  - It has the ×'s place, and leaves it to the × while the pointer's on the row, the tab
    shown included.
  - A split's row is its panes, each with its own ×: there it goes before them.
- Unsaved edits' and splits' dots, middle-click to close, and dragging to reorder stay.
- A tab dragged past its list's end, onto another worktree's tabs or header, moves to that
  worktree and shows there: where the tab it's dropped on is, or first. A collapsed worktree's
  header takes it too, and the worktree stays collapsed. Its content doesn't move: a terminal
  stays in its folder, a chat with its thread. The window comes back with the tab where it was
  put (`docs/browser.md` 1).
- The sidebar's search field and its "New worktree" row are gone.

## 6. Space menu

- Right-clicking the sidebar opens the current space's menu:
  - **Rename Space…**
  - **Edit Theme Color…**: the theme picker, for this space only.
  - **Delete Space**
- **Delete Space** asks first, and what happens to the space's projects: removed from Koyo,
  or moved to another space picked from a dropdown. It's greyed out with one space left.
- Each space keeps its own theme (light and dark gradients), starting from the window's.
  Switching spaces switches the window's colours. In the Arc layout every theme edit goes to
  the space shown.
- **⌥←** / **⌥→**, and **⌘⌥←** / **⌘⌥→**, show the space to the left and to the right, as a swipe does
  (section 7).
  Settings → Keyboard shortcuts lists them under Spaces.

## 7. Bottom bar

- The spaces, centred, each a mark in its colour: its theme's hue at a lightness that reads on
  the glass, or the text's colour while it has none. The space shown is a capsule at full
  strength, the others dimmed dots that brighten under the pointer, its name showing on glass
  over the dot. Switching, the capsule shrinks as the next grows, going a little past and
  settling, the strength easing.
- Two fingers drag the space sideways, one to one, fading a little as it goes, with a rubber
  band past the first and last space. Let go, a spring takes it at the fingers' speed: a flick
  goes on to the next space, a slow short pull settles back. Going on, the space carries on out
  and the next comes in from the other side in the same motion, slowing to rest. A click on a
  mark, a new space, and the keys below move the same way; with reduced motion the switch is
  at once.
- **⌥←** and **⌥→** (`arc::PreviousSpace`, `arc::NextSpace`) show the space to the left and
  to the right wherever the keyboard isn't in text: nothing focused, the sidebar, the file tree,
  a viewer. In text they stay a word back and on: a terminal (its program gets `ESC b` /
  `ESC f`), Koyo's text fields (the chat's prompt, the file editor, the address bar), and a web
  page, the agent desktop or a phone or simulator, whose own text field Koyo can't see.
  **⌘⌥←** and **⌘⌥→** do the same from wherever the keyboard is, the chat's prompt included.
  Pressed again while a switch is under way, they go on from the space it's headed for. At the
  first or last space the space gives a little toward the side with none and springs back, as
  a swipe's rubber band does; they never wrap round. Held, they repeat as the arrows do. In the
  Orca layout and on a settings page, where no spaces show, they do nothing.
- A chat's prompt has the keyboard only when the person puts it there: pressed in, its tab
  picked, a chat opened. A press outside it (the chat's background, its messages, the sidebar)
  takes the keyboard to the window, and another space or project shown leaves it there, its
  chat's prompt or not.
- A space that isn't shown says on its mark what its tiles would (section 3), so a chat that
  finished there is noticed: a small indigo dot up and right of the mark while something in it
  waits for the person (a chat unread, failed or asking, a terminal waiting), the tiles'
  spinner in that same place while something works, as strong as theirs but smaller, half as
  wide again as the dot. It's the most urgent of the space's projects
  (`Shell::space_activity`, from `project_activity`, which the tiles' corners use too), so it
  goes when the tiles' would. It's laid over the mark and takes no room: no mark moves as it
  comes or goes. It shows only by a dot, so on a space just left it comes once the capsule
  has shrunk back, out of the spring's way. The dot fades in; the spinner doesn't, as it's a
  view drawn again only as it turns, and a fade round it would stay where its first frame had
  it. The space shown has none: its tiles say it.
- On the left, in the +'s width, the Claude accounts button (`docs/claude-accounts.md` 6): the
  active account's week as a ring, opening the accounts panel; nothing while Koyo doesn't manage
  the accounts.
- A **+** on the right opens a menu:
  - **New Worktree…**: in the selected project, through the New Worktree dialog.
  - **New Project…**: added to the space shown.
  - **New Space…**

## 8. ⌘T: a new worktree

- **Worktree** is the launcher's fourth kind, after Browser, Chat and Terminal, with its own
  pill and icon. Tab goes through it like the others.
- Its field takes only the worktree's name. Enter opens the New Worktree dialog with that
  name filled in, on the selected project, and the branch is picked there. Nothing is listed
  under the field.

## 9. Model and persistence

- `settings.json` keeps the spaces (`spaces`: name, light and dark themes, projects in the
  sidebar's order, the project selected) and the space shown (`space`). An `icon` a space has from an earlier
  build is ignored.
- It keeps each project's Koyo-only name and icon (`project_looks`), and with them how its
  worktrees show in the sidebar (section 5): their order once one's been dragged (`worktrees`)
  and those collapsed (`collapsed`), each by its folder. What's kept of a worktree git no longer
  lists goes when the project's worktrees are next read.
- A new project joins the space shown, and so do projects no space holds, as every project
  the first time this runs, into a first space made then. A project removed leaves its space.
- ⌥← / ⌥→ and ⌘⌥← / ⌘⌥→ change the space shown (`space`) one either way, as a swipe does, and only in the
  Arc layout (section 7).
- Selected another way (app control, the Orca layout, a link), a project's own space is
  shown. An empty space shows no project at all: the window shows none of another space's
  tabs, and ⌘W, ⌘T, the switcher and the side panel have nothing to act on, until a project
  is picked.
- A space lists its projects in the sidebar's order, so moving a project in the Orca list or
  through app control moves its tile too.

## 10. Islands

- Right of the sidebar, each part of the window is an island: a rounded card in the theme's
  colour with a barely-there shadow, the window's glass between them as a margin as thin as the
  window's edge.
  - Each pane of the tab shown; a split's sides are islands of their own, the glass between
    them the handle that resizes them.
  - The Changes or Files panel, the glass before it the handle that resizes it.
  - The places a symbol led to, and the bottom panel, under the panes.
  - A settings page, and what's said where there's no tab.
- Only in the Arc layout. Orca keeps its one card, holding the sidebar, the tab strip, the panes
  and the panel beside them, hairlines between them.
- A page is drawn in the window's own frame, so the island's rounded corners clip it as they
  clip the rest of what's on it.

