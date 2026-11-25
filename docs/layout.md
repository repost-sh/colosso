---
title: "Layout"
---

Koyo has one layout: a sidebar of projects and their branches, a top rail across the window, and the panes between them. Each person arranges it in Settings → Appearance → Layout, or by right-clicking the piece to change. Spaces group projects, and switching spaces is the one animated move between them.

## Settings

Settings → Appearance → Layout:

| Setting | Choices | Default | `settings.json` key |
|---|---|---|---|
| Tabs | Top rail / Sidebar | Top rail | `tabs` |
| Address bar | In the page / In the sidebar | In the sidebar | `address_bar` |
| Changes and Files | Worktree / Top rail | Top rail | `changes_and_files` |
| Unpinned pages | Per space / Shared | Shared | `loose_pages` |
| Sidebar glass | Slider, Clear to Solid | Clear (left end) | `sidebar_glass` |

```json
{
  "tabs": "TopRail",
  "address_bar": "Sidebar",
  "changes_and_files": "TopRail",
  "loose_pages": "Shared",
  "pinned_zone_dismissed": false
}
```

The Theme, Chat backdrop and Fonts sections are unchanged.

Koyo writes every setting into `settings.json` on save, defaults included. A new default therefore reaches only a file without that key: an existing install keeps the `loose_pages` value it already stored.

### Tabs

**Tabs** moves every tab together: the pinned pages, the unpinned pages, the Settings tab and the selected branch's tabs. Pinned and unpinned pages are never split between the rail and the sidebar, since they form one ordered list.

| Tabs | Pinned pages | Unpinned pages | Branch tabs |
|---|---|---|---|
| Top rail | Icon buttons in a pill, after back and forward | Tabs after the pill, then their **+** | After a hairline, then their **+** |
| Sidebar | Tiles above Projects | Rows under the tiles, then their **+** | Rows under each branch's row |

### Address bar

**Address bar** is chosen independently of **Tabs**.

- **In the sidebar**: a rounded field at the top of the sidebar, under its top row when it has one. Pages have no toolbar. ⌘L, or a click on the field, opens the launcher on the page shown.
- **In the page**: each page keeps its own toolbar with its address, back, forward and reload.

Back, forward and reload go wherever the address goes, so they are never shown twice. With the address bar in the page they live only in the page's toolbar, and the window has none. With the address bar in the sidebar they sit together, reload right after forward, in the window's top row: packed into the top rail after the sidebar toggle when **Tabs** is on the top rail, on the right of the sidebar's top row when **Tabs** is on the sidebar.

| Tabs | Address bar | Top of the window | Address, ← → ⟳ |
|---|---|---|---|
| Top rail | Sidebar | Rail: traffic lights, sidebar toggle, ← → ⟳, pinned pill, unpinned tabs, +, hairline, branch tabs, + | Address bar first in the sidebar, under the rail |
| Top rail | Page | Rail: traffic lights, sidebar toggle, pinned pill, unpinned tabs, +, hairline, branch tabs, + | The page's toolbar |
| Sidebar | Sidebar | Sidebar top row: traffic lights and sidebar toggle on the left, ← → ⟳ on the right | Address bar under that row |
| Sidebar | Page | Sidebar top row: traffic lights and sidebar toggle | The page's toolbar |

### Changes and Files

**Changes and Files** places the selected branch's two panel buttons.

- **Top rail**: at the rail's right end, after the empty glass that moves the window.
- **Worktree**: on the branch's row in the sidebar, shown on hover and kept while its panel is open.

Either way they act on the selected branch: Changes opens its changed files, Files its file tree, and Changes counts the lines added and removed.

**Top rail** is offered only while **Tabs** is on the top rail. With **Tabs** on the sidebar there is no top rail: the window's top row is the sidebar's, the panes reach the window's top edge, and the buttons sit on the branch's row. The Settings row and the button's right-click menu show **Top rail** greyed out then. The stored choice is kept, so moving the tabs back to the top rail puts the buttons back there.

## Right-click menus

Each piece of the window changes its own setting from a native menu. A choice applies at once, exactly as the Settings row does.

