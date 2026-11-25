# The app crate's side of the native seam: an inventory

Read-only inventory of `koyo-rs/koyo/src` for `docs/native-layer.md`, taken 2026-11-16 at
`ce2f3c7fc` on `schulzfel/browser-ui-fixes`. Paths are relative to `koyo-rs/koyo/` unless they
start with `vendor/` or `koyo-browser-host/` (relative to `koyo-rs/`). Nothing was built or run.
Every fact is `[read]` from source unless it carries `[inferred]`.

Concerns: (1) keyboard and first responder, (2) mouse press ownership and capture, (3) menus,
(4) windows and panels (Space, level, key window, parent and child, `hidesOnDeactivate`,
collection behaviour, ordering, style mask), (5) geometry (frames, whole points, transactions,
presentation), (6) native view lifecycle (place, park, release, `addSubview`,
`removeFromSuperview`), (0) not the seam. `0*` means not the seam, but the code creates a native
`NSView` inside one of GPUI's windows: such a view can take the first responder or a press, so it
is a seam citizen.

## 1. Every direct touch of AppKit, CoreGraphics or the ObjC runtime

One row per function or tight group of functions. The concern column holds the primary concern;
secondary ones are named in the text. Rows are grouped by file; the line count after each file
name is from the command in section 2.

### keyboard/appkit.rs (37 code lines; macOS, not in tests)

| Site | Function | What it does | Concern |
| --- | --- | --- | --- |
| `src/keyboard/appkit.rs:47-63` | `know` | Learns a GPUI window's `NSWindow` and GPUI's `NSView` (via `native::window::gpui_view`), stored as `Weak`s under GPUI's window id. | 1 |
| `src/keyboard/appkit.rs:66-83` | `known`, `key_of` | Loads the weak pair; maps an `NSWindow` pointer back to a GPUI window id by a linear search of `KNOWN`. | 1 |
| `src/keyboard/appkit.rs:86-107` | `page_of`, `classify` | `isKindOfClass: NSView`, then the host's `native::page_of`; names a first responder as GPUI's view, a page, the window or another view. | 1 |
| `src/keyboard/appkit.rs:110-113` | `first_of` | `[window firstResponder]`. | 1 |
| `src/keyboard/appkit.rs:118-142` | `Real::first_responder`, `Real::make_first_responder` | The app's only `makeFirstResponder:` calls: 131 GPUI's view, 137 the page's keys view from the host's `key_view` (only if it is in this window), 139 nil. | 1 |
| `src/keyboard/appkit.rs:144-166` | `Real::page_window`, `Real::has_marked_text` | Which known window a page's views are in (host `key_view`); `respondsToSelector: hasMarkedText` then `hasMarkedText` on the first responder. | 1 |
| `src/keyboard/appkit.rs:168-176` | `Real::make_key`, `Real::is_key` | `makeKeyWindow`; `isKeyWindow`. | 4 |
| `src/keyboard/appkit.rs:183-215` | `impl Responders for Told` | The vendored window's hooks: `changed` (every `makeFirstResponder:`), `pressed` (every press before it is handed on, with the native view hit, so concern 2 too), `key` (every key before delivery), `gpui_first` (who gets a key equivalent first). | 1 |
| `src/keyboard/appkit.rs:220-232` | `describe` (debug) | `NSApplication isActive`, each known window's holder, first responder and key state, for the probe's `keyboard`. | 1 |
| `src/keyboard/appkit.rs:235-247` | `observe` | `NSNotificationCenter addObserverForName:object:queue:usingBlock:`; observers kept for the app's life. | 1 |
| `src/keyboard/appkit.rs:250-255` | `over` | Records a floating window's `NSWindow` pointer (as `usize`) as belonging to a GPUI window id. | 4 |
| `src/keyboard/appkit.rs:260-277` | `closing` | On `NSWindowWillCloseNotification`: forgets a known window; else finds the owner of a floating window or a Chromium window's `parentWindow`, and passes its `isKeyWindow`. | 4 |
| `src/keyboard/appkit.rs:281-310` | `install` | Installs `Real` (`hooks::install`), `Told` (`set_responders`), the host's `set_keyboard` callback (views leaving or arrived, concern 6), and five notifications: menu begin and end tracking (3), app active and inactive (4), window will close (4). | 1 |

### native/menus/appkit.rs (75; macOS, compiled in tests)

| Site | Function | What it does | Concern |
| --- | --- | --- | --- |
| `src/native/menus/appkit.rs:21-28` | extern block | `CGEventGetIntegerValueField`; fields 41 (`kCGEventSourceUnixProcessID`) and 45 (`kCGEventSourceStateID`). | 3 |
| `src/native/menus/appkit.rs:32-74` | `install` | Once per process: starts `watch`, swizzles `-[NSMenu popUpMenuPositioningItem:atLocation:inView:]`, `+[NSMenu popUpContextMenu:withEvent:forView:]`, `+[NSMenu popUpContextMenu:withEvent:forView:withFont:]`, `-[NSPopUpButtonCell performClickWithFrame:inView:]`; debug builds add `net`. | 3 |
| `src/native/menus/appkit.rs:78-83` | `may_show` | GPUI window to `NSWindow`, then `verdict`; `Ok` for a test window, which has no `NSWindow`. | 3 |
| `src/native/menus/appkit.rs:87-111` | `verdict` | A nil view uses the latest input's window (`windowWithWindowNumber:`); reads `windowNumber`, `isOnActiveSpace`, `isVisible` (concern 4 too). | 3 |
| `src/native/menus/appkit.rs:113-124` | `app`, `window_of` | `sharedApplication`; `[view window]`. | 3 |
| `src/native/menus/appkit.rs:130-165` | `watch` | `+[NSEvent addLocalMonitorForEventsMatchingMask:handler:]` on left, right and other mouse down and key down; notes window number, CGEvent source state and posting pid; an `NSEvent` with no `CGEvent` is noted as posted by Koyo's own pid. | 3 |
| `src/native/menus/appkit.rs:167-226` | `as_imp`, `before`, `wrap` | `class_addMethod`, else `method_setImplementation`, keeping the old IMP in a `OnceLock`. | 3 |
| `src/native/menus/appkit.rs:230-301` | `positioning`, `context`, `context_font`, `pop_up` | The four hooks. Each asks `verdict` for its view's window; refused returns NO or nothing; allowed records `let_show` and calls the old IMP. | 3 |
| `src/native/menus/appkit.rs:303-315` | `let_show` | Keeps the last 8 menus the gate let show. | 3 |
| `src/native/menus/appkit.rs:320-363` | `net` (debug) | A second `NSMenuDidBeginTrackingNotification` observer: walks `supermenu`, compares with `mainMenu`, and sends `cancelTrackingWithoutAnimation` to a menu the gate did not let show. | 3 |

### native/window.rs (44)

| Site | Function | What it does | Concern |
| --- | --- | --- | --- |
| `src/native/window.rs:22-62` | `fit` | Sets `layerContentsPlacement` to the kept corner, then (deferred) `setFrame:display:` for the new content size and `layer.display` so the frame presents in the resize's transaction. | 5 |
| `src/native/window.rs:71-90` | `draw_in_transaction` (not in tests) | Deferred `layer.display` on GPUI's view so GPUI's frame presents in the transaction a native view's move commits in; a thread-local flag folds repeated asks into one draw. | 5 |
| `src/native/window.rs:99-181` | `callout`, `effect_view` | Deferred: clears Titled, closable, miniaturizable, resizable and full-size-content from `styleMask`, clear background, `setOpaque(false)`, draws an `NSImage` mask with `NSBezierPath` and sets it on the window's `NSVisualEffectView`, `invalidateShadow`. | 4 |
| `src/native/window.rs:188-206` | `refresh_edge` | `invalidateShadow` at 100, 400 and 1000 ms. | 0 |
| `src/native/window.rs:209-213` | `show` | `orderFront:` without making the window key. | 4 |
| `src/native/window.rs:219-226` | `float_on_its_space` | `setCollectionBehavior(Managed + FullScreenAuxiliary)` and `setHidesOnDeactivate(true)`. | 4 |
| `src/native/window.rs:229-243` | `ns_window`, `gpui_view` | GPUI's `NSView` from `raw-window-handle`'s AppKit handle, and that view's `window`. Every app module that needs GPUI's `NSWindow` goes through here (section 3). | 4 |

