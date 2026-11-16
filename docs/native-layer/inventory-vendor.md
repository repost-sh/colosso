# Vendored GPUI crates: inventory of the native seam

The owner's goal: Koyo embeds Chromium's native `NSView`s in GPUI's `NSWindow`, and the bugs on that
seam were patched one symptom at a time. One native layer is to own the seam (`docs/native-layer.md`).
This file is the read-only inventory of the vendored crates under `koyo-rs/vendor/` that the design
starts from. Nothing in the checkout was edited, built or committed to make it.

Line numbers are of the working tree at `ce2f3c7fc` (branch `schulzfel/browser-ui-fixes`). Paths are
relative to `koyo-rs/vendor/` unless they start with `koyo-rs/`, `docs/` or `~/.cargo`. The unvendored
gpui-fast core is read from `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-fast-0.1.12/src/`,
written `core:` below. `[read]` means read in code. `[inferred]` means concluded from code, not run.

## 0. Method, and why `git diff <first vendoring commit>` undercounts

The baseline is each pristine `.crate` from `~/.cargo/registry/cache/`, unpacked into the gitignored
`.superpowers/native-layer/upstream/`. Each `.crate`'s SHA-256 equals the checksum the lockfile held
the commit before it was vendored (`b0bc02a78^`, `2624acac1^`, `b40c99b03^`): all four MATCH.

`git diff <first vendoring commit>..HEAD` misses patches for two crates, because their vendoring
commits were already patched.

- `gpui-fast-macos` was vendored in `b0bc02a78` with the press-ownership patch already in
  `src/fast/composition.rs` (`native_presses`). `git diff b0bc02a78 HEAD` reports +390 -5, and the
  pristine baseline reports +433 -38.
- `gpui-fast-apple` was vendored in `2624acac1` with a `Colosso:` present-in-transaction switch in
  `metal_renderer.rs`. `e7d119aa7` removed it, and no `Colosso` mark is left in `vendor/`.
- `gpui-component/src/native_menu/macos.rs` was patched in `cdcebcf5a` and `f38a22c3a`, then reverted
  to upstream in `c8b92637c`. It is byte-identical to upstream now. The menu gate lives in
  `koyo-rs/koyo/src/native/menus/`, outside this slice.

## 1. Patch sites

Columns: id, `file:line`, the function or item, the concern token (`keyboard`, `mouse`, `menus`,
`windows`, `geometry`, `lifecycle`, `unrelated`), whether a `Koyo:` comment marks it, LOGIC or HOOK,
and what it does. HOOK means the site reports an event to the app or asks the app for a decision.
LOGIC means the fork decides. `LOGIC (state)` is data the LOGIC rows keep.

### 1a. gpui-fast-macos (every hunk against upstream)

