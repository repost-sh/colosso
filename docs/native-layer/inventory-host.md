# Inventory: the browser host's side of the native seam

Read-only inventory for `docs/native-layer.md`, taken 2026-11-16 at `ce2f3c7fc` on
`schulzfel/browser-ui-fixes`. Scope: `koyo-rs/koyo-browser-host`, the seam facts in
`koyo-rs/koyo-browser-ipc`, the agents' input paths (`koyo-desk-host`, `koyo-desk-wire`,
`koyo-app-control`, `koyo/src/control`), the probe (`koyo/src/browser/native/probe.rs`) and the
menu gate (`koyo/src/native/menus.rs`). Paths below are relative to `koyo-rs/` unless they start
with `docs/`. Nothing was built or run except `grep`, `sed`, `awk` and `git log`.

Labels. **[read]** is what the code says, at the line cited. **[inferred]** is a conclusion drawn
from code read here, not run. **[guess]** is a claim about macOS or Chromium behaviour that
nothing in this checkout proves. A claim with no label is [read].

## Findings that matter most for the design

1. **Nothing at the point input enters Koyo tells the agent desktop's relay of the person's own
   input from the agent's.** The desk host posts both through one `Poster` with one private
   `CGEventSource` (`koyo-desk-host/src/input.rs:97-100`; the person's tab input at
   `koyo-desk-host/src/acts.rs:374-424`, the agent's acts at `acts.rs:182-260`), and nothing in
   any crate sets `kCGEventSourceUserData` (field 42) or any other tag. The menu gate tells a
   desk-host event from the hardware's by source state −1 and poster pid (fields 45 and 41,
   `koyo/src/native/menus/appkit.rs:27-28,150-151`), which is a claim about the window server
   that is still `[open]` on screen (`docs/browser.md` 14.4). Below `NSApp`, every `NSEvent` counts
   as the person's: GPUI's window says so outright (`vendor/gpui-fast-macos/src/fast/composition.rs:274-279`),
   `BrowserView::pressed_at` takes the page for the person (`koyo/src/browser/input.rs:23-38`), and
   the host's key handler sends `Typed` for any key with an `NSEvent` behind it
   (`koyo-browser-host/src/pane/keys.rs:33-36`). Probe events and desk-host events pass all three.
   Only CDP input carries no `NSEvent` at all.
2. **The host makes no menu decision and touches no `NSMenu`.** Of 65 AppKit/ObjC touchpoints in
   the host, none is a menu. A page's context menu leaves the host as data
   (`HostEvent::ContextMenu`, `koyo-browser-ipc/src/events.rs:188-201`) with no word of which input
   asked for it. The gate is in the app, judges a window's last noted press or key within 5 s
   rather than the event that opened the menu (`koyo/src/native/menus.rs:107-117,168-192`), and is
   the only code that sees a `<select>` popup or a menu of Chromium's own
   (`koyo/src/native/menus/appkit.rs:45-69`). Probe presses and CDP clicks leave no note, so a
   menu they cause inherits whatever the person last pressed in that window [inferred].
3. **DevTools are an ordinary native page, and the host has no notion of a tab.** A DevTools pane
   has its own `PaneId`, holder window and `native::Entry` (`koyo-browser-host/src/host/inspect.rs:97-107`).
   The host ties it to its page by two fields only, `Pane.inspector` and `Pane.inspected`
   (`koyo-browser-host/src/host.rs:62-65`). Where its views are is decided only by which app
   container last called `native::place` for that pane id (`koyo-browser-host/src/native.rs:283-300`),
   and `native::release` acts only for that same container (`native.rs:319-326`). The "DevTools
   showing in another tab" class therefore lives in the app's element tree and in last-place-wins,
   not in host state (section 3).

Also worth carrying into the design. The host patches two classes process-wide: CEF's window
class (`canBecomeKeyWindow`, `canBecomeMainWindow`, `toggleFullScreen:`,
`koyo-browser-host/src/native/holder.rs:105-184`) and Koyo's own `NSApp` class, which gains
`sendEvent:`, `activate` and `activateIgnoringOtherApps:` (`koyo-browser-host/src/embedded/nsapp.rs:83-131`).
The app adds a third set of patches, on `NSMenu` and `NSPopUpButtonCell`
(`koyo/src/native/menus/appkit.rs:32-74`). Holder windows are deliberately not child windows of
Koyo's window (`native.rs:310-311`).

## 1. Every place the host touches AppKit or ObjC

Concern codes: (1) keyboard or first responder, (2) mouse press ownership, (3) menus,
(4) windows and panels (holder level, key/main refusal, activation refusal, Space, ordering, child
windows), (5) geometry (frames, whole points, holder placement), (6) view lifecycle (place, park,
release, close, DevTools views, reparenting), (0) other. One row per call site or tight group, with
one primary concern.

