# Native pages

Written 2026-11-13 on branch `schulzfel/browser-extensions`. Drafted with Claude. Koyo's
browser pages are native, windowed pages, as Arc's and Chrome's are: Chromium presents its
own frames in Koyo's window and `AppKit` hands it its input. This is how that works, what
was tried, and what is checked. Keep it in step with `koyo-rs/koyo/src/browser/native/` and
`koyo-rs/koyo-browser-host/src/native.rs`.

Status marks: `[proven]` (run on this machine, evidence named), `[read]` (taken from source,
not run), `[proposed]`, `[not built]`, `[owner]` (the owner's to decide).

**Where it stands (2026-11-15): pages are native and nothing else.** Until then a page was
drawn off screen by a host process, copied into shared surfaces and composited by GPUI, with
the person's pointer and keys sent to it over a socket; native pages were built beside that
and off unless asked for, which was not what the owner had asked for and left his Koyo on the
off-screen path. That path is removed: the host's off-screen rendering, its surfaces, its
frame clock and its live-resize emulation; the app's frame drawing and its forwarding of
pointer, wheel, pinch, keys and composition; the protocol's messages for both (protocol 13);
and the setting and `KOYO_NATIVE_PAGES`, since there is nothing left to choose. What settled
it is in section 1. What was checked of native pages on the way, on screen and on the agent
desktop, is in sections 3 to 7: the picture, the pointer, typing, the keyboard going back to
Koyo and into the page again, edits, find, `DevTools`, responsive mode, the browser tools,
pages out of sight, closing and a clean quit, the cursor, full screen, permission prompts,
browsing data cleared at once, the holder following the window, and dividers dragged along a
page at a hand's speed and on half points (section 6).

---

## 1. Why [decided]

- An off-screen CEF browser is one CEF makes "Alloy style": extensions get no tabs, windows,
  toolbar or popups, and `chrome://settings` and `chrome://extensions` are blocked
  (`.superpowers/research/run/*.out`; CEF issues #3859, #4255).
- Off screen, each frame cost a GPU copy, a socket hop and a second compositor, and each
  input a round trip; and Koyo re-implemented what a browser window gives: passkeys,
  `DevTools`, menus, dialogs, the frame clock.
- **It dropped a quarter of a video's frames.** `[proven]` 2026-11-15, on the release build
  then installed, both ways from the same binary, a YouTube clip played muted on the real
  screen (M4 Max, 5K display at 2x), `getVideoPlaybackQuality()` counted over 30 s after 12 s
  of settling (`.superpowers/perf/`):

  | | Off screen | Native |
  | --- | --- | --- |
  | The stream YouTube offered | VP9 SDR, 2160p at most | AV1 10-bit HDR, up to 4320p, as it offers Arc |
  | 2160p60 | 452 of 1808 frames dropped (25.0 %), 45 shown a second | 0 of 1809 dropped |
  | 1080p60 | 297 of 1209 dropped (24.6 %), 45 a second | not measured |
  | 4320p60 | not offered | 0 of 1808 dropped |
  | Animation frames a second the page was given | 30.7 with the video paused, 45.4 playing | 60 to 70 (section 3.2) |

  The same quarter at 1080p as at 2160p: not what decoding cost, but how often the page was
  given a frame to draw. Off screen a frame began on the host's own clock, a display link
  that skipped a page still busy with its last frame (the removed `host/clock.rs`, itself
  there because an off-screen page left to Chromium's clock fell to half the display's
  rate). Natively Chromium runs its own compositor against the display, as in a browser.

## 2. What was settled

| Question | Answer | How it's known |
| --- | --- | --- |
| 1. One native Chrome-style page in a Koyo window | Works, with CEF in Koyo's own process and no CEF patch | `[proven]` section 3 |
| 2. What a browser-process crash takes with it | Koyo itself. Terminals and the window's shape come back | `[proven]` section 4 |
| 3. Pages agents drive out of sight | Work natively, shown or hidden to the engine as Koyo chooses | `[proven]` section 5 |
| 4. The pane's edge and the page during resizes | A frame or more apart on a split resize; Chromium's coordinator doesn't cover it | `[proven]` in software compositing, section 6 |

Evidence is under `.superpowers/native/probe/` (captures, `cdp.out`, `sync-*.out`) and
`.superpowers/native/logs/`.

## 3. How a native page works

### 3.1 CEF runs in Koyo's process [proven]

An `NSView` can't be shown in another process's window, so the browser process is Koyo. The
engine stays where it is (`Koyo Browser Host.app`, `script/browser-engine`): Koyo loads its
framework with `cef_load_library`, and the renderers, GPU process and the rest are still the
engine's helper apps running the browser host's binary, so what the host does in a renderer
(the passkey bridge) carries over unchanged.

- **`NSApp`.** Chromium asks `NSApp` whether it's dispatching an event (`CrAppProtocol`,
  `CrAppControlProtocol`). GPUI's `GPUIApplication` is given `isHandlingSendEvent`,
  `setHandlingSendEvent:` and a `sendEvent:` that sets the flag round `NSApplication`'s own, at
  run time, before CEF starts (`embedded/nsapp.rs`). `CefAppProtocol` isn't registered in the
  framework, so it can't be added; nothing has needed it. `[proven]`
- **Run loop.** GPUI runs `[NSApp run]`. CEF is started with `external_message_pump`, and a
  run-loop timer in the common modes runs `cef_do_message_loop_work` when CEF asks, at the
  latest every 1/30 s (`embedded/pump.rs`). CEF is started from that timer, never inside
  GPUI's frame, and what edits a page is done from it too. `[proven]`
- **Started as the app starts** (`browser::warm_up`), from the run loop. Not measured: what `cef_initialize` costs the main thread.
- **Koyo must run from a bundle that holds the engine.** Run as a bare binary with the engine
  elsewhere (`koyo-rs/target/debug/koyo`), every helper exits at once (GPU process
  `exit_code=5`, then "GPU process isn't usable"). Run from
  `X.app/Contents/MacOS/koyo` with the engine in `X.app/Contents/Frameworks`, as a packaged
  Koyo is laid out, they stay up. `[proven]`; the cause (the helpers' sandbox and the bundle
  paths it's given) is `[read]`. `script/dev-bundle` makes such a bundle of the debug build;
  from a bare binary (`script/dev`, `cargo run`) there are no pages, and a page says why.

### 3.2 The page is made in a window of CEF's, and its views move into the pane [proven]

CEF makes a Chrome-style browser only in a window of its own: given a parent view it makes it
Alloy style (`cef_types_mac.h:151`). So, as Cra does
(`.superpowers/research/cra-src/repo/Sources/Cra/UI/Windows/EmbeddedPages.swift`):

1. `cef_browser_view_create` with a delegate whose `GetBrowserRuntimeStyle` is Chrome, added
   to a frameless `cef_window_create_top_level` window (`koyo-browser-host/src/native.rs`).
2. That window, the **holder**, stays: clear, click-through, never key or main (its class's
   `canBecomeKeyWindow` and `canBecomeMainWindow` are replaced for holders), left out of screen
   captures, kept over the pane on screen so Chromium places its own menus and bubbles right,
   and a level under the usual windows' (`native/holder.rs`). Shown after Koyo's window while
   the app wasn't in front, a holder at the usual level was in front of it, over its page:
   the presses there were the holder's, which has no page in it, and the page took none for
   as long as Koyo ran, 2026-11-15. `[proven]` with the pointer's own clicks on the agent
   desktop: a click the page didn't count with its holder in front, and counted with every
   holder a level down.
3. Chromium draws the whole holder with one compositor, whose layers hang in its
   `ViewsCompositorSuperview`; the page's `WebContentsViewCocoa` over it only takes input. Both
   move into a view of Koyo's in a native surface of GPUI's composition
   (`native/surface.rs`), between GPUI's base content and its overlays.

What the spike showed (`.superpowers/native/probe/`):

- The page draws in the pane (`04-main.png`), animates (`requestAnimationFrame` counted at
  60 to 70 frames a second), and sizes with the pane.
- A click in the page makes `RenderWidgetHostViewCocoa` the first responder of Koyo's window,
  which stays key and main; typed keys reach the page; a button click lands; a wheel event
  scrolls it (`06-typed-crop.png`, `07-scrolled.png`).
- With the page focused, ⌘L is taken by GPUI as a key equivalent and focuses the address
  (`08-crop.png`): Koyo's ⌘ shortcuts go on working. ⌘T was taken the same way; what it
  opened wasn't checked.
- An extension sees the page: `chrome.tabs.query({})` returns it with its address, title and
  size, and `sender.tab` is set (`10-ext-crop.png`). Off screen both are empty
  (`.superpowers/research/run/01-basics.out`). Each page is its own window of type `popup`, so
  `tabs.query({active: true, currentWindow: true})` from the service worker is empty. Since:
  on the framework `script/cef` builds, a page is a `normal` window whose tab is the current
  one while the person is on it (`docs/extensions.md` 3).
- No CEF patch was needed for any of this. `[proven]` for this scope only.

Not known:

- **With GPU compositing** Chromium hands the page to the screen through a `CALayerHost`
  showing the GPU process's layers, which a capture taken in the process can't see: the
  captures above are with `--disable-gpu-compositing`. The owner ran the spike with the GPU
  on and found it much faster than off screen (2026-11-14), which is the evidence that it
  shows. Cra ships the same arrangement.
- The pieces are named by Chromium's own class names (`ViewsCompositorSuperview`,
  `WebContentsViewCocoa`), which an engine update can change. `[read]`

### 3.3 What Chrome does on its own, and is told not to [proven]

A Chrome-style page is a Chrome browser, and Chrome keeps it in its session. After a run that
didn't quit cleanly, the next start restored the session into two stock Chrome windows, title
bar and all, over Koyo. Koyo brings its own tabs back, so before CEF starts each profile's
`Sessions` folder goes and the profile is marked as having left cleanly, and Chrome is started
with `--no-startup-window --disable-session-crashed-bubble --hide-crash-restore-bubble`
(`embedded.rs`, `app.rs`). After that a killed Koyo came back with no Chrome window. A browser Chrome
still makes on its own is an extension's doing, and gets a client of its own
(`default_client`, `native/strays.rs`): a window an extension asked for stays Chrome's
window, and a tabbed window's page becomes a tab of Koyo's (`docs/extensions.md` 3).

Chromium also brings the app forward when it shows one of its windows, and every page's holder
is one: the spike was the active app after its first native page without anyone activating it,
and wasn't in an off-screen run of the same script. Seen, not isolated; Cra reports the same
and has its `NSApp` refuse activation while a page's window is shown; Koyo's now does the
same (`embedded/nsapp.rs`), and stayed behind while pages were made, with the screen locked.

## 4. A crash [proven] [owner]

With CEF in Koyo's process, a crash of Chromium's browser process is a crash of Koyo. A
renderer or the GPU process going down stays what it is today: one page, or a restart of the
GPU process.

- Tried: `kill -9` of Koyo with two pages and a terminal open. Relaunched, the three tabs were
  back in order with the tab shown, the terminal the same one (its handle unchanged, its shell
  still running in the daemon), the pages at their addresses. A change made in the half second
  before the crash (a split) was lost, as `window.json` is written 500 ms on
  (`docs/browser.md` 2). Chats weren't tried; they live in the agent host, as terminals live
  in the daemon.
- Seen on the way: a panic in the spike's own measuring code took every page and the window
  with it. Off screen, a host that stopped was started again under a window that stayed; in
  Koyo's process the engine starts once, and one that stops leaves its panes saying to restart
  Koyo.
- How often: this machine has 13 crash reports of the browser host, all within four minutes
  on 2026-11-08, and none since; two more came from poking extension APIs in Alloy style
  (`.superpowers/research/evidence/`). Each would have been a Koyo crash.
- What a crash loses then that it doesn't today: unsaved edits in editors, scroll positions,
  what was typed and not sent anywhere but a chat's draft, pages' form contents and history.

No way was found to keep the browser process apart and still have `AppKit` deliver input to
the page: showing another process's layers (`CALayerHost`, private) gives the frames but not
the input, the input method or accessibility. `[read]`

## 5. Pages out of sight [proven]

Today a page no pane draws is hidden for the engine unless an agent used it in the last 90 s
(`docs/app-control.md` 11, `view/sight.rs`). Native pages do the same. A page whose tab wasn't shown, asked over the `DevTools` protocol
(`.superpowers/native/probe/cdp.out`, `q3.sh`):

| Where its views are | `visibilityState` | Animation frames | Script | Screenshot | Protocol click |
| --- | --- | --- | --- | --- | --- |
| Out of every window | `hidden` | stop (13 in 5 s) | answers at once | answers in 255 ms | lands |
| Back in the holder, over the pane | `visible` | run (about 65 a second) | answers | 262 ms | not tried |
| Back in the holder, the holder moved past the screen's corner | `visible` | run (about 65 a second) | answers | 261 ms | lands |

So: **awake** (an agent uses it) is the views in the holder, moved off the pane; **asleep** is
the views out of every window. Shown again, the views go back into the pane and the page is
`visible` with nothing reloaded. The holder moved off the pane keeps one point on screen;
whether macOS ever counts it as covered there, which would hide the page, wasn't seen in these
runs and isn't ruled out.

As built (`koyo-browser-host/src/native.rs`): the app's `Visible` request says whether a page
is on screen for the engine, as before. On screen, its views are in the pane's view while a
pane draws it, else parked in its holder, the holder moved past the screen's corner; not on
screen, they're out of every window, which is what hides it. Chromium is started with
`--disable-backgrounding-occluded-windows`, so it never hides a page by what covers its
window: without it a parked page went `hidden` whenever Koyo was behind another app or the
screen was locked, and an agent's page stopped getting frames. `[proven]`: with the screen
locked, a page opened in the background answered `visible` with frames running, and took
`browser_eval`, `browser_click`, `browser_type`, `browser_navigate`, `browser_screenshot`,
`browser_dialog` and `browser_viewport` (390 × 844, read back from the page).

The off-screen path wasn't needed for pages out of sight, and is gone (2026-11-15).

## 6. Resizes [built; the owner saw it work]

The risk: the pane's edge is GPUI's and lands in GPUI's frame; the page is Chromium's and
lands when its next frame is ready.

- **Window resizes** already have the treatment: when `AppKit` asks the window to draw, GPUI
  presents inside Core Animation's transaction on every surface
  (gpui-fast-macos 0.1.12 `window.rs` `display_layer`, `fast/composition.rs`
  `set_presents_with_transaction`). `[read]`
- **Split and panel resizes** don't. Measured by stepping a split's ratio and reading the
  page's layer after each commit (the spike's `sync.rs`, since removed;
  `.superpowers/native/probe/sync-software.out`):
  the turn of the run loop that moves the pane's edge commits the page's content at its old
  size, and content at the new size arrives 21 to 32 ms later, six runs. Chromium's
  `CATransactionCoordinator` did not hold anything back: it waits only for a live resize of a
  window of Chromium's own. `[proven]` with software compositing; with the GPU on the content
  can't be read from Koyo's process, and nothing suggests it would differ.