| id | file:line | item | concern | mark | kind | what it does |
| --- | --- | --- | --- | --- | --- | --- |
| M01 | src/gpui_macos.rs:29-30 | `pub mod responders` | keyboard | yes | HOOK | Exposes the app's hook module. |
| M02 | src/responders.rs:16-35 | `trait Responders` | keyboard | yes (file, :1) | HOOK | The app's four callbacks: `changed`, `pressed`, `key`, `gpui_first` (default `true`). |
| M03 | src/responders.rs:37-44 | `RESPONDERS`, `set_responders` | keyboard | yes (file) | HOOK | Thread-local `Rc<dyn Responders>` the app installs once for all windows. |
| M04 | src/responders.rs:48-53 | `tell` | keyboard | yes (file) | HOOK | Clones the `Rc` out before calling, so a hook may re-enter (call `makeFirstResponder:`). |
| M05 | src/responders.rs:57-60 | `gpui_first` (crate) | keyboard | yes (file) | HOOK | Asks the app whether a first-offered key equivalent is GPUI's. With no app, `true`. |
| M06 | src/responders.rs:62-127 | `Offerer`, `Answer`, `Owner`, `Offers` | keyboard | yes (4) | LOGIC | One key, one owner. Keyed by `NSEvent.timestamp`. A first offer is GPUI's if `gpui_first`, else the page's. A later `AppKit` offer of a page-owned event (the hand-back) dispatches once. Every `Router` re-offer declines. |
| M07 | src/responders.rs:129-152 | `mod tests` | keyboard | no | n/a | Two unit tests of `Offers`. |
| M08 | src/window.rs:311-317 | `make_window_first_responder`, `make_panel_first_responder` | keyboard | no | HOOK | `extern "C"` shims that pass `NSWindow` or `NSPanel` as the superclass to M09. |
| M09 | src/window.rs:319-332 | `make_first_responder` | keyboard | yes | HOOK | Calls `AppKit`'s `makeFirstResponder:`, then calls `Responders::changed(window, before, after)` if it changed. It reports only and cannot veto. |
| M10 | src/window.rs:440-451 | `build_window_class` | keyboard | yes | HOOK | Installs the M08 override on `GPUIWindow` and on `GPUIPanel`. |
| M11 | src/window.rs:705 | `MacWindowState.last_key_equivalent` | keyboard | no | LOGIC (state) | Now holds `(KeyDownEvent, timestamp)`. |
| M12 | src/window.rs:706-709 | `MacWindowState.koyo_offers`, `koyo_router_offering` | keyboard | yes | LOGIC (state) | The window's `Offers`, and a flag that is true while M22 offers a key. |
| M13 | src/window.rs:722-724 | `MacWindowState.buttons_down` | mouse | yes | LOGIC (state) | The buttons GPUI was told are down, each with its `buttonNumber`. |
| M14 | src/window.rs:1145-1146, 1162 | `MacWindow::open` | keyboard | no | n/a | Initialises M12 and M13. |
| M15 | src/window.rs:2737-2742, 2771-2775 | `handle_key_event` | keyboard | yes | LOGIC | Suppresses a key down after a key equivalent only for the same keystroke at the same `timestamp`. Upstream compared the keystroke alone, so after "f" in a page the next "f" in GPUI was lost. |
| M16 | src/window.rs:2744-2770 | `handle_key_event` | keyboard | yes | LOGIC | On `performKeyEquivalent:`, picks `Router` or `AppKit` from M12's flag, calls `Offers::again`, and on a first offer drops the lock and calls M05, then `Offers::first`. `Decline` returns NO without dispatching to GPUI. This row also asks the app (HOOK via M05). |
| M17 | src/window.rs:2987-2988 | `handle_view_event` | mouse | yes | LOGIC | Computes the lost releases for this event (M19). |
| M18 | src/window.rs:3051-3053 | `handle_view_event` | mouse | no | LOGIC | Sends each lost release to GPUI's callback as a `MouseUp` before the event itself. |
| M19 | src/window.rs:3060-3127 | `lost_releases` | mouse | yes | LOGIC | A move with no button down means every tracked press is over. A press means each press whose button `NSEvent.pressedMouseButtons` says is up is over. A release updates the bookkeeping the way core's `dispatch_native` counts it. Each synthetic release logs one line at `koyo::input` (:3115-3118), an app log target inside the fork. |
| C01 | src/fast/composition.rs:78-83 | `MacComposition.native_presses` | mouse | yes | LOGIC (state) | `FxHashMap<buttonNumber, native view>`. It replaces upstream's `native_buttons` bitmask and single `native_owner`. |
| C02 | src/fast/composition.rs:99 | `MacComposition::new` | mouse | no | n/a | Initialises C01. |
| C03 | src/fast/composition.rs:220-248 | `route_keys` | keyboard | yes | LOGIC | Key down or up: calls `Responders::key(window)` (HOOK), so the app can move the first responder before delivery. Key down also runs C04. `FlagsChanged` is also sent to GPUI's view when the first responder isn't GPUI's view. It relies on upstream's de-duplication at window.rs:3028-3044. |
| C04 | src/fast/composition.rs:250-271 | `offer_first` | keyboard | yes | LOGIC | When ⌘ (bit 20) or ⌃ (bit 18) is held and the first responder isn't GPUI's view, sets M12's flag, calls GPUI view `performKeyEquivalent:`, and swallows the event if it returns YES. |
| C05 | src/fast/composition.rs:273-279 | `tell_press` | keyboard | yes | HOOK | Calls `Responders::pressed(window, over, buttonNumber)`. It serves the keyboard holder, and its input is a mouse press. |
| C06 | src/fast/composition.rs:282-285 | `send_window_event` | keyboard | no | LOGIC | Runs `route_keys` first for every event of every GPUI window, composing or not. True means the event stops there. |
| C07 | src/fast/composition.rs:286-293 | `send_window_event` | keyboard | no | HOOK | In a window with no native views, every press is told with `over = nil`. |
| C08 | src/fast/composition.rs:323-352 | `send_window_event` | mouse | yes | LOGIC | Looks up the button's owner. An up removes it and goes to it. A drag reads it. A down clears it, so each press decides anew. |
| C09 | src/fast/composition.rs:359-360 | `send_window_event` | mouse | no | LOGIC | Drags and ups go only to the press's owner. With no owner they go GPUI's way. Upstream hit-tested them when nothing was captured. |
| C10 | src/fast/composition.rs:367-370 | `send_window_event` | keyboard | no | HOOK | Tells the press with the hit-test target when that target is inside a native view, else nil. This runs before GPUI's capture phase (C12's :400). |
| C11 | src/fast/composition.rs:379-384 | `send_window_event` | mouse | no | LOGIC | An input-forwarder's press becomes its button's owner. |
| C12 | src/fast/composition.rs:422-426 | `send_window_event` | mouse | no | LOGIC | A native view's press becomes its button's owner before `AppKit` delivers it. |
| C13 | src/fast/composition.rs:780-784 | `set_composition_order` | mouse | yes | LOGIC | Drops the presses of views that left the composition. Upstream cleared presses only when no native view was left. Secondary concern is lifecycle. |

### 1b. gpui-fast-apple (all marks, and the unmarked hunks grouped)

None of these touch the seam's input, keyboard or window behaviour. They draw BGRA surfaces and take
pictures for the tab switcher. Two items do mention native views: `set_surface_pane` (A29), which
stands in for a native view in a picture, and `draws_over` (A03), which orders the overlay layers'
renderers.