| Where | Function | What it does | Concern |
| --- | --- | --- | --- |
| `koyo-browser-host/src/lib.rs:36-43` | `mock_keychain` | `NSBundle bundleWithPath:` (the engine) or `mainBundle`, Info.plist key `KoyoMockKeychain`, `NSNumber boolValue` | 0 |
| `koyo-browser-host/src/lib.rs:52-53` | `own_bundle` | `NSBundle mainBundle bundlePath` for the standalone host | 0 |
| `koyo-browser-host/src/location.rs:26` | `MANAGER` | `CLLocationManager new` on the UI thread, kept | 0 |
| `koyo-browser-host/src/location.rs:43` | `opened` | `authorizationStatus` read | 0 |
| `koyo-browser-host/src/location.rs:66-69` | `ask` | `requestWhenInUseAuthorization`: macOS's own prompt window | 0 |
| `koyo-browser-host/src/mac_app.rs:16-31` | `HostApplication` (`define_class!`) | `NSApplication` subclass `KoyoBrowserHostApplication`, `sendEvent:` sets `HANDLING_SEND_EVENT` around super. Standalone host only, never in Koyo | 0 |
| `koyo-browser-host/src/mac_app.rs:35-49` | CrApp protocol impls | `isHandlingSendEvent`, `setHandlingSendEvent:`, `CefAppProtocol` | 0 |
| `koyo-browser-host/src/mac_app.rs:55-61` | `install` | `sharedApplication` on the subclass, `isKindOfClass:` check | 0 |
| `koyo-browser-host/src/mac_app.rs:63` | `install` | `setActivationPolicy(Accessory)`: no Dock icon, may own windows | 4 |
| `koyo-browser-host/src/embedded/nsapp.rs:29-34` | `showing` | counter around CEF's `Window::show`, during which activation is refused | 4 |
| `koyo-browser-host/src/embedded/nsapp.rs:36-42` | `is_handling_send_event`, `set_handling_send_event` | CrApp flag accessors added to Koyo's `NSApp` class | 0 |
| `koyo-browser-host/src/embedded/nsapp.rs:44-51` | `send_event` | `sendEvent:` added to Koyo's `NSApp` class (GPUI's `GPUIApplication` has none of its own): every event Koyo dispatches passes here, then `NSApplication`'s | 0 |
| `koyo-browser-host/src/embedded/nsapp.rs:53-61` | `activate_ignoring` | `activateIgnoringOtherApps:` dropped while `SHOWING > 0`, else `NSApplication`'s | 4 |
| `koyo-browser-host/src/embedded/nsapp.rs:63-70` | `activate` | `activate` dropped while `SHOWING > 0` | 4 |
| `koyo-browser-host/src/embedded/nsapp.rs:84-88` | `install` | `sharedApplication`, `class_of`; refuses a plain `NSApplication` | 0 |
| `koyo-browser-host/src/embedded/nsapp.rs:89-119` | `install` | `class_addMethod` ×5 on Koyo's `NSApp` class; a method the class has already is left alone | 0 |
| `koyo-browser-host/src/embedded/nsapp.rs:121-129` | `install` | `class_addProtocol` `CrAppProtocol`, `CrAppControlProtocol` | 0 |
| `koyo-browser-host/src/native.rs:49-63` | `Entry` | per page: holder (CEF `Window`, `Retained<NSWindow>`), views (compositor, web contents), `shown_in` container, `home` view, `on_screen`, `size` | 6 |
| `koyo-browser-host/src/native.rs:163-170` | `make` | `cef::browser_view_create`, Chrome runtime style: CEF makes the page's views | 6 |
| `koyo-browser-host/src/native.rs:171-174` | `make` | `cef::window_create_top_level`: CEF makes the holder `NSWindow` | 4 |
| `koyo-browser-host/src/native.rs:185` | `window_created` | `Window::add_child_view`: the browser view into the holder | 6 |
| `koyo-browser-host/src/native.rs:187-188` | `window_created` | `window_handle()` as the content `NSView`, its `window` is the holder | 4 |
| `koyo-browser-host/src/native.rs:189` | `window_created` | `hold()`: the holder made a holder (holder.rs rows) | 4 |
| `koyo-browser-host/src/native.rs:194` | `window_created` | `park()` the new holder | 5 |
| `koyo-browser-host/src/native.rs:197` | `window_created` | `showing` around `window.show()`: CEF orders the holder in, `NSApp` refuses activation meanwhile | 4 |
| `koyo-browser-host/src/native.rs:203-208` | `park` | holder `frame`/`setFrame:display:NO` to all but 1 pt past the screen's bottom-left corner | 5 |
| `koyo-browser-host/src/native.rs:223-226` | `settle` | holder `contentView`, `find_view` by class name `ViewsCompositorSuperview`, `WebContentsViewCocoa` | 6 |
| `koyo-browser-host/src/native.rs:233-242` | `settle` (`into`) | `isDescendantOf:`, `addSubview:` compositor then web view, `setAutoresizingMask` width and height sizable: reparent into the container or the holder's content | 6 |
| `koyo-browser-host/src/native.rs:243-245` | `settle` (`into`) | `setFrame:` both views to the parent's `bounds` | 5 |
| `koyo-browser-host/src/native.rs:248-256` | `settle` | `contentView`, target `window`, web view's `window`: decides `Views::Leaving` (window changes) before the move | 1 |
| `koyo-browser-host/src/native.rs:264-265` | `settle` | `removeFromSuperview` ×2: out of every window, which hides the page for the engine | 6 |
| `koyo-browser-host/src/native.rs:268-270` | `settle` | `park()` whenever no pane shows the page | 5 |
| `koyo-browser-host/src/native.rs:283-290` | `place` | pointer identity of `container`; retains `container` and `home` when it changes | 6 |
| `koyo-browser-host/src/native.rs:304-314` | `place` | debug assert of whole points; holder `setFrame:display:NO` to `screen`; not a child window | 5 |
| `koyo-browser-host/src/native.rs:319-326` | `release` | clears `shown_in` only when `container` is the same pointer, then settles | 6 |
| `koyo-browser-host/src/native.rs:381-384` | `web_view` | web view's `window` | 1 |
| `koyo-browser-host/src/native.rs:387-391` | `key_view` | `find_view` `RenderWidgetHostViewCocoa` under the web view, with its window | 1 |
| `koyo-browser-host/src/native.rs:394-405` | `page_of` | `isDescendantOf:` web or compositor: which page a responder, or a pressed view, belongs to (also concern 2, via `koyo/src/keyboard/appkit.rs:201`) | 1 |
| `koyo-browser-host/src/native.rs:423-425` | `unhandled_key` | `performKeyEquivalent:` on `home` (GPUI's view) with CEF's `NSEvent` | 1 |
| `koyo-browser-host/src/native.rs:438-441` | `closed` | `removeFromSuperview` ×2 | 6 |
| `koyo-browser-host/src/native.rs:442-445` | `closed` | `closing()`, then CEF `Window::close` closes the holder | 6 |
| `koyo-browser-host/src/native.rs:457-461` | `class_of` | `msg_send![object, class]` (not `isa`, which KVO subclasses) | 0 |
| `koyo-browser-host/src/native.rs:465-470` | `find_view` | class-name compare and `subviews` recursion; also used by the probe for `GPUIView` | 6 |
| `koyo-browser-host/src/native/holder.rs:55-62` | `hold` | `patch_holders`, holder listed in `HOLDERS` | 4 |
| `koyo-browser-host/src/native/holder.rs:63` | `hold` | `setIgnoresMouseEvents:YES`: presses pass through to the window under it | 2 |
| `koyo-browser-host/src/native/holder.rs:64` | `hold` | `setLevel(NSNormalWindowLevel - 1)` | 4 |
| `koyo-browser-host/src/native/holder.rs:65-67` | `hold` | `setOpaque:NO`, `setHasShadow:NO`, `setBackgroundColor(clear)` | 4 |
| `koyo-browser-host/src/native/holder.rs:70` | `hold` | `setSharingType(None)`: out of screen captures | 4 |
| `koyo-browser-host/src/native/holder.rs:71` | `hold` | `setAnimationBehavior(None)` | 4 |
| `koyo-browser-host/src/native/holder.rs:72-76` | `hold` | `setCollectionBehavior` Transient, IgnoresCycle, FullScreenAuxiliary: Spaces and window cycling | 4 |
| `koyo-browser-host/src/native/holder.rs:82-84` | `closing` | ends a pending full-screen transition before CEF closes the window | 4 |
| `koyo-browser-host/src/native/holder.rs:105-112` | `patch_holders` | `class_of(window)`, once per class | 0 |
| `koyo-browser-host/src/native/holder.rs:114-138` | `patch_holders` | swizzles `canBecomeKeyWindow`, `canBecomeMainWindow` on CEF's window class (`class_getInstanceMethod`, `method_getImplementation`, `method_setImplementation`), for every instance of that class | 4 |
| `koyo-browser-host/src/native/holder.rs:141-154` | `refuse` | the replacement: `NO` for a holder, the class's original for any other window | 4 |
| `koyo-browser-host/src/native/holder.rs:157-184` | `patch_full_screen` | `toggleFullScreen:` added to CEF's class (`class_addMethod`) or replaced | 4 |
| `koyo-browser-host/src/native/holder.rs:188-231` | `toggle_full_screen` | a holder's toggle answered: delegate told "will" now and "did" on the next CEF UI task | 4 |
| `koyo-browser-host/src/native/holder.rs:246-262` | `tell_delegate` | `delegate`, `respondsToSelector:`, `NSNotification`, `performSelector:withObject:` | 4 |
| `koyo-browser-host/src/native/parts.rs:26-32` | `PageViewDelegate` | no Chrome toolbar, Chrome runtime style: which views CEF makes | 6 |
| `koyo-browser-host/src/native/parts.rs:47-91` | `HolderDelegate` | created/destroyed callbacks; frameless, no window buttons, no resize, minimize or maximize; 800×600 first bounds; `can_close` asks the browser | 4 |
| `koyo-browser-host/src/native/parts.rs:121-126` | `StrayLife::on_after_created` | a browser Chrome made itself: its `NSWindow` `setAlphaValue:0`, `setIgnoresMouseEvents:YES`, `orderOut:`, then closed | 4 |
| `koyo-browser-host/src/native/report.rs:19-23` | `describe` | web view's `window`, its class name, `isVisible` | 0 |
| `koyo-browser-host/src/native/report.rs:42-74` | `fit` | container `bounds`, holder `frame`, key view's, compositor's and web view's `frame`; `FITS` or `OFF` | 5 |
| `koyo-browser-host/src/native/report.rs:79-95` | `picture` | `layer`, `sublayers`, `firstObject`, `bounds`, `frame` of the compositor's layer | 5 |
| `koyo-browser-host/src/pane/keys.rs:33-36` | `PaneKeys::on_pre_key_event` | CEF's `os_event` (the `NSEvent*`) non-null on `RAWKEYDOWN` sends `HostEvent::Typed` | 1 |
| `koyo-browser-host/src/pane/keys.rs:49-54` | `PaneKeys::on_key_event` | a key the page left unhandled, its `os_event` to `native::unhandled_key` | 1 |

Adjacent, not ObjC and not counted: `koyo-browser-host/src/embedded/pump.rs:82-106` puts CEF's work
on a `CFRunLoopTimer` in `kCFRunLoopCommonModes` of the main run loop, so CEF runs while a menu or
a window drag tracks, and can send a new `HostEvent::ContextMenu` during a menu's tracking
[inferred]. `koyo-browser-host/src/app.rs:26-44` passes `--disable-backgrounding-occluded-windows`
(on screen for the engine is where the views are) and `--no-startup-window` (Chrome opens no window
of its own). `koyo-browser-host/src/menus.rs` and `dialogs.rs` work on CEF's models only.

### Totals

65 touchpoints. By concern: (0) 16, (1) 7, (2) 1, (3) 0, (4) 22, (5) 7, (6) 12. By file:
`native.rs` 26, `native/holder.rs` 14, `embedded/nsapp.rs` 8, `mac_app.rs` 4, `location.rs` 3,
`native/parts.rs` 3, `native/report.rs` 3, `lib.rs` 2, `pane/keys.rs` 2. Every file but
`mac_app.rs` runs inside Koyo's process; `mac_app.rs` runs only in the standalone host the engine's
tests start, and `native/report.rs` only when the probe asks.

### Commands that reproduce them

Run from the checkout's root. The first lists the candidate lines a broad pattern finds; the second
checks every candidate falls inside a row above or is a known false positive; the last two count
the table.

```sh
INV=docs/native-layer/inventory-host.md
rows() { sed -n '/^## 1\. /,/^### Totals/p' "$INV" | grep -E '^\| `koyo-browser-host/src/'; }
PAT='msg_send|ffi::|define_class|sel!\(|class!\(|NSApplication|NSWindow|NSView|NSEvent|NSColor|NSNotification|NSBundle|NSNumber|CLLocationManager|Retained|\.set[A-Z][A-Za-z_]*\(|\.(frame|bounds|window|contentView|subviews|isVisible|isDescendantOf|addSubview|removeFromSuperview|orderOut|name|isKindOfClass|authorizationStatus|requestWhenInUseAuthorization|bundlePath|objectForInfoDictionaryKey|boolValue)\(|window_handle\(|window_create_top_level|browser_view_create|add_child_view|\.show\(\)|window\.close\(\)|class_of\(|find_view\(|AnyProtocol|MainThreadMarker|setFrame'
cands() { (cd koyo-rs/koyo-browser-host/src && grep -nE "$PAT" $(find . -name '*.rs' | sort) | grep -vE ':[0-9]+:\s*(//|use |\*)' | sed 's|^\./||'); }

# 1. candidates, and how many per file
cands | awk -F: '{n[$1]++} END {for (f in n) print n[f], f}' | sort -rn

# 2. coverage: prints each candidate outside every row (expect only the false positives:
#    devtools.rs:90, host/start.rs:96 thread names; embedded.rs:158, menus.rs:18 CStr; passkeys.rs:72,236
#    `.name()`; native/holder.rs:21-24 a `use` block; native/holder.rs:92 a type alias)
awk -F'|' 'NR==FNR { s=$2; gsub(/[ `]/,"",s); sub(/^koyo-browser-host\/src\//,"",s);
  split(s,a,":"); m=split(a[2],r,"-"); k++; file[k]=a[1]; lo[k]=r[1]; hi[k]=(m>1?r[2]:r[1]); next }
  { split($0,b,":"); hit=0; for (i=1;i<=k;i++) if (file[i]==b[1] && b[2]+0>=lo[i]+0 && b[2]+0<=hi[i]+0) hit=1; if (!hit) print "outside every row: " b[1] ":" b[2] }' <(rows) <(cands)

