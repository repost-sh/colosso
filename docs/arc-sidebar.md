# Koyo's Arc sidebar: spaces, project tiles and worktree folders

Written 2026-11-08 on branch `schulzfel/arc-like-sidebar`. Drafted with Claude.

The Arc layout's sidebar is rebuilt to look and work like Arc's own. From the top: browser
controls and an address bar; the pages **pinned** for every space, as tiles; the **space**'s
own loose pages; the selected project's name and each of its worktrees as a folder with its
tabs under it; the projects of the space as tiles; and the spaces, switched along the
sidebar's foot. The pinned and the loose pages, and that order, are section 11, added
2026-11-13 on `schulzfel/pinned-browser-tabs`. This note is the spec the work followed; keep it
in step with the code in `koyo-rs/koyo/src/arc/` and `koyo-rs/koyo/src/spaces.rs`. The visual
check draws it, and then does what a person does in it (a space's mark saying a chat in it
works and waits, a tab's row and its tile saying the same, a tile dragged to another row and
back, the address bar following the pane pressed in, ⌘L on a page, and the pinned and loose
pages: the empty zone, a page's tab dragged onto it and onto the space's own pages, the tiles,
one in hand, one shown as the space changes, a space with no project), saving a picture of
each. It shows a chat, so it wants a data folder and
an agent host of its own, and says so if it isn't given them:

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
- A pinned or a loose page shown (section 11) is a page shown like any other: the bar says
  its host, a click or ⌘L opens the launcher on it, and back, forward and reload act on it,
  whichever space and project the sidebar lists, a space with no project too.
- The exception is a new browser tab opened empty: its start page has no address to show, so
  the bar is the page's own address field, the keyboard in it, until an address is entered.
- A page takes the keyboard when its tab is picked in the sidebar, as a terminal does, so its
  own keys (⌘R, ⌘F, ⌘G, ⌘[, ⌘], zoom, ⌘⌥I) work at once. In a split, the pane the person
  was in takes it, a file's editor too; a file they've only looked at, as the tree previews
  one, leaves the keyboard to the tab's first terminal or page. A chat takes none: its prompt
  has the keyboard once it's pressed (`Shell::keys_to_shown_tab`; section 9 has where the
  keyboard is).

## 3. Project tiles

The projects of the space shown, as tiles at the foot of the space, over the spaces' marks
(section 11.1 has the sidebar's order). They're the space's, so they swipe away with it.

- One rounded tile per project, its icon centred, 16 pt as a tab row's icon is (`tiles.rs`
  `PICTURE`). Tiles share the row's width and wrap onto new rows.
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
  the project selected stays selected. The pinned pages' tiles, at the sidebar's head, are a
  second grid of the same kind, with a hand of its own (`Shell::pinned_hand`), so the two
  move alike (section 11).
- A small indigo dot in the tile's top-right corner while any tab of the project needs
  attention (section 5). Indigo is what waits for the person (`Palette::waiting`), on a tile,
  a tab's row and a space's mark; amber is what's at work, and red is kept for what went wrong.
- While something in the project works (a chat running), the spinner in the same corner
  (`chat/spinner.rs`): a small ring in amber with a gap in it, its line bold for its size,
  going round. It's drawn at every frame the display shows, so it turns evenly rather than in
  steps, and all of them turn together. Wherever a chat is said to be working, a tab's row,
  a space's mark, the Orca layout's tabs and rows, it's this spinner.
- Hovering a tile shows a card with the project's name and stats: its worktrees, and its
  activity (chats running, terminals, tabs open). The card is under its tile, or over it
  where there's no room under, as at the sidebar's foot.
- Right-clicking a tile opens its menu:
  - **Move to ›**: the spaces, the current one checked, a divider, then **New Space…**,
    which makes a space and moves the project into it.
  - **Rename…**: a name kept only in Koyo; the folder isn't renamed.
  - **Change Icon…**: an image picked from disk stands in for the git host's avatar;
    **Reset Icon** goes back to it.
  - **New Worktree…**, **Reveal in Finder** and **Copy Path**, as before.
  - **Remove Project**: takes it out of Koyo, as before. Its files stay.

## 4. Project name

- The selected project's name, after the space's dot in its colour, under the space's loose
  pages (section 11). Nothing sits between it and the first worktree's header. The space's own
  name shows on glass over its mark at the foot while the pointer's on it (section 7), and
  nowhere else.