Two things are out of step in such a drag, and each has its own answer.

1. **The page's view against GPUI's frame.** The view moves in Core Animation's transaction,
   at the end of the turn; GPUI's frame, drawn at the screen's refresh, presents by itself,
   and the two can land a refresh apart: a pane made narrower would show its page over its
   neighbour for a frame. As built: once a frame has changed a native page's size, every
   move of a drag for the next quarter second is drawn at once and presented in the
   transaction (`native::window::draw_in_transaction`, asked for by the page element in
   `page.rs`; `NativePage::resizing`), the way a fitted pop-up is. Only then: a drag that
   resizes no page, and a page resized by anything but a drag, draw as ever. The first frame
   of a drag still presents by itself. GPUI's renderer can't be told to present a frame in
   the transaction once it's being drawn, and which frame will resize a page isn't known
   before it's laid out.
2. **The page's picture against its view.** Chromium's frame at the new size comes a frame
   or two after the view. Its layer isn't stretched meanwhile: Chromium gives the layer that
   holds the picture a frame of its own, the picture's size, at the view's top left
   (the harness's `layers`). What shows beside it in a pane made wider is what's behind the
   page, which is now the colour the page's own canvas is painted (the root's background,
   else the body's, else white; asked with the page's tone, `tone.rs`), where it was white
   whatever the page. A dark page no longer flashes a white strip at the edge.