| id | file:line | item | concern | mark | kind | what it does |
| --- | --- | --- | --- | --- | --- | --- |
| A01 | Cargo.toml:192, 232, 235, 237 | features | unrelated | no | n/a | Adds `objc2-io-surface`, `MTLAllocation` and `MTLResource`. |
| A02 | build.rs:6 | `mod fast_rounded_clip_shader` | unrelated | yes | n/a | Lint allow for `stage`. |
| A03 | src/fast/composition.rs:22 | `new_overlay_renderer` | unrelated | yes | LOGIC | An overlay renderer `draws_over(base)`: one command queue, shared pictures (picture capture). |
| A04 | src/fast/rounded_clip_shader.rs:17 | `source` | unrelated | yes | n/a | Adds `page_surface_fragment` to the rounded-clip shader set. |
| A05 | src/shaders.metal:903 | `page_surface_vertex`/`_fragment` | unrelated | yes | n/a | Draws a BGRA surface whose picture is its texture's top-left part (surface drawing). |
| A06 | src/shaders.metal:943 | `PageSurfaceVarying` sampling | unrelated | yes | n/a | Samples mips when shrunk (picture capture). |
| A07 | src/metal_renderer.rs:147 | `page_surfaces_pipeline_state` | unrelated | yes | LOGIC (state) | Pipeline for BGRA surfaces. |
| A08 | src/metal_renderer.rs:149 | `page_surfaces_in_flight` | unrelated | yes | LOGIC (state) | Keeps sampled pixel buffers until the GPU finishes, because an `IOSurface`'s in-use flag is cross-process. |
| A09 | src/metal_renderer.rs:157 | `pictures` | unrelated | yes | LOGIC (state) | The pictures taken of frames. |
| A10 | src/metal_renderer.rs:631 | `render_frame` | unrelated | yes | LOGIC | Clears what the scene asks for in pictures. |
| A11 | src/metal_renderer.rs:642 | `render_frame` | unrelated | yes | LOGIC | `take_pictures` after drawing. |
| A12 | src/metal_renderer.rs:647 | `render_frame` | unrelated | yes | LOGIC | Releases in-flight page surfaces in the completion handler. |
| A13 | src/metal_renderer.rs:1321 | `draw_surfaces` | unrelated | yes | LOGIC | Sends a BGRA surface to `draw_page_surface`. |
| A14 | src/metal_renderer.rs:1389 | `draw_page_surface` | unrelated | yes | LOGIC | Draws a BGRA surface with or without a picture. |
| A15 | src/metal_renderer.rs:1400 | `draw_page_surface` | unrelated | yes | LOGIC | A capture or pane buffer draws nothing and is noted for pictures. |
| A16 | src/metal_renderer.rs:1449 | `draw_page` | unrelated | yes | LOGIC | Encodes one page quad. |
| A17 | src/metal_renderer.rs:1474 | `page_surface_texture` | unrelated | yes | LOGIC | `MTLTexture` made directly over the `IOSurface`, skipping the CV cache. |
| A18 | src/metal_renderer.rs:1504 | `struct PageSurface` | unrelated | yes | n/a | Shader input. |
| A19 | src/metal_renderer.rs:1515 | `SURFACE_MARK` | unrelated | yes | n/a | Attachment key `KoyoSurfaceMark`. |
| A20 | src/metal_renderer.rs:1519 | `enum Mark` | unrelated | yes | n/a | Picture, capture or pane. |
| A21 | src/metal_renderer.rs:1570 | `mark_surface` | unrelated | yes | LOGIC | Writes the mark attachment. |
| A22 | src/metal_renderer.rs:1584 | `surface_mark` | unrelated | yes | LOGIC | Reads it. |
| A23 | src/metal_renderer.rs:1597 | `set_surface_picture` | unrelated | yes | HOOK | Pub API with no caller in `koyo-rs/` (left from the off-screen page path). |
| A24 | src/metal_renderer.rs:1606 | `PICTURE_EVERY` | unrelated | yes | n/a | 250 ms. |
| A25 | src/metal_renderer.rs:1610 | `set_surface_capture` | unrelated | yes | HOOK | Pub API: take a picture into this buffer. |
| A26 | src/metal_renderer.rs:1628 | `struct SurfacePane` | unrelated | yes | HOOK | Pub type: radius, fill, edge. |
| A27 | src/metal_renderer.rs:1637 | `set_surface_pane` | unrelated | yes | HOOK | Pub API: a buffer standing for a native view in pictures. |
| A28 | src/metal_renderer.rs:1646 | `struct Pictures` | unrelated | yes | n/a | Per renderer. |
| A29 | src/metal_renderer.rs:1662 | `struct WindowPictures` | unrelated | yes | n/a | Shared by a window's layers. |
| A30 | src/metal_renderer.rs:1675 | `struct Taken` | unrelated | yes | n/a | One picture. |
| A31 | src/metal_renderer.rs:1687 | `MetalRenderer::draws_over` | unrelated | yes | LOGIC | `pub(crate)`. Shares the queue and pictures with the base renderer. |
| A32 | src/metal_renderer.rs:1697 | `take_pictures` | unrelated | yes | LOGIC | Lower layer takes, upper layer draws over. |
| A33 | src/metal_renderer.rs:1733 | `reads` | unrelated | yes | LOGIC | Turns off `framebufferOnly` the first time a picture is asked for. |
| A34 | src/metal_renderer.rs:1744 | `take_picture` | unrelated | yes | LOGIC | Rate limit per request. |
| A35 | src/metal_renderer.rs:1783 | `shrink` | unrelated | yes | LOGIC | Copy, mip, draw into the buffer's surface. |
| A36 | src/metal_renderer.rs:1878 | `fill_panes` | unrelated | yes | LOGIC | Fills pane stand-ins in a picture. |
| A37 | src/metal_renderer.rs:1944 | `picture_source` | unrelated | yes | LOGIC | Reused copy texture. |
| A38 | src/metal_renderer.rs:2335 | `Page = 6` | unrelated | yes | n/a | Buffer index. |
| A39 | src/metal_renderer.rs:7, 24, 27, 30, 421, 469, 653 | imports, pipeline init, drop | unrelated | no | n/a | Seven unmarked hunks supporting A07 to A12. |