- A space with no project shows neither: no dot, and no name. It's a space like any other,
  with its loose pages and the pinned ones.

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
- A page's tab dragged onto the pinned pages at the sidebar's head is pinned, and onto the
  space's loose pages is one of them; a pinned page's tile or a loose page's row let go on a
  worktree's tabs or header is a tab there (section 11). While a pinned or a loose page is
  shown, no tab's row is the pill.
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

- Over it, the tiles of the space's projects (section 3).
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
- The keyboard is where the person puts it (`shell/keys.rs`), in either layout. A pane has it
  once they press in it, or ask for its tab: picked in the sidebar, opened (⌘T, a new terminal,
  a file), switched to (⌃⇥), opened again (⌘⇧T), shown in place of the tab or the split's pane
  they were typing in and closed, or come back to from a settings page they left it for.
- A chat's prompt is the exception: moving to a chat, by any of those or by another space or
  project shown, never puts the keyboard in its prompt, so ⌥← and ⌥→ go on moving between
  spaces. The prompt has the keyboard once the person presses in it, or in a chat they've
  just opened to type in (⌘T, a question asked from the launcher). A chat brought back with
  the window has none, and coming back from a subagent's chat leaves the prompt alone too.
- A press on anything that takes no keyboard (a chat's background or its messages, the
  sidebar, the glass) takes it out of the pane, to the window, where the window's own keys
  work and nothing is typed into; a press on a button, a menu or a scrollbar leaves it where
  it was. Another space, project or worktree shown leaves it at the window: no tab takes the
  keyboard for being the one shown. With the keyboard at the window, what a chat hears on its
  own root, its card's key (⌘⇧B) and its menu, still reaches the chat the person is in
  (`chat::heard_at_window`); the keys of its prompt (Esc to stop, ↑, ⌘↵) need the keyboard in
  the prompt.
- A new project joins the space shown, and so do projects no space holds, as every project
  the first time this runs, into a first space made then. A project removed leaves its space.
- ⌥← / ⌥→ and ⌘⌥← / ⌘⌥→ change the space shown (`space`) one either way, as a swipe does, and only in the
  Arc layout (section 7).
- Selected another way (app control, the Orca layout, a link, ⌃⇥), a project's own space is
  shown, and its tab there, not the loose page that space showed last (`follow_space`). A
  space with no project shows no project at all: the window shows none of another space's
  tabs, and the side panel has nothing to act on, until a project is picked; ⌃⇥ still goes to
  the tabs looked at last, wherever they are (`docs/tab-switcher.md`). ⌘T opens a page there,
  a loose one of the space, and ⌘W closes the loose or the pinned page shown (section 11).
- Each space has an `id`, given once and kept whatever its place among the spaces: its loose
  pages go by it (`docs/browser.md` 2).
- A space lists its projects in the sidebar's order, so moving a project in the Orca list or
  through app control moves its tile too.
- The pinned pages are no space's and no project's: they're kept in a file of their own in the
  data folder, `pinned-tabs.json` (section 11, `docs/browser.md` 10). `settings.json` keeps only
  whether their empty zone was put away (`pinned_zone_dismissed`). A space's loose pages are
  open tabs, kept with the window (`window.json`, `docs/browser.md` 2).

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

## 11. Pinned tabs and loose pages

Written 2026-11-13 on branch `schulzfel/pinned-browser-tabs`. Drafted with Claude. Arc's pinned
tabs, for pages, and under them the pages open in a space that are no project's. First built
with the pinned pages at the sidebar's foot; the owner turned it round the same day, to the
order below. What the owner decided is marked **[asked]**; what was decided while building it,
and is the owner's to change, **[ours]**.

### 11.1 The sidebar's order

From the top: **[asked]**

1. The window's controls and the address bar (sections 1, 2).
2. **The pinned pages**, as tiles. The same in every space: they're outside what swipes, and
   stay where they are as a space goes out and the next comes in.
3. **The space's loose pages**, as rows: the pages open in this space that are no project's.
   Each space has its own.
4. **The open project**: its name and its worktrees with their tabs (sections 4, 5). The
   loose pages and the project scroll together, between the pinned pages and the tiles.
5. **The space's projects**, as tiles (section 3).
6. The foot: the accounts, the spaces' marks, the + (section 7).

No space's name shows in the sidebar, and a space with no project shows no dot where a
project's name would be (section 4). **[asked]**

### 11.2 The pinned pages

- A tile per pinned page, as the projects' tiles are (section 3): 44 pt high, 10 pt corners,
  8 pt apart, sharing the sidebar's width, up to four to a row and three for five or six, in
  the `Look`'s tile shades, the page shown's lit as the selected project's is. **[asked: as
  the project tiles; ours: the same sizes and columns, no numbers of its own]**