`[proven]` on the agent desktop, software compositing: a split's divider dragged 200 points
along a page drew eleven frames in the transaction and none after the drag, and the page was
822 points wide before and 1023 after; the canvas's colour was read (238 246 255, and
20 24 28 once the page was made dark).

**A page sits on whole points.** A pane's edges fall between points whenever a pointer set
them: on a screen of two pixels a point a pointer is on half points, and a split's ratio
puts an edge anywhere. A native page can't be sized so. Its holder is a window, whose frame
macOS rounds, and Chromium lays the page out in whole points; and in a view half a point
over, the layer Chromium's picture is in (the one under the compositor view's own, resized
with it by Core Animation) is rounded up each time the view is resized and never comes back
down. The halves add up: after a few drags of the developer tools' divider the picture's
layer was 16 to 20 points taller than its view, so the picture sat that much too high, its
top cut off and a gap at its foot, in the page and in the tools alike, while every view's
frame was where it belonged. Seen by the owner, 2026-11-14, and `[proven]` in the harness:
a view 739.5 points high had a picture layer of 740, and back at 725 one of 726; whole-point
drags never moved it, 100 drags.

So the page is put on whole points (`native::surface::on_whole_points`, in
`NativePage::place`), view and holder both. Where it reaches its pane's edge, an edge goes
out to the next whole point, and the pane, which clips the page's view, trims what's over:
the page covers all of its pane, up to a point over at an edge and never under. Put on the
nearest whole points at first, a pane's edge half a point in rounded inwards as often as
not, and what GPUI draws behind the page showed in that half point: the page's canvas as
Koyo guesses it (`tone.rs`), white for a page whose root and body are transparent, a white
line down the left of Google Meet's dark page (the owner, 2026-11-15; his sidebar, dragged,
was 321.3164 points wide). `[read]`: the line's place, one pixel just left of the page and
down its whole height, fits this; not run on screen. An edge within its pane, a device's in responsive mode, goes to
the nearest whole point, so a size that was whole stays what it was. The host checks it in debug builds (`native::place`). Apart from
that, the docked tools' size, which a drag of their divider sets and the settings keep, is
kept in whole points too (`dock.rs`): tidier, not what keeps a page whole, as a split's
ratio or a window's size puts a pane's edge anywhere all the same.