# 3. rows by concern, and 4. rows by file
rows | awk -F'|' '{gsub(/ /,"",$5); n[$5]++} END {for (c in n) print c, n[c]}' | sort
rows | awk -F'|' '{s=$2; gsub(/[ `]/,"",s); sub(/^koyo-browser-host\/src\//,"",s); split(s,a,":"); n[a[1]]++} END {for (f in n) print n[f], f}' | sort -rn
```

## 2. The API between the app and the host that carries views or seam decisions

### Rust calls into the host (in-process, main thread)

| Item | Signature | Where | Called from `koyo/src` |
| --- | --- | --- | --- |
| `native::place` | `pub fn place(pane: PaneId, container: &NSView, home: &NSView, screen: NSRect)` | `koyo-browser-host/src/native.rs:283` | `koyo/src/browser/native/mod.rs:157` (`NativePage::place`), called each prepaint by `PageSurface::place_native`, `koyo/src/browser/page.rs:204-226` |
| `native::release` | `pub fn release(pane: PaneId, container: &NSView)` | `native.rs:319` | `koyo/src/browser/native/mod.rs:86` (`Hosted::into_surface`), run when the keyed `Kept<NativePage>` is released the first frame it isn't drawn, `koyo/src/native/surface.rs:259-274` |
| `native::set_keyboard` | `pub fn set_keyboard(hear: fn(PaneId, Views))` | `native.rs:370` | `koyo/src/keyboard/appkit.rs:298-301`: `Leaving` to `hooks::page_leaving`, `Arrived` to `hooks::page_arrived` |
| `native::Views` | `pub enum Views { Leaving, Arrived }` | `native.rs:353-363` | `koyo/src/keyboard/appkit.rs:15,299-300` |
| `native::key_view` | `pub fn key_view(pane: PaneId) -> Option<(Retained<NSWindow>, Retained<NSView>)>` | `native.rs:387` | `koyo/src/keyboard/appkit.rs:133` (`makeFirstResponder:` on it, only if its window is the one asked), `:145` (`page_window`) |
| `native::page_of` | `pub fn page_of(view: &NSView) -> Option<PaneId>` | `native.rs:394` | `koyo/src/keyboard/appkit.rs:94`, used for first responders (`:105`) and for presses (`:201`) |
| `native::describe` | `pub fn describe() -> String` | `native/report.rs:15` | `koyo/src/browser/native/probe.rs:198` (`pages`) |
| `native::class_of`, `native::find_view` | `pub fn class_of(&AnyObject) -> &'static AnyClass`; `pub fn find_view(&NSView, &CStr) -> Option<Retained<NSView>>` | `native.rs:457,465` | probe only: `koyo/src/browser/native/probe.rs:171`, `probe/look.rs:127,148,166,196` |
| `embedded::start` | `pub fn start(start: Start)` (`Start { stream, token, profiles, engine }`) | `koyo-browser-host/src/embedded.rs:27-37,51` | `koyo/src/browser/native/mod.rs:57`, through `koyo_browser_ipc::client::Embedded` (`koyo-browser-ipc/src/client.rs:50-57,185`) |
| `embedded::finish` | `pub fn finish(grace: Duration)` | `embedded.rs:79` | `koyo/src/browser/native/mod.rs:64`, run by the IPC client at shutdown |
| `native::unhandled_key` | `pub(crate) fn unhandled_key(pane: PaneId, event: *mut AnyObject) -> bool` | `native.rs:409` | crate-internal, from `pane/keys.rs:53`; it reaches the app's `home` view, set by `place` |