- A tile shows its page's site icon, the favicon the browser fetched, 16 pt as a tab row's
  icon is; with none kept, the first letter of its host; a globe for a page with no host, as
  a start page. **[asked: icon, initials or a globe, and the row's size; ours: one letter]**
- **A page's favicon has its corners rounded 2 pt wherever a tab shows it**: on a pinned
  tile, a loose page's row, a worktree's tab row and a split's halves, the chip of a tab in
  hand, and a tab along the Orca layout's strip (`Face::icon`, `FAVICON_CORNERS`). **[asked]**
- The picture of a page that isn't open, closed or not opened since the window came back, is
  drawn at 60 % strength, so it's plain which pinned pages are open. **[ours]**
- The pointer resting on a tile brings its card: the page's icon, its title, and its host.
  **[asked]**
- With no page pinned the region is a zone a tile high with a dotted outline, saying "Drag a
  page here to pin it". The pointer on it shows a × at its end (tooltip "Hide") that puts the
  zone away; that's kept in `settings.json` (`pinned_zone_dismissed`). **[asked; ours: the
  copy, the height, the ×]**
- Put away, the zone is back once a page's tab is taken off its list, saying "Drop to pin",
  and gone again when the tab's let go elsewhere. Nothing brings it back for good: pinning a
  page shows tiles instead, and with the last pin gone it stays away. **[asked; ours: that
  nothing un-hides it]**
- **Only a tab whose single pane is a page is pinned.** A split, a terminal, a chat, a file
  or a page's `DevTools` in a tab of their own isn't taken, and the region doesn't light for
  one. A loose page is always one. **[asked]**
- A page's tab dragged from a worktree's list, or a loose page's row, onto the region is
  pinned: off its list it's a chip; the zone, or a dotted outline round the tiles, lights
  under it; let go, it leaves where it was and is the last tile. **[asked; ours: it goes
  last rather than where it's dropped among the tiles]**
- It goes on showing if it was the tab shown, and its page isn't reloaded. **[ours]**
- Tiles move among themselves as project tiles do (`arc/tile_grid.rs`): the one in hand
  follows the pointer, the others step out of its way, let go it slides into its slot, and
  taken well away it goes back with nothing changed. **[asked]**
- Taken well away, the tile in hand travels as a chip, as a tab off its list does. Let go on
  a worktree's tab or header it's **unpinned** there: a tab of that worktree where that tab
  is, or first, and shown. Let go on a loose page's row it's a loose page of the space, where
  that row is. **[asked; ours: the chip, and the loose pages taking it]**

### 11.3 The loose pages

- **A space's own.** Each space lists the pages opened in it that belong to no project, as
  rows like a worktree's tabs: the page's icon and title, the one shown a pill, a × at its
  end on hover and on the one shown. Another space lists its own, and a space shown again
  shows the loose page it showed last. **[asked: per space]**
- **Only pages.** No chat, terminal or file is loose: each needs a worktree to be in.
  **[asked, for now]**
- **How a page comes to be one:** **[asked]**
  - **Opened where the person is in no worktree.** A browser tab opened (⌘T, a link picked
    in the launcher) while no project is shown, or while a loose or a pinned page is the tab
    shown, is a loose page of the space: after the loose page shown, or after them all, and
    shown. **[ours: that a loose or a pinned page shown counts as being in no worktree]**
  - **Dragged.** A page's tab taken off a worktree's list and let go on a loose page's row
    goes where that row is; a pinned page's tile let go there is unpinned there. Either
    shows.
  - With no loose page yet there are no rows: once a page's tab is off its list, or a pinned
    tile well away from the others, a dotted row under the pinned pages says "Drop to keep
    it in this space" and takes it. It stays until what's in hand is let go, and doesn't
    show for a tab moved along its own list. **[ours]**
- In a space with no project ⌘T opens the launcher for a page only: Tab goes to no other
  kind there, as a chat, a terminal and a worktree need a project. **[ours]**
- A row is dragged among the others, as a tab along its list, but joins none: each stays a
  page. Dragged onto a worktree's tab or header it's a tab of that worktree, shown; onto the
  pinned pages, pinned. **[ours]**
- ⌘W, its ×, or a middle click closes a loose page: it's gone. The one before it shows in
  its place, the one after when it was the first, and the selected worktree's tab when it
  was the only one. It isn't among the tabs ⌘⇧T brings back. **[ours]**
- A space deleted hands its loose pages on with its projects: to the space that takes them,
  after that space's own; they close when the projects are taken out of Koyo. **[ours]**

### 11.4 The tab shown

- The panes show one tab: a pinned page, else the loose page the space shows, else the
  selected worktree's tab. A click on a pinned tile or a loose row shows its page, which
  takes the keyboard. It's a page like any other: the same browser host, its keys (⌘R, ⌘F,
  zoom, ⌘[ and ⌘]), `DevTools` and right-click menu, and the sidebar's address and controls
  (section 2). **[asked]**