### 1c. gpui-base and gpui-component (marks, and the one seam hook)

| id | file:line | item | concern | mark | kind | what it does |
| --- | --- | --- | --- | --- | --- | --- |
| B01 | gpui-base/Cargo.toml.orig:71 | `block2` dep | unrelated | yes | n/a | For B02. |
| B02 | gpui-base/src/reduce_motion.rs:120 | `follow` | unrelated | yes | HOOK | Re-reads Reduce Motion on `NSWorkspace`'s notification. |
| B03 | gpui-base/src/input/base/kind.rs:380 | `TextareaExtras` | unrelated | yes | n/a | Styles laid over multi-line text. |
| B04 | gpui-base/src/input/editor/decorations.rs:475 | `set_decorations` | unrelated | yes | n/a | Editor decorations. |
| B05 | gpui-base/src/input/editor/decorations.rs:486 | `decorations` | unrelated | yes | n/a | Getter. |
| B06 | gpui-component/src/input/input.rs:133 | field `ink` | unrelated | yes | n/a | Text, caret and placeholder colours instead of the theme's (glass over a page, `985117942`). |
| B07 | gpui-component/src/input/input.rs:305 | `fn ink` | unrelated | yes | n/a | Setter. |
| B08 | gpui-component/src/message_scroller.rs:25 | field `hand_scrolls` | unrelated | yes | n/a | Chat list. |
| B09 | gpui-component/src/message_scroller.rs:82 | `bounds_for_item` | unrelated | yes | n/a | Chat list. |
| B10 | gpui-component/src/message_scroller.rs:88 | `hand_scrolls` | unrelated | yes | n/a | Chat list. |
| B11 | gpui-component/src/message_scroller.rs:94 | `is_scrollbar_dragging` | unrelated | yes | n/a | Chat list. |
| B12 | gpui-component/src/message_scroller.rs:99 | `scroll_offset` | unrelated | yes | n/a | Chat list. |
| B13 | gpui-component/src/message_scroller.rs:105 | `viewport_bounds` | unrelated | yes | n/a | Chat list. |
| B14 | gpui-component/src/input/popovers/cards.rs:25-48 | `trait PopoverBackdrop`, `set_popover_backdrop` | lifecycle | no | HOOK | The app puts a backdrop behind the editor's hover and diagnostic cards, placed as the card is prepainted "so a native view goes under the card rather than over". Koyo puts an `NSVisualEffectView` native surface there (`koyo-rs/koyo/src/native/glass.rs:9,21,26`). This is the only seam-aware code in either editor crate. |

The unmarked diff of these two crates is editor and text work, and none of it is on the seam.
Commits are listed per file. Searching the added lines for `focus|mouse|NSView|NSMenu|NSWindow|first_responder|native|msg_send|Hitbox`
finds only editor focus handling, `NSWorkspace` (B02) and B14.

| file | +/- | commits | concern |
| --- | --- | --- | --- |
| gpui-base/Cargo.toml, Cargo.toml.orig, src/reduce_motion.rs | +51 -7 | e7d119aa7 | unrelated (Reduce Motion) |
| gpui-base/src/async_util.rs, input/base/layout.rs, token_presentation.rs, editor/display_map/* (6 files), highlighting.rs, search.rs, test_support.rs | +2062 -563 | 638f1bb41 (+e0f2fc400) | unrelated (editor perf) |
| gpui-base/src/input/base/edits.rs, element.rs, state.rs, rope_ext.rs, tab_stops.rs, kind.rs, editor/decorations.rs, diagnostics.rs, indent.rs, lsp/* (8 files), editor/mod.rs, input/mod.rs | +2539 -598 | 6f83a40ce c7cfe4de9 edabe57e3 f5be25c87 e0f2fc400 c429d9861 c3dbd6d09 0b65d44ec 1a3d5d3a0 | unrelated (editor, LSP) |
| gpui-base/src/scrollable_mask.rs, selectable_text.rs, text/* (3 files) | +96 -17 | fe50f2854 21d6700c6 | unrelated (markdown, selection) |
| gpui-component/src/highlighter/* (5 files) | +996 -248 | 638f1bb41 6f83a40ce c7cfe4de9 | unrelated (highlighting perf) |
| gpui-component/src/input/mod.rs, overlay.rs, popovers/* (6 changed, 2 removed) | +1064 -545 | f5be25c87 c7cfe4de9 c429d9861 e0f2fc400 638f1bb41 | unrelated, except B14 |
| gpui-component/src/input/input.rs | +22 -3 | 985117942 | unrelated (B06, B07) |
| gpui-component/src/message_scroller.rs, shimmer.rs, text/compat.rs | +67 -5 | 1a3d5d3a0 345a89d28 fe50f2854 | unrelated |

## 2. Counts, and the commands that reproduce them

Run from the checkout root.

```sh
# baseline: the pristine crates (idempotent; writes only inside the checkout)
mkdir -p .superpowers/native-layer/upstream && (cd .superpowers/native-layer/upstream && for c in gpui-base-0.8.1 gpui-component-0.8.1 gpui-fast-apple-0.1.12 gpui-fast-macos-0.1.12; do tar -xzf ~/.cargo/registry/cache/index.crates.io-*/$c.crate; done)