### native/surface.rs (18)

| Site | Function | What it does | Concern |
| --- | --- | --- | --- |
| `src/native/surface.rs:53-81` | `Surface::new` | GPUI's `enable_window_composition` and `create_native_surface`; the container `NSView` from the platform handle; `addSubview:` of the view. | 6 |
| `src/native/surface.rs:86-94` | `Surface::mark` | `gpui_apple::set_surface_pane` on a 1-pixel buffer painted as a GPUI surface, so a capture of the window fills the native view's place. | 5 |
| `src/native/surface.rs:106-144` | `Surface::place` | Surface `set_bounds` in device pixels, visibility, then the view's `setFrame:` inside it; nothing is told when nothing changed. | 5 |
| `src/native/surface.rs:150-172` | `Surface::round` | Layer `cornerRadius`, `maskedCorners`, `masksToBounds`. | 5 |
| `src/native/surface.rs:175-182` | `Surface::show` | Surface `set_visible`. | 6 |
| `src/native/surface.rs:186-204` | `Surface::remove` | `removeFromSuperview` at once; `remove_surface` deferred to when the window is free. | 6 |
| `src/native/surface.rs:208-220` | `marker` | `CVPixelBufferCreate`, 1 by 1. | 0 |
| `src/native/surface.rs:230-246` | `on_whole_points` | Pure: edges at the clip go outwards to whole points, edges inside it to the nearest. | 5 |
| `src/native/surface.rs:249-274` | `Hosted`, `Kept`, `keep` | Keyed GPUI state holding a native view; on release `into_surface().remove()`. | 6 |

### native/host.rs (6), native/target.rs (12)

| Site | Function | What it does | Concern |
| --- | --- | --- | --- |
| `src/native/host.rs:40-74` | `host` | Keyed `Hosted<C>`; in `on_children_prepainted` inserts a hitbox and places the control's surface. The control is an `NSControl` in GPUI's window. | 6 |
| `src/native/host.rs:95-145` | `Hosted::new`, `Native::new`, `Native::remove` | `control.make`, `sizeToFit`, `Surface::new`, `Target::attach`; remove detaches the target, then the surface. | 6 |
| `src/native/target.rs:9-54` | `Target`, `attach`, `detach` | `define_class!` `KoyoNativeTarget`; `setTarget:` and `setAction:` on an `NSControl`. | 0* |

### native: glass, controls and chrome

| Site | Function | What it does | Concern |
| --- | --- | --- | --- |
| `src/native/liquid.rs:116-158` | `glass` | `surface::keep` of an `NSGlassEffectView` holder, placed from a paint-time `canvas`, not prepaint, so it stays under GPUI's overlays. | 0* |
| `src/native/liquid.rs:188-230` | `has_glass`, `new_glass`, `appear`, `glass_in` | `NSGlassEffectView`, private `set_adaptiveAppearance:`, appearance, `addSubview:`. | 0* |
| `src/native/liquid.rs:239-313` | `Native::new`, `mark`, `place`, `end_at` | Holder `NSView` and glass in a `Surface`; style, tint, corner radius; `removeFromSuperview` of the old glass on a theme flip. | 0* |
| `src/native/glass.rs:20-99` | `init`, `Glass`, `pane`, `Native` | `NSVisualEffectView` (menu material) in a `Surface` behind gpui-component's popovers (`set_popover_backdrop`). | 0* |
| `src/native/window_glass.rs:29-64` | `fill` | `NSGlassEffectView` added under GPUI's view in a floating window's content view (`addSubview:positioned:relativeTo:`). | 0* |
| `src/native/window_glass.rs:73-79` | `borderless` | Deferred `setStyleMask(Borderless + NonactivatingPanel)` and `setHasShadow(true)` on the tab switcher's panel. | 4 |
| `src/native/traffic_lights.rs:21-54` | `fade` | `standardWindowButton` alpha and `setHidden`, left alone in full screen. | 0 |
| `src/native/slider.rs:9-30` | `over_stops`, `stop`, `show_stop` | Builds and reads an `NSSlider` with tick marks. | 0* |
| `src/native/switch.rs:6-20` | `is_on`, `show` | Reads and sets an `NSSwitch`. | 0* |
| `src/native/notifications.rs:20-145` | `Clicks`, `clicks`, `post` | `UNUserNotificationCenter` delegate and requests. | 0 |
| `src/native/icon.rs:11-20` | `set_dock_icon` | `setApplicationIconImage:`. | 0 |
| `src/toggle.rs:85-125` | `switch`, `Flips: Control` | An `NSSwitch` hosted by `native::host` in settings pages. | 0* |
| `src/menu/switch.rs:37-90` | `RowSwitch: Control` | A small `NSSwitch` in a menu row, in a floating menu window. | 0* |
| `src/chat/composer/effort.rs:46-68` | `EffortSlider: Control` | An `NSSlider` in the chat composer, in the main window (hosted at line 117). | 0* |
| `src/settings_view/native_slider/appkit.rs:13-35` | `Stops: Control` | An `NSSlider` in settings. | 0* |

### browser/native/mod.rs (8; macOS, not in tests)

| Site | Function | What it does | Concern |
| --- | --- | --- | --- |
| `src/browser/native/mod.rs:31-42` | `say_when_forked` (debug) | `pthread_atfork` handler that logs a fork. | 0 |
| `src/browser/native/mod.rs:84-89` | `NativePage::into_surface` | The host's `native::release(pane, container)` before the surface goes. | 6 |
| `src/browser/native/mod.rs:93-105` | `NativePage::new` | Layer-backed container `NSView` in a `Surface`. | 6 |
| `src/browser/native/mod.rs:118-158` | `NativePage::place` | `on_whole_points`, `Surface::place`, card corners, resize time; GPUI view to window (`convertRect:toView:nil`) to screen (`convertRectToScreen:`); the host's `native::place(pane, container, home, screen)`, which moves the page's views and its holder window (concern 6 too). | 5 |
| `src/browser/native/mod.rs:161-163` | `NativePage::resizing` | Whether the size changed in the last 250 ms. | 5 |

### browser/native/probe.rs (71), probe/look.rs (45), probe/keys.rs (2); debug builds only