- **Switching space leaves a pinned page shown.** By a swipe, ⌥← and ⌥→, ⌘⌥← and ⌘⌥→, or a
  click on a space's mark, the sidebar's loose pages, project and tiles change and the panes
  don't: the pinned page stays, the keyboard where it was. A space with no project shows it
  too. A click on a space's mark doesn't take the keyboard to the window while a pinned page
  is shown, as a press elsewhere in the sidebar does (section 7). **[asked; ours: the mark's
  press]**
- The keyboard goes by section 9 with these pages as with any tab: one opened or picked
  takes it; one closed with the keyboard in it (its ×, its menu, ⌘W) hands it to what shows
  in its place; one moved (pinned, unpinned, dragged among the loose pages or onto a
  worktree) leaves it where it was (`Shell::close_apart`, `picked_tab_shown`). **[ours]**
- A loose page shown is its space's: another space shows what it showed last.
- Picking a tab of a worktree, or a project's tile, leaves a pinned or a loose page for that;
  so does anything else that shows a worktree's tab: ⌃⇥ picking one, a file opened, ⌘⇧T, a
  worktree's +, app control selecting a tab, worktree or project. A pinned page's page stays
  open, as it was, and shows again when its tile is picked. ⌃⇥ lists the pinned and the loose
  pages among the tabs looked at last, and shows one picked as its tile or row would
  (`docs/tab-switcher.md`). **[asked; ours: the list of what else leaves it]**
- A pinned or a loose page's `DevTools`, detached, split its tab as they split any page's;
  their pane has only its ×, as such a page's panes aren't dragged elsewhere. No side of one
  takes a tab in hand: it's split with nothing. **[ours]**
- In the Orca layout, which lists neither, the selected worktree's tab shows; the pinned and
  the loose pages stay as they are, and are back with the Arc layout. A page opened there
  with no checkout selected opens nothing, as before. **[ours]**

### 11.5 The address a page is pinned at, and closing it

From the owner's answers while it was built: closing a pinned page doesn't unpin it, and its
menu is Arc's.