# Koyo: marks by crate, then by file (README.md excluded)
(cd koyo-rs/vendor && grep -rn 'Koyo:' gpui-base gpui-component gpui-fast-apple gpui-fast-macos | cut -d/ -f1 | sort | uniq -c)
(cd koyo-rs/vendor && grep -rn 'Koyo:' gpui-base gpui-component gpui-fast-apple gpui-fast-macos | cut -d: -f1 | sort | uniq -c)

# diff against the pristine crate, per crate
for c in gpui-base:gpui-base-0.8.1 gpui-component:gpui-component-0.8.1 gpui-fast-apple:gpui-fast-apple-0.1.12 gpui-fast-macos:gpui-fast-macos-0.1.12; do echo "${c%%:*}: $(git diff --no-index --shortstat .superpowers/native-layer/upstream/${c##*:} koyo-rs/vendor/${c%%:*})"; done

# patch-site rows of this file, by crate prefix and concern, and by kind
F=docs/native-layer/inventory-vendor.md
grep -E '^\| [MCAB][0-9]{2} ' $F | awk -F'|' '{gsub(/ /,"",$2); gsub(/ /,"",$5); print substr($2,1,1), $5}' | sort | uniq -c
grep -E '^\| [MC][0-9]{2} ' $F | awk -F'|' '{gsub(/^ +| +$/,"",$7); print $7}' | sort | uniq -c
```

Output at `ce2f3c7fc`.

| crate | `Koyo:` marks | files changed vs pristine | +/- vs pristine |
| --- | --- | --- | --- |
| gpui-base | 5 (Cargo.toml.orig 1, reduce_motion.rs 1, kind.rs 1, decorations.rs 2) | 39 (3 new) | +4729 -960 |
| gpui-component | 8 (input.rs 2, message_scroller.rs 6) | 19 (2 new, 2 removed) | +2149 -803 |
| gpui-fast-apple | 37 (metal_renderer.rs 32, shaders.metal 2, build.rs 1, composition.rs 1, rounded_clip_shader.rs 1) | 6 | +700 -5 |
| gpui-fast-macos | 20 (window.rs 8, composition.rs 6, responders.rs 5, gpui_macos.rs 1) | 4 (1 new) | +433 -38 |
| total | 70 (and 2 in `vendor/README.md`) | 68 | +8011 -1806 |

Patch-site rows by concern. M and C are gpui-fast-macos, A is gpui-fast-apple, and B is the editor crates.

| prefix | keyboard | mouse | menus | windows | geometry | lifecycle | unrelated | rows |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| M (window.rs, responders.rs, gpui_macos.rs) | 15 | 4 | 0 | 0 | 0 | 0 | 0 | 19 |
| C (fast/composition.rs) | 6 | 7 | 0 | 0 | 0 | 0 | 0 | 13 |
| A | 0 | 0 | 0 | 0 | 0 | 0 | 39 | 39 |
| B | 0 | 0 | 0 | 0 | 0 | 1 | 13 | 14 |
| all | 21 | 11 | 0 | 0 | 0 | 1 | 52 | 85 |

Kinds among the 32 gpui-fast-macos rows: LOGIC 14, LOGIC (state) 4, HOOK 11, n/a 3 (inits, tests). M16
and C03 count as LOGIC and ask the app as well.

The fork has no patch for menus, for windows and panels (Space, level, key, parent and child), or for
geometry and pixel rounding. All of that is Koyo's, through raw `NSWindow` access
(`koyo-rs/koyo/src/native/window.rs`, `native/menus/`, `native/surface.rs` `on_whole_points`), or
upstream's: levels at window.rs:1246-1272, `NonactivatingPanel` at :1043, the spurious-key fix at
:3346-3358, and `window_rect` at composition.rs:920-930, which converts device pixels to points
without rounding.

## 3. The hook surface Koyo uses

| API | signature | where | Koyo calls it at |
| --- | --- | --- | --- |
| `gpui_macos::responders` | `pub mod` | src/gpui_macos.rs:30 | `koyo-rs/koyo/src/keyboard/appkit.rs:14` |
| `Responders::changed` | `fn changed(&self, window: &AnyObject, before: *mut AnyObject, after: *mut AnyObject)` | src/responders.rs:19, called from window.rs:328 | `appkit.rs:184` → `keyboard::hooks::heard` (`hooks.rs:109`) |
| `Responders::pressed` | `fn pressed(&self, window: &AnyObject, over: *mut AnyObject, button: isize)` | responders.rs:24, called from composition.rs:278 (by :292, :369) | `appkit.rs:194` → `hooks::pressed` (`hooks.rs:146`) |
| `Responders::key` | `fn key(&self, window: &AnyObject)` | responders.rs:27, called from composition.rs:233 | `appkit.rs:206` → `hooks::at_key` (`hooks.rs:190`) |
| `Responders::gpui_first` | `fn gpui_first(&self, window: &AnyObject, keystroke: &gpui::Keystroke) -> bool` (default `true`) | responders.rs:31, called from window.rs:2762 | `appkit.rs:212` → `hooks::gpui_first` (`hooks.rs:182`): `!in_page \|\| reserved(keystroke)` |
| `set_responders` | `pub fn set_responders(responders: Rc<dyn Responders>)` | responders.rs:42 | `appkit.rs:297` (in `install`) |
| `Offers`, `Offerer`, `Answer` | `pub fn again(&mut self, at: f64, from: Offerer) -> Option<Answer>`, `pub fn first(&mut self, at: f64, gpui_first: bool) -> Answer` | responders.rs:64, 75, 95, 102, 118 | tests only: `koyo-rs/koyo/src/keyboard/tests/keys.rs:7, 32-34` |
| GPUI view `performKeyEquivalent:` (hand-back) | ObjC selector, not a Rust API | window.rs:111-114 → `handle_key_equivalent` :2624 | `koyo-rs/koyo-browser-host/src/native.rs:424` (`unhandled_key`), probe `browser/native/probe.rs:412` |
| `set_surface_capture` | `pub fn set_surface_capture(buffer: &CVPixelBuffer, request: u32)` | gpui-fast-apple metal_renderer.rs:1624 | `koyo-rs/koyo/src/shell/pictures.rs:18, 72, 145` |
| `SurfacePane`, `set_surface_pane` | `pub struct SurfacePane { pub radius: f32, pub fill: Hsla, pub edge: Hsla }`, `pub fn set_surface_pane(buffer: &CVPixelBuffer, pane: SurfacePane)` | metal_renderer.rs:1631, 1642 | `native/surface.rs:11, 91`, `native/liquid.rs:28, 261` |
| `set_surface_picture` | `pub fn set_surface_picture(buffer: &CVPixelBuffer, size: Size<DevicePixels>)` | metal_renderer.rs:1601 | no caller |
| `PopoverBackdrop`, `set_popover_backdrop` | `fn shows(&self, &mut Window, &mut App) -> bool`, `fn place(&self, Bounds<Pixels>, Pixels, &mut Window, &mut App)`, `pub fn set_popover_backdrop(impl PopoverBackdrop, &mut App)` | gpui-component cards.rs:27-43 | `native/glass.rs:9, 21, 26` |

Upstream's composition and presentation APIs are unchanged by the fork, and Koyo reaches them through core.

| API | where | Koyo calls it at |
| --- | --- | --- |
| core `Window::enable_window_composition` → `WindowComposition::create_native_surface` (core fast/composition.rs:949) → `PlatformWindow::create_native_surface` | window.rs:2164 → composition.rs:623-648. A layer-backed container `NSView` with `masksToBounds`, `addSubview:`'d to GPUI's view. | `koyo-rs/koyo/src/native/surface.rs:54, 56` |
| `PlatformSurfaceAttachment::platform_handle` | composition.rs:978, returns the container `NSView` as `usize` | `surface.rs:59`. Koyo then calls `container.addSubview(view)` itself (`surface.rs:69`). |
| `set_bounds`, `set_visible`, `Drop` | composition.rs:941, 971, 983 (`removeFromSuperview` and `release`) | through core's managed surface |
| `WindowComposition::remove_surface` | core | `surface.rs:63, 196` |
| `set_composition_order` (core calls it each frame) | window.rs:2168 → composition.rs:653-824. When order or parents change it detaches and re-adds every view (:749-755). Then it sets frames (:807-821). | not called by Koyo |
| draw in transaction | No fork API. Koyo's `native::window::draw_in_transaction` (`koyo-rs/koyo/src/native/window.rs:71`) calls `layer.display()`. That runs upstream `display_layer` (window.rs:3510), which sets `set_presents_with_transaction(true)` on every surface's renderer (composition.rs:882) around one frame. | `koyo-rs/koyo/src/browser/page.rs:165`, and `fit` at `native/window.rs:22` the same way |

## 4. gpui-fast-macos event paths, as the code runs now

`GPUIWindow` and `GPUIPanel` override `sendEvent:` with `send_window_event` (composition.rs:203-218)
and `makeFirstResponder:` (M10). `GPUIView` maps the mouse selectors, `flagsChanged:`, `keyDown:`,
`keyUp:` and `performKeyEquivalent:` to `handle_view_event` and `handle_key_*` (window.rs:111-190).
Overlay views (`GPUIOverlayView`) accept hits only inside their surface's hitboxes
(`overlay_hit_test`, composition.rs:161-191) and forward to GPUI's view (:564-568). The per-window
switch is `NATIVE_INPUT_IVAR`, which is true while the composition has native views (:801-806).

### (a) Mouse down, drag and up

1. `NSApplication` → `send_window_event` (:281). `route_keys` does nothing for mouse events (:283).
2. No native views (:290): a press is told with `over = nil` (C07, HOOK). Then `super sendEvent:`.
   AppKit delivers to GPUI's view (`handle_view_event`, window.rs:2892), and steps 6 to 8 follow.
3. Native views present. Down: the button's owner entry is cleared (:340-343, fork). `hitTest:` runs on
   GPUI's view's superview point (:363-366). `native_for_target` walks superviews to a registered native
   view, and an overlay view means GPUI (:441-456, upstream). The press is told with that target or nil
   (:368-369, HOOK). **Koyo decides here.** `hooks::pressed` may move the first responder to GPUI's view
   now (`reconcile::at_press`), or note a click on a page (`CLICKED`) that makes the page the holder if
   its view takes the first responder within a second.
4. Over a native view (:385-413, upstream except as noted): GPUI hears the press first
   (`handle_view_event(root, sendEvent:)`, :400). If GPUI stops propagation or prevents default, the
   event ends (:408-411) and the native view never sees it. **The fork decides this, from GPUI's
   result.** Otherwise the button's owner becomes the native view (:422-426, C12), `native_event` is
   marked so that GPUI's view ignores a re-delivery (:428, window.rs:2896), and `super sendEvent:`
   lets AppKit deliver to Chromium's view (:432).
5. Drag: the owner is read (:339), and the event goes to it with no hit test (:359, C09). Without an
   owner it goes GPUI's way. Neither path re-enters GPUI before AppKit, because `observe` is false
   (:385). AppKit itself sends drags to the view that took the mouse down. Up: the owner is removed and
   used (:338). GPUI hears it first (:400), and `captured && up` keeps it going on to the native view
   (:409).
6. In GPUI's view, `handle_view_event` turns ⌃-left into right (:2930-2947, upstream), and
   `lost_releases` (M19, fork LOGIC) decides which tracked presses are over. Their `MouseUp`s go first
   (:3051-3053), then the event (:3054).
7. In core `Window::dispatch_event` (core window.rs:5687), `event_target` (core composition.rs:1484)
   returns `pointer_capture` while it is set, else `native_target` against the bounds that
   `finish_input` (core :616) sampled at the end of prepaint. `dispatch_mouse_event` (core
   window.rs:6039) hit-tests (`rendered_frame.hit_test` → core composition.rs:668, where a native
   view's bounds stop the walk) and runs the capture phase. The bubble phase is skipped while the
   pointer is over or captured by a native view (`bubble_mouse` → `native_mouse`, core :1463-1482).
   Then `dispatch_native` (core :1519) sets `pointer_capture` on a native press and clears it only once
   every button counted down has a counted up (:1573-1584), or the captured surface is removed (:1058-1061). This is the crate-private rule that
   `lost_releases` works around.
8. `complete_event` (composition.rs:462) stores the result and forwards an unhandled wheel to the
   native view underneath (:483-562, upstream).

### (b) Key down, key up, flagsChanged

1. For a key down with ⌘ or ⌃ while the app is active, `AppKit`'s key-equivalent pass comes first (c,
   source 1) `[inferred]: AppKit behaviour, not fork code`. If no view takes it, `send_window_event`
   runs.
2. `route_keys` (:230). Key down or up: `Responders::key` (:233, HOOK). **Koyo decides here.**
   `hooks::at_key` → `reconcile::at_key` moves the first responder to what the holder wants, unless
   text is being composed. The move goes through M09, so `changed` fires as well.
3. Key down only: `offer_first` (:254, fork LOGIC). If ⌘ or ⌃ is held and the first responder isn't
   GPUI's view, it offers the event to GPUI's view as `performKeyEquivalent:` with the Router flag, and
   YES swallows the event (:283-284).
4. Otherwise the event isn't a pointer event (:316), and `super sendEvent:` delivers it to the first
   responder. GPUI's view (`handle_key_down` and `handle_key_up`, window.rs:2632-2638 →
   `handle_key_event(false)`) drops a key down that repeats the last dispatched key equivalent at the
   same time (M15), then runs the IME or GPUI dispatch (:2779-2880, upstream). Chromium's view handles
   it itself, and if the page leaves it, see (c) source 3.
5. `FlagsChanged`: when the first responder isn't GPUI's view, `route_keys` also sends it to
   `handle_view_event(root, flagsChanged:)` (:238-243, fork LOGIC). GPUI drops a repeated modifier
   state (window.rs:3028-3044, upstream). Then `super sendEvent:` delivers it to the first responder as
   usual. `Responders::key` is not called for modifiers.

### (c) performKeyEquivalent

Three sources reach `handle_key_equivalent` (window.rs:2624) → `handle_key_event(true)`:

1. `AppKit`'s pass over the key window's views, only while Koyo is the active app. Offerer is
   `AppKit`.
2. `offer_first` (composition.rs:268). Offerer is `Router` (`koyo_router_offering`, :267-269).
3. A page's hand-back. `koyo-browser-host` `unhandled_key` (native.rs:409-427) calls
   `performKeyEquivalent:` on the page's home view with the same `NSEvent`. Offerer is `AppKit`, so
   sources 1 and 3 can't be told apart.

`handle_key_event` (window.rs:2743-2770, M16): `Offers::again(timestamp, from)` answers a repeated
event without asking. A first offer drops the lock and asks `Responders::gpui_first`. **Koyo decides
here**: true when GPUI's view holds, or when the key is reserved (`keyboard/reserved.rs`).
`Offers::first` records the owner. `Decline` returns NO. `Dispatch` stores `last_key_equivalent`
(:2770) and runs GPUI dispatch. A key equivalent with modifiers other than fn never reaches the input
handler (:2869-2871, upstream), which is why a page's hand-back can't type twice.

## 5. What could move to the app, and what has to stay

Fork decisions that could live in Koyo behind a small hook:

1. **The whole `Offers` state machine (M06, M11, M12, M16).** The fork could report
   `offered(window, timestamp, from, keystroke) -> Answer` and keep only the Router flag. Koyo already
   drives `Offers` from its own tests (`keyboard/tests/keys.rs`). `[inferred]` Risk in the current
   split: a key with no ⌘ or ⌃ (⌥-arrows, Escape, F-keys) that a holding page leaves reaches GPUI's
   view with the hand-back as its first offer, unless `AppKit`'s pass offered it earlier. Then
   `gpui_first` is false (the page holds and the key isn't reserved), `Offers::first` declines, and the
   key goes nowhere. Docs §10 says every other key Koyo binds is Koyo's once the page leaves it. The
   tests cover only ⌘ keys with an earlier offer. Not run.
2. **`offer_first`'s policy (C04):** the ⌘/⌃ mask and the "first responder isn't GPUI's view" test.
   Either fold it into `gpui_first`, or add `Responders::offer_first(window, event) -> bool`.
3. **Veto on the first responder (M09).** `changed` reports after the fact, so Koyo puts a refused
   page back "on the next turn" (docs/keyboard.md §5), and a page holds the first responder for that
   turn. A `should_change(window, to) -> bool` asked before `super` would refuse it in place.
   `[inferred]` Chromium may not expect `makeFirstResponder:` to fail.
4. **Press outcome (C10).** The press is told before GPUI's capture phase decides whether the native
   view gets it (:400-411), so Koyo infers "the person clicked the page" from a first-responder change
   within a second (`hooks.rs:146-176`). A second hook after :411 or :432 saying "delivered to native
   view X" or "kept by GPUI" would replace the time window.
5. **The `koyo::input` log line in `lost_releases` (M19 :3115).** An app log target inside the fork.
   It could be `Responders::lost_release(button)`.

What upstream's structure keeps in the fork:

1. **Interposition points are the fork's own runtime classes.** `sendEvent:`, `makeFirstResponder:`
   and `performKeyEquivalent:` are methods of `GPUIWindow`, `GPUIPanel` and `GPUIView`, registered by
   the fork's `#[ctor]` (window.rs:101-309, 435-451, composition.rs:203-218). Koyo could only reach
   them by swizzling those classes. An `NSEvent` local monitor runs before the window and never sees
   the hit test or GPUI's result.
