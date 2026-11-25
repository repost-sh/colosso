# The sidebar and the top rail

Rewritten 2026-11-24 on branch `schulzfel/single-layout`. Drafted with Claude.

`docs/layout.md` is the spec of Koyo's one layout: what the person sees and chooses. This note is
its implementation reference: how the sidebar and the top rail behave, what they keep, and where
the code is. Keep it in step with `koyo-rs/koyo/src/shell/sidebar/`, `shell/top_rail/`,
`shell/spaces/`, `shell/rails.rs` and `spaces.rs`. What the owner decided is marked **[asked]**;
what was decided while building, and is the owner's to change, **[ours]**.

The settings are **Tabs**, **Address bar**, **Changes and Files**, **Unpinned pages** and
**Sidebar glass**, in Settings → Appearance → Layout (`settings/layout.rs`), each but the last also
in the right-click menu of the piece it changes. Their choices and defaults are in
`docs/layout.md`. The icon rail down the window's left is gone, a gutter in its place:
**Settings…** is in the app's Koyo menu, and ⌘, opens it, as a tab among the unpinned pages that
closes once anything else shows (11.9).

The visual check draws the sidebar and the rail, then does what a person does in them (a
space's mark and a tab's row saying a chat works and waits, a tab dragged to another row and
back, the address bar following the pane pressed in, ⌘L on a page, the pinned and unpinned pages
dragged onto, shown, and left as the space changes), saving a picture of each. It shows a chat,
so it wants a data folder and an agent host of its own, and says so if it isn't given them:

```sh
Q=$PWD/.artifacts/qa/sidebar && mkdir -p $Q/tmp &&
  KOYO_AGENT_PROVIDER=fake KOYO_DATA_DIR=$Q/data TMPDIR=$Q/tmp \
  KOYO_CONFIG_DIR=$Q/config KOYO_LOG_DIR=$Q/logs \
  script/cargo run -p koyo --features visual-check -- --visual-check-sidebar
```

---

## 1. Top row and top rail

- **Tabs on the top rail.** The rail runs across the whole window, over the sidebar and the
  islands (section 10). **[asked]** From the left:
  1. The traffic lights, the sidebar toggle, and the Downloads button right after the toggle
     (`docs/browser.md` 16.3). **[asked]**
  2. Back, forward and reload, when the address bar is in the sidebar.
  3. **The pinned pages**, a button each showing its page's site icon alone, in a pill: a
     rounded rectangle lighter than the rail, with no edge, the tell that they're every
     project's, not a branch's tabs (11.2). With none pinned, the zone in its place.
  4. **The unpinned pages**, as tabs like the branch's (11.3), the Settings tab while it's open
     (11.9), then their **+**.
  5. A **hairline**, then **the selected branch's tabs** and their **+**. The hairline is drawn
     whenever a branch is selected; the unpinned pages' **+** always precedes it. **[asked]**
  6. Empty glass that moves the window, at least 96 pt of it, the tabs narrowing to fit.
  7. At the rail's end, Changes and Files when they are set to the top rail (section 5).
- **Tabs on the sidebar.** There is no rail: the sidebar's top row holds the traffic lights,
  the toggle and the Downloads button, with back, forward and reload on the right when the
  address bar is in the sidebar. **[asked]** The row is over the sidebar alone. The island beside
  it rises to the gutter along the window's top, as it stops at the one along its bottom, and
  that gutter moves the window as the row's empty glass does. The window's top edge has nothing
  else on it: Changes and Files, set to the worktree, are on the branch's row (section 5).
- ⌘B folds the sidebar and the status bar away. The rail stays, so with **Tabs** on the top rail
  its pages and tabs stay to pick; with **Tabs** on the sidebar its tabs fold with it.
- **Back, forward and reload** are the sidebar's page controls (`shell/sidebar/address.rs`, `PageControls`).
  **[asked]** They act on the page of the pane the person is in (section 2), and in a pane of
  another kind on the tab's first page, so a page beside a chat can be reloaded from the chat.
  They're greyed out on a tab with no page and on the Settings tab, whose pages ⌘[ and ⌘] walk
  (11.9); the rail has no Settings back and forward of its own. With the address bar in the page
  they are in the page's toolbar only.
- With the address bar in the sidebar a page has no toolbar of its own. Responsive mode and the
  `DevTools` are in the page's right-click menu (Responsive mode, Inspect, Show or Hide
  DevTools). The chip that said who drives a page is gone from the toolbar; the bar over a page
  an agent uses still offers to take it.

## 2. Address bar

With the address bar in the sidebar (`shell/sidebar/address.rs`):

- A rounded, translucent field at the top of the sidebar, under its top row when it has one.
- It says what's shown: a browser tab's page address, host style (`example.com`); a terminal's
  running program, as its title says; a chat's name; "File" on a file tab; "Settings" on the
  Settings tab.
- In a split it says the pane the person is in: the one they last pressed in or gave the
  keyboard. Pressing the page of a chat and a page shows the page's host, pressing the chat the
  chat's name, and with two pages each has its turn. The pane is remembered while the sidebar, a
  dialog or another window has the keyboard (`Pane::entered_at`, `Tab::current`). A page's
  `DevTools` stand for their page.
- **Clicking the bar, or ⌘L from anywhere in the window, opens the launcher on the page**
  (`docs/new-tab.md` 1): the page's whole address in the field, all of it selected, what the
  history has for it underneath. An address or a search picked there loads in that page, the one
  the controls act on (section 1). There's one way to change an address, so the bar is never a
  field for it.
- With no page in the tab shown there's no address to change: a click on the bar does nothing,
  and ⌘L opens nothing. A new tab is ⌘T's.
- A pinned or an unpinned page shown (section 11) is a page like any other: the bar says its
  host, a click or ⌘L opens the launcher on it, and back, forward and reload act on it, whichever
  space and project the sidebar lists, a space with no project too.
- The exception is a new browser tab opened empty: its start page has no address to show, so the
  bar is the page's own address field, the keyboard in it, until an address is entered.

With the address bar in the page, each page's toolbar has its address, and ⌘L puts the keyboard
in that page's address field from anywhere in the tab.

Either way, a page takes the keyboard when its tab is picked, as a terminal does, so its own keys
(⌘R, ⌘F, ⌘G, ⌘[, ⌘], zoom, ⌘⌥I) work at once. In a split, the pane the person was in takes it, a
file's editor too; a file they've only looked at, as the tree previews one, leaves the keyboard
to the tab's first terminal or page. A chat takes none: its prompt has the keyboard once it's
pressed (`Shell::keys_to_shown_tab`; section 9).

