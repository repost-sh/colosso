# Browser pages

A browser tab shows a native page: Chromium's own views, run by the engine inside Koyo's
process, moved into the tab's pane (`docs/native-pages.md`). The person types an address
(⌘L), clicks and types in the page, scrolls, finds (⌘F), zooms (⌘=, ⌘-, ⌘0), opens DevTools
(⌘⌥I, F12), switches to responsive mode (⌘⌥R), pins a page by dragging its tab onto the tiles
(Arc layout), and uses the site controls off the address field.

## Sub-features

- `page-open`: a page opens in a tab, loads and draws in its pane.
- `page-address`: ⌘L focuses the address field, and a typed address loads.
- `page-input`: a click puts the keyboard in the page, and typed keys reach a field once each.
- `page-agent`: `browser_navigate`, `browser_eval`, `browser_snapshot`, `browser_click` and
  `browser_type` act on a page over CDP, shown or out of sight.
- `page-shot`: `browser_screenshot` renders the page, and a probe shot shows it in the window.
- `page-zoom`: ⌘= twice, then ⌘0, changes and restores the page's CSS width.
- `page-find`: ⌘F opens the find bar.
- `page-devtools`: ⌘⌥I docks DevTools.
- `page-responsive`: ⌘⌥R and `browser_viewport` size the page, and a click lands at the scaled
  place.
- `page-menu`: a right-click's native menu opens. Escape closes it, and the window stays live.
- `page-pinned`: a pinned page's tile shows the page, and the page takes input.
- `page-extensions`: extensions' icons, popups and menus (needs the patched engine).

## How to get to it (user POV)

- ⌘T, then type an address. Or the new-tab button, then the page kind. A link that opens a tab.
- ⌘L, ⌘R, ⌘[ / ⌘], ⌘F, ⌘G, ⌘=, ⌘-, ⌘0, ⌘⌥I, F12, ⌘⌥R.
- In the Arc layout, the pinned tiles and the loose pages.
- App control: `tab_open {"kind":"browser","url":…}`, and the `browser_*` operations.

## Driving it with koyo-verify

Preconditions: the baseline. The fixture page `file://<checkout>/.superpowers/v/fixture/focus.html`
has an input `#field`, and it records every `keydown`'s `key` in `window.keys`. The window is
made key (SKILL.md, Keyboard focus) before any probe click.

- **Open.** `$B app tab_open '{"kind":"browser","url":"file://'$PWD'/.superpowers/v/fixture/focus.html","viewport":{"fill":true},"select":true}'`
  answers `{pane, tab, worktree}`. `$B app browser_list '{}'` lists the page with `open: true`
  and `onScreen: true`. `$B probe pages` prints a line for it, and `FITS` when its views fill the
  pane.
- **Agent acts.** Do these before any probe input, which makes `browser_*` answer
  `person_has_control` for a few seconds. `$B app browser_eval '{"pane":<pane>,"expression":"document.title"}'` answers
  `{"value":"verify focus"}`. `$B app browser_snapshot '{"pane":<pane>}'` lists the textbox with
  a ref.
- **Screenshots.** `$B pageshot page-engine <pane>` saves what the engine renders.
  `$B shot page-window` saves the window with the page in its pane. The two must agree.
- **Click into the page, then type.** Find the page's holder in `$B probe windows` (the line
  with `through true`) and compute the point (SKILL.md, Finding a point to click). Then run
  `$B probe 'hover <x> <y>'`, which should print `over RenderWidgetHostViewCocoa` or
  `over WebContentsViewCocoa`. Then
  `$B probe 'click <x> <y>' 'wait 200' state 'type abc' 'wait 300'`. `state` prints
  `first responder RenderWidgetHostViewCocoa`, and
  `$B probe 'eval <hostPane> JSON.stringify(window.keys)'` answers `["a","b","c"]`, one
  each. A key typed twice is the bug fixed in cdcebcf5a. Use the probe's `eval` here, because
  `browser_eval` right after probe input answers `person_has_control`. For the on-screen
  picture, call `desktop_screenshot {"window":<id>}` and run `$B keep <png> page-typed`.
- **Back to Koyo with ⌘L.** ⌘L opens the address panel, a `GPUIPanel` window of its own that
  takes key. Keep it for the end of a run (SKILL.md, rule 5).
  `$B probe 'key 37 l cmd' 'wait 300' windows` prints
  `taken by the window as a key equivalent`, and a `GPUIPanel … key true` line. The main
  window's `state` keeps naming the page's view, since that is the main window's own first
  responder. Close the panel with `$B probe escape`. On the agent desktop the main window
  doesn't get key back afterwards.
- **Zoom.** `$B probe 'key 24 = cmd' 'key 24 = cmd' 'wait 300'`. Then
  `browser_eval` of `innerWidth` is smaller than before. `key 29 0 cmd` restores it.
- **Native menu.** `$B probe 'rdown <x> <y>' 'wait 300' escape 'wait 300' 'hover <x> <y>' state`.
  The menu closes, and a later click still reaches the page. This is the "window left dead"
  bug fixed in cdcebcf5a. The menu itself shows on the owner's desktop, so keep it brief and
  don't shoot it.
- **Pinned page.** Use `$B prepare --layout Arc --pin "file://$PWD/.superpowers/v/fixture/focus.html"`.
  After launch, `$B shot pinned` shows its tile in the sidebar. Click the tile, then click in
  the page as above. `$B probe pages` lists its host pane, and
  `$B probe 'eval <hostPane> window.keys'` reads its keys, since app control can't reach it.
- **Extensions.** These need the patched engine and its switches (the `browser-extensions`
  checkout's `docs/extensions.md`). Pass them with
  `$B prepare --native-switches "load-extension=<dir>"`. On the stock engine, `Koyo.log` says
  extensions' controls are off and the site controls have no extensions row. A press inside an
  extension's popup doesn't land on the agent desktop (Gotchas).
- **Proof.** Both screenshots, `state` before and after each focus change, and `window.keys`
  read back.

## Gotchas

- A page shown in no window draws nothing. `onScreen: false` in `browser_list` means the page
  isn't drawn by a pane, and only an agent's command keeps it awake for 90 s.
- Without `viewport`, a page an agent opens is in responsive mode at 1440 × 900, scaled into the
  pane. Pass `"viewport":{"fill":true}` for a page that fills its pane, as a person's does.
- The probe's `eval` takes the engine's host pane id from `probe pages`, which isn't
  `tab_list`'s pane id. To pair them, compare `location.href`.
- `browser_*` acts go over CDP and don't move AppKit's first responder. They prove nothing about
  focus: use probe clicks and `state` for that.
- A press inside an extension's popup, or any Chromium window that isn't Koyo's, is a first
  click. On the agent desktop the test app is never active, so the popup never becomes key and
  the press is lost. That check needs the app in front on a real desktop, which only the owner
  can allow.
- Native menus (a right-click's, an extension icon's) show on the owner's desktop, wherever the
  app is. Open them only to check that they close.
- `$B shot` always shows a page's area blank, because pages are composited on the GPU and the
  in-process capture doesn't see them. Use `desktop_screenshot` (then `$B keep`) or
  `$B pageshot`.
- On 2026-11-16 the fixture page, which sets no colour scheme, drew a dark background on
  screen but a white one in `browser_screenshot`. Compare looks on the on-screen picture.
- `probe pages` also lists pinned pages that no window shows (`views in no window`), so a
  journey's page isn't always `page 1`.
