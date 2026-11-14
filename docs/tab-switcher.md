# Koyo's tab switcher: ⌃⇥ on glass, with a picture of each tab

Written 2026-11-13 on branch `schulzfel/tab-switcher`. Drafted with Claude. A mix of macOS's
app switcher (⌘⇥) and Arc's tab switcher: the glass, size, shape and rounded corners of the
first, and inside it the second's cards, a picture of each tab with its icon and title under
it. It goes by Arc's rules: the tabs the person looked at last, wherever they are, five at
most. It takes the place of the list ⌃⇥ opened before. What the owner decided is marked
**[asked]**; what was decided while building it, and is the owner's to change, **[ours]**.
Keep this note in step with `koyo-rs/koyo/src/shell/switcher.rs` and `model/visits.rs`. A
check runs it in real windows off screen (section 7).

## 1. What it lists

- **The tabs looked at last, the latest first: five, the one shown among them.** One rule,
  with nothing else to remember: whatever the panes show counts as looked at, a worktree's
  tab, a pinned page or a space's loose page, in any project and any space. **[asked]**
- **With fewer than five looked at, the open tabs not looked at yet fill the list**, after
  those that were: as the window comes back, with only the tab shown looked at, ⌃⇥ still goes
  among the tabs that are open. **[asked]** The nearest come first, each lot in the order
  it's listed in: **[ours]**
  1. the tabs beside the one shown: its worktree's, or its space's loose pages;
  2. the rest of what the sidebar lists: the space's loose pages, the open project's
     worktrees;
  3. other projects' tabs, and other spaces' loose pages.
- A pinned page is listed once it's been looked at: until then it's a tile, with no page
  open (`docs/arc-sidebar.md` 11.7). **[ours]**
- The one shown is the first card, and the pick starts on the second: ⌃⇥ tapped goes back to
  the tab before, or, with none looked at before it, on to the nearest tab open. **[asked]**
- What follows from the rule: **[ours]**
  - A tab moved keeps its place: pinned, unpinned, made loose, or dragged to another worktree.
    The switcher knows a tab by its first pane, which goes with it.
  - A tab closed leaves the list. A pinned page closed stays a tile, but its page is gone
    until the tile is picked again, so it leaves the list too.
  - Moving through the cards doesn't reorder them. Only the tab let go on is looked at.
  - Another space shown with a pinned page staying in the panes isn't a new look at it: it
    never left.
  - A split is one card, as it's one tab.
- Nothing is kept for the next launch, neither the order nor the pictures: which tabs were
  looked at starts anew, with the tab the window shows. **[asked: the pictures; ours: the
  order]**
- With one tab open there's nothing to switch to, and ⌃⇥ does nothing.

## 2. Keys