## 3. Projects

The sidebar lists the projects of the space shown as rows, each with its branches under it
(section 5).

- A row has a folder icon, the project's name, a **New worktree** button that shows on hover,
  and the activity mark below while it's folded. **[asked]** The folder is open while its
  branches show and closed while they're folded away. The row is lit under the pointer.
- There is no "Projects" label and no Add project button in the sidebar; the foot's **+** has
  New Project…. A space with no project shows the add-project row. **[asked]**
- **A click on the row folds the project's branches away**, tabs and all, or brings them back,
  at once and without a slide; the state is kept for the next launch (section 9). Folded, ⌥↑ and
  ⌥↓ pass over the project's tabs, and only the row brings them back.
- **Activity.** Indigo is what waits for the person (`Palette::waiting`), on a row, a tab's row
  and a space's mark; amber is what's at work, and red is kept for what went wrong.
  - Folded, a row says at its end how the project's doing (`Shell::project_activity`): the
    spinner while a chat in it is at work, an indigo dot once something in it waits for the person
    (a chat that finished and hasn't been seen, failed or asks, a terminal waiting for input).
    Open, it says nothing: the tabs' rows say it.
  - The spinner (`chat/spinner.rs`) is a small ring in amber with a gap in it, its line bold for
    its size, going round. It's drawn at most 30 times a second while Koyo is in front and 10
    while another app is (`beat.rs`), all of them turning together, in the same frames. Wherever a
    chat is said to be working, it's this spinner.
- **Dragging a row** moves its project among the space's, as a tab moves in its list
  (`shell/reorder.rs`). The order is the sidebar's (`settings.projects`), saved at once: the
  projects of other spaces keep their places in it, and the project selected stays selected.
- Right-clicking a row opens its menu (`projects.rs`, `project_items`):
  - **Move to Space ▸**: the spaces, the current one checked, a divider, then **New Space…**,
    which makes a space and moves the project into it. The target space is shown. **[asked]**
  - **Set Profile ›**: the browsing profiles, the one its pages use checked, a divider, then
    **New Profile…**, which makes one and puts the project on it (`docs/browser.md` 15).
  - **New Worktree…**, **Reveal in Finder** and **Copy Path**.
  - **Remove Project**: takes it out of Koyo. Its files stay.

## 4. Spaces

A space holds a set of projects, and the sidebar lists only the space shown. Every space uses
the one theme, which **Edit Theme…** in the menus and Settings → Appearance → Glass edit, light
and dark; switching spaces never changes the window's colours. `settings.json` drops themes
stored on spaces. A space shows again at the project it showed last.

- **Marks.** The foot lists the spaces, centred, each a mark in the text's colour. **[asked]**
  The space shown is a capsule at full strength, the others dimmed dots that brighten under the
  pointer, the space's name showing on glass over the dot. A mark's springs and element ids are
  keyed by `space.id`, not by its place. **[asked]** Switching, the capsule shrinks as the next
  grows, going a little past and settling, the strength easing.
- **Swipe** (`shell/spaces/swipe.rs`). Two fingers over the sidebar drag the space sideways, one to one,
  fading a little as it goes, with a rubber band past the first and last space. Let go, a spring
  takes it at the fingers' speed: a flick goes on to the next space, a slow short pull settles
  back. Going on, the space carries on out and the next comes in from the other side in the same
  motion, slowing to rest. A click on a mark, a new space and the keys (section 6) move the same
  way; with reduced motion the switch is at once.
- **The top rail slides with it**, in the same direction on the same spring, following a swipe's
  fingers: the unpinned pages when they're per space, the selected branch's tabs and the Changes
  and Files buttons on the rail. The pinned pages, and the unpinned pages when shared, stay put,
  as they stay in the sidebar (11.1).
- **A mark's activity.** A space that isn't shown says on its mark what its projects' rows
  would, so a chat that finished there is noticed: a small indigo dot up and right of the mark
  while something in it waits for the person, the spinner in that same place while something
  works, as strong as the dot's but smaller, half as wide again as the dot. It's the most urgent
  of the space's projects (`Shell::space_activity`, from `project_activity`). It's laid over the
  mark and takes no room, so no mark moves as it comes or goes. On a space just left it comes
  once the capsule has shrunk back, out of the spring's way. The dot fades in; the spinner
  doesn't, as it's a view drawn again only as it turns. The space shown has none: its rows say it.
- **Reordering.** Pressing a mark and dragging sideways moves it as tabs move in their list; let
  go, it settles into the slot and the order is kept. A press let go without moving shows that
  space. Taken well away from the row it slides back. The space shown stays shown while its mark
  moves. The keys and the swipe follow the new order.
- **Space menu.** Right-clicking a mark, or empty space in the sidebar, offers **Rename Space…**
  and **Delete Space**. Delete asks first what happens to the space's projects: removed from
  Koyo, or moved to another space picked from a dropdown. It's greyed out with one space left.

## 5. Branches and their tabs

Each branch is a row under its project's row (`shell/sidebar/projects.rs`, its tabs `shell/sidebar/rows.rs`).

- The name is the branch in words, without the prefix new branches get (Settings → General: the
  person's GitHub user unless they set another): `schulzfel/some-nasty-name` shows as "Some nasty
  name", `main` as "Main" (`BranchUnit::title`). A detached checkout keeps "detached at …", and
  the main checkout's has a muted "checkout" after it.
  **[asked]**
- The row is a project's row's structure one step in: the branch's glyph, its name cut short
  with an ellipsis, muted unless it's selected, the selected branch's the pill; at its end the
  count of terminals waiting on the person, and how its checkout's doing (the spinner while a
  chat works, the dot once something waits).
- **Under the pointer** **Changes** and **Files** fade in at its end (150 ms, as the sidebar's
  other hovers do) when **Changes and Files** is set to the worktree. A tab's ×, or a split's
  pane's, is a 24 pt square, its glyph 18 pt.
- **A click on the row.** With **Tabs** on the top rail it selects the branch. With **Tabs** on
  the sidebar it collapses the branch's tabs, or opens them, and doesn't select; its
  **Changes** and **Files** and its tabs do. A tab picked selects its branch.
  **[asked]** The tabs slide closed or open in the time a tab takes to slide into place (240 ms,
  easing out); at once with reduced motion. Collapsed, all its tabs are hidden, the tab shown
  among them: the panes still show it. Only its row opens it again, so a tab moved onto it, or
  shown some other way, leaves it collapsed. Collapsed, ⌥↑ and ⌥↓ pass over its tabs.
- **Changes and Files** open the branch's panel beside the chat on its changed files or its file
  tree (`right_panel/buttons.rs`), selecting the branch first if it isn't, and close it when
  pressed on the selected branch with that panel shown. Changes says how many lines the selected
  branch's changes add and remove, once they've been read. The button whose panel shows stays,
  lit as the tab shown's pill. Each branch keeps whether its panel's open and on which
  (`right_panel/side.rs`), so going to another branch finds that one's as it was left. ⌘⇧E, ⌘⌥B
  and the chat's details card act on the selected branch's. Set to the top rail, the two buttons
  sit at the rail's end and act on the selected branch the same way.
- A right-click on the row opens the branch's menu (`projects::worktree_menu`):
  - **Reveal in Finder** and **Copy Path**.
  - **Stop Terminals…**, after a separator, while it has terminals.
  - **Delete Worktree…**, after a separator: the usual dialog, which says what deleting would
    lose before anything goes. Not offered on a main checkout, which git can't delete. Its
    switch, and the New Worktree dialog's, are the settings' own, macOS's `NSSwitch`.
- **Dragging a row moves its branch**, row and tabs together, among the project's, as a tab
  moves in its list (`shell/reorder.rs`): it follows the pointer, the branches it passes slide
  into the place it left once it's past their middle, and let go it slides into the place it's
  over. It joins no other project. The order is the project's (`Project::branches`), kept for the
  next launch (section 9); a branch it doesn't name, as a new one, comes last.
- **Tab rows** (with **Tabs** on the sidebar): 36 pt high with 10 pt corners, a 16 pt
  icon, the title cut short, the selected tab a pill of white laid thin over the colours, so it
  takes their hue, with no shadow; a hover tint, and a close button at its end on hover and on the
  tab shown. While nothing's at its end, the title and what was said under it run on to the row's
  end.
  - **Indentation.** A branch's tab rows are indented under the branch's row by the same step the
    branch's row is under its project's: project, branch, tab, one equal step each, one shared
    constant (`shell/elements.rs`, `INDENT`). The indent is the row's inner padding before its
    content, never a margin, so every row's hover and selected background stays the sidebar's full
    width, branch rows and tab rows alike. **[asked, 2026-11-24]**
  - A chat's icon is Claude's logo in its colour, claude.ai's own, as a page shows its own
    (`Pane::favicon`), wherever a tab shows its icon.
  - A chat's row, but for the tab shown's, says under its title the start of what was said last
    in it, the person's message or the agent's reply, smaller and in the labels' grey, the two
    lines in the row's height. The tab shown shows the chat beside the sidebar, so its row has
    the title alone. Pages' rows don't.
  - A split tab's row shows its panes side by side: each pane's icon and title, sharing the row,
    a hairline between them and no fill of their own. Each pane shows its × while the pointer's
    on it; one with unsaved edits shows its dot always.
  - A row says at its end how the tab's doing (`Shell::tab_activity`): the spinner while a chat
    in it is at work, an indigo dot once something in it waits for the person, which wins over the
    spinner. It has the ×'s place and leaves it to the × while the pointer's on the row; a
    split's row has it before its panes.
  - Unsaved edits' and splits' dots, middle-click to close, and dragging to reorder stay.
- A tab dragged past its list's end onto another branch's tabs or row moves to that branch and
  shows there: where the tab it's dropped on is, or first. A collapsed branch's row takes it too,
  and the branch stays collapsed. Its content doesn't move: a terminal stays in its folder, a
  chat with its thread. The window comes back with the tab where it was put (`docs/browser.md`
  1).
- A page's tab dragged onto the pinned pages is pinned, and onto the unpinned pages is one of
  them; a pinned page's tile or an unpinned row let go on a branch's tabs or row is a tab there
  (section 11). While a pinned or an unpinned page is shown, no tab's row is the pill.

## 6. Moving by keys

`docs/layout.md` lists the keys. How they work:

- **⌥←** and **⌥→** (`projects::PreviousProject`, `projects::NextProject`) show the project to the left
  and to the right, and past the space's first or last on into the space beside, at the project
  nearest (its last going left, its first going right), the space sliding in as a swipe brings
  it. A space with no project is passed over. A pinned or an unpinned page shown is left for the
  project's tab.
- **⌥⇧←** and **⌥⇧→** (`spaces::PreviousSpace`, `spaces::NextSpace`) show the space to the left and to
  the right, at the project it showed last, a space with no project too.
- **⌥↑** and **⌥↓** (`tabs::PreviousTab`, `tabs::NextTab`) step every tab in the order listed: the
  pinned pages, the unpinned pages, the Settings tab while it's open, then the branch tabs
  (sidebar: every unfolded project's uncollapsed branches in list order; rail: the selected
  branch's tabs). **[asked]** The highlight moves at once. At either end they nudge, as the space
  keys do. The tab shown doesn't take the keyboard, so pressed again they go on from it. There's
  no ⌘ form, as a text field's ⌘⌥↑ and ⌘⌥↓ add a cursor.
- They work wherever the keyboard isn't in text. In text ⌥← and ⌥→ stay a word back and on, and
  ⌥⇧ a word's selection: in a terminal (its program gets `ESC b` / `ESC f`, and ⌥⇧← and ⌥⇧→ as
  they are), Koyo's text fields, and a web page, the agent desktop or a simulator, whose text
  field Koyo can't see. The ⌘⌥ forms work from wherever the keyboard is, the chat's prompt
  included.
- Pressed again while a switch is under way, they go on from the project, or the space, it's
  headed for. Held, they repeat as the arrows do. While the Settings tab is shown, ⌥← and ⌥→ do
  nothing. Settings → Keyboard shortcuts lists them under Spaces.

## 7. The foot

- On the left, in the **+**'s width, the Claude accounts button (`docs/claude-accounts.md` 6):
  the active account's week as a ring, opening the accounts panel; nothing while Koyo doesn't
  manage the accounts. It lives here, not on the rail. **[asked]**
- The spaces' marks, centred (section 4).
- A **+** on the right opens a menu (`shell/sidebar/foot.rs`):
  - **New Worktree…**: in the selected project, through the New Worktree dialog.
  - **New Project…**: added to the space shown.
  - **New Space…**

## 8. ⌘T: a new worktree

- **Worktree** is the launcher's fourth kind, after Browser, Chat and Terminal, with its own
  pill and icon. Tab goes through it like the others.
- Its field takes only the worktree's name. Enter opens the New Worktree dialog with that name
  filled in, on the selected project, and the branch is picked there. Nothing is listed under
  the field.

## 9. Model and persistence

- `settings.json` keeps the spaces (`spaces`: name, projects in the sidebar's order, the project
  selected) and the space shown (`space`); a theme or an `icon` a space has from an earlier build
  is dropped, and so is a `sidebar_blur`. It keeps the layout's settings: `tabs`, `address_bar`,
  `changes_and_files`, `loose_pages` and `sidebar_glass`, and `pinned_zone_dismissed`. Koyo writes
  every setting on save, defaults included, so a new default reaches only a file without that
  key.
- `project_looks` keeps, for each project, how its branches show in the sidebar (section 5):
  their order once one's been dragged (`worktrees`) and those collapsed (`collapsed`), each by
  its folder, and whether they're all folded away under its row (`folded`, section 3). Whether a
  branch's panel is open isn't kept: the window opens with every panel closed. What's kept of a
  branch git no longer lists goes when the project's branches are next read.
- The keyboard is where the person puts it (`shell/keys.rs`). A pane has it once they press in
  it, or ask for its tab: picked in the sidebar, opened (⌘T, a new terminal, a file), switched
  to (⌃⇥), opened again (⌘⇧T), shown in place of the tab or the split's pane they were typing in
  and closed, or shown again as the Settings tab over it closes.
- A chat's prompt is the exception: moving to a chat, by any of those or by another space or
  project shown, never puts the keyboard in its prompt, so the ⌥ keys go on moving. The prompt
  has the keyboard once the person presses in it, or in a chat they've just opened to type in
  (⌘T, a question asked from the launcher). A chat brought back with the window has none, and
  coming back from a subagent's chat leaves the prompt alone too.
- A press on anything that takes no keyboard (a chat's background or its messages, the sidebar,
  the glass) takes it out of the pane, to the window, where the window's own keys work and
  nothing is typed into; a press on a button, a menu or a scrollbar leaves it where it was.
  Another space, project or branch shown, or another tab by ⌥↑ or ⌥↓, leaves it at the window.
  There, what a chat hears on its own root, its card's key (⌘⇧B) and its menu, still reaches the
  chat the person is in (`chat::heard_at_window`); the keys of its prompt (Esc to stop, ↑, ⌘↵)
  need the keyboard in the prompt.
- A new project joins the space shown, and so do projects no space holds, as every project the
  first time this runs, into a first space made then. A project removed leaves its space. A
  space lists its projects in the sidebar's order, app control's moves included.
- Selected another way (app control, a link, ⌃⇥), a project's own space is shown, and its tab
  there, not the unpinned page that space showed last (`follow_space`). A space with no project
  shows no project at all: the window shows none of another space's tabs, and the side panel has
  nothing to act on, until a project is picked; ⌃⇥ still goes to the tabs looked at last,
  wherever they are (`docs/tab-switcher.md`). ⌘T opens a page there, an unpinned one of the
  space, and ⌘W closes the unpinned or the pinned page shown (section 11).
- Each space has an `id`, given once and kept whatever its place among the spaces: its unpinned
  pages go by it (`docs/browser.md` 2).
- The pinned pages are no space's and no project's: they're kept in a file of their own in the
  data folder, `pinned-tabs.json` (section 11, `docs/browser.md` 10). `settings.json` keeps only
  whether their empty zone was put away (`pinned_zone_dismissed`), and whether the spaces share
  their unpinned pages (`loose_pages`, 11.3). The unpinned pages are open tabs, kept with the
  window (`window.json`, `docs/browser.md` 2).
- The browsing profiles, and the one each project is on, are a file of their own too,
  `browser-profiles.json`, so a build that doesn't know them can't drop them from `settings.json`
  (`docs/browser.md` 15).

## 10. Frame, islands and glass

- **The frame.** **[asked]** The sidebar is on the window's glass (`SidebarFrame`, the shades of
  `sidebar::Look`) and the panes are on islands (`theme::card::island`). With **Tabs** on the top
  rail, the rail runs across the whole window above the sidebar and the islands. With **Tabs** on
  the sidebar there is no rail: the sidebar's top row holds the traffic lights and the toggle,
  and the islands rise to the gutter at the window's top (`Shell::frame`).