### Requests over the in-process socket (`koyo-browser-ipc/src/requests.rs`)

| Request | Fields | Sent by the app | Done in the host |
| --- | --- | --- | --- |
| `Visible` (`:61`) | `pane, visible` | `koyo/src/browser/view/sight.rs:94-99` (`set_visible`, from `on_screen()`: drawn, or an agent used it within 90 s) | `host.rs:198-201` → `host/panes.rs:125-127` → `native::on_screen` (`native.rs:344`) → `settle` |
| `Resize` (`:28`) | `pane, viewport` | `koyo/src/browser/view/lifecycle.rs:185` (only while no pane draws it) | `host.rs:178` → `host/panes.rs:91-93` → `native::size` (`native.rs:330`), honoured only while parked |
| `Edit` (`:90`) | `pane, command: Edit` | `koyo/src/browser/input.rs:47-51` (GPUI actions ⌘C etc.; takes the page for the person) | `host.rs:205-216`: `focused_frame().undo()` and the rest, no AppKit |
| `MenuCommand` (`:115`) | `pane, menu, command: Option<i32>` | `koyo/src/browser/menu.rs:324-334`, and `koyo/src/browser/host/events.rs:130` when no view takes the menu | `host.rs:222-226` → `menus.rs:66-74`: CEF callback `cont` or `cancel` |
| `OpenDevTools` (`:129`) | `pane, devtools, viewport, inspect, dock` | `koyo/src/browser/view/lifecycle.rs:101-107` (open), `koyo/src/browser/view.rs:364-371` (`reveal`) | `host.rs:232-238` → `host/inspect.rs:15-108` |
| `ExitFullscreen` (`:69`) | `pane` | `koyo/src/browser/fullscreen.rs:136` | `host/panes.rs:295-299` |
| `Cdp` (`:148`) | `pane, call, method, params` | `koyo/src/browser/host.rs:288-313` (`send_call`) | `host.rs:239-244` → `host/inspect.rs:136-163` |

### Events back (`koyo-browser-ipc/src/events.rs`)

| Event | Fields | Sent in the host | Handled in the app |
| --- | --- | --- | --- |
| `Typed` (`:140`) | `pane` | `pane/keys.rs:35`, at most every 500 ms a pane (`keys.rs:64,73-85`) | `koyo/src/browser/host/events.rs:85` → `BrowserView::typed`, `koyo/src/browser/input.rs:43-45` → `person_acts(Implicit)` |
| `ContextMenu` (`:188`) | `pane, menu, x, y, entries, target` | `menus.rs:50-62` | `host/events.rs:118-136` → `BrowserView::context_menu`, `koyo/src/browser/menu.rs:219-241` |
| `DevTools` (`:160`) | `pane` (the DevTools pane), `ask: DevToolsAsk` | `host/inspect.rs:353-356` | `host/events.rs:92,143-150` → the inspected page's `devtools_asked`, `koyo/src/browser/dock.rs:247-262` |
| `Closed` (`:147`) | `pane` | `host/panes.rs:222` | `host/events.rs:88` → `BrowserView::closed` (`view.rs:336-345`), and DevTools leave their tab (`koyo/src/shell/browser_tabs.rs:148-150`) |
| `Fullscreen`, `Cursor`, `OpenUrl` (`:100,116,152`) | | `host/panes.rs:285`, `:271`; `pane.rs:299-302` | `host/events.rs:66,72,89` |