As Arc and macOS have them. **[ours, but asked: Arc's behaviour]**

- **The panel never has the keyboard.** Every key goes on being heard in Koyo's window, where
  the person was, as ⌘⇥ leaves the keyboard with the app in front. So one window hears ⌃ go
  down, ⇥ and ⌃ let go, in order, and nothing is missed while a window takes the keys from
  another (section 6). **[ours]**
- ⌃⇥ opens the switcher with the second card picked, its panel unseen. ⌃ let go shows the tab
  picked, whether the panel shows yet or not: tapped, ⌃⇥ goes back to the tab before, and no
  panel ever shows.
- ⌃ held 200 ms: the panel shows. **[ours: 200 ms]** It's made as ⌃⇥ is pressed and only shown
  then, so it's there the moment the hold is up.
- ⇥ and ⌃⇧⇥ move the pick on and back, round the ends, and show the panel at once: past the
  tab before, the person has to see where the pick is.
- Escape leaves things as they were, and isn't passed on to the pane the person is in. So
  does a press in Koyo's window, and Koyo no longer being the app in front, as for the app's
  other floating windows (`popup.rs`).
- A click on a card shows its tab. The pointer moved over a card picks it; resting where the
  panel opens, it picks nothing.
- No arrows and no Enter: with ⌃ held they'd be ⌃←, ⌃→ and ⌃↩, of which macOS keeps the first
  two for its own Spaces. **[ours]**
- Showing a tab of another project or space shows that project in its space, as a tab picked
  in the sidebar does: a worktree's tab leaves a pinned or a loose page for it, a loose page
  of another space shows that space, and a pinned page shows over whatever space is shown.
  The tab picked takes the keyboard (`docs/arc-sidebar.md` 9).
- Known limit: GPUI tells a window of the modifier keys only as they change from what it last
  told that window. If ⌃ went down while another window had the keyboard and ⌃⇥ is pressed
  with it still held, Koyo's window is never told ⌃ was let go, and the panel stays until
  Escape, a click, or ⌃ pressed and let go again.

## 3. The panel

- **Its own window**, floating over Koyo's, as macOS's switcher floats over the apps: a
  borderless window exactly the panel's size, so macOS draws its shadow and its bright rim.
  Glass drawn inside Koyo's window (`native/liquid.rs`) can't: its shadow is cut off at the
  element's edge and its rim is left out. **[ours]**
- **Its glass is the app switcher's own:** `NSGlassEffectView` with the private subvariant
  `appSwitcher`, the name macOS gives that glass, set the way `liquid.rs` sets its private
  `adaptiveAppearance`, only where the view answers to it. Without it, the public Regular
  style: frosted, never Clear, which the chat's prompt uses and which shows too sharply what's
  behind. **[asked: frostier than the chat's prompt, and the App switcher glass, picked over
  the Dock's against ⌘⇥ (section 6); ours: how]** Before macOS 26, the blur the app's other
  floating windows have.
- **Size and shape**, from a photo of ⌘⇥ on macOS 27 (its icons taken as 128 pt; about ±10%):
  ⌘⇥ is about 213 pt high and, with five apps, about 860 pt wide, its corners about a quarter
  of its height round, the app picked in a lighter rounded square. The panel is a little
  wider for five tabs and a little lower, its corners concentric with the cards'. First
  built at 1,136 × 208 pt, which the owner found too big; then at 836 × 161 pt, which they
  had made 15% bigger. **[asked: the size; ours: each measure]**

  | | |
  |---|---|
  | Panel | 960 × 185 pt for five cards, 21 pt of glass round them, 42 pt corners |
  | Card | 174 × 143 pt, 21 pt corners, 12 pt apart |
  | Picture | 156 × 98 pt (about 16:10), 12 pt corners, 9 pt inside the card |
  | Title | under the picture: the tab's icon (a page's favicon), 16 pt, then its title, 14 pt |
  | Picked | a lighter fill behind the card, as macOS lights the app it picks |

- **It goes by its screen.** It's centred over Koyo's window, hanging over that window's
  sides if it's the narrower, and moved to lie on the screen the window is on, 24 pt in from
  the Dock, the menu bar and the screen's sides. On a screen too small for it at that size
  it shrinks as a whole, cards, text and corners alike, to fit. **[asked: that it goes by
  the screen; ours: how]**
- The panel's window is borderless: a titled one, as GPUI opens floating windows, has corners,
  a rim and a shadow of its own, which showed round the glass's as a second edge. It's made
  borderless once GPUI has built it, not while: `AppKit` tells GPUI of the change as it makes
  it, which GPUI can't hear mid-build. **[ours]**
- A tab of another project than the one shown carries that project's tile on its picture's
  corner, small, so two `zsh` terminals are told apart. **[ours]**

## 4. The pictures

- **Taken from what Koyo draws.** GPUI's Metal renderer (`koyo-rs/vendor/gpui-fast-apple`,
  `vendor/README.md`) copies the tab's part of a frame it has just drawn and shrinks it into a
  small picture on the GPU, 312 × 196 pixels, a card's picture twice over: nothing is copied
  to the CPU, and macOS asks no permission. It works for every kind of tab alike: terminal,
  chat, file, page, split, device, desktop. **[ours]**
- **How** (`shell/pictures.rs`). A tab's picture is a small surface with two buffers over it.
  One is painted over the tab in Koyo's window: it draws nothing, and has the renderer take the
  picture of what the frame shows under it. The other is painted in the tab's card. The first
  is painted from the window's root, where the tab's panes were laid out, not among them:
  they're drawn into a layer of their own, where the corners of the card they're on clip them,
  and the renderer takes pictures of what's painted in the frame itself.
- **Across the window's two layers.** Where the window has glass or another native view in it,
  it draws in two layers, one under the view and one over it, each a frame of its own: what's
  on glass, a chat's prompt, is in the upper. The lower layer's renderer takes the picture, and
  the upper's draws its own part of the frame over it (`metal_renderer::set_surface_capture`
  says how). macOS draws the glass itself, between the two, so no frame holds it: each glass
  element paints a marker over itself, which draws nothing (`native/surface.rs`), and the
  picture has a pane there, under what's on the glass, filled as the element is where there's
  no glass, its edge a pixel of the picture wide so it shows at that size. **[ours]**
- **What's in it:** all the tab is drawn in, across its whole width from its top, as high as
  a card's picture's shape makes that; or, in a window wider than that shape, its whole height
  across its middle. A split's panes are in it side by side. What the app draws on glass is
  in it, on a pane that stands for the glass: a new chat, which is nearly all prompt, shows
  its prompt.
- Each tab keeps one, by its first pane, in memory only **[asked]**: taken again while the
  window is drawn anew with the tab shown, at most four times a second (Koyo draws only what
  changes), and once more as the switcher opens. A tab left has a picture at most a quarter
  second old. A tab closed lets its picture go.