`[proven]` on the real screen with the GPU on, by the harness's drags (events handed to the
window as `AppKit` hands a hand's; nothing here can move the real pointer), each drag a slow
start on the divider and then up to forty jumps of 1 to 400 points on whole points, half
points and anywhere between, a move every 1 to 33 ms, and after each the views' frames, the
picture's layer and the size the page itself reports checked against the pane (`pages`,
`fit.sh`), 485 drags in all: the tools' divider docked at the bottom (210), right (90, 25 of
them in the Orca layout) and left (40), and a split's divider between two pages side by side
(85, 25 of them on Orca's hairline) and one above the other (60); and 30 bursts of odd split
ratios and window sizes. No page was off its pane. The tools opened, closed and moved between
every side and a pane of their own, from their own controls and from Koyo's setting, fitted
each time. Before the fix the first drag onto a half point failed the
same check.

**A divider is picked up at any speed.** GPUI starts a drag once it has heard the pointer
move more than two points from the press, and a hand at its usual speed is off a divider nine
points thick, over the page beside it, by its first move. Two things kept such a drag from
being picked up, and the owner couldn't resize the developer tools reliably, 2026-11-15.

*Whose a button's moves are.* gpui-fast's window gave each drag move to whatever lay under
the pointer, a native view or GPUI, and kept one native view as the owner of every button
until it had counted each button's release (gpui-fast-macos 0.1.12 `fast/composition.rs`,
`send_window_event`). So a drag pressed on a divider lost the moves made over the page, and a
release the window never saw, one a menu the press opened had tracked, left that view the
owner for good. Now **a press owns its button**: the drag and the release of a button go to
whoever took its press, a native view or GPUI, wherever the pointer is by then, and each press
says anew whose its button is. `gpui-fast-macos` is in `koyo-rs/vendor/` for it
(`vendor/README.md`). With every pointer event traced, GPUI's view got the press, each move
and the release of every drag, first moves of 30 to 80 points off the divider among them.

*Where GPUI takes the page to be.* GPUI draws a frame in two passes. Once the first, prepaint,
is done it notes where the window's native views are, and what of its own a press finds is
what none of them covers (gpui-fast 0.1.12 `fast/composition.rs`, `finish_input` and
`hit_test`). The page's view was moved in the second pass, as its element was painted, so each
frame noted it where it had been a frame before. After a fast drag the last frame's move was
a divider's width or more: the divider then lay where GPUI still took the page to be, a
press on it counted as behind the page, and nothing was picked up until something else had the
window drawn again. A slow drag ends on a move of a point, which left the divider's middle
clear: slow worked, fast didn't. The page's view is now placed as its element is prepainted
(`browser/page.rs`, `place_native`), with its hitbox, and the island it rounds its corners by
says where it is then too (`theme::card::prepainted`). Traced inside GPUI's drag start, one
fast drag in three found its divider not hovered at the press before; none of thirty-two
after: fourteen on a split's gap, twelve on the tools' divider docked at the right, six more
on the build without the tracing, first moves of 12 to 60 points both ways, each landing on
the point expected.

Before these, a hit region over the whole window was put up on a seam's press, drawn at once
(commit 455480ba2). It raced the pointer's first move and treated neither cause; it's gone.