2. **Synthetic releases must enter GPUI's platform callback.** `MacWindowState.event_callback` and the
   window-state mutex are crate-private, and the releases must reach GPUI before the event they precede
   (window.rs:3049-3054). So `lost_releases` stays, or moves to the root. The root is in the unvendored
   core: `dispatch_native` keeps `pointer_capture` until every press has its release (core
   composition.rs:1573-1584), and `native_mouse` and `event_target` then send every press to the
   native view. A root fix means vendoring gpui-fast core too.
3. **Press routing (C08 to C12) runs inside `sendEvent:`, before `AppKit` dispatch**, on
   `fast_composition.native_views` and `native_event`, which are crate-private. It is mechanism, not app
   policy, and it fits the fork.
4. **Where GPUI takes a native view to be** is core's (`finish_input` at prepaint end, `hit_test`).
   Koyo must place views during prepaint (`native/surface.rs` `place`, `browser/page.rs`
   `place_native`) and cannot change that rule without vendoring core.
5. **`set_composition_order` re-mounts every view when the order changes** (composition.rs:743-755,
   upstream). Its own comment says this costs mouse tracking and the first responder.
   `[inferred]` A page view that holds the keyboard is detached and re-added whenever the surface tree
   or order changes, for example when an overlay surface is created. Whether `AppKit` reports the
   resulting first-responder loss through `makeFirstResponder:` (and so through M09) is not known from
   code, and should be tested.