The wire has no field anywhere for input provenance, a window, a Space or a tab.

## 3. The DevTools path, end to end

**Asked.** ⌥⌘I, the context menu's Inspect or Show DevTools, or an agent, reach
`BrowserView::inspect` (`koyo/src/browser/dock.rs:150-171`), which sets `open_devtools`. The next
render runs `make_devtools` (`dock.rs:174-202`): a new `BrowserView::inspector(page, at, ...)`
entity (`koyo/src/browser/view.rs:183-198`, `Kind::DevTools { inspected, inspect }`) with a fresh
`PaneId` from `NEXT_PANE` (`view.rs:209`), stored in the page's `devtools: Option<Devtools { view,
dock }>` (`dock.rs:110-114,193-197`). With the setting `Detached` it emits `BrowserEvent::Detach`.

**Opened in the host.** The inspector's `ensure_open` sends `OpenDevTools { pane: page, devtools,
viewport, inspect, dock }` once the page is open (`lifecycle.rs:93-107`). The host
(`host/inspect.rs:15-108`) queues it on the page until the page's browser exists (`:23-39`),
refuses a second inspector (`:72-75`), starts a loopback WebSocket `Bridge` on an ephemeral port
with a token path (`devtools.rs:81-98`), observes the page's DevTools messages, records
`page.inspected = Inspected { devtools, session, side }` (`devtools/session.rs:24-32`) and
`devtools_pane.inspector = Some((page, bridge))` (`host/inspect.rs:97-104`), and makes the frontend
as a native page with no profile: `native::create(devtools, &url, viewport, None)` (`:107`), at
`devtools://devtools/bundled/devtools_app.html?ws=127.0.0.1:<port>/<token>&can_dock=true`
(`devtools/embedder.rs:15,28`). With no profile, `make` builds it with the DevTools client
(`native.rs:156-157`, `pane.rs:25`), whose `devtools://` popups go nowhere and whose links become
`OpenUrl` (`pane.rs:297-305`). CEF's own DevTools window is never used (no `show_dev_tools` call
anywhere in the host).

**Its views.** The DevTools pane gets its own holder window (`hold`), its own `Entry` in
`native::PAGES`, its own keyboard tracking, its own `Visible`, exactly as a page does. Nothing in
`native.rs` knows a pane is DevTools.

**Docked or detached.** Entirely app-side. Docked, the page's `body` draws the inspector entity
beside the page past Koyo's divider (`dock.rs:271-365`). Detached, the shell puts it in a pane of
its own to the right of the page's pane (`koyo/src/shell/browser_tabs.rs:232-251`), and takes it
out on `Undock` (`:146,255-264`). The side the frontend asks for (`DevToolsAsk::Dock`) is kept in
`Inspected::side` in the host (`host/inspect.rs:341-358`) and in the settings and
`Devtools::dock` in the app (`dock.rs:216-229`).

**Shown in a pane.** The inspector's render draws `page_element`, whose `PageSurface` keeps a
`Kept<NativePage>` under the element id `("browser-native-page", pane)` (`koyo/src/browser/page.rs:204-226`)
and calls `native::place` with its own container each prepaint. Undrawn for a frame, the keyed
state is released and `native::release` runs (`koyo/src/native/surface.rs:259-274`).

**Closed.** `Close` on the DevTools pane ends the frontend's session on the page at once and frees
`page.inspected` (`host/panes.rs:140-162`). A page that closes closes its DevTools too
(`host/panes.rs:207-225`).

**What ties DevTools to a tab, and who owns it.**

| State | Owner | Where |
| --- | --- | --- |
| `Pane.inspected: Option<Inspected { devtools, session, side }>` | host, on the page's pane | `koyo-browser-host/src/host.rs:62-63`, `devtools/session.rs:24-32` |
| `Pane.inspector: Option<(PaneId, Bridge)>` | host, on the DevTools pane | `host.rs:64-65` |
| `native::Entry { holder, views, shown_in, home, on_screen, size }` for the DevTools pane id | host's native layer | `native.rs:49-63` |
| `Kind::DevTools { inspected: Entity<BrowserView>, .. }` | app, the inspector view | `koyo/src/browser/view.rs:56-61` |
| `BrowserView.devtools: Option<Devtools { view, dock }>` | app, the page view | `view.rs:140`, `dock.rs:110-114` |
| the workspace pane holding the inspector when detached | app shell | `koyo/src/shell/browser_tabs.rs:232-251` |
| `Kept<NativePage>` keyed by element id and pane, one container each | app, GPUI element state | `koyo/src/browser/page.rs:213-219`, `koyo/src/native/surface.rs:259-274` |

No host state names a tab or a window. The views go to the container of the latest `place` call
for that pane id (`native.rs:285-300`); a `release` from any other container is ignored
(`native.rs:321`). So which tab shows a DevTools page is whichever drawn element last placed it.

**The "DevTools showing in another tab" class.** I found no path where a single frame leaves a
DevTools page's views in a container of a tab that isn't drawn. The candidates below are
[inferred] and unverified:

- Two drawn sites for one pane id in the same window (docked in the page's body and as a pane of
  its own, during a dock change) make two `NativePage`s with two containers. Each `place` moves the
  views (`same` is false each time, `native.rs:285-287`), so the views ping-pong to the last one
  prepainted, and the other's surface shows nothing.
- A detached DevTools pane is a workspace pane like any other, so it can be moved to another tab
  or window with the tab and pane tools while `page.devtools` still says `Detached`; it then shows
  in that other tab legitimately, and `Undock` removes it from wherever it is
  (`browser_tabs.rs:255-264`).
- The holder of a DevTools page follows its last `place` rectangle while `shown_in` is set
  (`native.rs:312-313`) and is parked only on `release`. Chromium's own child windows (bubbles,
  autofill) are placed relative to the holder (`native.rs:7`), so a stale holder rectangle would put
  them over another tab [guess].
- The probe's `pages` command (`native::describe`) prints each page's window, `drawn by a pane`,
  and `FITS`/`OFF` per page, which is the quickest check of any of these.

## 4. The context-menu path, end to end

**Into the page.** A right press reaches Chromium's `RenderWidgetHostViewCocoa` in one of three
ways, or not at all:

| Source | How it reaches the page | Noted by the gate's monitor? |
| --- | --- | --- |
| The person's right button | window server → `NSApp nextEvent` → local monitor (`koyo/src/native/menus/appkit.rs:130-165`) → `NSApp sendEvent:` (`koyo-browser-host/src/embedded/nsapp.rs:44-51`) → GPUI's `sendEvent:` (`vendor/gpui-fast-macos/src/fast/composition.rs:279-331`; `tell_press` first) → hit test → the page's view (the holder ignores the mouse, `holder.rs:63`) | yes, with its real source state and pid |
| `browser_click` with `button: right` | `koyo/src/control/exec/browser/acts.rs:169-231` → `Input.dispatchMouseEvent` over CDP (`acts.rs:281-290`) → host `inspect.rs:136` → `send_dev_tools_message`: a WebMouseEvent straight to the renderer, no `NSEvent` | no |
| The agent desktop's `desktop_click` right, into a test Koyo on the agent desktop | desk host `Poster::click` (`koyo-desk-host/src/input.rs:206-215`) → `CGEventPostToPid` → the same path as the person's | yes, as state −1 and the desk host's pid [guess for the pid, see section 5] |
| Probe `rclick`/`rdown` | `NSEvent mouseEventWithType:` handed to `[window sendEvent:]` (`koyo/src/browser/native/probe.rs:211-222,331-339`), skipping `NSApp` | no (`menus/appkit.rs:126-129`) |

**In the host.** Chromium asks CEF; `PaneMenus::on_before_context_menu` drops Find, Print and
Chrome's Inspect rows (`koyo-browser-host/src/pane.rs:136-145`, `menus.rs:13-39`), and
`run_context_menu` takes the menu (`pane.rs:147-160`): the callback is kept in `MENUS`
(`menus.rs:23-25,48`) and `HostEvent::ContextMenu { pane, menu, x, y, entries, target }` goes to the
app (`menus.rs:50-62`). It returns 0, which leaves the menu to Chromium, only when CEF gives no
params, model or callback (`pane.rs:155-157`) [read]; what Chromium does then is [guess]. The host
sees no event, no window and no input source here.

**In the app.** `BrowserView::context_menu` keeps it as `menu_request` and cancels an older one
(`koyo/src/browser/menu.rs:219-241`). The next render's `settle` calls `show_menu`
(`view.rs:459-461`, `menu.rs:244-276`), which asks `menu_gate` (`menu.rs:281-286`):

1. `control.agent()` is some agent → `Refusal::Agent`. A CDP click claims the page for its agent
   first (`koyo/src/browser/agent.rs:276-281`), so an agent's `browser_click` right always lands
   here. The person's press reaches `pressed_at` (`input.rs:23-38`, from the capture listener at
   `page.rs:230-237`) before the page asks for its menu, and takes the page back (`d33840cad`).
2. Else `menus::may_show(window, ...)` (`koyo/src/native/menus/appkit.rs:78-83`) → `verdict`
   (`:87-111`): the GPUI window's `NSWindow`, its `windowNumber`, `isOnActiveSpace && isVisible`,
   then `judge` (`menus.rs:168-192`), which reads that window number's last noted press or key and
   applies `decide` (`menus.rs:107-117`): not on screen → `NotOnScreen`; nothing in 5 s → `NoInput`;
   posted by a program (`classify`, `menus.rs:92-103`) → `Posted`; else shown.

Refused, the menu is cancelled for the page (`menu.rs:266-273`). Allowed, `NativeMenu::show`
pops an `NSMenu` up with `popUpMenuPositioningItem:atLocation:inView:`, which the gate's swizzle
judges a second time (`appkit.rs:230-245`). A pick dispatches `MenuPick` (`menu.rs:337-349`) →
`picked` → `person_acts(Implicit)` → `MenuCommand` (`menu.rs:296-322`).

**Where the gate decides, and what it can't see.**

- It decides per window, not per event. The question it answers is "was the last press or key in
  this window, within 5 s, the hardware's?" A CDP click or a probe press leaves no note, so after
  the person pressed anywhere in that window in the last 5 s their menus would be judged as the
  person's [inferred]. The page's lease covers CDP; nothing covers the probe's `rclick`.
- An `NSEvent` with no `CGEvent` is noted as Koyo's own (`appkit.rs:142-147`). Whether `-[NSEvent
  CGEvent]` is null for a synthesized event, and what a synthesized event's source fields read, are
  [guess]; the probe's `escape` goes through `postEvent:atStart:` and the monitor
  (`probe.rs:289-315`).
- Before the monitor is installed (`WATCHING` false) only the screen decides (`menus.rs:172-176`).
- In a debug build, a menu that begins tracking without the gate's leave is cancelled
  (`appkit.rs:320-363`).

**Chromium's own menus.**

| Menu | Who shows it | Does the host see it? | Does the gate? |
| --- | --- | --- | --- |
| A page's context menu | the app, from `HostEvent::ContextMenu` | yes, as CEF's model | yes, twice |
| A `<select>`'s options | Chromium's browser process, `WebMenuRunner` via `-[NSPopUpButtonCell performClickWithFrame:inView:]` | no | yes (`appkit.rs:64-69,286-301`) |
| Chromium's own context menu (CEF falling back) | `+[NSMenu popUpContextMenu:withEvent:forView:]` and `...withFont:` | no | yes (`appkit.rs:52-63,249-281`) |
| DevTools' right-click menus | the frontend's `DevToolsHost`, through the page's ordinary context-menu path [guess], so `HostEvent::ContextMenu` for the DevTools pane, shown with no page rows (`menu.rs:250-253`) | yes [guess] | yes |
| DevTools' "⋮" menu | an HTML menu inside the frontend [guess] | no | not a native menu |
| Bubbles, autofill and permission prompts of Chrome style | views widgets in `NSWindow`s of Chromium's [guess] | no | no, they aren't menus |

## 5. The three agent input paths

### A. CDP: `browser_click`, `browser_type`, `browser_press_key`, `browser_scroll`

- **Enters.** The control listener (`koyo/src/control/listener.rs:81,120`) → dispatch
  (`koyo/src/control/exec/mod.rs:116-118`) → `acts::click` (`koyo/src/control/exec/browser/acts.rs:169`)
  → `page::call` (`koyo/src/control/exec/browser/page.rs:93-146`) → `agent::cdp`
  (`koyo/src/browser/agent.rs:267-290`), which claims the page's lease for the actor and keeps it
  awake 90 s → `Browsers::send_call` (`koyo/src/browser/host.rs:288-313`) → `AppRequest::Cdp` →
  host `inspect::cdp` (`koyo-browser-host/src/host/inspect.rs:136-163`) → `send_in_session`
  (`:187-220`) → `BrowserHost::send_dev_tools_message`. The tool definitions are
  `koyo-app-control/src/ops/browser/act.rs:206-233`.
- **Form there.** JSON: `Input.dispatchMouseEvent {type: mouseMoved|mousePressed|mouseReleased, x,
  y, button, clickCount}` (`acts.rs:215-226`), `Input.dispatchKeyEvent` (`acts.rs:292-302`,
  `browser/keys.rs:66`), `Input.insertText` (`acts.rs:326-333`).
- **What tells it apart.** There is no `NSEvent` or `CGEvent` at any point: it never passes `NSApp`,
  the local monitor, GPUI's `sendEvent:`, `tell_press` or `pressed_at`. CEF's keyboard handler gets
  `os_event` null for its keys, so no `Typed` (`pane/keys.rs:23-25,34`, the code's own claim, and a
  key left unhandled is not offered as a shortcut, `native.rs:415-417`). The app also knows by the
  lease (`control.agent()`).

### B. The agent desktop: `CGEventPostToPid` from the desk host

- **Reaches Koyo only as an app on the agent desktop**, a test Koyo (`docs/browser.md` 14). The
  desk host targets only the agent's windows (`koyo-desk-host/src/acts.rs:35-44`). The person's own
  Koyo draws the desktop's picture and sends tab input down the link; it receives no posted events
  [inferred].
- **Created and posted** (`koyo-desk-host/src/input.rs`):

```rust
// input.rs:97-100: one private source for everything the host posts
let source = ns::owned(unsafe { sys::CGEventSourceCreate(sys::SOURCE_PRIVATE) })?; // -1