**A release GPUI never hears is let go all the same.** GPUI's core counts a press over a
native view as that view's until it has seen the release of every button down (gpui-fast
0.1.12 `fast/composition.rs`, `dispatch_native`), and while it does, a press anywhere in the
window is the view's: none of GPUI's own content answers. A release can go where the window
never sees it: a menu tracks the pointer itself from the moment it opens and takes the release
of the button that opened it, a page's menu (Koyo's) and the menus Chromium shows itself (a
`<select>`'s, the developer tools' own) alike, and a drag session takes the left button's.
After a right click in a page or in its tools, no tab could be picked and no divider moved
until Koyo was started again, or until the page's tab had been left, which takes its surface
and the press with it (the owner, 2026-11-15: "once, switching back to the page with ⌃⇥ and
dragging the divider unlocked it"). The first answer, the menu telling GPUI of the releases it
took (commit cdcebcf5a), covered Koyo's menu only and is gone. Now GPUI's view keeps the
presses it passed on, and lets go of them, telling GPUI first, once it's plain they're over: a
move with no button down, or a press while the button the hand pressed for one is up
(`vendor/gpui-fast-macos`, `window.rs`, `lost_releases`). `[read]`, not run on screen.

**A key the page leaves is a shortcut or nothing.** A key Chromium didn't handle goes back to
Koyo's view, so Koyo's shortcuts work with the keyboard in a page; Chromium counts a
character's key-down as unhandled even where the page then takes the character as text. Handed
back as a key press, GPUI found no binding for it and passed it to the text system, which
typed it into whatever had the keyboard: the page, a second time, "f" coming out "ff". It's
handed back as a key equivalent (`native.rs`, `unhandled_key`), which GPUI matches against its
bindings and never sends to the text system. `[proven]` with the keyboard's own events on the
agent desktop: one key, one `input` event in the page, where there were two. GPUI takes a key
equivalent it didn't use to be about to come again as a key press, and let the next key press
go by if it was the same keystroke: "f" in a page, then "f" in a chat, and the chat's "f" was
lost. It's matched by the very event now, its time with it (`window.rs`, `handle_key_event`).

**The modifiers are the window's.** GPUI hears the modifier keys change only through its own
view, and with the keyboard in a page Chromium's view hears them instead: ⌃ let go after ⌃⇥
never reached GPUI, and the tab switcher stayed up with its pick, the tab never changing
(the owner, 2026-11-15, from a pinned page). The window hands every change of the modifiers to
GPUI's view too while another view has the keyboard, and GPUI counts a change it hears twice
once (`fast/composition.rs`, `route_keys`).

**Where the keyboard is, in a page or in Koyo, is `docs/keyboard.md`'s.** One holder per window
says whether GPUI's view or a page's has the keys, and only it moves the window's first
responder: as GPUI's focus comes to a page or leaves it, as the person clicks or asks for a tab,
and before a page's views leave the window (a view that has the keyboard as it's taken out of
its window leaves the window itself with it, which hands no key on, and after a tab with the
keyboard in its page was left nothing typed reached a chat's composer or a terminal). The host
only says where a page's views go and which page a view is (`native.rs`, `set_keyboard`,
`key_view`, `page_of`); a page that takes the keyboard on its own is refused.

How these were found matters for the next check. The harness hands its events to the window
(section 9), which is not where the pointer's own come in: it can't show which window of the
app is in front, and its events carry no button number. The checks above that say *real* were
made on Koyo's agent desktop (`.superpowers/native/desk2/`), with the pointer's and keyboard's
own events: a click on a page's button counted by the page, the page's menu opened by a right
click and Inspect picked from it, and drags of the tools' divider, a split's gap and the
sidebar's edge, both ways, twenty in all, each read back from the views' frames. A window
there isn't one `AppKit` takes to be visible, so the harness's `tick` has it drawn and counted
as seen (`view/sight.rs`, `UNSEEN_COUNTS`).

Seen on a real screen with the GPU on by the owner, 2026-11-14, dragging a split's divider beside a native page: it works. Holding the edge until Chromium's frame is in, the step kept in reserve, isn't needed.

## 7. What the host does for a page, and what's checked

