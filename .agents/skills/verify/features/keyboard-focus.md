# Keyboard focus

Exactly one place in Koyo's window has the keyboard: one of Koyo's own views (GPUI), the
address field, a chat's composer, or a native page (Chromium's view). A click moves the
keyboard where it lands. Selecting a tab leaves it at the window. Koyo's shortcuts (⌘L, ⌘T, ⌘W,
⌃⇥) work wherever the keyboard is, and a key the page doesn't handle comes back to Koyo once.
These are the sequences a focus refactor must keep working, each scripted end to end.

## Sub-features

- `focus-page-in`: a click in a page gives it the keyboard (`RenderWidgetHostViewCocoa`).
- `focus-page-out`: ⌘L, or a click on Koyo's own UI, takes it back (`GPUIView`).
- `focus-composer`: a press in a chat's composer gives it the keyboard, and typed text lands
  there.
- `focus-pinned-page-composer`: pinned tab → click in its page → click in a chat's composer →
  type. The text is in the composer and none of it in the page.
- `focus-tab-switch`: selecting another tab releases the page's views, and the keyboard isn't
  left in a hidden page.
- `focus-shortcuts-in-page`: ⌘L, ⌘T and ⌃⇥ act while the page has the keyboard, and their keys
  never reach the page.
- `focus-once`: each key typed in a page arrives once.

## How to get to it (user POV)

- Clicks on a page, on the address field, on the composer, on sidebar rows and on pinned tiles.
- ⌘L, ⌘T, ⌘W, ⌃⇥, ⌥↑/⌥↓, Escape.

## Driving it with koyo-verify

Preconditions:

- `$B prepare --tabs Sidebar --pin "file://$PWD/.superpowers/v/fixture/focus.html"`, launched and
  confirmed.
- A new chat tab in the fixture worktree, with nothing sent yet:
  `$B app tab_open '{"kind":"chat","select":false}'`.
- A fresh launch, with `$B probe state` printing `key true, main true` (SKILL.md, Keyboard
  focus). Once the window loses key on the agent desktop, nothing brings it back:
  `desktop_focus` refuses Koyo's window. Without `key true`, stop and report, because every
  click below would be a first click.
- Points measured on this window from `$B shot focus-start` and `$B probe windows`, and each one
  checked with `hover`. Call them `TILE`, `PAGE`, `CHATROW` and `COMPOSER`.

The journeys, one transcript each (`$B run focus-<id>`):

- **`focus-page-in`.** `$B probe 'click TILE' 'wait 500' 'hover PAGE' 'click PAGE' 'wait 200' state`.
  `hover` prints `over RenderWidgetHostViewCocoa` or `WebContentsViewCocoa`, and `state` prints
  `first responder RenderWidgetHostViewCocoa`.
- **`focus-once`.** `$B probe 'type xyz' 'wait 300'`, then `$B probe pages` for the pinned
  page's host pane, then `$B probe 'eval <hostPane> JSON.stringify(window.keys)'`. It answers
  `["x","y","z"]` with nothing doubled.
- **`focus-shortcuts-in-page`.** Run this last, since it opens the address panel (SKILL.md,
  rule 5). Through the window server: `desktop_key {"keys":"cmd+l","window":<id>}`, then
  `$B probe windows`, which shows a `GPUIPanel` with `key true`. Read the page's keys with
  `probe eval`. In the 2026-11-16 run, a real ⌘L reached the page as `Meta`, `l` *and* opened
  the panel. Whether the page should see it is the refactor's call, so record what you see.
  Through the probe (`key 37 l cmd`), the window takes it as a key equivalent and the page
  sees nothing. Close the panel with `$B probe escape`.
- **`focus-pinned-page-composer`** (the full sequence):
  1. `$B probe 'click TILE' 'wait 500' 'click PAGE' 'wait 200' state` →
     `RenderWidgetHostViewCocoa`.
  2. Show the chat: `$B probe 'click CHATROW' 'wait 500' state`. The keyboard should leave the
     page. Record what `state` prints, since that is what the refactor decides.
  3. `$B shot focus-chat`, measure `COMPOSER`, then
     `$B probe 'hover COMPOSER' 'click COMPOSER' 'wait 200' state 'type into the composer' 'wait 300'`.
     `state` prints `GPUIView`.
  4. Check both ends: `$B shot focus-typed` shows `into the composer` in the composer.
     `$B probe 'eval <hostPane> JSON.stringify(window.keys)'` shows no key from step 3.
     Only when `state` prints `main true`, send with `$B probe 'key 36 "\r"' 'wait 1500'`, and
     `$B app chat_read '{"chat":"<id>"}'` has `You said: into the composer` (see
     [chats.md](./chats.md), Gotchas, for why).
- **`focus-tab-switch`.** With the page holding the keyboard, run
  `$B probe 'key 126 "" alt' 'wait 300' state 'type q'`. The `q` doesn't land in the page
  (`window.keys` unchanged), and `state` doesn't name a page's view.
- **`focus-page-out` by a click.** Click on Koyo's own UI (a sidebar row, checked with `hover` to
  print `GPUIView`), then run `state`. It prints `GPUIView`.
- **Proof.** For each step, the `state` lines in the transcript, a shot before and after, and
  `window.keys` and `chat_read` as the second view.

## Gotchas

- Everything here depends on the window being key, which it is only from launch until
  something takes key away. A refactor that changes how GPUI reacts to `windowDidBecomeKey`
  may change that too. Check `key true` before each step.
- Probe clicks are handed to the window with `sendEvent:`. They exercise AppKit's and GPUI's
  handling, not the window server's routing. To prove which window a real click reaches, use
  `desktop_click` with desktop points (`desktop_windows` gives the window's frame) and note it
  with `$B note`.
- `type` uses key code 0. Shortcuts need `key <code> <chars> <mods>`, which tries the window's
  and the menu's key equivalents first, as AppKit does.
- Pinned pages aren't reachable through app control. Use the probe's `pages` and `eval` for
  them.
- Not proven by a run yet: the pinned-tile click and the full `focus-pinned-page-composer`
  sequence. The 2026-11-16 run lost key to the address panel before it reached them. The
  first run that does them records its points and outcome here. Proven on 2026-11-16 (see
  browser-pages.md): a click in a page gives it the keyboard, and typed keys arrive once.
