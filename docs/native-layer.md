# The native layer: one owner for the seam between GPUI and Chromium

Written 2026-11-16 on branch `schulzfel/browser-ui-fixes` at `ce2f3c7fc`. Drafted with Claude.
This is a design for review. Nothing in it is built. It replaces fixing seam bugs where their
symptoms show with one layer that owns the seam, and it says how to get there in steps that
each end in a check.

Status marks, as in `docs/native-pages.md`: `[read]` (taken from source, not run), `[proven]`
(run, evidence named), `[proposed]`, `[open]` (not known yet, with the step that settles it),
`[owner]` (the owner's to decide).

## 1. Summary

Koyo's browser pages are Chromium's own `NSView`s inside GPUI's `NSWindow`. GPUI assumes it
owns the window. Chromium's views assume the same. Each UI bug of the last two days sits where
the two meet, and each was fixed at its symptom.

The design has five parts.

1. **One layer, the seam, owns everything a native view touches.** It is a crate of its own,
   `koyo-seam`, under both the app and the browser host. It owns six concerns: the keyboard,
   mouse presses, menus, windows and panels, geometry, and the native views' lifecycle. Nothing
   else in Koyo calls `AppKit` for these, and a lint keeps it so.
2. **Input is labelled once, where it enters.** Every rule reads the label. No rule works out
   on its own whose input it is.
3. **The vendored GPUI crates keep hooks only.** They report events and ask for decisions.
   They hold no policy. Of the fork's 18 deciding sites today, the policy leaves and the
   mechanics stay (section 6.1).
4. **A test bench sees the seam.** Pure tests run the code the hooks run. A rig runs real
   `AppKit` events through the real window code with a stand-in page. Scripted sequences on
   the agent desktop assert who holds the keyboard and which windows exist.
5. **A rule.** A seam bug is fixed in the layer, with a bench script that reproduces it.

What changes for the owner: the classes of bug in section 2 stop coming back in new shapes,
because each has one owner and one test that fails when it returns. What the next engineer
inherits: one crate to read for any question about focus, presses, menus, panels, frames or a
page's views, and a bench that says whether a change broke the seam.

`ce2f3c7fc` is undone by a forward commit first, and its behaviour comes back inside the layer
(section 8).

## 2. The inventory

Three inventories were taken at `ce2f3c7fc`, read-only, each with the shell commands that
reproduce its counts. They are beside this file.

| File | Covers |
| --- | --- |
| `docs/native-layer/inventory-vendor.md` | The vendored crates, diffed against the pristine crates.io packages |
| `docs/native-layer/inventory-app.md` | `koyo-rs/koyo/src` |
| `docs/native-layer/inventory-host.md` | `koyo-rs/koyo-browser-host`, and the three ways an agent's input enters |

The counts differ from the brief's (68 marked sites in 11 files, 31 app files). The brief's
figures could not be reproduced by any grep tried. The figures below can.

### 2.1 Where the seam is touched today

| Concern | Vendored GPUI (patch sites) | App (`koyo/src`, table rows) | Browser host (touchpoints) |
| --- | --- | --- | --- |
| 1. Keyboard and first responder | 21 | 11 | 7 |
| 2. Mouse press ownership | 11 | 3 | 1 |
| 3. Menus | 0 | 12 | 0 |
| 4. Windows and panels | 0 | 15 | 22 |
| 5. Geometry | 0 | 12 | 7 |
| 6. Native views' lifecycle | 1 | 9 | 12 |
| Native views in GPUI's window that are not pages (sliders, switches, glass) | 0 | 12 | 0 |
| Not the seam | 52 | 21 | 16 |
| Total | 85 | 95 | 65 |

- **Vendored GPUI.** 70 `Koyo:` marks in four crates. 85 patch sites against upstream, of which
  33 are the seam's. All 32 keyboard and mouse sites are in `gpui-fast-macos` (`window.rs`,
  `fast/composition.rs`, `responders.rs`). Of those 32, 18 decide something, 11 are hooks, and
  3 are neither. `gpui-fast-apple`'s 39 sites are the renderer's and not the seam's.
  `gpui-component` has one seam hook, `PopoverBackdrop`.
- **App.** 35 files touch `AppKit`, CoreGraphics or the ObjC runtime. 13 are seam files, 9 put
  a native view in a GPUI window, 13 are not the seam.
- **Host.** 65 touchpoints in 9 files. It patches two classes for the whole process: CEF's
  window class (`canBecomeKeyWindow`, `canBecomeMainWindow`, `toggleFullScreen:`) and Koyo's
  `NSApp` class (`sendEvent:`, `activate`, `activateIgnoringOtherApps:`). The app patches a
  third set, on `NSMenu` and `NSPopUpButtonCell`.
- **`makeFirstResponder`.** Ten lines in `koyo-rs`. Five ask for a move: three in the app's
  reconciler (`keyboard/appkit.rs:131,137,139`) and two in the vendored window
  (`window.rs:674` after simple full screen, `window.rs:1232` at open), outside the reconciler.

### 2.2 What the inventory shows

Each finding is `[read]` unless marked.

1. **Whose input it is gets answered in four places, four ways.** The vendored hit test tells
   the keyboard of every press as the person's (`fast/composition.rs:274-279`). The menu
   gate's event monitor reads the event's source and poster (`native/menus/appkit.rs:130-165`).
   GPUI's capture phase tells the page's lease that any press over the page is the person's
   (`browser/input.rs:23-38`). The host counts any key with a system event behind it as typed
   by the person (`pane/keys.rs:33-36`). Only the gate looks at who posted the event. So one
   posted click can be refused its menu, give its page the keyboard, and lock agents out of the
   page for five seconds. Inferred from the code, not run.
2. **The menu gate judges a window's last input, not the event that asked for the menu.** A
   click through the page's protocol or a probe press leaves no note. A menu they cause
   inherits whatever the person last pressed in that window within five seconds. The page's
   lease covers the protocol's clicks. Nothing covers the probe's. Inferred.
3. **The agent desktop's host posts the person's relayed input and the agent's acts through
   one private event source with no tag** (`koyo-desk-host/src/input.rs:97-100`). Nothing in
   the receiving process can tell the two apart.
4. **`ce2f3c7fc`'s `Offers` cannot tell `AppKit`'s pass from a page's hand-back.** Both arrive
   as `performKeyEquivalent:` on GPUI's view (`koyo-browser-host/src/native.rs:424`). A key
   with no ⌘ or ⌃ that a page hands back is offered there for the first time, the app says the
   page comes first, and the key goes nowhere. This is the review's second bug, found again
   from the code.
5. **The root of the dead UI after a menu is in a crate that is not vendored.** gpui-fast's
   core keeps a press captured by a native view until it has counted a release for every
   button (`dispatch_native`, core `fast/composition.rs:1573-1584`). `lost_releases` in the
   fork works around it by telling GPUI of releases it never heard.
6. **Upstream detaches and re-adds every native view when the order of surfaces changes**
   (`set_composition_order`, `fast/composition.rs:743-755`). A page that holds the keyboard
   loses it then. Whether `AppKit` reports that through `makeFirstResponder:` is `[open]`
   (step 7).