The host runs in Koyo's process as a library (`koyo_browser_host::embedded`): Koyo gives it
one end of a socket and talks to it over `koyo-browser-ipc`. Profiles, page state, dialogs,
menus, permissions, passkeys, `DevTools`, the protocol bridge, agents' commands and edits are
the host's code behind the protocol's requests, as they were for the off-screen path this
replaced; "as off screen" below marks what was carried over unchanged. Keyboard focus was too,
until protocol 14: the app moves it now (`docs/keyboard.md`).
The page itself is `native.rs` in the host (a pane's browser is a Chrome-style view in a
holder) and `browser/native` in the app (a page's view in its pane).

- **One engine a run.** A profile folder serves one browser process, and CEF starts once in a
  process, Koyo's. Off macOS and in tests, which start no engine in their own process, a stub
  of the same few calls says there are no pages (`browser/native_off.rs`).
- **What the app doesn't send a native page:** its pointer, wheel and keys, which `AppKit`
  hands the page itself; GPUI has no key handler on a native page. A press is still heard on
  its way to the page, so GPUI's focus follows it and the person has the page.
- **What goes over the protocol as before:** `Edit` (⌘C and the rest are the engine's
  commands in the focused frame), `Visible`, `Resize` (the size of a page no pane draws).
- **What the app calls directly**, because it carries views: `native::place` as it draws the
  pane (the page's views into the pane's view, its holder over it on screen),
  `native::release` when the pane stops drawing it, and for the keyboard `native::key_view`,
  `native::page_of` and `native::set_keyboard` (`docs/keyboard.md`).
- **Keys the page leaves** go from CEF's `OnKeyEvent` to the app's own view in that window
  (`pane/keys.rs`), as they would have had the page not had the keyboard. ⌘ and ⌃ shortcuts
  don't need it: GPUI is asked about key equivalents first.
- **Keys before the page hears them** (`OnPreKeyEvent`, `pane/keys.rs`). A key with an event of the
  system's behind it is the person's: the app is told (`HostEvent::Typed`, protocol 10, at
  most twice a second a pane) and counts them as at the page, as a press does, so an agent's
  command waits. And Escape takes a page out of full screen without the page hearing it:
  Chromium's own does that only in a window of its own that has the keys.
- **The cursor** is Chromium's to set (`OnCursorChange` lets it), and the app is told which
  it is as before. GPUI sets the window's cursor after each of its frames from what's under
  the pointer, which over a native page was nothing, so an arrow: the page element names the
  page's cursor for its place, and the two agree.
- **The page's views fill their place by themselves**: they resize with the app's view they
  are in, and Chromium, which lays them out again by its holder's size each time that
  changes, puts them where they already are, the holder being the size of that view. A
  trace of every frame set on them through 1,700 placements showed no other (the trace is
  gone; the harness's `pages` says whether each page fits). An earlier step that put them
  back after each resize answered a page pushed up after the tools closed; that was the
  picture's layer, not the views (section 6), and the step is gone.
- **The holder follows Koyo's window**: it's put over the pane as the pane is drawn, and
  GPUI draws a window again when its bounds change, a move too. It isn't made a child of
  Koyo's window, which would move it for nothing: tried, and with a child window lying over
  it the page took no pointer.
- **Clearing browsing data** can't stop the engine under its pages here, and a profile's
  folder can't be deleted under a running Chromium. So every profile moves to a fresh folder
  (`AppRequest::ClearData`, protocol 10; `host/panes.rs`): the root keeps how many times it
  was cleared (`generation`), a profile's folder is `profile-<name>` and after that
  `profile-<name>.g<times>`, the host closes every page, and the app opens each again at the
  address it showed (`Browsers::reopening`, `BrowserView::reopen`). The old folders go as the
  host next starts, before CEF opens the root, and come off Chrome's list of profiles
  (`Local State`), which it would make them again from.
- **The window delegate's `CanClose`** has to be implemented: the Rust wrapper's default is
  no, where CEF's is yes, and a holder that can't close keeps its page from closing and CEF
  from shutting down.
- **A class is asked with `[object class]`**, never read off `isa`: a window or view under
  key-value observing has a subclass there the runtime made (`native::class_of`).

Checked means run in the debug build as a bundle, through the host: on screen
(`.superpowers/native/test/`), or, where it says so, on Koyo's agent desktop
(`.superpowers/native/desk/`), both 2026-11-14. A window on the agent desktop isn't on
screen: there the harness has GPUI draw it (`tick`), a page counts as out of sight unless a
browser tool used it in the last 90 s (and is hidden again once a press makes it the
person's), and the pointer's moves are handed to the views (`hover`), as no tracking area
gets them. Captures need software compositing, under which a page in a window that isn't on
screen often took neither moves nor presses: what the pointer does was checked with the GPU
on, what a page shows with it off.

| What | Native | State |
| --- | --- | --- |
| Picture in the pane | Chromium's own layers | checked (software compositing for the captures; the owner saw it with the GPU on) |
| Address, title, loading, history | the host's handlers | checked |
| Pointer | `AppKit` to the page's view; a press gives the person the page | checked: clicks, the wheel |
| Keys and text | `AppKit` to the page's view | checked: typing in a field |
| Keyboard into the page and back | the app's keyboard holder moves the first responder (`docs/keyboard.md`); `Focus` before protocol 14 | checked with `Focus`: ⌘L takes it to Koyo, a click gives it back; with the holder, not yet on screen |
| ⌘C, ⌘X, ⌘V, ⌘A, ⌘Z, ⇧⌘Z | `Edit`, as off screen | ⌘A checked |
| Koyo's shortcuts with the keyboard in a page | GPUI is asked first | checked: ⌘L, ⌃Tab |
| Find bar | the host's `Find`; the bar is GPUI's, over the page | checked |
| `DevTools` | a native page of the frontend, same bridge; since `docs/browser.md` 13, its own dock menu, × and device toggle, and no bar of Koyo's | checked, docked, with Koyo's bar; since, its own toolbar seen with no bar of Koyo's, its dock menu and × at work not yet |
| Responsive mode | Chromium's emulated size and scale | checked: 390 × 844 at 40 %, a click at the scaled place, a tool's screenshot at full size |
| `window.open` | the host's `OnBeforePopup`: the same pane goes there | checked |
| Browser tools: eval, click, type, navigate, back, reload, screenshot, dialog, viewport | the protocol through the host | checked, shown and out of sight |
| Page out of sight, awake for an agent | views parked in the holder | checked |
| Closing a tab | browser closed, holder destroyed | checked |
| Quitting | pages closed, holders destroyed, `cef_shutdown` | checked: about 40 ms |
| Koyo stays behind when a page is made | `NSApp` turns Chromium's activation down | checked |
| Restored, pinned and loose pages | made when shown, as before | a restored page checked; rest by construction |
| Dialogs, sign-in, file choosers, certificate errors | the host's handlers | `confirm` checked through `browser_dialog`; the rest not |
| Context menu | the host's menu handler; Koyo's native menu at the click | checked: asked, shown, closed with Escape |
| Zoom | the host's zoom level | checked: ⌘= twice, 554 to 443 CSS pixels wide; ⌘0 back |
| Full screen, filling the pane | the page's own full screen in the pane it has; Chrome's window stays as it is (it can't go full screen beside another, which the holder is set to) | checked: entered, Escape leaves it |
| Full screen, filling the screen (the default) | Koyo's window goes full screen, macOS's own, with that page alone, as off screen | checked, agent desktop: the window and the page 2560 × 1440, the page's picture filling it; Escape and the page's own `exitFullscreen` each bring the window and the pane back. Escape is the host's (section 3): Chromium's own didn't leave it |
| Cursor | Chromium's own, and GPUI told the same | checked with the owner's pointer on the real screen, 2026-11-14: the app's log of the cursor it had set, twice a second while they used it, has a hand over a link, an I-beam over a field and the resize cursor on a divider. An earlier finding here, that it was an arrow nearly everywhere, was measured by moving the pointer without the app hearing of it, and showed nothing |
| Permissions | the host's handlers, in Koyo's process now; Koyo's own prompt | checked, agent desktop: notifications asked, Allow, the page told `granted`; put away, `denied`. Location and the camera asked and blocked, the page told so. Allowed, those two go on to macOS's own prompt, which shows on the person's screen: not tried. `script/bundle` and `script/dev-bundle` give Koyo the usage reasons macOS asks for |
| Passkeys | the host's bridge, as off screen | checked, agent desktop: turned down at once (`NotAllowedError` in 2 ms), as an unsigned build does (`docs/browser.md` 9). The sheet itself needs a signed build |
| Trackpad swipe back, pinch | Chromium's own | not checked |
| The bar's glass dark or light by the page | the page says how light it is: once a second while drawn, the background colour at a 12 by 8 grid of its points, through its session (`browser/tone.rs`). Pictures and gradients read as what's behind them | checked: the bar changed tone over a page turned dark |
| The person "has" a page by typing in it | the host says a key of the person's was pressed (`Typed`) | checked: after typing with no press, an agent's command was told the person has the page |
| Clearing browsing data | every profile to a fresh folder, pages closed and opened again (section 3) | checked: a cookie, `localStorage` and a granted permission gone, the page back at its address; the old folder deleted at the next start |
| A window moved | GPUI draws the window again, and the holder is put over the pane | checked, agent desktop: the window moved 80 by 45 points, the holder with it |
| A split's divider dragged along a page | drawn in the transaction; the page's canvas colour behind it (section 6) | checked by the owner on the real screen with the GPU on, 2026-11-14: "it works perfectly". The page went through fourteen widths in the app's log of that drag |
| A pane's edge between points (a drag, a split's ratio) | the page on the whole points nearest its pane's edges, its holder with it (section 6) | checked on the real screen with the GPU on by the harness's drags, on half points and anywhere between: the tools' divider at each side, splits both ways, odd ratios and window sizes; frames, the picture's layer and the page's own size fit after each |

## 8. Measured against the off-screen path

The way out, from a page's frame to the glass, is section 1's video numbers. The input's way
in was measured too, both ways from one debug build, before the off-screen path was removed
(2026-11-14; `.superpowers/native/latency.sh`,
`probe/lat-*.json`): 300 keys and 300 wheel turns, 40 ms apart, each stamped as it's handed to
Koyo's window and again by the page (`www/latency.html`) in its handler and in its next
animation frame. Both modes from the same debug build, GPU compositing, one run each.

| | Off screen | Native |
| --- | --- | --- |
| Key: window to page's handler | median 0.57 ms, p90 1.73, max 9.77 | median 0.34 ms, p90 0.40, max 1.14 |
| Key: window to page's next frame | median 18.0 ms, p90 32.5, max 36.9 | median 0.86 ms, p90 10.9, max 24.6 |
| Wheel: window to page's handler | median 4.12 ms, p90 11.5, max 28.4 | median 3.38 ms, p90 9.45, max 12.0 |
| Wheel: window to page's next frame | median 4.15 ms, p90 11.7, max 28.4 | median 3.48 ms, p90 9.50, max 12.2 |

What it says: a key reaches the page about as fast either way, and with less spread natively;
the page's frame after a key begins about 17 ms sooner natively, as off screen a frame begins
on the host's clock at the display's next refresh and natively Chromium begins one at once.

What it doesn't say:

- Nothing about the way out, from the page's frame to the glass, which is where off screen
  pays its copy, its socket and its second compositor. That needs the screen: a high-speed
  capture, or presentation times from both compositors.
- The inputs were `NSEvent`s handed to the window, not the system's, so the window server's
  part is in neither number.
- The off-screen run's window wasn't key and its app wasn't active; the native run's were
  (section 3.3). Whether that moves the off-screen frame number wasn't checked.
- One run, a debug build, and no real page.

## 9. Running it

There is nothing to turn on: a page is native, and there is no other kind.

- Koyo has to run from a bundle that holds the engine (section 3.1): a packaged Koyo does
  (`script/bundle`); for the debug build, `script/dev-bundle` makes and runs
  `koyo-rs/target/dev-bundle/Koyo Dev.app`. From a bare binary a page says it needs the
  bundle, and shows nothing.

```sh
script/dev-bundle
```

In a debug build, `KOYO_NATIVE_SWITCHES="a b=c"` adds Chromium switches and
`KOYO_NATIVE_PROBE=<folder>` takes
commands from `<folder>/cmd` (`browser/native/probe.rs`): captures of the app's own window
taken in the process, and clicks, keys, scrolls and drags made as `NSEvent`s and handed to
the window, so nothing is injected into the system. Its `keyboard`, `bounds <name>` and bare
`key` commands are the keyboard's (`docs/keyboard.md` 9). The checks in this document were run with
it, on a copy of the bundle with data, settings, logs and temporary folders of its own inside
the checkout (`.superpowers/native/test/`), and on another for Koyo's agent desktop
(`.superpowers/native/desk/`, its environment in its `Info.plist`, as the desktop starts apps
without one).

The engine's own tests (`koyo-browser-host/tests/smoke`) run the host as a process of its
own, from the engine's bundle, speaking the same protocol: its pages are in holders parked
off the screen, which is how a page out of sight is kept (section 5), so they load, run
script, draw and answer the `DevTools` protocol with no window of Koyo's to be in.
`[proven]`: the suite's seven tests pass so, in 3.5 s.

## 10. Where it lives

| Path | What |
| --- | --- |
| `koyo-browser-host/src/lib.rs`, `main.rs` | The host as a library, and the binary made of it: CEF's helpers, and a host process of its own for the engine's tests |
| `koyo-browser-host/src/embedded.rs` | The host's start and end in Koyo's process; CEF found in the engine; Chrome's saved sessions dropped |
| `koyo-browser-host/src/embedded/pump.rs` | CEF's work run from the app's run loop |
| `koyo-browser-host/src/embedded/nsapp.rs` | Chromium's `NSApp` protocols on the app's class; activation turned down while a page's window shows |
| `koyo-browser-host/src/native.rs` | A pane's page: its browser view, its holder, where its views are, which tell the app's keyboard as they move |
| `koyo-browser-host/src/native/holder.rs`, `parts.rs` | The holder's window made inert, and its answer to Chromium's full screen request; the delegates; the client of browsers Chrome makes on its own |
| `koyo-browser-host/src/native/report.rs` | For the harness: where each page's views and its picture's layer are, and whether they fit the pane |
| `koyo-browser-host/src/host/start.rs` | Hello, CEF's settings and start, in Koyo's process or the host's own |
| `koyo-browser-host/src/host/edit.rs` | An editing command, the one thing of a page's input the app asks for |
| `koyo-browser-host/src/pane/keys.rs` | Keys a page leaves, handed to the app's view; the person's keys told to the app; Escape out of full screen |
| `koyo-browser-host/src/host/panes.rs` | Panes from open to closed; profiles' folders by how often they were cleared; clearing without stopping |
| `koyo-browser-host/tests/smoke/` | The engine's tests, against the host as a process of its own |
| `koyo-browser-ipc/src/client.rs` | `Connection::start_embedded`, and `start` for a host process |
| `koyo/src/browser/native/mod.rs` | The host started in the app; a page's view in its pane |
| `koyo/src/browser/native_off.rs` | The same call in tests, which start a host process, and off macOS, where there are no pages |
| `koyo/src/browser/native/probe.rs`, `probe/look.rs` | The harness, in debug builds |
| `koyo/src/browser/page.rs`, `input.rs` | The page element: where the page's view goes, a press heard on its way to the page, Koyo's shortcuts and edits |
| `koyo/src/browser/engine.rs` | The engine in Koyo's own bundle, and what a Koyo run without one says |
| `koyo/src/browser/host.rs`, `host/events.rs`, `host/starting.rs`, `view/lifecycle.rs` | The host started once; clearing data under running pages: which pages open again, and how |
| `koyo/src/native/window.rs` | A frame drawn in the transaction a page's view moves in |
| `koyo/src/native/surface.rs` | A view in a native surface; a pane's bounds on whole points, as a page takes them |
| `koyo/src/surfaces.rs` | Rings of shared surfaces, which the simulator's and the agent desktop's pictures come as (a page's no longer do) |
| `koyo/src/browser/responsive/` | The size and scale a page is told |
| `koyo/src/browser/tone.rs`, `lightness.rs` | How light a page says it is, for the glass over it, and its canvas's colour, for what's behind it |
| `script/dev-bundle`, `script/dev` | The debug build as a bundle round the engine; the hot-reload build with the engine put in the bundle `dx` runs it from |

## 11. Koyo doesn't fork itself [proven]

With the engine in its process, Koyo is a process that must not be forked. A fork's child
runs the system's `atfork` handlers before it becomes anything else, and one of them reaches
into Chromium for a lock: taken while one of the engine's threads held it, the child waited
on it for good, a copy of Koyo that never became `git`, and the thread that asked for git
waited with it. That copy outlived Koyo, holding the control socket and the browser profile:
the next Koyo logged "Another Koyo already answers" and "CEF did not initialize", and had no
pages. Seen 2026-11-15, the child sampled in `xpc_atfork_child` under `fork`, called from
`std::process::Command::spawn`, within ten seconds of a start.

The standard library spawns a program without forking unless it's asked for something only a
fork can do. Three things asked:

- A program named, not given by its path, with a `PATH` of its own to look in: git
  (`koyo-git/src/command.rs`) and npm (`koyo-lsp/src/servers/node.rs`). Each is now found on
  that path first and run by where it is.
- A step in the child before it becomes the program (`pre_exec`), to unblock its signals: the
  daemons' start (`koyo-daemon`). The thread that spawns unblocks its own for the spawn
  instead, and puts them back.
- The same step to hand a host its end of the app's socket on a descriptor of its own: a
  device's host, and the browser's when it runs as a process (`koyo-browser-ipc/src/inherit.rs`).
  The socket is now the host's standard input, which the standard library puts in place as it
  spawns, and its alone; the host moves it off there and puts the null device in its place,
  so the engine's helpers, started with their parent's standard input, don't get it.

Git, the daemons' start and the socket's handing over each have a test that counts the forks
its thread makes: none, the daemon still starting with no signal blocked and the socket still
reaching only its host. Two things keep it so. The lint turns down the standard library's
calls that always fork (`koyo-rs/clippy.toml`, `disallowed-methods`). And a debug build logs,
with where from, any fork made once the engine has started (`browser/native/mod.rs`,
`say_when_forked`): a named program with its own `PATH`, which no lint sees, or a crate's own
fork.

## 12. For the owner

Decided: the crash scope (section 4) is accepted, 2026-11-14.

Open:

1. **A bare binary has no pages** (section 3.1): `cargo run` and `script/dev` start Koyo
   outside a bundle, where the engine's helpers exit. `script/dev-bundle` is the debug build
   with pages.
2. **Private parts relied on**: Chromium's view class names, two methods of CEF's window class
   and three of `NSApplication` replaced at run time. Cra relies on the first two.
3. **Resizes** (section 6): built, and to be judged by eye in a split drag on the real
   screen with the GPU on, which nothing but a person at the screen can do.
4. **`script/check`** stops on timing-dependent tests when the Mac is busy with other builds
   (file watching, inlay hints): different ones each run, each passing alone. The browser
   host's smoke test of a trackpad pinch times out now and then within the suite, and
   passes alone, six times of six.
5. **The own CEF build** (`script/cef`): not needed for the page. Extensions' icons, popups
   and installs need it, with Koyo's patches (`docs/extensions.md` 2.2).