| Right-click on | Menu |
|---|---|
| Empty space in the sidebar | The space shown: Rename Space…, Delete Space, then Edit Theme…, Appearance ▸, Toggle Sidebar, Tabs ▸, Address bar ▸, Changes and Files ▸, Unpinned pages ▸, Settings… |
| Empty glass on the top rail | Edit Theme…, Appearance ▸, Toggle Sidebar, Tabs ▸, Address bar ▸, Changes and Files ▸, Unpinned pages ▸, Settings… |
| The Changes or Files button | Show on Worktree / Show on Top rail (one checked), Settings… |
| The address bar, in the sidebar or the page | Address bar in the Sidebar / in the Page (one checked), Settings… |
| Empty space in the tab area | Tabs on the Top rail / on the Sidebar, Unpinned pages: Shared / Per space, Settings… |
| A space's mark | That space: Rename Space…, Delete Space |
| A project's row | Move to Space ▸, then the project's existing items |

A tab's own menu (close, pin, copy to worktree) is unchanged. The placement items live on the empty area around the tabs, so a tab's menu stays short.

## Sidebar

From the top, with **Tabs** on the sidebar and **Address bar** in the sidebar:

```text
1. Traffic lights and sidebar toggle on the left; back, forward and reload on the right
2. Address bar
3. Pinned pages, as tiles
4. Unpinned pages, as rows, then +
5. Projects of the space shown, each with its branches, each branch with its tabs
6. Foot: Claude accounts, space marks, +
```

Rows 2, 3 and 4 leave the sidebar when their setting puts them elsewhere, and back, forward and reload leave row 1 with the address bar.

When the content is taller than the sidebar, rows 4 and 5 scroll together. The top row, the address bar, the pinned tiles and the foot stay fixed. The search box is removed, and project tiles are removed: projects are rows.

Every project and every branch collapses. A click on a project's row folds its branches away or brings them back. With **Tabs** on the sidebar, a branch's tabs are listed under its row, and a click on the branch's row folds them away or brings them back. Each project's and each branch's state is kept for the next launch. Projects and branches reorder by dragging their rows.

With **Tabs** on the sidebar, the pinned tiles and the unpinned rows sit above the projects.

## Top rail

From the left, with **Tabs** on the top rail:

```text
1. Traffic lights, sidebar toggle, then back, forward and reload when the address bar is in the sidebar
2. Pinned pages, in a pill (or the "Drag a page here to pin it" zone)
3. Unpinned pages, the Settings tab while open, then +
4. Hairline, the selected branch's tabs, then +
5. Empty glass that moves the window
6. Changes and Files, when set to Top rail
```

Items 1 to 4 are packed together from the left.

## Spaces

A space holds a set of projects. The sidebar lists only the space shown, and every space uses the one theme.

### Switching

| Action | Result |
|---|---|
| Click a space's mark | Shows that space |
| Two-finger swipe over the sidebar | Drags the space sideways, with a rubber band past the first and last space |
| ⌥⇧← / ⌥⇧→ | Shows the space to the left / right |
| ⌘⌥⇧← / ⌘⌥⇧→ | The same, from anywhere, the chat's prompt included |

A space shows again at the project it showed last. A space with no project still has its unpinned pages and the pinned ones.

### Keyboard shortcuts

| Keys | Action |
|---|---|
| ⌥⇧← / ⌥⇧→ | Previous / next space |
| ⌥← / ⌥→ | Previous / next project row, on into the space beside past the first or last |
| ⌥↑ / ⌥↓ | Previous / next tab, in the order the tabs are listed |
| ⌘⌥⇧← / ⌘⌥⇧→ | Previous / next space, from wherever the keyboard is |
| ⌘⌥← / ⌘⌥→ | Previous / next project, from wherever the keyboard is |

⌥ shortcuts work wherever the keyboard is not in text. In a terminal, a text field or a web page, ⌥← and ⌥→ keep their word-by-word meaning, so the ⌘⌥ forms exist. None of them wrap round: at either end the space gives a little and springs back. A space with no project is passed over by ⌥← and ⌥→. They do nothing while the Settings tab is shown.

⌥↑ and ⌥↓ follow **Tabs**: down the sidebar's rows, or left to right along the rail. A folded project's or a collapsed branch's tabs are passed over. They have no ⌘⌥ form, since a text field's ⌘⌥↑ and ⌘⌥↓ add a cursor.

### Motion

| Motion | Status |
|---|---|
| The space sliding out and the next sliding in, by click, keys or swipe | Kept |
| The swipe following two fingers, its rubber band and its spring | Kept |
| The capsule shrinking as the next mark grows | Kept |
| Dragged items sliding aside and settling, for tabs, branches, projects, pinned tiles and spaces | Kept |
| A branch's tabs sliding closed or open as it collapses or expands | Kept |
| Hover fades, and the activity dot fading in | Kept |
| The highlight sliding behind the selection on ⌥↑ / ⌥↓ and ⌥← / ⌥→ | Removed: the highlight moves at once |