6. **No rounding where native frames are set** (`window_rect`, composition.rs:920-930). Whole-point
   placement stays Koyo's (`native::surface::on_whole_points`). Moving it into `MacNativeSurface::set_bounds`
   (:941) is possible, but only Koyo knows which edges are pane edges.

## 6. How upstream tests run

- **gpui-fast-macos**: `autotests = false`, `doctest = false` (Cargo.toml.orig:15, 21). Only lib unit
  tests exist: `display_link.rs:534`, `pasteboard.rs:304` (real `NSPasteboard`s with unique names,
  `Pasteboard::unique` :41), `window.rs:4191` (`display_id_for_screen` against a fake `NSObject`
  subclass built with `ClassBuilder`), and Koyo's `responders.rs:129`. No test makes a `GPUIWindow` or
  sends an event through `send_window_event`, `handle_view_event` or `handle_key_event`.
- **gpui-fast-apple**: `metal_atlas.rs:278` and `text_system.rs:783` unit tests, and
  `MetalHeadlessRenderer` behind `test`/`test-support`/`bench-support` (re-exported at
  gpui-fast-macos `gpui_macos.rs:23-24`).
- **core TestPlatform**: test windows keep `PlatformWindow`'s defaults, so `enable_window_composition`,
  `create_native_surface` and `set_composition_order` return `unsupported` (core platform.rs:1312-1325).
  Core's 16 composition tests (core fast/composition.rs:1620-2150) reach into crate-private state
  (`window.fast_composition.0.borrow_mut()`, `TestPlatformSurface` :1714) and can't be written
  outside core.