// input.rs:136-152: a mouse event
let Some(event) =
    ns::owned(unsafe { sys::CGEventCreateMouseEvent(self.source(), kind, cg(at), button) })
else { return; };
sys::CGEventSetIntegerValueField(ns::raw(&event), field::CLICK_STATE, i64::from(clicks.max(1)));
sys::CGEventSetFlags(ns::raw(&event), flags);
Poster::post(target, &event, true);

// input.rs:108-131: stamped for its window, posted to its process alone
sys::CGEventSetIntegerValueField(event, field::WINDOW_NUMBER, window);          // 51
if pointed {
    sys::CGEventSetIntegerValueField(event, field::WINDOW_UNDER_POINTER, window); // 91
    sys::CGEventSetIntegerValueField(event, field::WINDOW_THAT_CAN_HANDLE, window); // 92
    if let Some(set) = SET_WINDOW_LOCATION.function::<SetWindowLocation>() { /* window-local point */ }
}
sys::CGEventPostToPid(pid, event);
```

  Keys are `CGEventCreateKeyboardEvent(source, code, down)` with flags (`input.rs:286-296`), text is
  keycode 0 with `CGEventKeyboardSetUnicodeString` and flags 0 (`input.rs:324-346`), scrolls are
  `CGEventCreateScrollWheelEvent2` with phases (`input.rs:235-263`). Once per app the host posts an
  AppKit-defined "application activated" event (`NSEvent otherEventWithType:13 ... subtype:1`, its
  `CGEvent` posted to the pid, `input.rs:157-184`) so the app thinks it's active and its window key
  [read]; in a test Koyo that would reach `NSApplicationDidBecomeActive` and the keyboard's
  `hooks::active(true)` (`koyo/src/keyboard/appkit.rs:306`) [guess].
- **Form in Koyo's process.** A real `NSEvent` from `nextEventMatchingMask:` whose `CGEvent` has:
  `windowNumber` from field 51; the window-local point; `clickCount` from field 1; the flags set;
  source state ID −1 [read: the source is made private]; source pid (field 41) the desk host's
  [guess: the gate depends on it, `docs/browser.md` 14.4 still `[open]`]; user data (field 42) 0
  [read: nothing sets it]; timestamp and event number as `CGEventCreate*` and the window server
  give them [guess].
- **The person's own input on the agent desktop is posted the same way.** `Host::person`
  (`koyo-desk-host/src/acts.rs:374-424`) relays the tab's input through the same `self.poster`. At
  the receiver nothing tells the two apart.
- **Accessibility acts post nothing.** A plain left click on a snapshot ref is `AXPress`
  (`acts.rs:192-218`): no event exists, so no gate note and no `tell_press` [read]; what an AX press
  does to a GPUI view is [guess].
- **Can it be told apart from HID in-process?** By source, yes, if the fields arrive as posted:
  `CGEventGetIntegerValueField(cg, kCGEventSourceStateID = 45)` is −1 for a private source and the
  hardware's is not; `kCGEventSourceUnixProcessID = 41` names the poster. The gate already reads
  both, in the local monitor only (`koyo/src/native/menus/appkit.rs:21-28,140-153`); nothing else
  in Koyo does, and the host never does (CEF hands it the `NSEvent` for keys, `pane/keys.rs:30`, and
  no mouse event at all). By intent (person relayed vs agent), no: there is no tag. The desk host
  could stamp one per event with `CGEventSetIntegerValueField(event, 42, tag)` before
  `CGEventPostToPid`, or keep two sources and `CGEventSourceSetUserData` on each. Whether field 42
  survives `CGEventPostToPid` into the receiver's `-[NSEvent CGEvent]` is [guess] and needs one
  probe run.

### C. The probe: `KOYO_NATIVE_PROBE`

- **Exists in debug builds only** (`koyo/src/browser/native/mod.rs:23-24`). It polls
  `<folder>/cmd` every 100 ms on GPUI's executor (`probe.rs:107-151`) and acts on the first visible
  GPUI window (`probe.rs:160-168`).
- **Form.** Mouse: `+[NSEvent mouseEventWithType:location:modifierFlags:0 timestamp:systemUptime
  windowNumber:<the window's> context:nil eventNumber:0 clickCount:n pressure:1|0]` handed to
  `-[NSWindow sendEvent:]` (`probe.rs:331-357`). Keys: `keyEventWithType:10/11 ... windowNumber
  keyCode`, offered first to the window's and the main menu's `performKeyEquivalent:`, then
  `sendEvent:` (`probe.rs:382-437`). Scroll: `CGEventCreateScrollWheelEvent2(NULL source)` with
  fields 91/92 set, made an `NSEvent` and sent to the window (`probe.rs:359-378`). Escape:
  `postEvent:atStart:YES` into the app's queue (`probe.rs:289-315`). `activate` calls
  `activateIgnoringOtherApps:YES` (`probe.rs:278-285`), which the host's hook passes unless a page
  window is being shown.
- **What tells it apart.** `eventNumber` 0, and it never passes `NSApp`'s queue or the local monitor
  (except `escape`). From GPUI's `sendEvent:` on it is the person's to every heuristic: `tell_press`,
  `pressed_at`, and `Typed` (its keys have an `NSEvent`). Its synthesized events' `CGEvent` source
  fields are [guess].

## 6. Every heuristic that answers "whose input is this?"

| Where | Heuristic | What it gates |
| --- | --- | --- |
| `koyo-browser-host/src/pane/keys.rs:23-36` | a `RAWKEYDOWN` with CEF's `os_event` non-null is the person's; at most every 500 ms (`:64,73-85`) | `HostEvent::Typed` → `person_acts(Implicit)`: the page is the person's, agents refused for 5 s |
| `koyo-browser-host/src/native.rs:409-417` | a page's unhandled key is the app's only while a pane shows the page (`shown_in`), and only with a real `NSEvent` | `performKeyEquivalent:` on GPUI's view |
| `koyo-browser-host/src/embedded/nsapp.rs:53-70` | an activation asked while a page window is being shown is Chromium's, not the person's | `activate`, `activateIgnoringOtherApps:` |
| `koyo-browser-host/src/native/holder.rs:63,114-154` | no press, key or main status is ever the holder's | the pointer, key and main windows |
| `koyo/src/native/menus.rs:92-103` (`classify`) | state −1 → a program; pid = desk host → the desk host; pid = Koyo → Koyo; else the person | native menus |
| `koyo/src/native/menus.rs:107-117,168-192` (`decide`, `judge`) | the window's last noted press or key within 5 s, on the active Space and visible | native menus |
| `koyo/src/native/menus/appkit.rs:142-147` | an `NSEvent` with no `CGEvent` was made in this process | native menus |
| `koyo/src/browser/menu.rs:281-286` (`menu_gate`) | a page an agent holds asked for its menu because of the agent | a page's context menu (`Refusal::Agent`) |
| `koyo/src/browser/control.rs:106-140,189-219` | the person's acts make `Holder::Human`; `Implicit` lapses after `IMPLICIT_HOLD` 5 s, `Explicit` holds | agents' claims refused with `PersonHasControl` (`:108-111`) |
| `koyo/src/browser/input.rs:23-38,43-45,47-51` | any press GPUI's capture phase sees over the page, `Typed`, and GPUI's edit actions are the person's | `person_acts(Implicit)` |
| `koyo/src/browser/menu.rs:296-306` | a pick from a shown menu is the person's | `person_acts(Implicit)` |
| `koyo/src/browser/agent.rs:276-281` | a CDP command with an actor is that agent's | lease claim; refused while the person holds the page |
| `vendor/gpui-fast-macos/src/fast/composition.rs:274-279` (`tell_press`) | "the press is the person's", with no check of its source | `koyo/src/keyboard/appkit.rs:194-204` → `hooks::pressed`: where the keyboard goes |
| `koyo/src/agent_desktop/mod.rs:441-454` (`Desk::input`) | input from the desktop's tab is the person's | `person_acts(Implicit)` on the desktop: agents' acts refused (`koyo/src/agent_desktop/control.rs:31-62`) |
| `koyo-desk-host/src/acts.rs:332-337` | the person's tab input stops an agent's typing | `RefusalCode::PersonActing` |
| `koyo-desk-host/src/keyboard.rs:27-30,82-86` | a key from `kCGEventSourceStateHIDSystemState` since a menu opened is the person's ("no event of the host's own is among" them) | closes an agent's menu that took the keyboard |
| `koyo-desk-host/src/keyboard.rs:22,116` | a menu opened within 1.5 s of the person's press in the tab (`lively`, `acts.rs:393,407,416`) is the person's | leaves that menu up |

"Person has control" is the same lease type for a page (`koyo/src/browser/control.rs`) and for the
agent desktop (`koyo/src/agent_desktop/control.rs:1-7,28-62`, which reuses `browser::control::Control`).
Neither looks at an event's source; both trust where the input arrived.

## 7. What the engine test suites run, and what they see of the seam

**`koyo-browser-host/tests/smoke`** (9 tests, all `#[ignore]`, need a packaged engine,
`tests/smoke/main.rs:1-7`). The host runs as a process of its own (`mac_app.rs`, an `Accessory`
app), its pages in holders parked off the screen, talked to over the socket. They cover a page at
its size, frames while `Visible` and none while hidden, `Resize` while unshown (`main.rs:31-86`);
DevTools as a pane of their own, a second refused, close and reopen, closing with their page
(`main.rs:87-137`); full screen and back with a timely quit (`main.rs:139-171`); a stuck host
giving way (`main.rs:173-196`); CDP beside DevTools (`agents.rs:8-96`); passkeys, location and media.
They can't see: `native::place` or `release` (no app view exists), the holder over a pane, whole
points, the first responder, presses, any `NSEvent`, menus (no `ContextMenu` is asked for), the
embedded `NSApp` patches, activation, or Spaces.

**`koyo/src/browser/host_tests`** (20 tests, all `#[ignore]`, `host_tests/mod.rs:1-8`). A
`BrowserView` in a GPUI test window against the real host, again as its own process. Native views
are compiled out under test (`koyo/src/browser/page.rs:48,124,155,204` are
`cfg(all(target_os = "macos", not(test)))`), and `Surface::new` fails in a test window
(`koyo/src/native/surface.rs:50-52`). They see the IPC and the app's logic: `Visible` when a tab
is hidden and shown (`keeping.rs:38-100`), an agent's CDP click and Enter landing
(`keeping.rs:141-169`), GPUI focus after the address bar (`input.rs:45-62`), full screen
(`input.rs:69-95`), dialogs from an agent's click (`dialogs.rs:118`). They can't see any `NSView`,
`NSWindow`, holder, first responder, AppKit event or native menu; `may_show` passes every menu for a
window with no `NSWindow` (`koyo/src/native/menus/appkit.rs:76-80`).

**Unit tests in the host** (`koyo-browser-host/src`, 42 `#[test]`s): one on the seam,
`native/tests.rs:32-42` (a closing page tells the keyboard `Leaving` while it's still listed).
The rest are menus' mnemonics, DevTools' address, shim and session, passkeys, profiles and site
settings. The menu gate's tests are in the app (`koyo/src/native/menus/tests.rs`, 10) and the
page's half in `koyo/src/browser/view/tests.rs:261`.

**What sees the seam today** is only the probe, by hand, in a debug Koyo: `pages`, `views`,
`layers`, `keyboard`, `state`, `windows`, `shot` (`probe.rs:8-46`). No automated suite places a
native view in an app window.