7. **Several `AppKit` settings have more than one writer.** A floating window gets "join all
   Spaces" from the vendored window and then `float_on_its_space` overwrites it. About eight
   sites move the key window, and the keyboard hears of only its own. The process has two
   local event monitors, two observers of a menu starting to track, and four ways to name a
   window (GPUI's id, the `NSWindow` pointer, `windowNumber`, `AnyWindowHandle`).
8. **A hosted `NSSlider` or `NSSwitch` can take the first responder on a click.** The keyboard
   then calls it astray and puts it back a turn later. Nothing sets `refusesFirstResponder`.
   Inferred.
9. **DevTools are an ordinary page to the host, which has no notion of a tab.** A page's views
   go to whichever container last called `native::place` for its id, and a release from any
   other container is ignored (`native.rs:283-326`). Two drawn sites for one page in one frame
   would pass the views back and forth. No single-frame path that puts DevTools in another tab
   was found. The candidates are in `inventory-host.md` section 3, all inferred.
10. **No test runs the seam's real code.** `keyboard/appkit.rs`, `browser/native/`,
    `place_native` and `draw_in_transaction` are compiled out of tests. The menu tests cover
    the pure verdict only. `ce2f3c7fc`'s key tests re-implement `window.rs:2754-2765` and
    script the order of offers. Upstream's only real-window harness calls `dispatch_event`
    directly and skips `sendEvent:`.

## 3. The layer

### 3.1 Where it lives

A new workspace crate, `koyo-rs/koyo-seam`. `[proposed]`

- It depends on `objc2`, `objc2-app-kit` and `objc2-foundation`. It does not depend on GPUI or
  on CEF.
- `koyo-browser-host` and `koyo` both depend on it. Today the host reaches the app's keyboard
  through a function pointer (`native::set_keyboard`) because the keyboard lives above it. With
  the layer below both, the host calls it directly.
- The vendored `gpui-fast-macos` does not depend on it. The fork exposes one hooks trait
  (section 6), and the app installs an adapter of a few lines that forwards to the layer.

A crate and not a module of `koyo`, for three reasons. The host needs it. A lint can fence a
crate (section 6.3). And its tests can run real `AppKit` on the main thread without building
the app.

### 3.2 What it holds

All state is on the main thread, in one value. `[proposed]`

```rust
pub struct Seam {
    windows: HashMap<WindowNumber, WindowSeam>,  // one identity: AppKit's windowNumber
    pages: HashMap<PageId, Page>,                // every native page, shown or not
    app: AppState,                               // in front or not, menus tracking, the desk host's pid
    journal: Journal,                            // the last labelled inputs (section 5)
    counters: Counters,                          // each backstop that fired (section 9.5)
}

struct WindowSeam {
    kind: WindowKind,                            // Main, Float { kind, parent }, Holder(PageId), Chromium, System
    keyboard: Keyboard,                          // today's Held: surface, key, dialog, menus, tabs
    presses: HashMap<Button, Press>,             // who took each button's press, and its label
    natives: Vec<Native>,                        // each native view in it: Page, Control, Glass
    keys_down: HashMap<KeyCode, Went>,           // where each key's last key-down went, for its key-up
}

enum PageState {
    Asleep,                                      // views out of every window
    Parked,                                      // views in its holder, the holder off the screen
    Shown { window: WindowNumber, container: ViewId, frame: WholeRect },
    Closed,
}
```

`AppKit` is behind one trait, as the keyboard's is today (`keyboard/reconcile.rs`), widened to
the six concerns. The real one is the only code that sends `AppKit` a message for them. Tests
have a fake for the pure tier and the real one for the rig (section 9).

### 3.3 Ownership

| Concern | The layer owns | Moves in from | Stays outside |
| --- | --- | --- | --- |
| Keyboard | The holder per window. The one mover of the first responder. Every key's route (section 4.1) | `koyo/src/keyboard/*`, `gpui_first` and `Offers` from the fork, `unhandled_key` from the host | What the person asked for (⌘L, ⌃⇥, a tab shown): the shell still says it, through `seam::keyboard::set` |
| Presses | Whose each button's press is. Which presses are over (section 4.2) | `hooks::pressed`, `CLICKED`, the fork's `lost_releases` rule | The mechanics of routing an event to a view, which need the fork's private state |
| Menus | The gate. The patches on `NSMenu` and `NSPopUpButtonCell`. The debug net | `koyo/src/native/menus*` | Building a menu's rows |
| Windows and panels | The registry of windows by kind. Space, level, parent, `hidesOnDeactivate`. Every move of the key window. The holders. The `NSApp` patches | `native/window.rs` (`float_on_its_space`, `show`), `window_glass::borderless`, `keyboard/appkit.rs` (`make_key`, `closing`), host `native/holder.rs`, host `embedded/nsapp.rs` | Opening a GPUI window, which is GPUI's. A window's look (glass, callout mask) |
| Geometry | One function from a pane's bounds to a native frame. Every `setFrame:` on a native view or holder. Presenting in the transaction | `surface::on_whole_points`, `Surface::place`, `NativePage::place`, host `native::place`'s frames, `draw_in_transaction`, `fit` | Layout, which is GPUI's |
| Lifecycle | Each page's state (asleep, parked, shown, closed) and the only `addSubview:` and `removeFromSuperview` on a native view | `native/surface.rs` (`new`, `remove`, `keep`), host `native.rs` (`place`, `release`, `settle`, `closed`) | Making the browser and its views, which is CEF's |

### 3.4 Invariants

Each is checked by a pure test, and where it can break at run time, by a debug assertion that
logs and counts. `[proposed]`

**Keyboard**

- K1. A window's first responder is what its holder wants, at every key and press. Only the
  layer calls `makeFirstResponder:`.
- K2. One key-down has one owner at a time. GPUI gets it first or the native view does. The
  other gets it only if the first had no use for it, and only if it still has the keyboard.
- K3. A key-up goes to everyone who got its key-down, and to nobody else.
- K4. A native view leaving its window never holds the first responder as it leaves.
- K5. A native view that is not a page never holds the first responder.

**Presses**

- P1. A press owns its button. Its drags and its release go to whoever took the press.
- P2. No press outlives its button. When a menu or a drag session ends, the presses it
  swallowed the releases of are over, and GPUI has been told.
- P3. A press GPUI kept never reaches a native view.

**Menus and panels**

- M1. No native menu appears on the screen the person sees unless the person's own input, in a
  window on that screen, asked for it (`docs/browser.md` 14, unchanged).
- M2. No floating panel of Koyo's appears on a Space other than its parent window's.

**Windows**

- W1. A holder is never key or main, takes no mouse, and sits a level under ordinary windows.
- W2. Every change of the key window made by Koyo goes through the layer, and every change
  made by anything else is heard by the layer from `AppKit`'s notifications.
- W3. Each window has one identity in the layer, its `windowNumber`.

**Geometry**

- G1. A native view's frame is on whole points.
- G2. A page's frame covers its pane within the clip, with at most one point over at an edge
  and never under.
- G3. A native view moves in the same transaction as the GPUI frame that moved its pane.

**Lifecycle**

- L1. A page's views are in at most one container. A second claim in the same frame is
  refused, logged and counted.
- L2. A page shown is in the window and under the tab that drew it. The layer records which,
  and the probe can read it.
- L3. A closed page has no views, no holder and no entry.

## 4. The two hard cases

### 4.1 Keys: one decision per key, and no memory of offers

Today a key-down can reach GPUI's view three ways, and `Offers` remembers one event to keep
them apart. The design removes the need to remember.

There are three entries, and each is labelled by where it comes from.

| Entry | Who calls it | Today |
| --- | --- | --- |
| `Pass::Equivalent` | `AppKit`'s pass of key equivalents over the key window's views, only while Koyo is in front | `performKeyEquivalent:` on GPUI's view |
| `Pass::Window` | The window's `sendEvent:` for a key going down or up | `route_keys`, `offer_first` |
| `seam::page_left_key(page, event)` | The host, when a page leaves a key | `performKeyEquivalent:` on GPUI's view, which can't be told from the first entry |

The first two ask one pure function. The third is a call of its own.

```rust
pub enum KeyRoute {
    GpuiFirst,  // Offered to GPUI. If GPUI has no use for it, it goes on as AppKit would send it.
    Deliver,    // AppKit delivers it to the first responder, as it would have.
}

pub fn route(key: &Key, pass: Pass, first: Responder, kept: bool) -> KeyRoute
```

`first` is the window's first responder after K1 has been enforced for this key, so the
function reads what is true and not what the holder hopes. `kept` is the policy below.

| First responder | Key | `Pass::Equivalent` | `Pass::Window`, down | Hand-back |
| --- | --- | --- | --- | --- |
| GPUI's view | any | `GpuiFirst` (upstream's behaviour) | `Deliver` (to GPUI's view) | never happens |
| A page | one Koyo keeps | `GpuiFirst` | `GpuiFirst` | happens only if GPUI had no use for it and the page left it too. GPUI is asked again and has no use for it |
| A page | any other | `Deliver` | `Deliver` (to the page) | GPUI gets it, if that page still has the keyboard |

Why this needs no memory. A key Koyo keeps is offered to GPUI at whichever entry sees it
first. If GPUI takes it, it is never delivered, so no later entry sees it. If GPUI has no use
for it, asking again gives the same answer. A key Koyo doesn't keep is never offered before
the page has had it, and the hand-back is the one way it reaches GPUI.

**Key-ups.** The layer keeps, per window, where each key code's last key-down went
(`keys_down: Map<KeyCode, Went>`), written at every key-down and so never stale. A key-up
goes to everyone who got its key-down: the page if it was delivered, and GPUI too if GPUI took
it at once or from a hand-back. This is K3. `AppKit` sends no key-up for a key let go while ⌘
is held, which is why the entry is overwritten by the next key-down and not kept until an up.

**The hand-back.** `seam::page_left_key(page, event)` checks that `page` still has the
keyboard in its window. If it does, the layer calls the fork's `hand_back(view, event)`, which
runs the same key-down path a key equivalent runs today, and marks the key code as GPUI's too.
If the keyboard has moved since (the person clicked the composer while the renderer was
thinking), the key is dropped and counted. A key is never given to an owner that didn't have
it when it was pressed.

**Which keys Koyo keeps.** Decided by the owner, 2026-11-16: policy A. The layer takes the
policy as one predicate, and two fit it.

| Policy | Koyo keeps | For | Against |
| --- | --- | --- | --- |
| A. As the owner's build behaves today with Koyo in front | Every ⌘ or ⌃ key GPUI has a binding for | No change for the owner. ⌘R and ⌥⌘I work when a renderer hangs. No shortcut of Koyo's waits for a trip to the renderer. A ⌘ key Koyo doesn't bind goes on to the page | A web app's own shortcut never reaches it where Koyo binds the same key (⌘F), as today |
| B. Chrome's split, as `ce2f3c7fc` made it | The eight tab and window keys of `reserved.rs` | Web apps get their own ⌘ shortcuts | ⌘F, ⌘R, ⌘G, ⌘=, ⌘-, ⌘0, ⌘[, ⌘], ⌥⌘I, ⌥⌘R and ⌘C/X/V/A/Z wait for the renderer. A page can block them. A hung renderer never hands them back |

A makes the behaviour with Koyo behind the same as with Koyo in front, which is the bug
`ce2f3c7fc` set out to fix, and changes nothing else. B stays possible later by changing the
predicate. Under A, "GPUI has a binding" is
found by offering: `GpuiFirst` hands the key on when GPUI has no use for it, so ⌘`, ⌘H, ⌘M and
the menu's keys go on working.

What this fixes, by construction:

- **⌘L reaching both.** With Koyo behind, the window entry offers it to GPUI first, and GPUI
  takes it. This is what `AppKit`'s pass does with Koyo in front.
- **A page handing a key back late.** Each hand-back is judged alone, against who has the
  keyboard then. This was the first bug in `ce2f3c7fc`'s review.
- **F12 and bindings without ⌘ or ⌃ while Koyo is not in front.** They go to the page, and
  the hand-back gives GPUI whatever the page leaves, with no "first offer" to misjudge. This
  was the second bug.
- **⌃⇥'s key-up reaching the page.** Its key-down went to GPUI alone, so its key-up does.

What it doesn't change: a key a page hands back runs GPUI's whole key dispatch.
The fork can't run bindings alone, since its only way into GPUI is the window's event
callback, and the core is not vendored. That callback runs GPUI's whole key dispatch, its
interceptors and key-down listeners with it. What the fork can do, and now does, is keep such
a key from the input context, which is where a key with no character (Escape, the arrows) went
first, and from where the text system typed it into the page. Koyo's arrow
repeater (`key_repeat.rs`) starts on a handed-back arrow and stops on a key-up at GPUI's view.
Today that key-up goes to the page alone. The key-up rule above sends it to GPUI as well.
Inferred from the code, and a sequence in step 5 checks it (⌥→ held in a page).

`AppKit`'s pass asks GPUI's view and never Chromium's. `[read]` A page's views sit under
GPUI's view (`fast/composition.rs:628-640`), and GPUI's `performKeyEquivalent:` doesn't call
its superclass (`window.rs:2624-2630`), so the pass stops there.

**Built so far** (2026-11-16, ahead of step 5, for a bug the owner hit: arrows in a page's
field typed a blank). The first and third entries: `koyo-seam`'s `keys::route` decides
`Pass::Equivalent` and the hand-back, asked through the fork's `Responders::key_route`. The
hand-back is `koyoHandBackKey:` on GPUI's view. A key handed back runs through the window's
event callback and never the input context, and its key-up goes to GPUI's view too. Until step
5 the route answers by the modifiers alone (⌘ or ⌃ is offered to Koyo's bindings first), a
handed-back key is dropped only when GPUI's view has the keyboard by then, and `Pass::Window`
isn't built, so a ⌘ key still reaches both while Koyo isn't in front. `[proven]` on the agent
desktop by `script/seam-bench/arrows-in-page.sh`
(`.superpowers/v/evidence/20261116-205438-arrows-in-page/`).

The tests are section 9. The pure test walks the table for every binding in the real keymap,
under each policy. The rig sends real key events through the real `sendEvent:` with a stand-in
page that hands keys back at once, after a delay, and after the keyboard has moved.

### 4.2 Presses: a ledger with causes

The fork keeps the mechanics. It finds the view under a press, lets GPUI's capture phase see
the press first, and delivers each drag and release to whoever took the press. That needs the
fork's private state and is mechanism, not policy.

The layer keeps the ledger.

```rust
struct Press { owner: PressOwner, input: InputId, told_gpui: bool }
enum PressOwner { Gpui, Native(ViewId) }
```

- The fork reports each press twice. Once before routing (`pressed`), and once after GPUI's
  capture phase has decided (`press_routed`), saying whether a native view got it. The second
  report replaces the keyboard's one-second guess (`CLICK_LASTS`). A page that takes the first
  responder after a press that was delivered to it, with no other input since, is the
  person's. A press GPUI kept grants nothing.
- A press ends on its release, or on a cause the layer hears: a menu ended tracking, a drag
  session ended, a drag of the window ended (`performWindowDragWithEvent:` returns), a live
  resize ended, the view left the window. For each press that ended without its release, the
  layer tells the fork which buttons to release, and the fork tells GPUI. This is P2.
- Today's inference stays as a backstop: a move with no button down, or a press while the
  button is up. Each time the backstop fires with no cause known it is counted and logged. A
  bench run where it fires fails, unless the run is about the backstop.
- The fork asks `presses_over` while it holds its window's lock (`window.rs:2895,2987`). The
  answer is a read of the layer's own state and sends `AppKit` nothing.

The root in gpui-fast's core stays where it is (finding 5). Fixing it there means vendoring a
fourth crate, the largest one. `[owner]` The recommendation is not to. The ledger makes the
workaround small, tested and loud.

## 5. The input label

### 5.1 The label

```rust
pub enum Origin {
    Person,              // the hardware's, or a tool of the person's posting for them
    Relayed,             // the person's, through the agent desktop's tab
    Posted { by: Pid },  // posted by a program: the agent desktop's host for an agent, or another
    Engine,              // an agent's, through the page's protocol. No NSEvent exists.
    Probe,               // made by the in-process probe
    Koyo,                // made by Koyo itself
}

pub struct Input { id: InputId, origin: Origin, kind: Press | Key | Scroll, window: WindowNumber, page: Option<PageId>, at: Instant }
```

### 5.2 Where each input is labelled

Each path has exactly one place. `[proposed]`

| Path | Where it enters | How it is labelled |
| --- | --- | --- |
| The hardware | `NSApp`'s `sendEvent:` (already patched by the host, moves into the layer) and the fork's hooks | Its poster is neither the desk host nor Koyo: `Person` |
| The agent desktop's host | The same | The poster is the desk host: `Posted`. With the tag below: `Relayed` or `Posted` |
| Another program | The same | Poster named: `Posted { by }`, treated as the person's tool, as the gate does today |
| The page's protocol | `browser::agent::cdp`, as it sends `Input.*` | `seam::engine_input(page, kind)` writes the journal. No `NSEvent` exists |
| The probe | The probe's event helpers | They send through `seam::inject(event, Origin::Probe)`, the only way to hand a made-up event to a window |
| Koyo's own (the probe's `escape`, a synthetic release) | Where it is made | `Origin::Koyo` |

There is no ambient "current input". A label is a pure function of an event
(`seam::input::label(event)`), plus a side table for the events the probe injects. A rule that
runs inside a hook gets the event from the hook and labels that event. An ambient label would
leak: CEF's pump runs in the common run-loop modes (`embedded/pump.rs`), so during a menu's or
a window drag's tracking loop, which starts inside a press's dispatch, CEF's work would read
that press as its cause.

Rules that run later (a page's menu after a trip to the renderer, a page asking for the
keyboard) read the journal: the last input for that window or page, from any path. The journal
is fed at `NSApp`'s `sendEvent:` and at the fork's hooks, once per event.

The journal narrows finding 2 and doesn't close it. It is still "the last input", not "the
input that caused this". What changes is that a probe press and a protocol click are now in
it with their labels, so a menu they cause is judged by them and not by what the person
pressed before. A menu that a page opens by itself within five seconds of the person's press
is still taken for theirs, as today. Chromium gives no way to know the cause.

**What a posted event says of itself.** `[proven]` 2026-11-16, in the menu gate's proof
(`.superpowers/v/evidence/20261116-234153-menu-gate/`): a right-click the agent desktop's host
posted with `CGEventPostToPid` arrived in the test Koyo's event monitor with the host's
process as its poster (29277, checked against `ps`) and with a source state of 1498617647.
So the poster survives, and the state doesn't arrive as -1: a private source has a state id
of its own. The menu gate's test for -1 never matches a posted event; a posted input is
known by its poster alone. What state the hardware's own events arrive with is `[open]`: the
gate logs it at each menu it shows, so the owner's first right-click in a build with the gate
answers it. Until then no state is taken for a program's.

**The idle clock.** `[proven]` the same night: `desktop_open` and `desktop_click` reset
`HIDIdleTime` (83 s before an open, 4.6 s after), and the probe's and app control's calls
don't. A check that the person is away has to be read before a `desktop_*` call, never after
one.

**A tag from the desk host.** `[open]`, settled in step 2. The desk host would stamp each
event it posts with `kCGEventSourceUserData`: one value for an agent's act, one for the
person's relayed input. Whether that field, and the source's state and poster, arrive intact
in the receiver's `-[NSEvent CGEvent]` after `CGEventPostToPid` is not proven. A throwaway
spike that posted to its own process received no events at all and settled nothing
(`.superpowers/native-layer/spike/tag.m`). The menu gate proof (section 10) answers the state
and poster question with its first `desktop_click`. Without the tag, the two are both
`Posted`, as today.

### 5.3 What each rule reads

Two questions were mixed up until now. Who is using this window, and may this appear on the
person's screen. A test Koyo on the agent desktop is used by an agent through posted events,
and that agent is that window's user. It must still never put a menu on the owner's screen.

| Rule | `Person` | `Relayed` | `Posted` | `Engine` | `Probe` |
| --- | --- | --- | --- | --- | --- |
| A press moves the keyboard's holder | yes | yes | yes | no | yes |
| A press or key makes the page the person's (the lease) | yes | yes | yes, as today `[owner]` | no | yes, as today |
| A native menu may show (M1) | yes, if its window is on the screen the person sees | no | no | no | no |
| A floating panel may show | yes | on its parent's Space only (M2) | on its parent's Space only | n/a | on its parent's Space only |
| The press is in the ledger | yes | yes | yes | no | yes |

The lease row is today's behaviour written down. Whether a posted click should lock agents out
of a page is a product question and not this design's to change. `[owner]`

## 6. The hooks the forks keep

### 6.1 `gpui-fast-macos`

One trait replaces `Responders`, and one function goes in. `[proposed]`

```rust
pub trait SeamHooks {
    // Events out. The fork reports and does nothing else.
    fn first_responder_changed(&self, window: &NSWindow, before: Option<&NSResponder>, after: Option<&NSResponder>);
    fn pressed(&self, window: &NSWindow, event: &NSEvent, over: Option<&NSView>);
    fn press_routed(&self, window: &NSWindow, event: &NSEvent, to: Routed);   // Gpui | Native(view)
    fn released(&self, window: &NSWindow, event: &NSEvent);
    fn remounted(&self, window: &NSWindow);                                    // under the lock

    // Decisions in. The fork asks and obeys.
    fn key(&self, window: &NSWindow, event: &NSEvent, keystroke: &Keystroke, pass: Pass) -> KeyRoute;
    fn presses_over(&self, window: &NSWindow, event: &NSEvent) -> Buttons;    // under the lock
}

pub fn set_seam_hooks(hooks: Rc<dyn SeamHooks>);
pub fn hand_back(view: &NSView, event: &NSEvent) -> bool;
```

With no hooks installed the windows behave as upstream's.

The app implements the trait in an adapter of a few lines. The adapter is where GPUI's types
stop: it turns a `Keystroke` into the layer's `Key`, and a GPUI window into its
`windowNumber`. The fork makes the `Keystroke`, since its conversion from an `NSEvent` is
private to it (`events.rs:111`).

**Two hooks run while the fork holds its window's lock**, and the contract says so.
`presses_over` is asked inside `handle_view_event`. `remounted` is told inside
`set_composition_order`, which holds the lock across upstream's detach and re-add
(`fast/composition.rs:657,749-755`). Neither may send `AppKit` anything. Moving the first
responder there could make `AppKit` ask GPUI's view for its text input, which takes the same
lock (`window.rs:3989-3994`). So `remounted` only notes that the window's views were taken out
and put back, and the layer makes the first responder right on the next turn of the run loop
and, by K1, before the next key or press whichever comes first.

What the fork keeps, all mechanism:

- the overrides of `sendEvent:` and `makeFirstResponder:` on its two window classes, which
  call the hooks;
- press routing by button (`native_presses`), since it needs the composition's private state;
- telling GPUI of the releases `presses_over` names, since that needs the window's private
  callback;
- the modifiers handed to GPUI's view while another view has the keyboard;
- matching a key equivalent to its key-down by the event's time;
- obeying `KeyRoute` and sending a key-up to GPUI's view when the layer says GPUI had its
  key-down.

What leaves the fork:

| Leaves | Goes to |
| --- | --- |
| `responders::Offers`, `Offerer`, `Answer`, `Owner`, and their tests | Nowhere. Section 4.1 needs no memory |
| `offer_first`'s ⌘ and ⌃ rule, `koyo_router_offering` | `seam::keys::route` |
| `Responders::gpui_first` | `seam::keys::route` |
| `lost_releases`' rule for which presses are over | `seam::presses` |
| The `koyo::input` log line | The layer's log |
| The two `makeFirstResponder:` calls at open and after simple full screen | They stay, and the layer hears them as changes, as today |

Size. The fork's seam patch is +433 -38 against upstream today. Taking out the `Offers`
mechanism removes about 155 lines, and the new hooks add some back. An estimate, not a
measurement: about 300 lines, all of it hooks and mechanism. The fence (6.3) fixes the list of
marks allowed, which is the number that matters.

### 6.2 The other crates

- `gpui-component` keeps `PopoverBackdrop`, its one seam hook.
- `gpui-fast-apple` has no seam patch and gets none.
- gpui-fast's core is not vendored and stays so (section 4.2).
- The browser host keeps CEF's side: making the browser and its views, the handlers, the
  protocol bridge. Its `native.rs` becomes a thin caller of the layer for where the views go.
  The host also runs as a process of its own for the engine's smoke suite (`mac_app.rs`),
  with its own `NSApp` class and no GPUI window. The layer works there with no adapter
  installed: pages are parked or asleep, and the `NSApp` patch is installed only when the host
  is embedded.

### 6.3 The fence

A script, `script/seam-fence`, run by `script/lint`. `[proposed]`

- It fails when a file outside `koyo-seam` names a fenced selector or call:
  `makeFirstResponder`, `makeKeyWindow`, `makeKeyAndOrderFront`, `orderFront`, `setLevel`,
  `setCollectionBehavior`, `setHidesOnDeactivate`, `addChildWindow`, `popUpMenuPositioningItem`,
  `popUpContextMenu`, `addLocalMonitorForEvents`, `addSubview` and `removeFromSuperview` on a
  native view, `setFrame` on a native view or window, `sendEvent`, `postEvent`, `activate`.
- It has an allow-list with a reason per entry (the probe's reads, the simulator's trackpad
  monitor, a window's look).
- It fails when the vendored crates' seam files hold a `Koyo:` mark that is not a hook, by a
  list of the marks allowed.
- It starts in report mode in step 2, printing today's counts as the baseline, and fails the
  lint from step 10.

## 7. Windows, panels and the rest of the six concerns

The keyboard and presses are section 4. The others in brief. Each is `[proposed]`.

**Menus.** The gate moves in as it is, and reads the journal in place of its own notes. Its
event monitor goes, since `NSApp`'s `sendEvent:` labels every event. The share picker
(`browser/site.rs:198-231`) is checked against the gate, as it pops up a system menu without
asking. Menus as data for agents stay `[not built]` and out of this design's scope.

A real Edit menu is this step's too. Built 2026-11-17 (`koyo/src/edit_menu.rs`). Koyo's menu
bar had one menu, with Settings and Quit, and `AppKit` puts its own items only in a menu titled
Edit: Emoji & Symbols (⌃⌘Space), Start Dictation and AutoFill. Without it, ⌃⌘Space in a page's
field opened nothing (the owner, 2026-11-16); the app-wide binding that opens the viewer stays
until the owner has seen the menu's own item open it, once.

- **An item presses its key.** Undo, Redo, Cut, Copy, Paste and Select All each press their
  own key in the window in front, so an item and its key are one path wherever the keyboard
  is: a page's edit through the engine, an input's own action, a terminal's.
- **The menu takes no key.** GPUI gives a menu item a key only from a binding of the item's
  action, and none is bound, so the items show no shortcut and `AppKit` can give the menu none:
  it can't take ⌘C from a page or an input, and a key Koyo binds nothing to in a page (⌘K) goes
  on to the page as before. Decided 2026-11-17: safe by construction comes before the look.
- **Later, if the shortcuts are missed:** the layer sets each item's key equivalent on its
  `NSMenuItem` after GPUI builds the menu. `AppKit` asks the window's views for a key before
  the menu, so Koyo's bindings and a page would still come first, and the menu would get a key
  only where no view takes it. That has to be seen on screen before it ships: ⌘C, ⌘V and ⌘Z in
  a page's field, a GPUI input and a terminal each done once and by the same owner as today,
  and ⌃⌘Space opening the viewer once.
- **A pick in a page goes past Koyo.** `[read]` With a page's view the first responder,
  `AppKit` sends the system's `copy:` (and `cut:`, `paste:`, `selectAll:`) to it, and the page
  does the edit itself: the same result on the page, without the pick making the page the
  person's, as its key does. Accepted: a menu opens only on the person's own input.
- The gate lets the menu bar's own menus through, as before.

**Windows and panels.** The layer registers each window as it is first seen, with a kind.
`seam::windows::float(window, parent, kind)` is the one writer of a floating window's Space,
level, parent and `hidesOnDeactivate`. A float becomes a child window of its parent as it
shows, which keeps it on the parent's Space and screen (M2). `docs/keyboard.md` 11 lists what
is not known about child windows (ordering under `hidesOnDeactivate`, full screen, the
switcher's panel that opens unseen). Those need Koyo in front and are checked in step 9 on the
bench host of section 9.4. Every `makeKeyWindow` and activation in the app goes through
`seam::windows::make_key(window, cause)`. The layer hears every other change from
`NSWindowDidBecomeKeyNotification` and `NSWindowDidResignKeyNotification`, in place of GPUI's
activation observer and a check each frame. The holders' rules (W1) and the `NSApp` patches
move in from the host unchanged.

**Geometry.** One pure function, `seam::geometry::frame(pane, clip) -> WholeRect`, with G1
and G2 as property tests. The layer sets every native frame and the holder's from it. The
docked tools' size uses the same function. `seam::geometry::present_with(window)` is the one
way to present GPUI's frame in the transaction a native view moves in, replacing `fit`'s and
`draw_in_transaction`'s separate ways.

**Lifecycle.** The page state machine of section 3.2. `seam::pages::show(page, container,
window, frame)`, `park(page)`, `sleep(page)`, `close(page)`. Each transition that takes views
out of a window first moves the keyboard off them (K4) inside the same call, so the pair of
callbacks through the host (`Views::Leaving`, `Views::Arrived`) goes. L1 refuses a second
container for a page in one frame, which is the one mechanism found that could show a page's
views in the wrong place. The fork's `remounted` lets the layer put the keyboard
back after upstream's detach and re-add (finding 6). Hosted controls and glass are
registered as natives that take no keys, with `refusesFirstResponder` set (K5).

## 8. What happens to `ce2f3c7fc`

It is undone by a forward commit, and what it set out to do is rebuilt in the layer.

**Why not fix it in place.** Both review bugs come from its mechanism. `Offers` guesses who is
offering a key because a page's hand-back and `AppKit`'s pass look the same. The saved commit 6
makes the memory bigger (a ring of 64 events, a flag the host sets during a hand-back). That
treats the symptom. Section 4.1 removes the guess.

It also changed more than it said. Before it, with Koyo in front, GPUI took every ⌘ key Koyo
binds in `AppKit`'s pass, and that is how the owner's build behaves. `ce2f3c7fc` made every
key but eight the page's first. That is policy B of section 4.1, a product change the owner
didn't ask for. He has since picked policy A.

**What is undone** (migration step 1):

- in the fork: `responders::Offers`, `Offerer`, `Answer`, the `gpui_first` hook, `offer_first`,
  `koyo_router_offering`, and the `Offers` branch in `handle_key_event`. The match of a key
  equivalent to its key-down by the event's time stays, as it is from an earlier commit;
- in the app: `hooks::gpui_first`, `keyboard/reserved.rs` (it would be dead code, which the
  lint refuses in a binary crate, and it would come back with policy B), and
  the tests in `keyboard/tests/keys.rs` that test a copy of the window's logic;
- in the docs: `docs/keyboard.md` 10 is marked superseded by this file, and
  `vendor/README.md`'s "One key, one owner" bullet goes.

**What is kept:**

- the probe's `keyboard`, `bounds` and bare `key` commands, and `marks.rs` with its test;
- `docs/keyboard.md` 11 on panels and Spaces.

**What the tip is after step 1.** `d33840cad`'s behaviour for keys, which is the owner's
build's. With Koyo in front, `AppKit`'s pass gives GPUI its ⌘ keys first, and nothing is
handled twice. With Koyo behind, which happens on the agent desktop and not at the owner's
desk, ⌘L in a page reaches both Koyo and the page again. The tip is shippable from step 1.
Step 5 closes the agent desktop's case, built as section 4.1.

**The stash** (`ui-fixes-commit6-wip-20261116-1654`, `1c25b59d2`). Not used. Two of its ideas
are in this design in another form: the hand-back is marked by the host (here it is a call of
its own), and one decision function is shared by `window.rs` and the tests (here the fork has
no decision to share). Its test of the owner's typing pattern becomes a bench sequence in step
5. The stash and `.superpowers/wip/` can be dropped once step 5 is in. `[owner]`

## 9. The test bench

Four tiers. Each seam bug gets a test in the lowest tier that can show it.

### 9.1 Tier 0: pure tests in `koyo-seam`

The state and the decisions: `keys::route`, the press ledger, the holders, the gate's verdict,
`geometry::frame`, the page state machine. They run with `cargo test -p koyo-seam` against a
fake `AppKit`. These are the functions the hooks call. No glue is re-implemented, because the
fork holds no decision. The keyboard's 48 tests and the gate's 10 move here.

### 9.2 Tier 1: the rig, real `AppKit` events through the real window code

A stand-in page: an `NSView` subclass that does what Chromium's does at the seam. It takes
the first responder on its own (at once and a turn later), eats key-downs or hands them back
(at once and after a delay), opens a tracking loop on a right press so the release is lost,
and leaves its window while it has the keyboard.

- **In `koyo-seam`**, a test binary with its own `main` (`harness = false`), since `AppKit`
  needs the main thread. Real `NSWindow`s that are never ordered in. It checks the layer's
  real `AppKit` half: the first responder moves, the menu patches refuse, frames land on whole
  points, a page's views are in one container.
- **In `koyo`**, an app mode `--seam-check` behind a feature, built as the visual checks are.
  It opens a real `GPUIWindow`, puts the stand-in page in a real `native::Surface`, and sends
  `NSEvent`s through `seam::inject`, so the fork's `send_window_event`, `handle_key_event` and
  the hooks all run. This is the tier that would have caught both bugs in `ce2f3c7fc`.

`[open]`, settled in step 4: whether the app-mode rig can run with its windows ordered out. If
it needs them on a screen, it runs on the agent desktop through `desktop_open`, as the test
Koyo does.

### 9.3 Tier 2: scripted sequences on the agent desktop

The verify skill and the probe, as a suite: the sequences are the scripts in
`script/seam-bench/`, each named for the bug it reproduces, with `lib.sh` shared among them. A
sequence doesn't launch the test Koyo: only the Koyo tool `desktop_open` starts it on the
agent desktop, and that's an agent's call, not a shell's. An agent opens and confirms the
instance (the verify skill's Launch), runs the sequence against it, and makes any
`desktop_*` call the sequence asks for between its parts. A sequence drives the instance
through the probe and app control, and asserts on:

- `probe seam`, which prints what the layer keeps. Built so far: whose each window's last
  press or key was, each page's last input through its protocol, and the processes known to
  post. To come as each concern moves in: each window's holder and first responder, the press
  ledger, each page's state and container, the floats, and the counters;
- the window server's list (`.superpowers/bin/winlist` moves into the skill), for what is on
  the person's screen;
- the page's own record of keys and clicks, read through the probe's `eval`.

A sequence passes when its assertions hold and every backstop counter is zero.

The first sequences, one per bug in the brief:

| Sequence | Reproduces |
| --- | --- |
| `ctrl-tab-from-page` | ⌃⇥ from a page never switching |
| `dead-ui-after-menu` | The UI dead after a menu or DevTools |
| `key-once`, `shortcut-under-typing` | Keys typed twice, shortcuts lost under typing |
| `devtools-own-tab` | DevTools showing in another tab |
| `divider-fast-drag` | The DevTools divider not picked up |
| `composer-after-pinned` | The composer losing the keyboard after pinned tabs |
| `cmd-l-once` | ⌘L reaching the page and Koyo |
| `no-menu-for-agents`, `no-panel-off-space` | Menus and panels on the owner's screen |
| `page-covers-pane` | The white line beside a page |

What this tier can't do: on the agent desktop a test Koyo is the app in front only right after
launch (`.agents/skills/verify/SKILL.md`, Keyboard focus). `AppKit`'s pass of key equivalents,
key-window changes, `hidesOnDeactivate` and Spaces can't be exercised there.

### 9.4 Tier 3: sequences that need Koyo in front `[owner]`

These need a login session where the test Koyo can be the app in front without touching the
owner's screen. Three places it could run. Each needs the owner's approval, since each writes
outside the checkout.

| Where | For | Against |
| --- | --- | --- |
| A second macOS user on this Mac, logged in through Screen Sharing's virtual display | The real GPU and the real `AppKit`. No hardware. The bench runs over `ssh` with `launchctl asuser` | Shares the CPU and GPU with the owner's work. A user account, its grants (Accessibility) and a copy of the bundle live outside the checkout. Whether a session not on a physical display composites pages as a real one does is not known |
| A macOS virtual machine | Fully apart. Snapshots make each run start clean | About 60 GB outside the checkout. The GPU is paravirtual, and a page's compositing may differ. Focus, key window, Spaces and menus are the real `AppKit` |
| A spare Mac | The most faithful, and no load on the owner's | Hardware, and a runner to keep |

The owner wants to try a Mac mini on his network first (2026-11-16), which is the third row.
How it's reached and driven is `[open]`. Failing that, the recommendation was: try the second user first, in a one-day spike that runs `cmd-l-once` and
`no-panel-off-space` there. It costs the least and keeps the real GPU. If a session without a
physical display doesn't composite pages, a spare Mac is the durable answer, and the virtual
machine is the fallback for focus and window sequences only.

Until one exists, tier 3's sequences are the owner's to run by hand from a checklist, as
`docs/keyboard.md` 9 is today, and step 9 says which.

### 9.5 Counters

Every backstop counts each time it fires: a first responder healed at a key or press, a page
refused the keyboard, a release GPUI was told of that it never heard, a menu the debug net
cancelled, a second container refused, a frame off whole points. `probe seam` prints them. In
the owner's build they go to the log as today. A counter that moves in a bench run is a
failure. This is what makes a fix at the symptom visible: a symptom fix leaves the counter
moving.

## 10. The migration

One step per commit, or a pure move and then a change where a step moves a file other
branches edit. Each step ends with its checks green: `script/lint` by hand, the tests named,
and the bench sequences named, with evidence under `.superpowers/v/evidence/`.

| Step | Change | Ends verified by | Estimate | A build for the owner |
| --- | --- | --- | --- | --- |
| 0 | The menu gate proof on the agent desktop, as planned. No code. Done 2026-11-16: all three refused, no menu window (section 5.2) | The three right-clicks' window lists and log lines. Decides whether `d33840cad` goes into the owner's next build | an hour, once the owner is idle |  |
| 1 | `ce2f3c7fc`'s key mechanism undone by a forward commit (section 8) | The fork's diff has no `Offers`. `script/lint`, with no dead code left. The app's and the fork's tests. `key-once` on the agent desktop. The tip is shippable | half a day |  |
| 2 | The crate `koyo-seam`, empty of rules: the input label and journal, `NSApp`'s `sendEvent:` telling the layer of each press and key (the patch itself stays installed by the host until step 9), `seam::inject` for the probe, `seam::engine_input` for the protocol, `probe seam`, `script/seam-fence` in report mode, `script/seam-bench`. The desk host's tag is put off: it needs a change in the desk host and a rebuilt host, and no rule waits on it. No behaviour changes | Tier 0 for the label. The engine's smoke suite while the owner is idle, for the moved `NSApp` patch. A bench sequence `labels`: a probe click, a `desktop_click` and a `browser_click` each show their label in `probe seam` | 2 days |  |
| 3 | Menus read the label (done 2026-11-17: the gate's rule is `koyo-seam`'s and reads the journal; its monitor is gone; the desk host is known by its bundle). Still to do: The gate moves in, its monitor goes, the share picker is checked. The Edit menu, for `AppKit`'s own items (built 2026-11-17, section 7; its items on screen are the owner's check) | The gate's tests in tier 0. `no-menu-for-agents`, which is step 0's proof as a script. The owner's own right-click and a `<select>` | 1 day | yes: menus |
| 4 | The keyboard moves in: `keyboard/*` to `koyo-seam`, with the registry of pages (which page a view is, and a page's keys' view, registered by the host as it makes and closes them), which the keyboard is the first to read, in place of the host's `key_view` and `page_of`. `press_routed` in place of the one-second window. Controls refuse the first responder. Key and active state still come from GPUI's observer, until step 9 | The 48 tests moved. The rig's first version. `composer-after-pinned`, `ctrl-tab-from-page`, and `docs/keyboard.md` 9's journeys 1 to 5, 7, 8 and 11 as sequences | 3 days |  |
| 5 | Keys: `keys::route`, `page_left_key`, the fork's `hand_back`, key-ups (section 4.1), under policy A | The table test over the real keymap. The rig with hand-backs at once, late, and after the keyboard moved, and with marked text. `cmd-l-once`, `key-once`, `shortcut-under-typing` with the owner's typing pattern, F12 while not in front, ⌥→ held in a page. Dead keys and an input method (`docs/keyboard.md` 9, journey 10) by the owner or in tier 3, since the probe can't compose text | 2 days | yes: keys |
| 6 | Presses: the ledger, causes for a press's end, the fork keeps the mechanics | Tier 0. The rig with a lost release. `dead-ui-after-menu`, `divider-fast-drag` | 2 days |  |
| 7 | Lifecycle: the page state machine, the host's `native.rs` as its caller, one container per page, the remount hooks | Tier 0. The engine's smoke suite while the owner is idle. `devtools-own-tab`, a tab left with the keyboard in its page | 3 days |  |
| 8 | Geometry: one frame function, the layer sets every native frame, one way to present in the transaction | Property tests for G1 and G2. `page-covers-pane`, which reads the pixel column at a dark page's left edge with the sidebar at a fractional width. The existing fit drags | 1 to 2 days | yes: steps 6 to 8 together |
| 9 | Windows and panels: the registry, one writer for a float, child windows, every key-window move through the layer, key and active state from `AppKit`'s notifications. The `NSApp` class patch (`sendEvent:`, the activation refusal) and the holders' window rules move in from the host | `no-panel-off-space` on the agent desktop by the window server's list. Tier 3 or the owner's checklist for child-window ordering, full screen, `hidesOnDeactivate`, and another app and back (journey 9) | 3 days, plus the tier 3 host | yes: panels |
| 10 | The fence fails the lint. `vendor/README.md`, `docs/keyboard.md` and `docs/native-pages.md` point here. The rule goes into the repository's agent instructions | `script/seam-fence` green with its allow-list. The whole bench green | 1 day |  |

Why this order. The tip is made shippable first. The label comes before the rules that read
it. The keyboard's state moves before the key routing that reads the holder. Lifecycle comes
before geometry, since the layer must own a page's views before it sets their frames. Windows
come last, because their checks need the bench host of section 9.4. Step 3 doesn't wait for
step 4: the gate already names windows by `windowNumber`.

**The branches on top.** browser-profiles, chat-backdrop and browser-extensions rebase onto
this branch. Extensions edits the host's `native.rs` and the shell's close path. Step 7 moves
`native.rs`'s placement code. It lands as a pure move and then a change, and the coordinator
is told a day ahead so extensions can rebase between the two.

**The owner's build.** He runs `f8e31e468`. Every step from 1 on leaves a tip he could run,
but he gets a build only where the table says: after step 3 (menus, if step 0 passed), after
step 5 (keys), after step 8 (presses, lifecycle and geometry together) and after step 9
(panels). Under policy A he should notice no change at step 5 at his desk.

**The estimates** are rough, in working days of one agent thread, and count the tests and the
bench sequences. They don't count waiting for the agent desktop or for the owner to be idle.
About 19 days in all.

## 11. The rule

To go into the repository's agent instructions at step 10. `[proposed]`

> The seam between GPUI and a native view has one owner, `koyo-seam` (`docs/native-layer.md`).
>
> 1. No fix at the seam outside the layer. If a bug involves the first responder, a mouse
>    press and its release, a native menu, a window's key state, Space or level, a native
>    view's frame, or a native view entering or leaving a window, it is fixed in `koyo-seam`.
> 2. A seam fix starts with a bench script that reproduces the bug and fails. It ends when
>    that script passes with every counter at zero.
> 3. The vendored GPUI crates hold hooks only. A change there that decides anything is refused
>    by `script/seam-fence`.
> 4. No rule reads an event to work out whose it is. It reads the label.
> 5. A new backstop needs a counter, and a counter that fires in the bench is a bug.

## 12. Risks

| Risk | Where it lands | What is watched |
| --- | --- | --- |
| The source fields or a tag don't survive `CGEventPostToPid` | Step 0 answered state and poster: the poster's pid arrives, and a private source's state arrives as an id of its own, not -1 (section 5.2). Step 2 answers the tag | A program is known by its poster's pid, so the layer has to know each poster: the desk host's pid is told as it starts, and an unknown program's input counts as the person's. If the tag is lost, relayed and agent input stay one label, as today. No rule in 5.3 depends on telling them apart |
| A Koyo that didn't start the agent desktop's host doesn't know its pid | Step 3, done 2026-11-17 | The layer asks, once for each process that posts an input, which bundle it runs from, and knows an agent desktop's host by its bundle (`codes.koyo.agent-desktop.host`). `[proven]` in a test Koyo: the host's click is kept as `Posted(DeskHost)`. A host packaged under another identifier wouldn't be known: `script/desk-host` and the layer name the same one |
| The hardware's own source state hasn't been read in the app | Step 3 | The gate logs it at each menu it shows. Until it's read, no state is taken for a program's |
| A handed-back key runs GPUI's whole key-down path, not bindings alone | Step 5 | Unchanged from today. The key-up rule covers the arrow repeater. A sequence holds ⌥→ in a page |
| A hook that runs under the fork's lock sends `AppKit` something | Steps 6 and 7 | The contract in 6.1. The rig runs a remount with the keyboard in the stand-in page, which would deadlock at once |
| The journal takes a page's own menu for the person's, within five seconds of their press | Step 3 | As today. Written down in 5.2. Chromium gives no cause to read |
| A child window changes a float's ordering or the switcher's unseen open | Step 9 | Needs Koyo in front. Not merged without tier 3 or the owner's check |
| Refusing a second container (L1) breaks a legitimate hand-over between two elements in one frame, as when DevTools change dock | Step 7 | The refusal is logged and counted first, and enforced only after the bench shows zero in dock changes |
| The stand-in page doesn't behave like Chromium | Every rig test | Each rig case is paired with a tier 2 sequence against the real engine. The rig is for speed, the sequence is the proof |
| Moving the `NSApp` patch out of the host changes when it is installed relative to CEF's start | Step 9 | A page loading in the test Koyo, and the `labels` sequence. The engine's smoke suite doesn't show it: it runs the host as a process of its own, with an `NSApp` class of its own (`mac_app.rs`) |
| The engine's smoke suite puts a system prompt on the person's screen | Any step that runs it | Its `location::` test has macOS ask for the person's location, on their screen (seen 2026-11-17). It's run with `--skip location::`, and only while the person is away |
| The move of `native.rs` collides with browser-extensions | Step 7 | The pure move lands alone, announced a day ahead |
| The fence is too strict and gets a long allow-list | Step 10 | Each entry needs a reason. The count of entries is printed by the lint |
| gpui-fast is updated upstream | Any time | The fork's seam diff is hooks only, so a rebase is small. The rig runs after it |
| The core's press capture changes upstream | Any time | The ledger's backstop counter shows it at once |
| Tier 3 never gets a host | Step 9 | Panels and key-window changes stay on the owner's checklist, and the doc says so |

## 13. Rejected

- **Pages as textures inside GPUI.** It removes the seam and costs the video and scrolling
  performance native pages were chosen for (`docs/native-pages.md` 1). Decided by the owner.
- **A bigger `Offers` (the saved commit 6).** Section 8.
- **An ambient "current input" set around `sendEvent:`.** It leaks into whatever runs inside a
  nested tracking loop (section 5.2).
- **Vendoring gpui-fast's core to fix press capture at its root.** A fourth and larger fork
  for one rule the ledger covers. `[owner]` can overrule.
- **The layer as a module of `koyo`.** The host could not depend on it, and it could not be
  fenced or tested apart.
- **Swizzling GPUI's window classes from the app instead of hooks in the fork.** It would
  remove the fork's seam patch entirely, but press routing needs the composition's private
  state, and a swizzle is harder to read than a hook.

## 14. Open questions for the owner

1. Which keys Koyo keeps while a page has the keyboard (section 4.1). Decided 2026-11-16:
   policy A.
2. Where tier 3 runs (section 9.4). The owner wants to try a Mac mini on his network first.
   How it's reached is being found out.
3. Whether a posted click should make a page the person's and lock agents out for five
   seconds (section 5.3). The design keeps today's behaviour.
4. Whether gpui-fast's core stays unvendored (section 4.2).
5. Whether the commit-6 stash is dropped after step 5 (section 8).