- Right of the sidebar, each part of the window is an island: a rounded card in the theme's
  colour with a barely-there shadow, the window's glass between them as a margin as thin as the
  window's edge.
  - Each pane of the tab shown; a split's sides are islands of their own, the glass between them
    the handle that resizes them.
  - The Changes or Files panel, the glass before it the handle that resizes it.
  - The places a symbol led to, and the bottom panel, under the panes.
  - The Settings tab, and what's said where there's no tab.
- **The sidebar is one with the rail.** **[asked]** By default the sidebar lies on the window's
  glass with nothing of its own on it, as the top rail does, blurred by the window's own blur as
  the rest of the window is: no fill, no wallpaper (`docs/chat-backdrop.md` 2), and no hairline
  before the islands, whose corners are all rounded, those beside the sidebar among them.
- **Sidebar glass** is how see-through the sidebar is: from clear, the default, through the
  card's own at the middle to solid; the cards stay the theme's own colour. Its surface is what's
  on the glass short of the margin before the islands, unfilled by default (`fill` and
  `SidebarFrame` in `shell/sidebar/frame.rs`).
- A page is drawn in the window's own frame, so the island's rounded corners clip it as they clip
  the rest of what's on it.

## 11. Pinned and unpinned pages

Written 2026-11-13 on branch `schulzfel/pinned-browser-tabs`, rewritten 2026-11-24 for the one
layout. Pinned pages belong to every space; unpinned pages to no branch. An unpinned page is
`loose` in the code (`model/loose.rs`, `loose_pages`).