- **The only real-`NSWindow` harness is `VisualTestPlatform`** (core platform/visual_test.rs, macOS
  with `test-support`). `open_window` delegates to the real `MacPlatform` (:130-136), so the window is a
  real `GPUIWindow` with Metal. But `VisualTestAppContext::simulate_*` call `window.dispatch_event`
  directly (core app/visual_test_context.rs:326-328), which bypasses every fork path in §4. Koyo uses it
  in `koyo-rs/koyo/src/visual.rs`, run as app modes with `--features visual-check` because macOS makes
  windows only on the main thread, and in `koyo-rs/koyo-bench/src/harness.rs:71`.
- **Koyo's own**: `keyboard/tests/fake.rs` fakes `AppKit` for the reconciler. The debug probe
  (`KOYO_NATIVE_PROBE`, `koyo-rs/koyo/src/browser/native/probe.rs:295-432`) builds `NSEvent`s and calls
  `[window sendEvent:]`, which does run `send_window_event`, but its mouse events carry no
  `buttonNumber` for other buttons (docs/native-pages.md:383-391). `[inferred]` A test that runs the
  fork's real path needs a real `GPUIWindow` (VisualTestPlatform's open path in a main-thread app mode)
  plus constructed `NSEvent`s sent with `sendEvent:`. Nothing like that exists today.