| Site | Function | What it does | Concern |
| --- | --- | --- | --- |
| `src/browser/native/probe.rs:74-105` | extern blocks | `CGWindowListCreateImage`, `CGEventCreateScrollWheelEvent2`, `CGEventSetIntegerValueField`, ImageIO writers, `CFRelease`. | 0 |
| `src/browser/native/probe.rs:161-172` | `window`, `class_name` | The first visible `NSApp.windows` entry whose class name starts with `GPUI`; class names through the host's `class_of`. | 4 |
| `src/browser/native/probe.rs:189-194` | `run` `front`, `activate` | `orderFrontRegardless`; `activateIgnoringOtherApps(true)` and `makeKeyAndOrderFront:` (lines 278-285). | 4 |
| `src/browser/native/probe.rs:287-315` | `escape` | An Escape `NSEvent` put in the app's queue with `postEvent:atStart:`, which a tracking menu reads. | 3 |
| `src/browser/native/probe.rs:200-228` | `run` `click`, `rclick`, `rdown`, `move`, `scroll` | Mouse `NSEvent`s and a wheel built from a `CGEvent`, handed to `-[NSWindow sendEvent:]` (helpers `mouse`, `mouse_event`, `scroll` at 331-378). | 2 |
| `src/browser/native/probe.rs:229-254` | `run` `type`, `key` | Key down and up `NSEvent`s; with `key`, offered first to the window's then the main menu's `performKeyEquivalent:`, else `sendEvent:` (helper `key` at 382-437). | 1 |
| `src/browser/native/probe.rs:269-272` | `run` `frame` | `setFrame:display:` on the window. | 5 |
| `src/browser/native/probe.rs:440-475` | `shot` | The window's picture as a PNG. | 0 |
| `src/browser/native/probe/keys.rs:36-39` | `make_key` | `makeKeyWindow` without activating the app. | 4 |
| `src/browser/native/probe/look.rs:12-62` | `windows`, `state` | Reads number, class, title, frame, visible, key, occlusion, alpha, content class, `ignoresMouseEvents`, level, `orderedIndex`; first responder class, key, main, occlusion, app active. | 4 |
| `src/browser/native/probe/look.rs:64-80` | `views` | The view tree with frames and hidden state. | 6 |
| `src/browser/native/probe/look.rs:193-247` | `layers`, `layer_tree` | The `CALayer` tree under GPUI's view: frame, hidden, opacity, contents, background, delegate. | 5 |
| `src/browser/native/probe/look.rs:116-133` | `draw_unseen` | `layer.display` on GPUI's view of each visible, occluded window (the `tick` loop). | 5 |
| `src/browser/native/probe/look.rs:164-175` | `draw` | `layer.display` on GPUI's view, found by class name `GPUIView`. | 5 |
| `src/browser/native/probe/look.rs:137-160` | `hover` | `hitTest:`, then `mouseMoved:` to GPUI's view and to the view hit. | 2 |
| `src/browser/native/probe/look.rs:278-305` | `drag` | A left press, moves a pause apart, and a release, through `sendEvent:`. | 2 |
| `src/browser/native/probe/look.rs:84-112` | `cursor` | `NSCursor currentCursor` against six shared cursors; window origin on screen. | 0 |
| `src/browser/native/probe/look.rs:178-189` | `quit` | `terminate:`. | 0 |
| `src/browser/native/probe/look.rs:250-254` | `numbered` | The app's window with a given `windowNumber`. | 4 |

### browser: share picker and passkeys

| Site | Function | What it does | Concern |
| --- | --- | --- | --- |
| `src/browser/site.rs:198-231` | `share` | `NSSharingServicePicker showRelativeToRect:ofView:preferredEdge:` from GPUI's view, deferred. A system picker from a GPUI button press, with no call to `menus::may_show`. Whether it pops up through one of the four wrapped methods is `[inferred: unknown]`. | 3 |
| `src/browser/passkeys/sheet.rs:35-118` | `Sheet::open`, `Shown::perform` | Runs an `ASAuthorizationController` over Koyo's `NSWindow`. | 4 |
| `src/browser/passkeys/sheet/delegate.rs:31-215` | `Delegate`, `answer_of` | `presentationAnchorForAuthorizationController:` (75-82) returns Koyo's `NSWindow`: the system sheet is a window over it that takes the keys. The rest turns the credential into the page's answer. | 4 |
| `src/browser/passkeys/sheet/requests.rs:1-358` | request builders | AuthenticationServices requests. | 0 |
| `src/browser/passkeys/sheet/tests.rs:8-360` | tests | AuthenticationServices request checks. | 0 |

### Not the seam

| Site | Function | What it does | Concern |
| --- | --- | --- | --- |
| `src/devices/view.rs:334-375` | `watch_trackpad` | A second process-wide `addLocalMonitorForEventsMatchingMask:` (rotate, smart magnify, scroll phase) for the simulator view. Device gestures. | 0 |
| `src/theme/system_accent.rs:23-100` | `follow`, `take_up`, `read`, `under` | Accent colour under each appearance; a notification observer. | 0 |
| `src/picker.rs:989-999` | haptic tick | `NSHapticFeedbackManager` through `objc` 0.2. | 0 |
| `src/viewer/bitmap.rs:11-138` | bitmap context | `CGBitmapContext` for the file viewer. | 0 |
| `src/viewer/pdf.rs:10-123` | PDF pages | `CGPDFDocument`, `CGPDFPage`. | 0 |
| `src/viewer/picture.rs:12-353` | image decode | ImageIO `CGImageSource`. | 0 |
| `src/settings_view/native_visual.rs:45-414` | `run` and helpers | Visual check (feature `visual-check`): opens the shell, `cx.activate(true)`, finds `NSSlider` and `NSSwitch` views, `sendAction:to:`. | 0 |
| `src/shell/switcher/visual.rs:96-98` | panel check | Visual check (feature `visual-check`): `setHidesOnDeactivate(false)` on the switcher's panel. | 0 |
| `src/shell/switcher/visual.rs:225-324` | `present`, `seen`, `made`, `native` | Visual check: `displayLayer:`, `isVisible`, `isKeyWindow`, `styleMask`, glass presence. | 0 |

### 1b. Seam code with no direct AppKit call

These call the modules above, or decide seam behaviour in GPUI terms. They are not in the counts.

| Site | What it does | Concern |
| --- | --- | --- |
| src/popup.rs:119-133 | Every floating window opens as `WindowKind::PopUp`, key only for a dialog (`focus: kind.is_task()`), unseen for the switcher, on the parent's display. | 4 |
| src/popup.rs:155-156, 188 | Inside `open_window`: `float_on_its_space`, `keyboard::opened_over`; a dialog holds `keyboard::DialogOver`. | 4 |
| src/popup.rs:200, 343, 365 | `native::window::show`, `fit`, `refresh_edge`. | 4 |
| src/popup.rs:256-277 | `listener` and `pressed`: a GPUI capture-phase press anywhere in the parent closes its floating windows. | 2 |
| src/popup.rs:307-326 | `settle`: 80 ms after an activation change, closes a popup when neither it nor its parent `is_window_active`. | 4 |
| src/browser/page.rs:125-126, 204-225 | `place_native` in prepaint: `surface::keep` of a `NativePage` and `place`, so GPUI's hit map has the page where it is this frame. | 6 |
| src/browser/page.rs:155-168 | Capture-phase mouse move during an active drag while a page `resizing()`: `draw_in_transaction`. | 5 |
| src/browser/page.rs:230-237 | Capture-phase `MouseDownEvent` inside the page's bounds: `BrowserView::pressed_at`. | 2 |
| src/browser/input.rs:23-45 | `pressed_at` (focus, `person_acts`, `resolve_menu`) and `typed` (`person_acts`). | 2 |
| src/browser/menu.rs:244-293 | `show_menu`, `menu_gate`, `resolve_menu`: the page's menu asks the gate; an agent's lease refuses first. | 3 |
| src/browser/view.rs:212-215 | `observe_window_visibility` sets `window_visible`, which decides page sight. | 4 |
| src/browser/view.rs:239, 421-439 | `keyboard::page_closed` on release; `focus_address`; `focus_moved` to `entered_page` or `left_page`. | 1 |
| src/browser/dock.rs:141, 354 | ⌥⌘I out of DevTools: `keyboard::set`; docked tools' size rounded to whole points. | 1 |
| src/shell.rs:139-146, 165 | Window activation observer: `keyboard::follow_focus` and `key_state`; GPUI's focus into the shell's root: `follow_focus`. | 1 |
| src/shell/keys.rs:97-103, 250-278, 285 | `focus_pane`, the tab shown, `keys_follow_settings` (each frame): `keyboard::set`, `tab_shown`, `key_state`. | 1 |
| src/shell/switcher.rs:139, 210 | `window_glass::fill` on the panel; `popup::show`. | 4 |
| src/shell/rails.rs:144 | `traffic_lights::fade`. | 0 |
| src/menu/view.rs:58 | `native::window::callout` as a menu window's size changes. | 4 |
| src/agent_desktop/mod.rs:247 | `native::menus::desk_host(Some(pid))`. | 3 |
| src/control/exec/app.rs:84-85 | `app_focus` tool: `cx.activate(true)` and `activate_window()`. | 4 |
| src/control/auth.rs:181, src/chat/tabs.rs:34 | `activate_window()`; `cx.activate(true)` on a notification click. | 4 |
| src/browser/passkeys/mod.rs:189 | `native::window::ns_window` as the passkey sheet's anchor. | 4 |