### 11.1 The order

Pinned pages, then unpinned pages, then the Settings tab while it's open, then the branch tabs,
wherever **Tabs** puts them: on the rail (section 1), or in the sidebar from the top, under the
address bar, over the projects (`docs/layout.md`). **[asked]**

- **The pinned pages** are the same in every space: they're outside what swipes, and stay where
  they are as a space goes out and the next comes in.
- **The unpinned pages**, per space, are the space's and swipe with it. Shared (2026-11-20, the
  owner: "when the unpinned tabs are shared they should not swipe with the space, they should
  stay there like the pinned ones on space change"), they're outside what swipes, as the pinned
  pages are, by a swipe, the keys or a space's mark. **[asked]** In the sidebar, shared or per
  space, they and the projects scroll together, one scroll area under the pinned tiles
  (`shell/sidebar/tab_rows.rs`, `docs/layout.md`). **[asked]**
- No space's name shows in the sidebar but over its mark while the pointer's on it. **[asked]**

### 11.2 The pinned pages

- A tile per pinned page: 44 pt high, 10 pt corners, 8 pt apart, sharing the sidebar's width, up
  to four to a row and three for five or six, in the `Look`'s tile shades, the page shown's lit.
  **[ours]**
- A tile shows its page's site icon, the favicon the browser fetched, 16 pt as a tab row's icon
  is; with none kept, the first letter of its host; a globe for a page with no host, as a start
  page. **[asked: icon, initials or a globe, and the row's size; ours: one letter]**
- **A page's favicon has its corners rounded 2 pt wherever a tab shows it**: on a pinned tile, an
  unpinned page's row, a branch's tab row and a split's halves, the chip of a tab in hand, and a
  tab on the rail (`Face::icon`, `FAVICON_CORNERS`). **[asked]**
- The picture of a page that isn't open, closed or not opened since the window came back, is
  drawn at 60 % strength. **[ours]** The pointer resting on a tile brings its card: the page's
  icon, title and host (`shell/sidebar/card.rs`, `hover.rs`). **[asked]**
- With no page pinned the region is a zone a tile high with a dotted outline, saying "Drag a
  page here to pin it". The pointer on it shows a × at its end (tooltip "Hide") that puts the
  zone away; that's kept in `settings.json` (`pinned_zone_dismissed`). Put away, the zone is back
  once a page's tab is taken off its list, saying "Drop to pin", and gone again when the tab's
  let go elsewhere. Nothing brings it back for good: pinning a page shows tiles instead, and
  with the last pin gone it stays away. **[asked; ours: the copy, the height, the ×, that nothing
  un-hides it]**
- **Only a tab whose single pane is a page is pinned.** A split, a terminal, a chat, a file or a
  page's `DevTools` in a tab of their own isn't taken, and the region doesn't light for one. An
  unpinned page is always one. **[asked]**
- A page's tab dragged from a branch's list, or an unpinned row, onto the region is pinned: off
  its list it's a chip; the zone, or a dotted outline round the tiles, lights under it; let go,
  it leaves where it was and is pinned **where the pointer has it among the tiles**: before the
  first, between any two, or after the last. **[asked, 2026-11-23: "they can choose precisely
  where it goes"]** While it's over the region the tiles lay themselves out as they will be with
  it among them: every tile from the slot the pointer is nearest on slides one along, that slot
  is washed and outlined, dashed, in the accent colour, and a row it needs opens under the
  others. Let go, it fills that slot and nothing else moves. Taken off the region, the tiles
  slide back (`shell/tile_grid.rs`, `TileHand::lands`). **[ours: the nearest slot, the slot's
  light]** It goes on showing if it was the tab shown, and its page isn't reloaded. **[ours]**
- Tiles move among themselves (`shell/tile_grid.rs`, a hand of its own, `Shell::pinned_hand`):
  the one in hand follows the pointer, the others step out of its way, let go it slides into its
  slot, and taken well away it goes back with nothing changed. **[asked]** Taken well away, it
  travels as a chip, as a tab off its list does. Let go on a branch's tab or row it's
  **unpinned** there: a tab of that branch where that tab is, or first, and shown. Let go on an
  unpinned page's row it's an unpinned page of the space, where that row is. **[asked; ours: the
  chip, and the unpinned pages taking it]**
- **On the top rail** (2026-11-23, the owner: "pinned browser tabs should sit in the top rail
  inside an overlay pill ... the tell sign that this is shared, not a project tab but a pinned
  tab"), the same pages are buttons in a pill after back and forward (`shell/top_rail/pinned.rs`):
  **[asked]**
  - Each button is 26 pt square and shows its page's picture alone, as a tile does
    (`shell::pinned_picture`), 2 pt apart, in a pill 3 pt round them on every side: white over the
    rail, faint in the dark appearance and stronger in the light, and **no edge**: it reads by its
    fill alone. **[asked, 2026-11-23: "that border is bad"]** **The corners nest**: the pill is a
    rounded rectangle with 9 pt corners, not a capsule, and a button, 3 pt in from its edge, has
    9 − 3 = 6 pt corners, the tab rows' own; the slot lit where a page lands has the same.
    **[asked, 2026-11-23: "the radiuses must match"; ours: 9 and 6]** The page shown's button is
    lit. Hovered, a button's tooltip says its page's title. **[ours: the sizes and shades, the
    tooltip rather than the card]**
  - Picked, a button shows its page (11.4); dragged along the pill, it moves among them with the
    tiles' physics; right-clicked, it has the tile's menu (11.5). Taken off the pill it goes back
    to its slot: the rail has nowhere to unpin it by dragging, so Unpin is in its menu. **[ours]**
  - A page's tab of the branch, or an unpinned page's tab, dragged over the pill is off its list
    and is pinned where the pointer has it among the buttons, the buttons making room for it and
    the pill growing by one. **[asked]**
  - With none pinned, the zone is a pill of the same shape and fill, with no edge, saying "Drag a
    page here to pin it", its × at its end under the pointer putting it away (the same
    `pinned_zone_dismissed`): a 26 pt button 3 pt in from the zone's edge with 6 pt corners. While
    a page's tab is in hand it says "Drop to pin", edged in dashes, the one edge it has; put
    away, it's back so once a page's tab is taken off its list. **[asked; ours: the dashes only
    while a tab is in hand]**

### 11.3 The unpinned pages

- **A space's own.** Per space, each space lists the pages opened in it that belong to no
  branch, as rows like a branch's tabs, or tabs on the rail: the page's icon and title, the one
  shown a pill, a × at its end on hover and on the one shown. Another space lists its own, and a
  space shown again shows the page it showed last. **[asked: per space]**
- **Or shared** (2026-11-20, the owner: "have the unpinned tabs be either shared per project or
  not (in options)"). **Unpinned pages** is **Per space**, or **Shared**, the default: every
  space lists every unpinned page, one list in one order, whichever space or project is shown
  (`settings.json` `loose_pages`, `PerSpace` or `Shared`; app control's `loosePages`). A stored
  choice is kept. The pinned pages are every space's either way. **[asked]**
  - Each page is filed under a space whichever is chosen: the one it was opened in, dragged or
    unpinned into, or that took it from a space deleted. Shared, a page opened is filed under the
    space shown then, wherever it goes in the list. The setting only says which a space lists, so
    changing it closes, moves and loses nothing: shared, every space's pages show together, in
    the order they had; back to per space, each space lists those filed under it, in the order
    the shared list had them. **[ours: filed where it was opened rather than where it was last
    seen]**
  - Shared, an unpinned page shown stays shown as the space changes, as a pinned page does
    (11.4), and with none shown the next space shows its project's tab rather than the page it
    showed last. Each space still remembers the one it showed last, which is the one shown when
    it was left; back to per space, a space that showed one filed under another shows its
    project's tab instead. **[asked, 2026-11-20: "stay there like the pinned ones on space
    change"]**
  - Picked where it's listed, by its row or ⌃⇥, a shared page shows in the space shown, which
    doesn't change; ⌘⇧T brings one back among the space shown's. **[ours]**
- **Only pages.** No chat, terminal or file is unpinned: each needs a branch to be in. **[asked,
  for now]**
- **How a page comes to be one:** **[asked]**
  - **Opened where the person is in no branch.** A browser tab opened (⌘T, a link picked in the
    launcher) while no project is shown, or while an unpinned or a pinned page is the tab shown,
    is an unpinned page of the space: after the one shown, or after them all, and shown. **[ours:
    that an unpinned or a pinned page shown counts as being in no branch]**
  - **Dragged.** A page's tab taken off a branch's list and let go on an unpinned page goes where
    that page is; a pinned page's tile let go there is unpinned there. Either shows.
  - **The drop spot.** With none yet, once a page's tab is off its list, or a pinned tile well
    away from the others, a dashed spot, on the rail and in the sidebar, says "Drop to keep it
    here" and takes it as the last, shown. It stays until what's in hand is let go, and doesn't
    show for a tab moved along its own list. **[ours; the words `docs/layout.md`'s]**
  - **The +.** The **+** after the unpinned pages, always there, opens the launcher for an unpinned
    page whatever tab is shown (`docs/new-tab.md`, `launcher::Opens`). On the rail it's an icon; in
    the sidebar it's a row like theirs, the plus and **New Tab**, filled at rest and brighter under
    the pointer (`unpinned::new_tab`). **[asked]** ⌘T opens the same wherever the person is in no branch, a space with no
    project or a pinned or an unpinned page shown: Tab goes to no other kind there.
    **[`docs/layout.md`]**
- A row is dragged among the others, as a tab along its list, but joins none: each stays a page.
  Dragged onto a branch's tab or row it's a tab of that branch, shown; onto the pinned pages,
  pinned. Right-clicked, its menu has **Copy to Worktree**, as a pinned tile's (11.5): a new tab
  of the selected branch at its address, shown there; the page stays. Greyed out with no branch
  selected. **[ours; asked: the item]**
- ⌘W, its ×, or a middle click closes an unpinned page: it's gone. The one before it shows in
  its place, the one after when it was the first, and the selected branch's tab when it was the
  only one. **[ours]** A page that asks to confirm leaving asks first, and goes only once the
  person leaves; Cancel keeps it (`docs/browser.md` 17). **[asked, 2026-11-24: as Chrome]** It's
  among the tabs ⌘⇧T brings back, once it went, from the one history branches' tabs are kept in: back at its place among its space's unpinned pages, that space shown, at the
  address it showed, of the profile it kept (the space shown's if its own is gone). App
  control's `tab_reopen` brings back branches' tabs only (`ClosedFrom`, `shell/reopen.rs`).
  **[asked, 2026-11-15]**
- A space deleted hands its unpinned pages on with its projects: to the space that takes them,
  after that space's own; they close when the projects are taken out of Koyo. **[ours]**
- **On the rail** the pages are tabs like the branch's, after the pinned pages' pill and before
  their **+**, the hairline and the branch's tabs (`shell/top_rail/unpinned.rs`): the one shown lit, picked to
  show, closed by their × or a middle click, dragged along among themselves, or onto the pill,
  where they're pinned (11.2). A page's tab of the branch's let go on one goes where that tab
  is, shown. **[asked: before the branch's tabs, a separator between; `docs/layout.md`: the spot
  and the +; ours: the rest]**

### 11.4 The tab shown

- The panes show one tab: a pinned page, else the unpinned page the space shows, else the
  selected branch's tab. A click on a pinned tile or an unpinned row shows its page, which takes
  the keyboard. It's a page like any other: the same browser host, its keys (⌘R, ⌘F, zoom, ⌘[
  and ⌘]), `DevTools` and right-click menu, and the address and controls of section 2.
  **[asked]**
- **Switching space leaves a pinned page shown.** By a swipe, the keys, or a click on a space's
  mark, the sidebar's unpinned pages, projects and the rail's tabs change and the panes don't:
  the pinned page stays, the keyboard where it was, a space with no project too. A click on a
  space's mark doesn't take the keyboard to the window while a pinned page is shown, as a press
  elsewhere in the sidebar does (section 9). **[asked; ours: the mark's press]** Per space, an
  unpinned page shown is its space's: another space shows what it showed last. Shared, it's every
  space's, and switching space leaves it shown as it leaves a pinned page
  (`Workspace::shown_in_every_space`).
- The keyboard goes by section 9 with these pages as with any tab: one opened or picked takes
  it; one closed with the keyboard in it (its ×, its menu, ⌘W) hands it to what shows in its
  place; one moved (pinned, unpinned, dragged among the unpinned pages or onto a branch) leaves
  it where it was (`Shell::close_apart`, `show_tab_holding`). **[ours]**
- One path for every page (2026-11-15, the owner: "no behaviour difference between them"): a
  pinned or an unpinned page is found, shown, closed, has its `DevTools` taken out of its tab and
  opens its links the way a branch's page does, each found wherever its tab is
  (`Workspace::locate`, `shell/browser_tabs.rs`, `shell/closing.rs`, `Shell::show_tab_holding`).
  What stays apart are the rules of the pages themselves: a pinned page closed stays pinned, an
  unpinned one is gone; their tabs are a page and its `DevTools`, nothing more; a link from one
  opens an unpinned page; they keep their own profile's project; and app control's reach (11.6).
- Picking a tab of a branch, or a project's row, leaves a pinned or an unpinned page for that;
  so does anything else that shows a branch's tab: ⌃⇥ picking one, a file opened, ⌘⇧T, a
  branch's +, app control selecting a tab, branch or project, the ⌥ keys showing a project, in
  another space too, or a branch's tab. A pinned page's page stays open, as it was, and shows
  again when its tile is picked. ⌃⇥ lists the pinned and the unpinned pages among the tabs looked
  at last (`docs/tab-switcher.md`). **[asked; ours: the list of what else leaves it]**
- A pinned or an unpinned page's `DevTools`, detached, split its tab as they split any page's;
  their pane has only its ×. No side of one takes a tab in hand: it's split with nothing.
  **[ours]**
- **On the top rail** (2026-11-23), while a pinned or an unpinned page is shown none of the
  branch's tabs in the rail is lit, the selected branch's row stays lit in the sidebar, and
  Changes and Files, wherever they are, still open that branch's panel beside the page. Picking a
  branch's tab, or a branch's row, leaves the page for the branch's tab; the rail's **+** after
  the branch's tabs opens a tab of the branch, as a branch row's **+** does. A page opened
  otherwise while one is shown, or with no branch selected, is an unpinned page of the space.
  **[asked: the rail shows them; ours: the rest]**

### 11.5 The address a page is pinned at, and closing it

From the owner's answers while it was built: closing a pinned page doesn't unpin it, and its
menu is the Arc browser's.

- **A pinned page keeps the address it's pinned at**: where its page was when it was pinned. Its
  page may go on anywhere from there. **[asked, from the Arc browser's menu]**
- **⌘W, or Close from its menu, closes the page and leaves it pinned.** Its page and its
  `DevTools` go, its tile stays, fainter, and what the space shows under it shows in its place.
  Picked again, it opens at the address it's pinned at. **[asked: closing doesn't unpin; ours:
  that it goes back to the pinned address rather than the one it last showed]** A page that
  asks to confirm leaving asks first, shown if it wasn't, and the tile's page closes only once the
  person leaves; Cancel keeps it open (`docs/browser.md` 17). **[asked, 2026-11-24: as Chrome]** A
  pinned page closed or removed isn't among the tabs ⌘⇧T brings back. **[ours]**
- Right-clicking a tile or a button opens its menu: **[asked]**
  - **Edit Pinned Page ›**: **Replace Pinned URL with Current** pins it at the address its page
    shows, greyed out while that's the one it's pinned at; **Edit…** is a dialog with the address
    in a field, read as the address bar reads one (`example.com` is `https://example.com`), words
    that are no address changing nothing. An open page stays where it is; a closed one opens at
    the new address.
  - **Close**: as ⌘W. Greyed out when there's nothing to close.
  - **Unpin**: a tab of the selected branch, after its others, shown there; with no branch
    selected, an unpinned page of the space. **[ours: where it goes with no branch]**
  - **Copy to Worktree**: a new tab of the selected branch at the address its page shows (or
    opens at, closed), after its others, shown there, of that branch's project's profile; the
    pinned page stays as it is. Greyed out with no branch selected. **[asked: the item; ours: its
    name, and the profile]**
  - **Remove Pinned Tab**: the page goes and it's pinned no longer. **[ours: the Arc browser's
    "Remove Favorite"]**

### 11.6 Links, sign-ins and agents

- A link that asks for a new tab or window (`target="_blank"`, `window.open`, a ⌘-, ⇧- or
  middle-click) from a pinned or an unpinned page, and **Open Link in New Tab** from its
  right-click menu or its `DevTools`, open an unpinned page of the space shown, right after an
  unpinned opener or after them all, as a page tab's opens a tab beside it: in front, or behind
  for a ⌘- or a middle click, as Chrome does. A page a script opened keeps its opener, so a
  sign-in's popup answers the page and closes itself (`koyo-browser-host/src/pane/popups.rs`).
  **[ours: as a page tab]**
- **A link to a web page clicked in a chat**, or the details card's pull request, opens in a
  browser tab right after the chat's, shown, as a link opened beside its page is
  (`files::OpenLinkedPage`), never in the system's browser. Only chat surfaces change: the
  markdown file preview keeps its behaviour. **[asked]**
- A page keeps a browsing profile, its cookies and sign-ins (`browser::profile`,
  `docs/browser.md` 15):
  - one pinned or dragged out of a branch keeps its project's, whichever project is selected,
    follows its project onto another profile, and is of that profile again after a relaunch;
  - one opened outside any project keeps Default, the person's own, the same in every space
    (`browser::NO_PROJECT`);
  - one a pinned or an unpinned page opens keeps its opener's, so it's signed in as its opener
    is;
  - one moved onto a branch's tabs keeps its profile while the window lives, and is its branch's
    project's once brought back with the window. **[ours]**
- Sign-ins survive a relaunch: a profile is a folder under `browser/profiles/` in the data
  folder, with session cookies kept (`persist_session_cookies`,
  `koyo-browser-host/src/host/panes.rs`); `browser::host_tests::keeping::
  a_sign_in_outlives_the_engine_quitting_and_starting_again` proves it against the packaged
  engine. **[checked 2026-11-13]** The history keeps a pinned or an unpinned page's visits under
  the project whose profile it keeps (`Shell::profile_place`).
- **App control's reach** (2026-11-15, the owner): agents see the person's pages and can't use
  them without asking. `browser_list` lists a pinned or an unpinned page with its title and
  address, as the person's (`persons`, `place`), and `tab_list` under `persons`; every browser
  tool on one fails `consent_required` until the person allows that agent that page in a prompt
  `browser_claim` asks. Allowed, it's that agent's, on the connection it asked on, while its
  lease lasts: until it lets go, the person takes the page back, 10 minutes pass without it
  acting, or the page closes, moves or is pinned elsewhere. `browser_duplicate` asks for a copy
  in the agent's branch instead. A call naming no pane never means one of these pages, and the
  window tools (`tab_select`, `tab_close`, `pane_split`, …) refuse them outright. What an agent
  opens out of sight leaves such a page shown. `docs/app-control.md` 5, The person's own pages.
  **[asked]**

### 11.7 Kept for the next launch

- **Pinned pages** survive a relaunch, a reboot and an update: `pinned-tabs.json` in the data
  folder (`settings::data_dir()`, which `KOYO_DATA_DIR` moves), beside `window.json` and apart
  from it, so a window file that can't be read doesn't take them with it (`docs/browser.md` 10).
  **[asked]**
- Each comes back in the order of the tiles, at the address it last showed, titled as it was,
  with its site's icon (`history-icons/`), pinned where it was, of the profile it had. The one
  that was shown is shown again; no other page is made until it's picked. **[asked; ours: that
  the one shown is shown again]**
- Written 500 ms after a change and on the way out, through the file's one writer
  (`kept_file::Writers`), only when it differs from what the file holds. Read tolerantly: see
  `docs/browser.md` 10. **[asked]**
- **Unpinned pages** are open tabs, so they're kept with the window, in `window.json`, by the
  `id` of the space each is filed under (`docs/browser.md` 2): each comes back in its space, in
  order, at the address it showed, titled as it was, of the profile it had, the one each space
  showed shown. The file lists them in the order of the shared list, a run of pages of one space
  at a time, so they come back in that order shared and each in its space per space, and a build
  from before the setting reads every page into its space. Those of a space the settings no
  longer have are the space shown's, rather than lost. **[ours]**

### 11.8 Where it lives

| Path | What |
| --- | --- |
| `koyo/src/shell/sidebar/address.rs`, `shell/sidebar/foot.rs`, `shell/spaces/swipe.rs`, `shell/spaces/tab_steps.rs` | The address bar and page controls, the foot's marks and menu, the swipe, the ⌥↑ / ⌥↓ steps |
| `koyo/src/shell/sidebar/projects.rs`, `shell/sidebar/rows.rs`, `shell/sidebar/tab_rows.rs` | The projects' rows, the branches' rows and their tabs |
| `koyo/src/shell/sidebar/frame.rs`, `shell/rails.rs` | The sidebar's frame and the rail across the window |
| `koyo/src/model/pinned.rs` | The pinned pages on the workspace: pinning, moving, closing, the pinned address, unpinning, removing |
| `koyo/src/model/loose.rs` | The unpinned pages, each filed under a space, which a space lists (its own, or all of them shared), the tab shown among pinned, unpinned and a branch's, and a page's tab moving between the three |
| `koyo/src/settings/loose_pages.rs`, `settings_view/appearance.rs` | Unpinned pages, per space or shared, and its row in Settings → Appearance's Layout section |
| `koyo/src/shell/pinned.rs` | The shell's side of the pinned pages: what a pin takes, picking, closing, the pinned address |
| `koyo/src/shell/loose.rs` | The shell's side of the unpinned pages: opening one, closing, moving |
| `koyo/src/shell/browser_tabs.rs`, `shell/closing.rs`, `shell/keys.rs` | A page's events, closing a pane and showing a tab, one way for a branch's, a pinned and an unpinned page (`Workspace::locate`, `Shell::show_tab_holding`) |
| `koyo/src/shell/kept/pinned.rs` | `pinned-tabs.json`: reading it tolerantly, writing it, and bringing the pinned pages back |
| `koyo/src/shell/kept/loose.rs` | The unpinned pages in `window.json`, and bringing both kinds back as the window opens |
| `koyo/src/shell/sidebar/pinned.rs` | The pinned region: tiles, the zone, the chip, the menu and its actions |
| `koyo/src/shell/sidebar/unpinned.rs` | The unpinned rows, their menu, and the spot that takes the first |
| `koyo/src/control/exec/browser/pages/` (`listing.rs`, `consent.rs`, `grants.rs`), `asking.rs`, `asker.rs` | App control's reach: the person's pages listed, the gate and the consents it keeps, the prompts, and who asks |
| `koyo/src/shell/tile_grid.rs` | The grid of the pinned tiles with its `TileHand`, and a page from elsewhere landing among them |
| `koyo/src/shell/top_rail/pinned.rs`, `shell/top_rail/unpinned.rs`, `shell/top_rail.rs` (`pages`) | The top rail: the pinned pages' pill and zone, the unpinned pages' tabs and their spot, their +, the hairline |
| `koyo/src/shell/sidebar/card.rs`, `hover.rs` | A pinned page's card |
| `koyo/src/shell/reorder.rs`, `reorder/hand.rs` | The unpinned rows' list, project and branch rows, and a tab off its list (`TabHand::left_list`, `taken_at`, `takes_for`) |
| `koyo/src/spaces.rs` | A space's `id` |
| `koyo/src/launcher/here.rs`, `launcher/mod.rs` (`Opens`) | The launcher where the person is in no branch, or asked for by the unpinned pages' +: an unpinned page only |
| `koyo/src/shell/sidebar/tests/`, `shell/top_rail/tests.rs`, `shell/tile_grid/tests.rs`, `shell/kept/tests/`, `model/pinned/tests.rs`, `model/loose/tests.rs`, `settings/tests.rs`, `control/tests/` | The tests |
| `koyo/src/shell/sidebar/visual/pins.rs`, `shell/sidebar/visual/rail.rs` | The pictures (`sidebar-pinned-*.png`, `sidebar-unpinned-*.png`, `rail-*.png`) |
| `koyo/src/model/settings_tab.rs`, `navigation.rs`, `shell/settings_tab.rs`, `settings_view/page_list.rs` | The Settings tab (11.9): on the workspace, opened once, shown over the tab shown, closed by anything else shown, its pages and Back and Forward; on the shell, its close and its content, with its tests in `settings_tab/tests.rs` |
| `koyo/src/shell/top_rail/unpinned.rs` (`settings_tab`), `shell/sidebar/unpinned.rs` (`settings`) | The Settings tab's tab on the rail and its row in the sidebar |

### 11.9 The Settings tab

Written 2026-11-23. The owner: "make so settings becomes an unpinned tab (remove the <- next to
settings) - going away from settings means closing the tab", with "a favicon like a gear or
something from the icon pack we already have in the app".

- ⌘,, **Settings…** in the app's Koyo menu and the right-click menus, and app control's
  `settings_open` open Settings as a tab among the unpinned pages: with **Tabs** on the sidebar,
  a row after the unpinned pages, over the projects; on the top rail, a tab after them, before
  the hairline (section 1). Its icon is the icon pack's gear (`IconName::Settings`), where a
  page's own icon goes, and it says "Settings". There's one at most: opened while it's open, it's
  picked, on the page asked for if one is. **[asked]**
- It's shown over the tab that was shown, a branch's, a pinned or an unpinned page, as a pinned
  page is over an unpinned one (11.4), and the sidebar stays as it is beside it. Settings' own
  pages are listed inside the tab, at its left, under their search field, and the page picked is
  beside them. They're the tab's content, on the card, not the sidebar's glass. There's
  no arrow back. **[asked]**
- It's open only while it's shown: anything else shown closes it, another tab, a pinned or an
  unpinned page, a branch, a project or a space, a tab or a file opened, the Changes or Files
  panel. So do its ×, a middle click on it, ⌘W, Escape and app control's `settings_close`; the
  tab it was over shows again. **[asked]**
- It keeps the pages it showed: Back and Forward (⌘[ and ⌘] outside a page, the mouse's side
  buttons) walk them as a page's walk its history, and stop at its first and last page; while
  it's closed they have nothing to walk. The keyboard's `GoBack` and `GoForward` for Settings
  pages stay; the rail has no buttons for them. A new Settings tab opens on the page the last one
  showed, with nothing behind it. **[asked: the rail's Settings arrows go; ours: the rest]**
- It's no branch's or space's, and only shows: it isn't dragged, pinned, copied or moved to a
  branch, isn't among the tabs ⌃⇥ switches between, isn't kept for the next launch, and closed
  it isn't among the tabs ⌘⇧T brings back, which would bring back a tab that closes again as
  soon as anything else shows. **[ours]**