With reduced motion every switch happens at once.

On a space switch the top rail slides with the sidebar, in the same direction, on the same spring, following a swipe's fingers:

| On the rail | On a space switch |
|---|---|
| Pinned pages | Stay put |
| Unpinned pages, Shared | Stay put |
| Unpinned pages, Per space | Slide out, the new space's slide in |
| The selected branch's tabs | Slide out, the new space's branch's slide in |
| Changes and Files, on the rail | Slide with the branch's tabs |

### Space marks

The foot lists the spaces as marks, centred. The space shown is a capsule; the others are dimmed dots that brighten under the pointer and show the space's name. All marks share the text's colour, since there is one theme.

A mark of a space not shown carries its activity: an indigo dot while something there waits for the person, the spinner while something works. The **+** on the right offers New Worktree…, New Project… and New Space….

### Reordering spaces

| Gesture | Result |
|---|---|
| Press a mark and drag sideways | The mark follows the pointer, the others slide aside |
| Let go | It settles into the slot; the order is kept for the next launch |
| Press and let go without moving | Shows that space, as a click |
| Drag well away from the row | It slides back, nothing changes |

The space shown stays shown while its mark moves. ⌥⇧← / ⌥⇧→ and the swipe follow the new order.

### Moving a project to another space

A project is in exactly one space. A space may hold one project or many, as the person arranges them.

Right-click a project's row, then **Move to Space ▸**: the spaces with the current one checked, a divider, then **New Space…**, which makes a space and moves the project into it.

### Space menu and theme

Right-clicking a space's mark, or empty space in the sidebar, offers **Rename Space…** and **Delete Space** for that space or the space shown. Delete asks first whether its projects leave Koyo or move to another space, and is greyed out with one space left.

There is no per-space theme. Switching spaces never changes the window's colours, and Settings → Appearance → Glass edits the one theme, light and dark. Themes stored on spaces are dropped from `settings.json`.

## Pinned and unpinned pages

Pinned pages belong to every space. Unpinned pages belong to no branch: per space, each space lists its own; shared, every space lists them all. Their rules for opening, closing, pinning, unpinning and reopening are unchanged by this page.

### Dropping a tab into the unpinned pages

A page's tab dragged off a branch's list can always land among the unpinned pages, on the rail and in the sidebar. With none yet, a dashed spot appears while the tab is in hand, "Drop to keep it here", and takes it. Let go there, the page becomes the last unpinned page and shows.

### New unpinned page

The **+** after the unpinned pages, on the rail and in the sidebar, opens the launcher for a browser page only:

- No kind pill on the left, and no Chat or other kinds on the right
- Tab does not change the kind
- The field alone, with the placeholder "Search or enter address"
- Enter opens an unpinned page in the space shown, and shows it

⌘T opens the same launcher wherever the person is in no branch: a space with no project, or a pinned or unpinned page shown. Use the **+** when a branch is selected and the new page should still be unpinned. Use ⌘T on a branch's tab for any kind of tab on that branch.

## Links in a chat

A link to a web page clicked in a chat opens in a Koyo browser tab, not in the system's default browser.

## Removed

| Removed | Instead |
|---|---|
| The search box in the sidebar | ⌘T launcher |
| Project tiles | Project rows |
| Per-space themes and Edit Theme Color… | One theme, edited in Settings → Appearance |
| The sliding highlight on keyboard moves | The highlight moves at once |

## Naming

Modules, actions, flags, tests and docs are named by what they show: sidebar, top rail, spaces, pinned pages, unpinned pages, marks, rows.

## Build order

Each step ends in a state that builds, passes `script/check` and can be looked at in the app:

1. Unpinned pages: the drop spot and the browser-only **+**, on the rail
2. Spaces in the sidebar: marks, switching, shortcuts, motion, the rail sliding with it
3. Move to Space ▸, space menu on the mark, one theme
4. Reordering spaces by dragging their marks
5. **Tabs** setting: pinned tiles, unpinned rows and branch tabs in the sidebar, with the drop spot and **+** there too
6. **Address bar** and **Changes and Files** settings
7. Right-click menus for each piece
8. Removals: the layout switch, the search box, project tiles, the keyboard highlight slide
9. Renaming modules, actions, flags, tests and docs by what they show

See [New tab](new-tab.md) for the launcher, [Browser](browser.md) for pinned and unpinned pages' storage, and [Tab switcher](tab-switcher.md) for ⌃⇥.