- The switcher draws them as a page's frame is drawn, a surface the renderer samples, in the
  card's rounded corners.
- A tab not shown since launch has no picture: its card shows its icon large on the theme's
  card colour.
- **What it costs.** The check (section 7) draws a frame of a 1600 × 1000 point window with a
  picture taken of it and without, waiting for the GPU each time: over six runs a picture added
  from nothing it could tell to 3.5 ms to a frame, about a millisecond in most, in a debug
  build on a MacBook Pro with an M4 Max, the middle of 40 frames each, with the machine busy. In
  the app nothing waits for it, and it's four frames a second at most. The check times a frame
  drawn as one; in a window of two layers the upper one's part is copied and drawn as well,
  which isn't timed. Reading a frame back
  takes the window's layer off its display-only mode (`framebufferOnly`), from the first
  picture on; what that costs a frame that takes none isn't measured: the check's frames are
  off screen. Koyo's window is see-through, which already keeps its frames from going straight
  to the display.
- Not taken another way: ScreenCaptureKit asks for Screen Recording, `CGWindowListCreateImage`
  is gone since macOS 15, GPUI can't draw a tab's views shrunk, and a page's own frames cover
  pages only.

## 5. Built in steps

1. The order (`model/visits.rs`): every tab shown stamped, pinned and loose pages too; the
   five latest across the workspace, then the open ones not looked at, the nearest first; a
   tab picked shown wherever it is (`switcher/show.rs`),
   as its row or its tile picked shows it. A worktree's tab in another space had that space's
   last loose page shown over it, by whatever selected it: mended where the shell follows a
   project into its space (`follow_space`, `docs/arc-sidebar.md` 9), for app control and
   links as for ⌃⇥. Unit tests, and a shell test across two spaces and a loose page.
   **Built.**
2. The glass, compared with ⌘⇥ (section 6). **The owner picked the App switcher's.**
3. The panel: its window (`popup.rs`'s switcher kind, `native/window_glass.rs`), the cards
   with their icons and badges (`switcher/card.rs`), the keys. It replaces the list. **Built;
   reworked once (section 6).**
4. The pictures (`shell/pictures.rs`, the renderer's `set_surface_capture`), and their cost
   measured. **Built.**

## 6. What was tried and dropped

- **The glass was picked by eye.** A debug build showed the looks side by side to set against
  ⌘⇥: the app switcher's `appSwitcher` subvariant, the Regular and Clear styles, and the
  Dock's, a video player's and Control Center's variants. The owner picked the app
  switcher's, and the comparison went.
- **A panel that took the keyboard**, as the list it replaced did, showed only some of the
  time. With the keys in the panel, Koyo's window never heard ⌃ let go; GPUI then didn't tell
  it of the next ⌃ pressed, taking nothing to have changed, and the switcher, which asked the
  window whether ⌃ was still held before showing the panel, was told it wasn't and showed the
  tab without it. A watch on the keyboard every 30 ms was added to catch what went unheard.
  All of that went with the panel that never has the keyboard.
- **Shrinking the panel to fit a narrow window**, when it's a window of its own and needn't:
  it goes by its screen instead (section 3).
- COSMIC's Alt+Tab (Pop!_OS) was read for what it does: its panel does take the keyboard, as
  the windows behind it are other apps', and a release unheard before the panel had it left
  the switcher stuck for a long time (cosmic-launcher #231), mended by asking for the
  modifiers as the panel gets the keyboard. It shows at once, with no hold, and a request for
  one of 150 to 250 ms stands open (#301).

## 7. The check

The switcher in real windows off screen: the shell on this checkout with a new chat's tab and
three files', ⌃⇥ as typed, and what macOS says of the panel's window: unseen until ⌃ has been
held, never with the keyboard, with no edge of its own and the glass behind what GPUI draws;
then ⌃⇥ again and ⌃ let go, heard in the shell's window, and the tab picked shown. Each tab's
frame is first presented as the app presents one, in the window's two layers, and its picture
read once the GPU has drawn it. The chat's, saved as `chat-picture.png`, is looked at for its
prompt: the pane that stands for the glass, and what's on it. The check says what a picture
adds to a frame (section 4). It saves what GPUI draws of the panel, the cards with their
tabs' pictures and without the glass, as `.superpowers/qa/switcher/switcher.png`.

```sh
Q=$PWD/.superpowers/qa/switcher && mkdir -p $Q/tmp &&
  KOYO_AGENT_PROVIDER=fake KOYO_DATA_DIR=$Q/data TMPDIR=$Q/tmp KOYO_CONFIG_DIR=$Q/config \
  KOYO_LOG_DIR=$Q/logs script/cargo run -p koyo --features visual-check -- \
  --visual-check-switcher
```
