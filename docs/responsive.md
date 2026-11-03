# Koyo responsive mode: browser pages and terminals at a chosen size

Specification, written 2026-11-02 on `main`. Author of the vision: Felipe Schulz. Drafted
with Claude. Source of truth for responsive mode; update it when a decision changes.

Chrome's device toolbar, for both kinds of pane that run something: a browser page shown
at a chosen width × height (or as a phone or tablet), and a terminal at a chosen
cols × rows, each dragged by its edges in the pane, typed in a bar, or set by an agent
through the MCP (`docs/app-control.md`, `docs/terminals.md` 8).

Status marks: `[decided]`, `[proposed]` (designed here, confirmed by implementation),
`[not built]`.

---

## 1. Decisions the owner made (2026-11-02) [decided]

- **Browser: size and device presets.** Free width × height, and presets (phones,
  tablets) that also set the device pixel ratio, a mobile user agent, touch, and
  `<meta viewport>` handling, as Chrome's device mode does. Rejected: size only (sites that
  look at the user agent or at `(pointer: coarse)` wouldn't switch).
- **Bigger than the pane: scaled down to fit.** A page renders at its true size and is
  shown smaller (the bar says "83%"); a terminal draws its cells in a smaller font.
  Pointer positions map through the scale. Rejected: capping at the pane's size (a phone
  is taller than most panes), scrolling inside the pane (the whole page isn't in sight).
- **Not remembered across restarts.** Every pane opens filling its space after the app
  starts. A browser pane's mode lives in the view; a terminal's pin is owned by the
  connection that set it and goes with it (section 3.2).

## 2. Browser pages

### 2.1 How it works [decided]

The page's viewport today is the size of the page element (`browser/page.rs`,
`PageSurface::prepaint`), sent as `AppRequest::Resize`. In responsive mode the view keeps
a `Responsive` state and the page element:

1. Computes the emulated viewport: `width × height` logical px at `scale` (the device's
   pixel ratio for a preset; the window's scale factor for free sizes).
2. Sends `AppRequest::Resize { viewport, dragging }` with *that* viewport, not the
   element's bounds (`dragging` is live sizing's, section 8: whether a grip is held).
   The host renders at `width·scale × height·scale` device pixels, so the page's
   `innerWidth`, `devicePixelRatio` and media queries are the device's. Responsive mode
   needs nothing more of the host or `browser-ipc`.