- **A pinned page keeps the address it's pinned at**: where its page was when it was pinned.
  Its page may go on anywhere from there. **[asked, from Arc's menu]**
- **⌘W, or Close from its menu, closes the page and leaves it pinned.** Its page and its
  `DevTools` go, its tile stays, fainter, and what the space shows under it shows in its
  place. Picked again, it opens at the address it's pinned at. Closing it is how it's taken
  back there. **[asked: closing doesn't unpin; ours: that it goes back to the pinned address
  rather than the one it last showed]**
- Right-clicking a tile opens its menu, as Arc's for a pinned tab: **[asked]**
  - **Edit Pinned Page ›**
    - **Replace Pinned URL with Current**: pins it at the address its page shows. Greyed out
      while that's the one it's pinned at.
    - **Edit…**: a dialog with the address in a field, read as the address bar reads one
      (`example.com` is `https://example.com`); words that are no address change nothing. An
      open page stays where it is; a closed one opens at the new address.
  - **Close**: as ⌘W. Greyed out when there's nothing to close.
  - **Unpin**: a tab of the selected worktree, after its others, shown there; with no
    worktree selected, a loose page of the space. **[ours: where it goes with no worktree]**
  - **Remove Pinned Tab**: the page goes and it's pinned no longer. **[ours: Arc's "Remove
    Favorite"; the brief's first Close did this]**
- A pinned page closed or removed isn't among the tabs ⌘⇧T brings back. **[ours]**

### 11.6 Links, sign-ins and agents

- A link that asks for a new window (`target="_blank"`, `window.open`) loads in the page
  itself, pinned or loose, as it does in any page tab today: the host opens no windows
  (`koyo-browser-host/src/pane.rs`, `on_before_popup`). ⌘[ goes back. **[ours: as a page tab]**
- **Open Link in New Tab** from a pinned or a loose page's right-click menu, and a link
  opened from its `DevTools`, open a loose page of the space shown, right after a loose
  opener or after them all, without leaving the page, as a page tab's opens beside it.
  **[ours]**
- A page keeps a browsing profile, its cookies and sign-ins (`browser::profile`):
  - one pinned or dragged out of a worktree keeps its project's, whichever project is
    selected, and is of that profile again after a relaunch;
  - one opened outside any project keeps the person's own, the same in every space
    (`browser::NO_PROJECT`);
  - one a pinned or a loose page opens keeps its opener's, so it's signed in as its opener
    is;
  - one moved onto a worktree's tabs keeps its profile while the window lives, and is its
    worktree's project's once brought back with the window. **[ours]**
- Sign-ins survive a relaunch: a profile is a folder under `browser/profiles/` in the data
  folder, with session cookies kept (`persist_session_cookies`,
  `koyo-browser-host/src/host/panes.rs`). `browser::host_tests::keeping::
  a_sign_in_outlives_the_engine_quitting_and_starting_again` proves it against the packaged
  engine: a session cookie, a cookie with an age and `localStorage` are all there once the
  engine has quit and started again. **[checked 2026-11-13]**
- The history keeps a pinned or a loose page's visits under the project whose profile it
  keeps (`Shell::apart_place`).
- App control lists and drives worktrees' tabs only: a pinned or a loose page isn't in
  `tab_list` or `browser_list`, and an agent can't drive it. What an agent opens out of sight
  leaves such a page shown. **[ours]**

### 11.7 Kept for the next launch

- **Pinned pages** survive a relaunch, a reboot and an update: `pinned-tabs.json` in the data
  folder (`settings::data_dir()`, which `KOYO_DATA_DIR` moves), beside `window.json` and
  apart from it, so a window file that can't be read doesn't take them with it
  (`docs/browser.md` 10). **[asked]**
- Each comes back in the order of the tiles, at the address it last showed, titled as it
  was, with its site's icon (`history-icons/`), pinned where it was, of the profile it had.
  The one that was shown is shown again; no other page is made until its tile is picked.
  **[asked; ours: that the one shown is shown again]**
- Written 500 ms after a change and on the way out, through `atomic_file`, only when it
  differs from what the file holds. Read tolerantly: see `docs/browser.md` 10. **[asked]**
- **Loose pages** are open tabs, so they're kept with the window, in `window.json`, by their
  space's `id` (`docs/browser.md` 2): each comes back in its space, in order, at the address
  it showed, titled as it was, of the profile it had, the one the space showed shown. Those
  of a space the settings no longer have are the space shown's, rather than lost. **[ours]**

### 11.8 Where it lives

| Path | What |
| --- | --- |
| `koyo/src/model/pinned.rs` | The pinned pages on the workspace: pinning, moving, closing, the pinned address, unpinning, removing |
| `koyo/src/model/loose.rs` | Each space's loose pages, the tab shown among pinned, loose and a worktree's, and a page's tab moving between the three |
| `koyo/src/shell/pinned.rs` | The shell's side of the pinned pages: what a pin takes, picking, closing, the pinned address |
| `koyo/src/shell/loose.rs` | The shell's side of the loose pages: opening one, picking, closing, moving; and a pinned or a loose page's own events |
| `koyo/src/shell/kept/pinned.rs` | `pinned-tabs.json`: reading it tolerantly, writing it, and bringing the pinned pages back |
| `koyo/src/shell/kept/loose.rs` | The loose pages in `window.json`, and bringing both kinds back as the window opens |
| `koyo/src/arc/pinned.rs` | The pinned region: tiles, the zone, the chip, the menu and its actions |
| `koyo/src/arc/loose.rs` | The loose rows and the strip that takes the first |
| `koyo/src/arc/tile_grid.rs` | The grid both kinds of tiles share, each with its own `TileHand` |
| `koyo/src/arc/card.rs`, `hover.rs` | A pinned page's card |
| `koyo/src/shell/reorder.rs`, `reorder/hand.rs` | The loose rows' list, and a tab off its list (`TabHand::left_list`, `taken_at`) |
| `koyo/src/spaces.rs` | A space's `id` |
| `koyo/src/launcher/here.rs` | The launcher where no checkout is selected: a page only |
| `koyo/src/arc/tests/pinned.rs`, `tests/loose.rs`, `shell/kept/tests/pinned.rs`, `tests/loose.rs`, `model/pinned/tests.rs`, `model/loose/tests.rs` | The tests |
| `koyo/src/arc/visual/pins.rs` | The pictures (`arc-pinned-*.png`, `arc-loose-*.png`) |
