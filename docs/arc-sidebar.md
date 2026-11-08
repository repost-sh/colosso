# Koyo's Arc sidebar: spaces, project tiles and worktree dividers

Written 2026-11-08 on branch `schulzfel/arc-like-sidebar`. Drafted with Claude.

The Arc layout's sidebar is rebuilt to look and work like Arc's own: browser controls and an
address bar at the top, the projects of a **space** as tiles, the space's name, and each
worktree of the selected project as a divider with its tabs under it. Spaces are switched
along the sidebar's foot. This note is the spec the work followed; keep it in step with the
code in `koyo-rs/koyo/src/arc/` and `koyo-rs/koyo/src/spaces.rs`. The visual check draws it:

```sh
KOYO_CONFIG_DIR=$PWD/.superpowers/qa/arc/config \
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
  browser tab shown, and are greyed out on any other tab and on a settings page.
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
- With a page shown it's the page's own address field: the host until it's clicked, or ⌘L or a
  new tab puts the keyboard in it, then the whole address to edit, Enter loading it.

## 3. Project tiles

Arc's favourites grid, holding the projects of the space shown.

- One rounded tile per project, its icon centred. Tiles share the row's width and wrap
  onto new rows.
- The icon comes from the repository's git host: the `origin` remote says which host
  (GitHub, GitLab, Bitbucket or another) and owner, and the owner's or repository's avatar
  is fetched once and kept in the app's data folder. With no remote, or no avatar to fetch,
  the project's initials stand in.
- The selected project's tile is highlighted. Clicking a tile selects that project.
- A small red dot in the tile's top-right corner while any tab of the project needs
  attention (section 5).
- While something in the project works (a chat running), terminal-style dots in the same
  corner: a braille cell's dots going round, a step every 80 ms. Tabs elsewhere that show a chat
  working use the same dots.
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
  the first worktree divider. The space's own name shows on glass over its mark at the foot
  while the pointer's on it (section 7).

## 5. Worktrees and their tabs

- Each worktree is an Arc-style divider: the worktree's name on the left in the small style
  Arc gives "Clear", and a thin line to the end. Hovering the divider fades in what the
  worktree offers at the end, in the destructive colour, the line making room for it; leaving
  fades it out and the line runs to the end again. No badges on it.
  - **Clear**, while it has tabs: it asks first, then closes every tab of the worktree,
    stopping its terminals and chats.
  - **Delete**, while it has none: the usual Delete Worktree dialog, which says what deleting
    would lose before anything goes. Its switch, and the New Worktree dialog's, are the
    settings' own, macOS's `NSSwitch`.
  - Nothing for a main checkout with no tabs: git can't delete it, and there's nothing to clear.
- Right-clicking a divider opens the worktree's menu (Reveal in Finder, Copy Path, Stop
  Terminals…, Delete Worktree…). Clicking its name selects the worktree.
- A **+ New Tab** row under each divider, in Arc's style: it selects the worktree and opens
  the ⌘T launcher.
- Tab rows as Arc's, 36 pt high with 10 pt corners: a 16 pt icon, the title cut short, the
  selected tab a pill of white laid thin over the colours, so it takes their hue, with no
  shadow; a hover tint, and a close button on hover.
- A split tab's row shows its panes side by side as Arc does: each pane's icon and title,
  sharing the row, a hairline between them and no fill of their own, the row's tint or pill
  being theirs. Each pane shows its × while the pointer's on it, making room for it then; one
  with unsaved edits shows its dot always.
- A tab that needs attention shows a dot, as the tiles do: a terminal waiting for input, a
  chat that's unread, failed or needs the person. It replaces the chat marks tabs had.
- Unsaved edits' and splits' dots, middle-click to close, and dragging to reorder stay.
- A tab dragged past its list's end, onto another worktree's tabs, divider or New Tab row,
  moves to that worktree and shows there: where the tab it's dropped on is, or first. Its
  content doesn't move: a terminal stays in its folder, a chat with its thread, and on the
  next launch each comes back with its own worktree.
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
  mark, and a new space, move the same way; with reduced motion the switch is at once.
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
- It keeps each project's Koyo-only name and icon (`project_looks`).
- A new project joins the space shown, and so do projects no space holds, as every project
  the first time this runs, into a first space made then. A project removed leaves its space.
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