## 2. Counts

### Files

Run from `koyo-rs/koyo`. Comment lines (first non-space characters `//`) are left out.

```sh
DIRECT='msg_send|\bsel!|\bclass!|define_class!|\bobjc2::|\bobjc::|objc2_app_kit|objc2_foundation|objc2_core_graphics|objc2_quartz_core|objc2_image_io|raw_window_handle|RawWindowHandle|HasWindowHandle|\bNS[A-Z][a-z][A-Za-z]*|\bCG[A-Z][a-z][A-Za-z]*|\bCA[A-Z][a-z][A-Za-z]*|extern "C|AnyObject|AnyClass'
grep -rnE "$DIRECT" src | grep -vE '^[^:]+:[0-9]+:\s*//' | cut -d: -f1 | sort | uniq -c | sort -rn
```

It gives 35 files and 624 code lines. Your 31 isn't reproducible from any grep I tried. A plain
`grep -rlE 'msg_send|objc2|cocoa|NSWindow|NSView|NSEvent|NSApp|NSMenu|NSPanel|CGEvent|raw_window_handle' src`
gives 46, because it counts doc comments and the CoreFoundation, CoreVideo and IOSurface users.
`grep -rlE 'objc2_app_kit|msg_send|\bobjc::' src` gives 30. Two earlier loose patterns
(`\bCA[A-Z]`, `\bNS[A-Z][A-Z]`) matched `CARD_RADIUS` and similar names. The `[a-z]` after the
prefix removes those.

| Code lines | File | Class |
| --- | --- | --- |
| 75 | native/menus/appkit.rs | seam |
| 71 | browser/native/probe.rs | seam (debug) |
| 45 | browser/native/probe/look.rs | seam (debug) |
| 44 | native/window.rs | seam |
| 37 | keyboard/appkit.rs | seam |
| 28 | native/liquid.rs | 0* |
| 25 | settings_view/native_visual.rs | 0 (visual-check) |
| 24 | browser/passkeys/sheet/requests.rs | 0 |
| 20 | viewer/picture.rs | 0 |
| 18 | viewer/bitmap.rs | 0 |
| 18 | native/surface.rs | seam |
| 17 | viewer/pdf.rs | 0 |
| 15 | native/window_glass.rs | seam (4) and 0* |
| 15 | native/notifications.rs | 0 |
| 14 | theme/system_accent.rs | 0 |
| 14 | browser/passkeys/sheet/delegate.rs | seam (4) |
| 13 | native/glass.rs | 0* |
| 13 | browser/site.rs | seam (3) |
| 12 | native/target.rs | 0* |
| 9 | shell/switcher/visual.rs | 0 (visual-check) |
| 9 | menu/switch.rs | 0* |
| 8 | toggle.rs | 0* |
| 8 | native/slider.rs | 0* |
| 8 | devices/view.rs | 0 |
| 8 | browser/passkeys/sheet.rs | seam (4) |
| 8 | browser/native/mod.rs | seam |
| 7 | settings_view/native_slider/appkit.rs | 0* |
| 7 | chat/composer/effort.rs | 0* |
| 6 | native/traffic_lights.rs | 0 |
| 6 | native/switch.rs | 0* |
| 6 | native/icon.rs | 0 |
| 6 | native/host.rs | seam |
| 5 | picker.rs | 0 |
| 3 | browser/passkeys/sheet/tests.rs | 0 |
| 2 | browser/native/probe/keys.rs | seam (debug) |

Totals: 13 seam files (3 of them the debug probe), 9 files of category `0*` that create or
configure a native view inside a GPUI window, and 13 files of category 0.

Outside the 35, seven files use only CoreFoundation, CoreVideo, IOSurface, Security or
AuthenticationServices: `surfaces.rs`, `shell/pictures.rs`, `shell/switcher/card.rs`,
`agent_desktop/picture.rs`, `devices/picture.rs`, `browser/passkeys/mod.rs`,
`browser/passkeys/sheet/answer.rs`. All are 0 (pixel buffers, entitlements, errors), except
`browser/passkeys/mod.rs:189`, which hands `ns_window` to the passkey sheet (concern 4, in 1b).
Two more use only GPUI's macOS crates: `keyboard/tests/keys.rs` (`gpui_macos::responders`) and
`theme/fonts.rs` (`gpui_apple::AppleTextSystem`).

```sh
OTHER='objc2_core_foundation|objc2_core_video|objc2_io_surface|objc2_user_notifications|objc2_authentication_services|objc2_security'
BRIDGE='gpui_macos::|gpui_apple::|koyo_browser_host::native::'
code() { grep -rnE "$1" src | grep -vE '^[^:]+:[0-9]+:\s*//' | cut -d: -f1 | sort -u; }
comm -23 <(code "$OTHER") <(code "$DIRECT")    # 7 files
comm -23 <(code "$BRIDGE") <(code "$DIRECT")   # 4 files; logs/filter.rs only names gpui_apple in a string
```

### Rows by concern

Counted from section 1's tables, so they change when the tables do. Run from
`.superpowers/native-layer/`:

```sh
awk -F'|' '/^\| `src\//{gsub(/ /,"",$5); c[$5]++} END{for(k in c) print k, c[k]}' inventory-app.md | sort
awk -F'|' '/^\| `src\//{split($2,a,":"); f=a[1]; gsub(/[ `]/,"",f); gsub(/ /,"",$5); print f, $5}' inventory-app.md | sort | uniq -c
```

95 rows in 35 files. The first command gives:

| Concern | Rows |
| --- | --- |
| 1 keyboard and first responder | 11 |
| 2 press ownership and capture | 3 |
| 3 menus | 12 |
| 4 windows and panels | 15 |
| 5 geometry | 12 |
| 6 native view lifecycle | 9 |
| 0* native view in a GPUI window | 12 |
| 0 not the seam | 21 |

The second gives, per seam file:

| File | 1 | 2 | 3 | 4 | 5 | 6 | 0* | 0 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| keyboard/appkit.rs | 10 | | | 3 | | | | |
| native/menus/appkit.rs | | | 10 | | | | | |
| native/window.rs | | | | 4 | 2 | | | 1 |
| native/surface.rs | | | | | 4 | 4 | | 1 |
| native/host.rs | | | | | | 2 | | |
| native/window_glass.rs | | | | 1 | | | 1 | |
| browser/native/mod.rs | | | | | 2 | 2 | | 1 |
| browser/native/probe.rs | 1 | 1 | 1 | 2 | 1 | | | 2 |
| browser/native/probe/look.rs | | 2 | | 2 | 3 | 1 | | 2 |
| browser/native/probe/keys.rs | | | | 1 | | | | |
| browser/site.rs | | | 1 | | | | | |
| browser/passkeys/sheet.rs | | | | 1 | | | | |
| browser/passkeys/sheet/delegate.rs | | | | 1 | | | | |

The other 22 files have only `0*` rows (native/liquid.rs 3, the other eight `0*` files 1 each)
or only `0` rows. Concern 2 has no production row with a direct AppKit call: the app's press
handling goes through the vendored window (`Told::pressed`, counted under 1) and GPUI events
(section 1b). The three rows of concern 2 are all the probe's.

### `makeFirstResponder` in all of koyo-rs

```sh
cd koyo-rs && grep -rnE 'makeFirstResponder' --exclude-dir=target .
```

Ten lines. Five are calls that ask for a move (three in the app, two in the vendored window). Line 325 is the override that forwards every ask, Chromium's included, to AppKit and reports the change.

| Site | What |
| --- | --- |
| koyo/src/keyboard/appkit.rs:131 | The reconciler: GPUI's view. |
| koyo/src/keyboard/appkit.rs:137 | The reconciler: a page's keys view. |
| koyo/src/keyboard/appkit.rs:139 | The reconciler: nil, the window itself. |
| vendor/gpui-fast-macos/src/window.rs:674 | `apply_simple_fullscreen_plan`: after a style-mask change, `makeKeyAndOrderFront:` and GPUI's view made first responder, outside the reconciler. |
| vendor/gpui-fast-macos/src/window.rs:1232 | `MacWindow::open`: GPUI's view made first responder as the window opens. |
| vendor/gpui-fast-macos/src/window.rs:325 | `make_first_responder`, the override's call to `super`; it reports every change to `Responders::changed`. |
| vendor/gpui-fast-macos/src/window.rs:443, 448 | The override registered on GPUI's panel and window classes. |
| vendor/gpui-fast-macos/src/window.rs:319 | Doc comment. |
| vendor/README.md:35 | Doc. |

`koyo-browser-host` has none. Chromium calls `makeFirstResponder:` from inside the engine, which
the override hears.

## 3. The proto-layer modules

All of them run on the main thread and keep their state in thread-locals or GPUI globals.

### keyboard/ (one holder per window; the only app-side mover of the first responder)

`mod.rs` owns `KEYBOARDS: RefCell<Keyboards>` and re-exports `Cause`, `Holder`, `Intent`,
`PaneId`, `Part` and `Surface`. `change` runs a closure on the holders, ends the click window on
any non-click change, reconciles each changed window and logs one line.

| API | Callers |
| --- | --- |
| `pub fn init(cx: &mut App)` | main.rs:101 |
| `pub fn set(window: &Window, surface: Surface, cause: Cause)` | shell/keys.rs:97, 103, 255, 278; browser/view.rs:423; browser/dock.rs:141 |
| `pub fn tab_shown(window: &Window, tab: u64) -> Option<Surface>` | shell/keys.rs:250 |
| `pub fn entered_page(window: &Window, pane: PaneId)` | browser/view.rs:435 |
| `pub fn left_page(window: &Window, pane: PaneId, part: Part)` | browser/view.rs:437 |
| `pub fn key_state(window: &Window)` | shell.rs:141 (activation), shell/keys.rs:285 (each frame) |
| `pub fn follow_focus(window: &Window, cx: &mut App)` | shell.rs:140 (activation), 165 (GPUI's focus into the shell's root) |
| `pub fn page_closed(pane: PaneId)` | browser/view.rs:239 |
| `pub fn opened_over(window: &Window, over: AnyWindowHandle)` | popup.rs:156 |
| `pub struct DialogOver`, `DialogOver::new(over: AnyWindowHandle) -> DialogOver`, `Drop` | popup.rs:188 |
| `pub fn describe() -> String` (debug, not test) | browser/native/probe/keys.rs:15 |
| `pub fn holder(window: AnyWindowHandle) -> Option<Holder>` (test) | shell/keys/tests.rs, browser/view/tests.rs |
| `Intent` values | launcher/mod.rs:179, launcher/act.rs, shell/browser_tabs.rs, shell/reopen.rs:100, shell/switcher/show.rs:21, control/exec/pane.rs:92 |

`state.rs` is pure. `Keyboards { windows: HashMap<WindowKey, Held>, inactive: bool }`, with
`Held { surface, key, dialog, menus: u32, tab, tabs: HashMap<u64, Surface> }`. A window's holder
is `Menu` while `menus > 0`, `Away` while a dialog is open, the window isn't key or the app is
inactive, and otherwise its surface. Methods: `holder`, `surface`, `set`, `left_page`, `key`,
`dialog`, `tab_shown`, `page_closed`, `click`, `menu`, `active`, `forget`, `closed_over`,
`unstick`. Types: `Holder { Gpui(Part), Page(PaneId), Menu, Away }`,
`Surface { Gpui(Part), Page(PaneId) }`, `Part { Window, Pane(u64), Bar(PaneId), Other }`,
eight `Intent`s, sixteen `Cause`s and `Change { window, from, to, cause }`.

`reconcile.rs` is pure over a trait and holds no state. `Responder { Gpui, Page(PaneId), Window,
Other }`. `trait AppKit { first_responder, make_first_responder, page_window, has_marked_text,
make_key, is_key }`. `home`, `wanted`, `make`, `reconcile`, `at_key` (skips marked text),
`at_press` (GPUI holder only), `heard -> Heard { Wanted, Refused(PaneId), Astray }`, `restore`.
Only `hooks.rs` and the tests call it.

`hooks.rs` holds `INSTALLED` (the `AppKit` and a `Later`), `MOVING: Cell<bool>` (a reconciler
move in progress) and `CLICKED: Cell<Option<(WindowKey, PaneId, Instant)>>` with
`CLICK_LASTS = 1 s`.

| API | Callers |
| --- | --- |
| `pub fn install(appkit: Rc<dyn AppKit>, later: Later)` | keyboard/appkit.rs:296; tests/fake.rs:108 |
| `pub fn reconcile(window: WindowKey) -> Followed` | mod.rs `change` |
| `pub fn end_click()` | mod.rs `change`, `set_in`; `heard`, `pressed` |
| `pub fn heard(window, before: Responder, after: Responder)` | appkit.rs `Told::changed`; tests' `Fake::moved` |
| `pub fn pressed(window, over: Responder, button: isize)` | appkit.rs `Told::pressed` |
| `pub fn gpui_first(window, keystroke: &Keystroke) -> bool` | appkit.rs `Told::gpui_first` |
| `pub fn at_key(window)` | appkit.rs `Told::key` |
| `pub fn page_leaving(pane)`, `pub fn page_arrived(pane)` | appkit.rs:298 (the host's `set_keyboard` callback) |
| `pub fn menu(open: bool)`, `pub fn active(active: bool)` | appkit.rs:304-307 (notifications) |
| `pub fn forget(window)`, `pub fn closed_over(over: Option<WindowKey>, was_key: bool)` | appkit.rs `closing` |

`appkit.rs` (macOS, not in tests) holds `KNOWN: HashMap<WindowKey, Known { Weak<NSWindow>,
Weak<NSView> }>`, `OVER: HashMap<usize, WindowKey>` (floating `NSWindow` pointer to owner) and
the observers. `know` is called from mod.rs `known`, `over` from `opened_over`, `install` from
`init`, `describe` from mod.rs `describe`.

`reserved.rs`: `pub const RESERVED: [&str; 8]` and `pub fn reserved(&Keystroke) -> bool`,
called by `hooks::gpui_first`. `rate.rs`: `MOST = 20`, `PER = 1 s`,
`pub type Later = Box<dyn Fn(Duration, Box<dyn FnOnce()>)>`, `may_put_back(window) -> bool` and
`tell(window, line, &Later)`; it holds `PUT_BACK`, `TOLD` and `LEFT_OUT` per window. `lines.rs`:
target `koyo::keyboard`, the `Display` impls, `Followed { Untold, Stayed, Moved }`, `line`,
`level`, `tell`, `write`, `refused`, `healed`, `left_out`.

### native/menus.rs and menus/appkit.rs (the menu gate)

`menus.rs` owns the verdict. `WITHIN = 5 s`; `PRIVATE_STATE = -1`;
`Input { state: i64, posted_by: i64 }`; `Posters { own, desk }`;
`Origin { Person, Posted(&str) }`; `Refusal { NotOnScreen, NoInput, Posted(&str), Agent }` with
`why()`. State: `DESK: AtomicI64` (the desk host's pid),
`NOTED { last: HashMap<isize, (Instant, Input)>, latest: Option<isize> }` keyed by
`windowNumber`, and `WATCHING: Cell<bool>`.

| API | Callers |
| --- | --- |
| `pub fn classify(input: Input, posters: Posters) -> Origin` | `judge`; tests |
| `pub fn decide(on_screen: bool, last: Option<(Duration, Origin)>) -> Result<(), Refusal>` | `judge`; tests |
| `pub fn desk_host(pid: Option<i32>)` | agent_desktop/mod.rs:247 |
| `pub fn install()` (appkit.rs) | native/mod.rs:29, from `native::init`, from main.rs:100 (tests included) |
| `pub fn may_show(window: &Window, what: &str) -> Result<(), Refusal>` (appkit.rs) | browser/menu.rs:284 |
| `Refusal::Agent` | made by browser/menu.rs:283 only; `decide` never returns it |

### native/window.rs

No state but `draw_in_transaction`'s `ASKED` flag.

| API | Callers |
| --- | --- |
| `pub fn fit(window: &Window, extent: Size<Pixels>, corner: Corner, cx: &mut App) -> bool` | popup.rs:343 |
| `pub fn draw_in_transaction(window: &Window, cx: &mut App)` (not test) | browser/page.rs:165 |
| `pub fn callout(window: &Window, at: f32, tip: f32, half: f32, radius: f32, cx: &mut App)` | menu/view.rs:58 |
| `pub fn refresh_edge(window: AnyWindowHandle, cx: &mut App)` | popup.rs:365 |
| `pub fn show(window: &Window)` | popup.rs:200 |
| `pub fn float_on_its_space(window: &Window)` | popup.rs:155 |
| `pub(crate) fn ns_window(window: &Window) -> Option<Retained<NSWindow>>` | keyboard/appkit.rs:251; native/menus/appkit.rs:79; native/traffic_lights.rs:22; native/window_glass.rs:30; browser/passkeys/mod.rs:189; shell/switcher/visual.rs:320 |
| `pub(crate) fn gpui_view(window: &Window) -> Option<Retained<NSView>>` | keyboard/appkit.rs:52; browser/native/mod.rs:142; browser/site.rs:205; shell/switcher/visual.rs:230 |

### native/surface.rs

`Surface { window, composited, view, placed, visible, frame, rounded, marker, marked }` caches
what `AppKit` was last told.

| API | Callers |
| --- | --- |
| `Surface::new(window: &Window, view: &NSView) -> Result<Surface, String>` | native/host.rs:131; native/glass.rs:80; native/liquid.rs:247; browser/native/mod.rs:97 |
| `Surface::place(&self, bounds, clip, window: &Window)` | native/host.rs:69; native/glass.rs:36; native/liquid.rs:302; browser/native/mod.rs:120 |
| `Surface::mark(&self, bounds, pane: SurfacePane, window: &mut Window)` | native/liquid.rs:266 |
| `Surface::round(&self, corners: Corners<bool>, radius: f64)` | browser/native/mod.rs:137 |
| `Surface::remove(self, cx: &mut App)` | `keep`'s release; native/host.rs:143 |
| `pub fn on_whole_points(bounds, clip) -> Bounds<Pixels>` | browser/native/mod.rs:119; its own test |
| `pub trait Hosted { fn into_surface(self) -> Surface }` | implemented by `NativePage`, liquid's and glass's `Native` |
| `pub fn keep<T: Hosted>(id, window, cx, make) -> Entity<Kept<T>>` | browser/page.rs:214; native/liquid.rs:127; native/glass.rs:44 |

### native/host.rs

`pub trait Control { type View; type Shown; fn make; fn show; fn acted }` and
`pub fn host<C: Control>(id, control: C, shown: C::Shown, window, cx) -> Option<(Size<Pixels>, Div)>`.
Each control keeps `Hosted { control, shown, native: Option<Native { surface, view, _target,
size }>, _acts }`. Callers: toggle.rs:96, chat/composer/effort.rs:117,
settings_view/native_slider/host.rs:27, menu/switch.rs:83.

### popup.rs (the floating windows)

Global `Popups { up: HashMap<Kind, Up>, showing: HashMap<AnyWindowHandle, u64> }`, with
`Up { window, over, source, _watching, settling, _keys: Option<DialogOver> }`.
`Kind { Menu, Dialog, Picker, Card, Switcher }`; one of each kind is up at a time.

| API | Callers (files) |
| --- | --- |
| `pub fn open<V: Render>(opening: Opening, parent: &mut Window, cx, build) -> Option<WindowHandle<V>>` | picker.rs:101, modal.rs:171, menu.rs:326, shell/switcher.rs:126, shell/sidebar/card.rs:135 |
| `pub fn show(window: &mut Window, cx)` | shell/switcher.rs:210 |
| `pub fn window(kind, cx) -> Option<AnyWindowHandle>` | modal.rs:201, menu.rs:345, 347, shell/switcher.rs:81, shell/sidebar/card.rs:152 (and tests, visual checks) |
| `pub fn root<V>(kind, cx) -> Option<&V>` | modal.rs:194, shell/switcher.rs:255, shell/sidebar/card.rs:100, 146 |
| `pub fn opened_from(kind, cx) -> Option<ElementId>` | menu.rs:156 |
| `pub fn close(kind, cx)` | picker.rs:116, modal.rs:189, menu.rs:242, shell/switcher.rs:134, 217, accounts/panel.rs:299, shell/sidebar/card.rs:140 |
| `pub fn close_from_inside(window, cx)` | modal.rs:206, browser/site/prompt.rs:98 |
| `pub fn listener() -> impl IntoElement` | shell/actions.rs:155 |
| `pub fn pressed(over, position, cx)` | menu.rs:136 (and `listener`) |
| `pub fn showing(over, what: &impl Hash, cx)` | shell.rs:167 |
| `pub fn fit(window, extent, corner, cx)`, `pub fn fit_height(height, window, cx)` | menu.rs:359; launcher/render.rs:35 |
| `pub fn refresh_edge(window: AnyWindowHandle, cx)` | popup.rs itself |

### browser/native/mod.rs, browser/page.rs, browser/input.rs

| API | Owns | Callers |
| --- | --- | --- |
| `pub fn start(launch: &Launch, sink: impl Sink) -> io::Result<Connection>` | The host started in Koyo's process. | browser/host/starting.rs:47 |
| `NativePage { pane, container, surface, size, resized }`; `new(window, pane) -> Result<NativePage, String>`, `place(&self, bounds, clip, window)`, `resizing(&self) -> bool`; `impl Hosted` | A page's container view in a GPUI surface, and the host told where to put the page. | browser/page.rs:162, 214-222 |
| `PageSurface { view, covered, blank, palette }`, an `Element` | Prepaint lays out, places the native page and inserts the hitbox; paint asks for transaction draws, tells sight, sets the cursor, listens for presses. | `BrowserView::page_element`, page.rs:275 |
| `PageSurface::place_native` (private, not test) | `surface::keep` and `NativePage::place` in prepaint. | page.rs:125-126 |
| `BrowserView::pressed_at(&mut self, event: &MouseDownEvent, window, cx)` | Focus into the page, `person_acts(Implicit)`, `resolve_menu`; side buttons step history. | browser/page.rs:234; browser/view/tests.rs:272 |
| `BrowserView::typed(&mut self, cx)` | `person_acts(Implicit)`. | browser/host/events.rs:86 (`HostEvent::Typed`) |
| `edits`, `steer`, `history_step`, `cursor_style` | Koyo's edit and steering actions, side buttons, cursor mapping. | page.rs:288, 189; dock.rs:389, 414 |

### browser/native/probe.rs (debug builds)

Started from main.rs:248 when `KOYO_NATIVE_PROBE` names a folder; it polls `<folder>/cmd` every
100 ms and appends to `<folder>/out`. Section 5 lists its commands.

## 4. Who decides whose input it is

| Site | Heuristic | What it gates |
| --- | --- | --- |
| src/keyboard/hooks.rs:146-176 | Any press of buttons 0 to 2 that reaches GPUI's window through the vendored hit test is the person's. No source check. | Arms `CLICKED`; a press on a page that already has the first responder makes it the holder at once (`Cause::Click`). |
| src/keyboard/hooks.rs:22-25, 134-139 | A page's view taking the first responder within 1 s of a press on that page, in that window, with nothing since, is the person's. | Whether the page's ask becomes `Cause::Click` or is refused. |
| src/keyboard/hooks.rs:88-92, keyboard/mod.rs:73-75, 88-90 | Any keyboard move with a cause other than a click ends the click window. | A late ask after ⌘L, ⌃⇥ or a focus move is refused. |
| src/keyboard/hooks.rs:47-54, 110-112 | `MOVING`: a change heard while the reconciler moves the first responder is its own. | Own moves vs asks. |
| src/keyboard/reconcile.rs:113-126 | A page taking the first responder is wanted only if the person's surface is that page. | `Refused` (put back next turn) or `Wanted`. |
| src/keyboard/hooks.rs:97-104 | A key or press arriving at a window `AppKit` says is key proves no menu or dialog has the keys. | `unstick` (cause healed). |
| src/native/menus.rs:92-103 | CGEvent source state -1 (private) is a program's; a posting pid equal to the desk host's or Koyo's own is a program's; anything else, a remapped button or Screen Sharing included, is the person's. | Menu verdict. |
| src/native/menus.rs:107-117 | On screen, last input in that window within 5 s, and that input the person's. | Every native menu Koyo's process pops up. |
| src/native/menus/appkit.rs:130-165 | Only events the app takes from its queue are noted. An `NSEvent` with no `CGEvent` counts as Koyo's own. A probe press handed to `sendEvent:`, or an agent's engine input, leaves no note. | `NoInput` refusals. |
| src/native/menus/appkit.rs:87-97 | A menu popped up at a screen point (nil view) is judged by the window of the latest noted input. | Chromium's view-less menus. |
| src/native/menus.rs:120-131 | The desk host's pid as told by the agent desktop; 0 before one started. | `classify`. |
| src/browser/menu.rs:281-286 | A page whose lease an agent holds refuses its menu before the gate looks at input. | `Refusal::Agent`. |
| src/browser/input.rs:23-38 via page.rs:230-237 | Any GPUI `MouseDownEvent` inside the page's bounds, in the capture phase, is the person's. No source check. | `person_acts(Implicit)`: the lease becomes `Human` for 5 s and agents get `person_has_control`; GPUI focus moves into the page. |
| src/browser/input.rs:43-45, browser/host/events.rs:85-87 | `HostEvent::Typed`, sent by koyo-browser-host/src/pane/keys.rs:34 when a key-down has a non-null `os_event`, at most twice a second per pane. | `person_acts(Implicit)`. |
| src/browser/control.rs:106-140 | The lease: `Human { mode }` refuses every agent; `Implicit` lapses 5 s (`IMPLICIT_HOLD`) after the last act; `Explicit` (Take control, toolbar.rs:192) stays. | Agents' browser commands. |
| src/browser/menu.rs:306, history.rs:200, view/navigation.rs:95, 130 | A menu pick, a history step, a GPUI navigation action are the person's. | `person_acts(Implicit)`. |
| src/browser/view/sight.rs | A page is on screen if drawn in a visible window, or while an agent's command on it runs and `AWAKE_AFTER` (5 s) after. | Whether the engine paints the page. |
| src/browser/native/probe.rs:331-437 | Probe input is `NSEvent`s handed to `sendEvent:`, never posted to the system; `escape` alone goes into the app queue. | Not a decision. These inputs reach the keyboard hooks and GPUI as presses. |
| src/control/exec/browser/pages/consent.rs:1-30 | A pinned or loose page is the person's by where it is (`Spot`), and reaches a client only with the person's grant. | Consent. Ownership by place, not by input. |
| src/browser/control.rs:248 (`Leased::person_acts`, for the agent desktop and each device) | The person's act on the agent desktop's tab or a device tab, heard through GPUI. | Those surfaces' own leases. Not the native seam. |

`[inferred]` from the rows above: the four press observers disagree on presses posted by a
program. On the agent desktop, an agent's `desktop_click` on a page arrives as a CGEvent the
desk host posted. The menu gate refuses its menu as "posted by the agent desktop's host".
`pressed_at` makes the page the person's (agents refused for 5 s), and the keyboard makes it the
holder. The host's `Typed` check counts keys with an `os_event` behind them, so the desk host's
keys and the probe's keys count as the person's too. Not run.

## 5. Tests and the probe

### What the tests run

Test counts are `grep -cE '#\[(gpui_kit::)?test\]' <file>`.

| Tests | Count | Real code | Fake or re-implemented |
| --- | --- | --- | --- |
| keyboard/tests.rs | 14 | `state.rs` and `lines.rs`, pure. | None. |
| keyboard/tests/reconciler.rs | 10 | `reconcile.rs`. | `Fake` `AppKit` (tests/fake.rs): a map of first responders and page windows. |
| keyboard/tests/told.rs | 23 | `hooks.rs`, `state.rs`, `rate.rs`, `lines.rs`, `mod.rs`. | `Fake` `AppKit`; `Fake::moved` stands in for the vendored window's report of each change; `Turns` stands in for the run loop; tests call `hooks::pressed`, `at_key`, `page_leaving` and `page_arrived` by hand where the window or host would. |
| keyboard/tests/inactive.rs | 2 | A real shell in a GPUI test window, `follow_focus` and `key_state` through frames. | `Fake` `AppKit`; `hooks::page_arrived` by hand. |
| keyboard/tests/keys.rs (ce2f3c7fc) | 5 | Vendored `responders::Offers`, `hooks::gpui_first`, `reserved`, the real keymap (`crate::init`). | `offered()` re-implements the window's glue at vendor/gpui-fast-macos/src/window.rs:2754-2765 (`again`, else `first(gpui_first)`). The order of offers (AppKit's pass, the router, the page's hand-back) is scripted; the routing in `fast/composition.rs` doesn't run. `Fake` `AppKit` for "a page holds". |
| keyboard/tests/marks.rs (ce2f3c7fc) | 1 | `crate::marks` through a real shell drawn in a GPUI test window. | None. |
| shell/keys/tests.rs | 12 | Shell keyboard flows; `keyboard::holder` asserts. | No `AppKit` installed: holders only. |
| browser/view/tests.rs:207-237 | 1 | ⌥⌘I out of DevTools moves the holder. | Holders only. |
| browser/view/tests.rs:239-277 (d33840cad) | 1 | `agent_claims`, `pressed_at` with a GPUI `MouseDownEvent`, `menu_gate`. | `may_show` returns `Ok` for a test window, so only `Refusal::Agent` is really tested. |
| native/menus/tests.rs | 10 | `classify`, `decide`, `note`, `judge`, `latest_window`. | Fake pids; `WATCHING` set by hand. Nothing of menus/appkit.rs runs: no swizzle, monitor, `isOnActiveSpace`. |
| native/surface.rs tests | 1 | `on_whole_points`. | None. |
| popup/tests.rs | 9 | Floating windows in GPUI test windows. | No `NSWindow`, so `float_on_its_space`, `show` and `fit` return early. |
| browser/control.rs tests | 4 | The lease. | None. |
| browser/input.rs tests | 1 | `history_step`. | None. |

Never compiled in tests: keyboard/appkit.rs (`cfg(not(test))`), browser/native/ (replaced by
browser/native_off.rs), `PageSurface::place_native` and `draw_in_transaction`
(`cfg(not(test))`). So `Real`, `Told`, `classify`, `closing`, `install`, `NativePage` and the
probe have no tests. menus/appkit.rs is compiled in tests and `crate::init` runs
`native::init` (main.rs:99-100), so a test that calls `crate::init` (keyboard/tests/keys.rs:87)
swizzles `NSMenu` and adds a local event monitor in the test process. No test goes through them.

### The probe's commands

`<folder>/cmd` holds one command a line. Results go to `<folder>/out`.

| Command | Reads or does |
| --- | --- |
| `wait <ms>` | Waits. |
| `clear-data` | `browser::clear_data`, as Settings does. |
| `drag <ms> <x> <y> …` | Left press, moves a pause apart, release, through `sendEvent:`. |
| `eval <pane> <script>` | `Runtime.evaluate` in any page, DevTools' own included. |
| `burst <keys or wheels> <n> <gap> [x y]` | n keys (`a`) or wheel turns, each stamped in `<kind>.json`. |
| `shot <name> [number]` | PNG of the window, or of the app's window with that number. |
| `front` | `orderFrontRegardless`. |
| `windows` | Every app window: number, class, title, frame, visible, key, seen, alpha, content class, `ignoresMouseEvents`, level, `orderedIndex`. |
| `activate` | `activateIgnoringOtherApps` and `makeKeyAndOrderFront:`. |
| `escape` | Escape posted to the app queue (closes a tracking menu). |
| `draw` | `layer.display` on GPUI's view. |
| `quit` | `terminate:`. |
| `pages` | The host's `native::describe`: each page's views, picture layer, and whether they fit the pane. |
| `click <x> <y> [number]` | Move, left down, left up, via `sendEvent:`; in another window by number. |
| `rclick <x> <y>` | The same with the right button. |
| `rdown <x> <y>` | Right down only, as while a menu tracks. |
| `move <x> <y>` | A mouse move. |
| `hover <x> <y>` | `mouseMoved:` handed to GPUI's view and to the view hit. |
| `scroll <dy>` | A wheel event from a `CGEvent`, at the pointer. |
| `type <text>` | Key down and up per character, to the first responder. |
| `key <code> <chars> [mods]` | Key down and up, offered as a key equivalent to the window and the main menu first. |
| `key` | `makeKeyWindow`, app left behind. |
| `keyboard` | Each kept window's holder, first responder kind, key state; app active. |
| `bounds <name>` | Where `chat-composer`, `browser-address`, `page-<pane>` or `tab-switcher` was drawn, from `crate::marks`. |
| `state` | First responder class, key, main, seen, app active, number. |
| `cursor` | The app's cursor and the window's place on screen. |
| `tick` | Toggles drawing occluded windows ten times a second and counting them as seen (`sight.rs`, `UNSEEN_COUNTS`), then draws the shell again. |
| `views` | The view tree. |
| `layers` | The layer tree under GPUI's view. |
| `frame <x> <y> <w> <h>` | `setFrame:display:`. |

What it can't do, read from the code: post an event through the system, so it can't make an
input the menu gate counts as the person's; read a window's Space (`isOnActiveSpace`),
collection behaviour, `hidesOnDeactivate` or parent window; report the menu gate's verdicts,
which only the log has; tell whether a menu is tracking.

## 6. Collisions and duplication

1. Four observers of a press, on three event paths, with different rules. The vendored hit test
   feeds `keyboard::hooks::pressed` (1 s window). The `NSEvent` local monitor feeds the menu
   gate (5 s window, posted events refused). GPUI's capture-phase `MouseDownEvent` feeds
   `BrowserView::pressed_at` (lease for 5 s) and `popup::pressed`. Only the menu gate looks at
   who posted the event. See the `[inferred]` note under section 4.
2. Two process-wide local event monitors: native/menus/appkit.rs:161-163 and
   devices/view.rs:369-373.
3. `NSMenuDidBeginTrackingNotification` has two observers: keyboard/appkit.rs:304 counts menus
   for key windows; native/menus/appkit.rs:353 (debug) cancels a menu the gate didn't allow.
   `[inferred]` a menu the net cancels still counts as opened for the keyboard until its end
   notification arrives.
4. A floating window's Space and level have two writers. Vendored
   vendor/gpui-fast-macos/src/window.rs:1266-1272 sets `NSPopUpWindowLevel` and
   `CanJoinAllSpaces + FullScreenAuxiliary` for `WindowKind::PopUp`. Then
   native/window.rs:219-226, called from popup.rs:155 inside the open callback, overwrites it
   with `Managed + FullScreenAuxiliary` and `hidesOnDeactivate` (`[inferred]`: GPUI makes the platform window before it runs the build callback). native/window_glass.rs:75 adds
   `NonactivatingPanel` to the switcher's panel. The result depends on call order.
5. The key window has many movers. The keyboard (`Real::make_key` after a floating window
   closes with the keys), the vendored window's four `makeKeyAndOrderFront:`, GPUI opening a
   dialog with `focus: true` (popup.rs:123), the `app_focus` tool (control/exec/app.rs:84-85),
   control/auth.rs:181, chat/tabs.rs:34, and the probe's `activate`, `key` and `front`. The
   holders hear the others only through GPUI's activation observer (shell.rs:139-141) and the
   per-frame `keyboard::key_state` (shell/keys.rs:285).
6. The first responder has one app-side mover, but the vendored window moves it too
   (window.rs:674 after simple full screen, 1232 at open). `[inferred]` AppKit itself makes a
   hosted `NSSlider` or `NSSwitch` first responder on a click when the control accepts it; the
   keyboard then sees `Responder::Other`, calls it astray and puts it back on the next turn.
   Nothing in the app sets `refusesFirstResponder` (grep finds none).
7. A window has four identities. The keyboard keys by GPUI window id and by `NSWindow` pointer
   (`OVER`), the menu gate by `windowNumber`, popup.rs by `AnyWindowHandle`. The probe finds
   "Koyo's window" by class-name prefix `GPUI` and `isVisible`. GPUI's view is found by the raw
   window handle (native/window.rs:234) and by class name `GPUIView` (probe/look.rs:127, 148,
   166, 196).
8. "On the person's screen" is computed twice. The menu gate uses `isOnActiveSpace &&
   isVisible` (menus/appkit.rs:103-110). Page sight uses GPUI's window visibility
   (browser/view.rs:212-214, view/sight.rs:76-80). "Koyo is in front" is the keyboard's
   `inactive` flag from notifications, `NSApplication isActive` in `describe`, and
   `is_window_active` in popup.rs's `settle`. "The window is key" is the keyboard's `Held.key`
   (from GPUI) and `isKeyWindow` in `unstuck`.
9. Presenting in Core Animation's transaction is done three ways: `fit` (deferred `setFrame`
   then `layer.display`), `draw_in_transaction` (deferred `layer.display`, deduped), and the
   probe's `draw` and `draw_unseen`.
10. Whole-point rounding happens in `on_whole_points` (points), again in `Surface::place`
    (`to_device_pixels`), in browser/dock.rs:354 for the docked tools, and by macOS for the
    holder window's frame (docs/native-pages.md 6).
11. Native views are placed at two times. Pages and hosted controls are placed in prepaint, so
    GPUI's hit map has them where they are. Liquid glass is placed in paint
    (native/liquid.rs:136-150), which surface.rs:96-105 says GPUI notes a frame late, at no cost for glass under a bar.
    native/glass.rs places from gpui-component's `PopoverBackdrop::place`, called at a time
    this crate doesn't show `[inferred: unknown]`.
12. "The person just acted here" has three different lengths: 1 s (`CLICK_LASTS`), 5 s
    (`WITHIN`), 5 s (`IMPLICIT_HOLD`). The host's `Typed` adds a fourth rate, twice a second.