3. Lays the page out centred horizontally and at the top of the area under the device
   bar, with a 16 px gutter, at `fit = min(1, room_w / width, room_h / height)` (or the
   person's chosen zoom). The native layer is placed at the scaled bounds; Core Animation
   scales the surface into it (contents gravity resize).
4. Maps every pointer position (mouse, wheel, pinch, context menu, `DevTools` inspect
   point, the input method's caret rectangle) as `(window − page origin) / fit`, in one
   function on `BrowserView`, so nothing reads `origin` raw.

Device behaviour goes over the page's `DevTools` protocol session (`browser/agent.rs`
`cdp`, sent by the app itself, not under an agent's lease):

| Preset sets | Command |
| --- | --- |
| mobile (meta viewport, overlay scrollbars, `screen` size) | `Emulation.setDeviceMetricsOverride { width: 0, height: 0, deviceScaleFactor: 0, mobile: true }` (0 keeps the real value; only `mobile` is overridden) |
| touch | `Emulation.setTouchEmulationEnabled { enabled: true, maxTouchPoints: 5 }` and `Emulation.setEmitTouchEventsForMouse { enabled: true, configuration: "mobile" }` |
| user agent | `Emulation.setUserAgentOverride { userAgent, platform, userAgentMetadata }` |

Leaving a preset (or responsive mode) clears them: `Emulation.clearDeviceMetricsOverride`,
touch and mouse-to-touch `enabled: false`, `setUserAgentOverride { userAgent: "" }`. They
are applied again whenever the host (re)opens the page (host restart, renderer crash,
reopen), since the session is new. The page is not reloaded on a change (as Chrome); a site
that reads the user agent on the server shows its mobile pages after a reload, and the bar
says so with a Reload link the first time the agent changes on a loaded page.

The person changing the size or device takes nothing from an agent holding the page
(as a person's terminal resize, `docs/terminals.md` 5.6); an agent's change needs the
page's lease (section 4.1).

### 2.2 Presets [decided]

`Responsive` (free size, desktop behaviour, the window's scale) and:

| Name (`device`) | Size | Scale | Agent |
| --- | --- | --- | --- |
| iPhone SE (`iphone-se`) | 375 × 667 | 2 | iOS Safari |
| iPhone 15 (`iphone-15`) | 393 × 852 | 3 | iOS Safari |
| iPhone 15 Pro Max (`iphone-15-pro-max`) | 430 × 932 | 3 | iOS Safari |
| Pixel 8 (`pixel-8`) | 412 × 915 | 2.625 | Android Chrome |
| Galaxy S24 (`galaxy-s24`) | 360 × 780 | 3 | Android Chrome |
| iPad mini (`ipad-mini`) | 744 × 1133 | 2 | iPadOS Safari |
| iPad Pro 11″ (`ipad-pro-11`) | 834 × 1194 | 2 | iPadOS Safari |

All presets are mobile and touch. Sizes are portrait; rotating swaps width and height.
User agents: iOS `Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)
AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1` (iPad:
`iPad; CPU OS 18_0`); Android `Mozilla/5.0 (Linux; Android 14; <model>) AppleWebKit/537.36
(KHTML, like Gecko) Chrome/<engine major>.0.0.0 Mobile Safari/537.36`, the engine's own
major version (`Browser.getVersion`), with matching `userAgentMetadata` (`mobile: true`,
platform `Android`/`iOS`). The table lives in one module (`browser/responsive/devices.rs`).

Limits: width and height from 50 to `floor(16384 / scale)` logical px (a surface's side
stays within 16384 device px); an agent's value outside is `invalid_input`, a drag stops
at the limit.

### 2.3 UI [decided]

- **Toggle**: a toolbar button (a phone-and-tablet icon, tooltip "Responsive  ⌥⌘R") beside
  the `DevTools` button, ⌥⌘R in the page's or toolbar's key context, and the page's
  context menu item "Responsive mode". ⇧⌘M (Chrome's) is the sidebar's here. Inspector
  panes have no toggle.
- **Device bar**, between the toolbar and the page while the mode is on, 32 px, the
  toolbar's look: a device menu (`Responsive ▾` / `iPhone 15 ▾`), width and height number
  fields (`393 × 852`, editable only in Responsive; Enter applies, Escape restores), a
  zoom menu (`Fit (83%)`, 50 %, 75 %, 100 %), a rotate button (presets only), and a close
  button (✕) that ends the mode.
- **The page area**: `p.sunken` round the page, centred in it, with a 1 px `p.hairline`
  outline round the page.
- **Handles** (Responsive only): a grip on the right edge, one on the bottom edge and one
  at the bottom-right corner, 8 px, outside the page. Dragging the right grip changes the
  width by twice the pointer's move (the page stays centred, as Chrome); the bottom grip
  the height; the corner both. The size shows live in the bar's fields and in a small
  `393 × 852` label under the pointer while dragging. Double-clicking a grip fits that side
  to the pane.
- **Agent's mark**: when an agent set the viewport, the device bar starts with
  "Set by claude-code" (muted) until the person changes it.

## 3. Terminals

### 3.1 How it works [decided]

A terminal has one size for every viewer (`docs/terminals.md` 5.4): the daemon takes the
smallest of the control-mode clients' sizes (`termd/src/session/stream.rs`
`settle_size`). Agents never attach; they send. So responsive mode is a size *pinned*
in the daemon, set by the app for the person or by a relay for an agent:

- `term-control`: `RequestBody::Pin { handle, cells: Option<(u16, u16)> }` (cols and rows;
  the cell px are the clients'), answered `ResponseBody::Pinned { size, pinned }`.
  `TerminalRecord` gains `pinned: Option<Pinned>`, `Pinned { cols, rows, by: Actor }`.
  `PROTOCOL` 1 → 2, so an app of this build replaces an older daemon through the existing
  "other build" notice.
- `termd`: while a pin is set, `settle_size` uses it instead of the clients' sizes; it
  resizes the pty and the engine and broadcasts `Event{size}` and the record as today.
  `Pin { cells: None }` unpins and negotiates again. A pin belongs to the connection that set
  it: when that client goes (`ClientGone`, the app quitting, the relay ending), the pin goes
  and the size is negotiated again. That is "not remembered across restarts" for both the
  person and agents. A later pin replaces an earlier one whoever set it.
- Lease: an agent's pin is checked and logged as a resize (`agent_may_act`, "resized to
  120x40"); a person's pin takes nothing from an agent and isn't logged, as a person's
  resize.
- Limits: cols and rows 2 to 1000, as agent-terminal (`cli/src/protocol/session.rs`), so a
  size that works there works here; outside is `invalid_input`. Pinning a terminal whose
  program exited fails `exited` (`ErrorCode::Exited`), as agent-terminal's exit 5.
- `terminal_open { size }` pins that size for the opening relay (the agent asked for it),
  instead of the size being negotiated away by the first view that attaches.

### 3.2 The view [decided]

`crates/term` `TerminalElement::prepaint` today derives `cols × rows` from its bounds and
the view sends them (`view/link.rs` `grid_changed`). With a pin on the record:

- The element lays out the pinned grid, not its bounds', in the bounds the pane gives it
  (the area under the device bar less the 16 px gutters, as a page's room, 3.3). Its font
  is the pane's choice (`Scale`): **Fit**, the largest that holds the grid in those bounds,
  at most the person's own size; a **percent** of the person's size (50, 75, 100 %), in
  0.25 px steps and never under 4 px, whatever the bounds; or **held** at the size last
  fitted while a grip is in hand. Below the person's size the glyphs are prepared at the
  smaller size (a `Look` with it), so cells are crisp, not a scaled bitmap. Fit follows
  agent-terminal's dashboard (`dashboard/src/components/terminal-view.tsx` `fitFont`): in
  0.25 px steps, never under 4 px, never over the person's own size, and with its
  hysteresis: a size that overflowed once for this room and grid isn't tried again until
  the room or the grid changes, so rounding in the font's metrics can't make it flicker
  between two sizes.
- The grid is placed as a page is (2.1, 3): centred across its bounds, at their top; one
  wider than them keeps its left edge at the gutter and runs past their right side, where
  the pane clips it, as a page wider than its area at a fixed zoom does. Only its cells
  and their background are the element's to paint; what is round it (the sunken area, the
  outline, the grips) is the app's. `Grid::cell_at` and `Grid::device` stay exact because
  the grid's metrics are the scaled ones; the pointer reaches the terminal over its grid.
- The view still sends its fill size as today (it's what the size returns to when
  unpinned), and doesn't send a size derived from the pinned layout.

### 3.3 UI [decided]

A pinned terminal is shown as a page in responsive mode is, with the same chrome:
`crates/koyo/src/responsive/` draws the device bar, the sunken area, the outline, the
grips and the drag label for both, so they are the same pixels. What differs is what each
pane gives it: units (pixels, cells), presets, limits, and how content is fitted.

- **Toggle**: the browser toolbar's button, on the terminal bar (`terminals/header.rs`): a
  phone-and-tablet icon, tooltip "Responsive" with the key bound in the terminal's key
  context (⌥⌘R), shown selected while the terminal is pinned; and ⌥⌘R in the terminal's key
  context (a terminal has no context menu yet).
  Turning the mode on pins the cells the room under the bar holds at the person's font, so
  the terminal shows at 100 %.
- **Device bar** (2.3's, 32 px): the preset menu (`Responsive ▾` for a free size; `80 × 24`,
  `120 × 40`, `200 × 60`, the sizes agent-terminal's TUI QA tries), columns and rows
  fields (editable for Responsive; Enter applies, Escape restores), the zoom menu
  (`Fit (83%)`, 50 %, 75 %, 100 %, a percent being the font's size as a share of the
  person's), rotate for a preset (it swaps columns and rows, the preset's name kept, as a
  device's), and ✕, which unpins.
- **The area**: `p.sunken` round the grid, the grid centred in it (3.2), a 1 px
  `p.hairline` outline round the grid.
- **Handles** (Responsive only): 2.3's grips, moving in whole cells (the right grip by
  twice the pointer's move, the grid staying centred); the size live in the fields and in
  the `120 × 40` label under the pointer while dragging; the pin is sent as the grip
  moves, one at a time (the latest once the daemon has answered the last), so the program
  follows every frame the daemon keeps up with, as a terminal window's does. The cells
  sent last aren't sent again until another pin replaces them; a pin that fails is sent
  again when it's asked for again, and the size shown goes back to the daemon's. Under Fit, a
  grip in hand holds the font, so the edge stays under the pointer; it fits again on
  release. Double-clicking a grip fits that side to the room at the person's font.
- **Pinned by an agent**: the bar starts with "Set by claude-code:7f3a" (muted), its size is
  a free one (Responsive), and the ✕ unpins (taking nothing from the agent). Every app view
  of the terminal shows the same pin.

## 4. MCP [decided]

### 4.1 `browser_viewport` (app catalog, `ops/browser`)

```
browser_viewport [browser] I { pane?, device?: "responsive"|"iphone-se"|…  (2.2's names),
                   width?, height?, scale?, mobile?, touch?, userAgent?,
                   orientation?: "portrait"|"landscape", zoom?: "fit"|50|75|100,
                   fill?: true, screenshot?: bool, requestId? }
  → { viewport: Viewport, settled: bool }
Viewport = { mode: "fill"|"responsive"|"device", device?, width, height, scale, mobile,
             touch, userAgent?, orientation, zoom, shownPercent, setBy?: "person"|<agent> }
```

- `fill: true` ends the mode (other fields refused with it). A `device` sets its row of
  2.2; `orientation` rotates it; `width`/`height` alone mean Responsive at that size (the
  other side kept, or the pane's); `scale`, `mobile`, `touch`, `userAgent` customise
  Responsive (refused with a preset, `invalid_input`).
- Under the page's lease, as `browser_navigate`: claims a free page, `person_has_control`
  when the person holds it. Shown in the window at once, with the agent's mark.
- Like `terminal_resize`, it answers once the page has re-laid out at the new size: the
  page reports the new `innerWidth`/`innerHeight` and a frame of that size has shown
  (5 s at most, then answered with `settled: false`). `screenshot: true` adds a screenshot
  of the result, as `browser_screenshot`'s image block, so one call is the change and its
  evidence.
- `browser_list`'s pages gain `viewport` (`mode: "fill"` with the element's size when off),
  so reading needs no tool of its own. `browser_screenshot` captures the emulated viewport.

### 4.2 `terminal_resize` (terminal catalog, `term-control/src/catalog`)

`docs/terminals.md` 8.5's line, built, with unpinning:

```
terminal_resize { terminal?: handle, cols, rows } | { terminal?, fill: true },
                then?: Then (terminal_send's: wait?, read?), requestId?
  → { size: { cols, rows }, pinned: null | { cols, rows, by }, wait?: WaitResult,
      read?: ReadResult }
```

As agent-terminal's `resize` (`cli/src/native/daemon/tabs/mod.rs` `settle`), one call
resizes and returns the settled screen: without `then`, it waits for the screen to hold
still 300 ms (`for: "stable", stableMs: 300`, 25 s timeout) and reads the screen; a
program redraws after `SIGWINCH`, so the screen read right after the resize would be the
old layout. A timeout answers `met: false` with the screen as it is, not an error.
`then: {}` skips both.

Scope `terminal.write`, under the terminal's lease (claims a free terminal, as
`terminal_send`). `terminal_list` and the terminal resources show `pinned`. The pin lasts
while the relay's connection does (3.1).

### 4.3 Guides

`koyo_guide` and `terminal_guide` gain a paragraph: to check a layout, set the size,
screenshot, then `fill: true`; the person sees what you do, and their own change wins.

## 5. What came from agent-terminal [decided]

agent-terminal (`/Users/schulzf/github.com/repost-sh/agent-terminal`, ours) already sizes
terminals for agents: `open --size`, `resize COLSxROWS` with `--wait`/`--wait-stable`
returning the settled screen, sizes 2 to 1000, exited programs refused, and a dashboard
that never resizes but fits its font to the grid. Taken: the limits, the
resize-then-settle answer (4.2, and its browser analogue 4.1), the font fitting with its
steps, floor and hysteresis (3.2), the QA sizes as presets (3.3), and `open`'s size
holding (3.1). Improved on it: the person can size the terminal too (handles and bar), the
size is shared by every viewer through the daemon's pin, and it lasts only as long as the
connection that set it.

## 6. Testing [decided]

- `browser`: unit tests for the fit and pointer mapping (scaled and not), the device
  table (limits, rotation), the CDP commands sent on set, clear, and reopen (a recording
  `Browsers` double as in `host_tests`), the person's change not taking the lease.
- `termd`: pin resizes the pty and engine and wins over clients; unpin renegotiates; the
  pin goes with its client; an agent's pin needs the lease and is logged; limits; an
  exited terminal refused; `terminal_open`'s size pinned.
- `term-control`: `terminal_resize` waits for a stable screen and reads it by default (a
  fixture that redraws on `SIGWINCH`, as agent-terminal's
  `resize_reflows_the_app_through_sigwinch`), `then: {}` skips it.
- `term`: the pinned layout's fit (steps, floor, cap at the person's size, hysteresis)
  and cell mapping.
- `app-control`, `term-control`: catalog (names, schemas, `requestId`), argument
  validation.
- MCP e2e: `koyo mcp` calls `browser_viewport` and `terminal_resize` against the built
  app; real-surface QA drives the UI (drag, presets, rotate, zoom, ✕) and the tools, with
  screenshots.
- `script/check` green.

## 7. As implemented (2026-11-02)

Where the build differs from the sections above, or settles what they left open:

- Crates: `koyo/src/responsive/` (the chrome both panes share: `mod.rs` placement,
  outline and zoom, `bar.rs` the device bar and its fields, `grips.rs` the grips, their
  drag as the `Resizable` trait, and the label), `browser/responsive/` (`mod.rs` fit and
  point mapping, `devices.rs`, `pane.rs`, `grips.rs` and `bar.rs` the page's side of the
  shared ones), `control/exec/browser/viewport.rs`,
  `app-control/src/ops/browser/viewport.rs`; `termd/src/session/pin.rs`,
  `term-control/src/exec/resize.rs`, `term/src/fit.rs`, `term/src/element/pinned.rs`,
  `term/src/view/responsive.rs`, `koyo/src/terminals/responsive.rs` (a view round each
  terminal view that draws the shared chrome while it's pinned, `responsive/frame.rs` the
  outline and grips over the grid as last laid out).
- Browser layer: the native layer keeps top-left gravity with its contents scale set to the
  shown frame's scale ÷ fit, rather than resize gravity, so a frame of the old size isn't
  stretched while the new one comes.
- The Android user agent's Chromium major comes from the host's hello, not
  `Browser.getVersion`. iOS presets send platform `iPhone`/`iPad` and `userAgentMetadata`
  with platform iOS and no brands; Android sends `Linux armv81`.
- `browser_viewport` settles when a frame within 1 device px of the size has shown (a page
  out of sight: its `innerWidth`/`innerHeight` alone) and the page's inner size is the size
  ÷ its zoom; a mobile page with no `<meta viewport>` (laid out about 980 wide) settles on
  the device's aspect ratio. With `screenshot: true` the answer carries `mimeType` and
  `data` and the image goes as an MCP image block; a failed capture fails the call after
  the viewport changed.
- `browser_viewport` details: `scale` 0.5 to 5; an empty call is `invalid_input`; `zoom` or
  `orientation` alone keep the current preset; a width, height or customisation on a
  preset switches to Responsive with desktop behaviour; an agent's viewport reaches a page
  out of sight at once. A free size follows the window's scale and is kept within 16384
  device px at whatever scale the window has (`Viewport::max_side` in `browser-ipc`, the
  limit the host's live sizing keeps to as well). A pane knows its window's scale from the
  moment it's made, before it's drawn.
- Browser UI details: turning the mode on starts at the area under the device bar, so the
  page shows at 100 %; a drag keeps the fit it started with, so the edge stays under the
  pointer; a page wider than its area keeps its left edge at the gutter; ⌥⌘R also works in
  the find bar; the context menu row is checked while on; the reload hint reads "Some
  sites switch only once the page loads again. Reload". Grip drags end when the button is
  found up, as a selection drag does. A size dragged or typed changes the mode only; the
  page is told it as the pane is next drawn, so a grip sends one size a frame however fast
  the pointer moves, and the window waits for the page's frame at it (section 8).
- Terminal limits answer `invalid_argument` (the terminal catalog's code), not
  `invalid_input`. A pin carries cells only; the cell's pixel size comes from the
  clients' smallest size, or the record's. An agent's unpin is logged "let it fill the views
  again". Every session hears that a client went, whatever its kind, so an app's pin goes
  even on a terminal the app never attached.
- `terminal_open { size }` pins when the open claims the terminal (the default); with
  `claim: false` the size is only the starting one. A refused pin leaves the open standing
  and answers `pinned: null` with `pinRefused: { code, message }`.
- An HTTP MCP session's daemon connection closes when the session ends, so its pins go with
  it.
- Terminal UI: the browser's chrome, pixel for pixel (3.3): the bars and grips of a page
  and a terminal side by side differ only in their numbers and preset names. While a grip
  is dragged the bar and label show the size asked for, and the grid the daemon's pin; a
  size typed or picked shows at once and the daemon's pin replaces it when it comes. A
  preset's size is checked against the pin, so a size from elsewhere (an agent, a drag)
  shows as Responsive. ⌥⌘R needs the terminal focused. A size field shows its number from
  its start again after Enter, Escape or leaving it (a 56 px field scrolls while a number
  is typed into it).
- `terminal_guide` gains a "Sizes" section; it says to read the screen, as
  `terminal_screenshot` isn't built.

## 8. Live sizing of browser pages (2026-11-02)

A page follows a drag of its pane's edge (a split's divider, a responsive grip, the window)
with a new layout every frame, as a Chrome window does. This is the browser host's job
(`crates/browser-host/src/host/{sizing.rs,live.rs,live/drive.rs}`); the app keeps sending
one size per frame, and says whether a drag is under way (`AppRequest::Resize { dragging }`).

- Why: resizing Chromium's off-screen view restarts its capture, which first paints the
  new layout scaled into the old size between bars. The host used to drop every paint not
  at the latest size, so a drag showed about one frame in two seconds.
- How: while a drag goes on, the view keeps a size with room to spare (the drag's size
  plus 512 px, within 16384 device px) and the page is laid out at each size by
  `Emulation.setDeviceMetricsOverride`, in the page's own session under ids of the host's.
  This is the same technique as DevTools' device mode, and costs one frame. The app is
  shown the top-left of each frame, the part the page fills. A drag past the view first
  pins the layout at what the view holds, then grows the view under it. When the app says
  the drag is over, the view is given its last size for real (once grown, if it's growing)
  and, once the page has painted at it (or 500 ms passed), the override goes.
- Which drags the app tells of (`browser/page.rs`, prepaint; `view/sized.rs` `Dragging`):
  a drag in hand in GPUI (a split's divider, the sidebar's and right panel's edges, docked
  DevTools' divider) and a responsive grip held, which the app sees end: as one is let go
  the app sends its size once more with `dragging: false`, even when it didn't change. And
  the window's edge, seen as the window's size changing since the page was last placed:
  GPUI has no live-resize state to say when that ends, so the host ends it 150 ms after its
  last size (`SETTLE`), as it ended every drag before. That timer stays on every drag, as a
  fallback.
- A size of its own (`dragging: false` with no drag under way: an agent's
  `browser_viewport`, a preset, the mode turned on, the sidebar shown) is laid out as on a
  page not loaded yet, below: one resize of the view, no override and no 150 ms settle. A
  size Chromium already has is not told again.
- The app's own device metrics (responsive mode) are kept: the host rewrites the app's
  `set`/`clearDeviceMetricsOverride` to the drag's size while one is under way, merges
  them into its own overrides, and puts them back when the drag ends.
- Pages that aren't loaded yet and DevTools are resized as before. There, one size is laid
  out at a time and only letterboxed paints (`content_rect` or `source_size` differing
  from the visible rect) are dropped. The paint at the size laid out lets the next go
  (`Queued::SizeNext`): it is the latest asked for when the host's queue gets to it, so a
  size asked for meanwhile replaces it and an older one is never laid out after it.
- The view grows once per drag, at its start and whichever way it goes, to the largest
  display (`largest_screen`), so a fast drag never has it grow again partway.
- The pane's edge and the page reach the screen together, as in a Chrome window
  (`crates/koyo/src/browser/view/sized.rs`): when a page's pane changes size, the page
  element sends the size and waits for the page's frame at it before the window is drawn,
  handling what the host says meanwhile (`browser/host/inbox.rs`). The pages resized in one
  frame wait until the same deadline, 250 ms after the first began (`frame_deadline`), so a
  frame waits 250 ms at most however many pages changed size. A heavy
  page dragged fast then goes at its own pace (openai.com: mostly under 50 ms, at most about
  120 ms). A page that misses five waits in a row isn't waited for until a frame catches up.
- And GPUI presents its frames in Core Animation's transaction while any page has a native
  view (`vendor/gpui-fast-apple`, `browser/frames.rs`), so what it draws around the page
  reaches the screen in the same frame as the page's layer, not one after.
- Measured on screen (QA app, a split dragged 600 pt in 0.1 s each way, the area under the
  page painted magenta for the test): 0 frames of ~720 showed any of it on openai.com.
- Measured by the engine smoke test `live_resize::a_page_dragged_wider_lays_out_again_every_frame`
  (120 sizes at 60 Hz): 118 frames and 118 distinct layouts in a 2 s drag, mostly 0 to
  4 px behind the drag, and the page edge to edge at the last size once it stops.
- A daemon of another build that doesn't stop on `Shutdown` or SIGTERM within 5 s is now
  killed with SIGKILL (`term-control/src/client.rs`, `stop_daemon`), so "Restart
  terminals" can't hang on it (test `term-control/tests/stop.rs`).
