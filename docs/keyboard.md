# Koyo's keyboard: one holder per window, one place that moves it

Written 2026-11-16 on branch `schulzfel/browser-ui-fixes`. Drafted with Claude. The owner kept
losing the keyboard ("I used the pinned tabs and then when I went back to this chat I couldn't
type in this box anymore; I had to restart Koyo") and asked for a refactor of how Koyo
controls input. Keep this note in step with `koyo-rs/koyo/src/keyboard/`.

## 1. Why

Two things decide where a key goes in Koyo's window, and nothing kept them together:

- **GPUI's focus**, inside GPUI's own view: which of Koyo's parts takes the key once the view
  has it (a chat's composer, a terminal, an editor, the launcher's field).
- **`AppKit`'s first responder**, the view of the window that gets the key at all: GPUI's own
  view, or one of a page's, Chromium's.

The app moved the second after the first, over the browser host's socket, as GPUI's focus came
to a page or left it. Chromium moves the first responder too, on its own: its view makes itself
the first responder of whatever window it's in as a page opens, loads or is told to focus. GPUI
never hears of it, so its focus stays on the composer while the page has every key; a click on
the composer changes nothing, as GPUI's focus is already there, and nothing gives the keyboard
back. Koyo logged nothing of any of it.

## 2. The holders

Each window has one **holder** (`keyboard/state.rs`):

| Holder | Has the keys | The first responder it wants |
| --- | --- | --- |
| `gpui (part)` | GPUI's view; the part is what GPUI's focus is in, by kind and pane | GPUI's view |
| `page N` | a page's own views: pinned, loose or worktree page, `DevTools` | the page's keys' view while its views are in the window, else GPUI's view |
| `a menu` | a native menu tracking the keys | none: left as it is |
| `away` | nothing in the window: another window (a dialog, a sheet, one of Chromium's) or another app | none: left as it is |

Under it is what the person put the keyboard in (the **surface**: GPUI's view or a page), which
has it again whenever nothing has taken it away. A window is first heard of with its own keys.

## 3. What moves it

Each move has a cause, and the log names it:

| Cause | From | Moves to |
| --- | --- | --- |
| click | a press on a page, heard before the window hands it on (section 5) | the page |
| focus moved | GPUI's focus came to a page's view, or left it for another part (`browser/view.rs`, `focus_moved`) | the page, or GPUI's view |
| tab shown | the shell handing the keyboard to the tab shown (`shell/keys.rs`, `hand_keys_over`) | what the tab hands it to |
| window key, window resigned | the window becoming the key window, or not (`shell.rs`) | the surface, or away |
| app active, app inactive | Koyo in front, or another app (`AppKit`'s notifications) | the surface, or away |
| menu opened, menu closed | a native menu tracking the keys, Koyo's or Chromium's (`AppKit`'s notifications) | a menu, then the surface |
| dialog opened, dialog closed | a dialog of Koyo's over the window, from open to closed (`popup.rs`) | away, then the surface once the window is key |
| window closed | another window of Koyo's that had the keys (a dialog, the tab switcher's panel) or one of Chromium's closing | the surface of the window the keyboard is kept for, made the key window |
| page closed | the page that had it closed (`browser/view.rs`) | the window's own keys |

GPUI's focus moves heard as a window stops being key aren't moves: GPUI's focus stays where it
was, though its views hear it leave. Two pages' focus moves are heard in no set order, so a page
left after another took the keyboard changes nothing. The mouse's side buttons only step through
a page's history, and give it no keyboard.

## 4. The reconciler

`keyboard/reconcile.rs` is the one place that moves `AppKit`'s first responder, on the main
thread, as it's asked: to what the holder wants (section 2), after each change of the holder,
and when the window becomes key or Koyo comes to the front, both of which are changes. `AppKit`
is behind a trait (`AppKit`): the app's is `keyboard/appkit.rs`, which knows the windows by
GPUI's ids, tells a first responder apart (GPUI's view, a page's view by the browser host's
`native::page_of`, the window itself, anything else) and moves it (a page's keys' view by the
host's `native::key_view`); tests have a fake (`keyboard/tests/fake.rs`).

## 5. What the windows tell it

Each of GPUI's windows tells the keyboard three things, through the vendored gpui-fast-macos's
`responders` (`vendor/README.md`), and `AppKit`'s notifications tell it the rest:

- **Every change of its first responder** (`makeFirstResponder:`), whoever made it. One the
  reconciler made is its own and is let be. A page's view taking it when the person didn't put
  the keyboard in that page is a page asking: it's refused, logged, and put back on the next
  turn, once whatever asked is done. Chromium asks for itself as a page opens, loads or is told
  to focus, in whatever window its views are in, and an agent's input to a page can have it
  ask. Anything else that moves it off what's wanted is put back the same way. A window put
  back more than 20 times in a second is left as it is until the second is up.
- **Every press, before it's handed on**, from the hit test the window makes to route it: over a
  page, it's the person's click, and the page is the holder before `AppKit` hands the press
  to it, so the page's own `makeFirstResponder:`, which can come later, after a trip to the
  renderer, is the person's. Over GPUI's own content, GPUI's view is made the first responder
  first if the holder is GPUI's and it isn't, so a press on the composer always gives it the
  keys; with a page holding, the press is GPUI's to judge: one that moves GPUI's focus moves
  the holder, and a button leaves the keyboard in the page.
- **Every key going down or up, before it's delivered.** A key that finds the first responder
  off what the holder wants has it moved there first, logged, and is delivered there: a key is
  never lost to the wrong view. Not while text is being composed (a dead key, an input method's
  marked text), which the key belongs to wherever it is. Key equivalents, Koyo's shortcuts
  and the menu's, are offered by the application before the window hears the key at all, and
  go as they did. The modifiers go to GPUI's view too, whoever has the keyboard, as before.

## 6. The log

One line each time a window's holder changes, at target `koyo::keyboard` in Koyo's log, with
what the first responder did:

```
keyboard: gpui (pane 7) → page 12 (click); first responder gpui → page 12
keyboard: page 12 → away (window resigned)
keyboard: page 13 asked for the keyboard (gpui → page 13) while gpui (pane 7) has it: refused
keyboard: gpui (pane 7) has it, a key found the first responder elsewhere; first responder page 13 → gpui: healed
```

Surfaces are named by kind and by pane (the shell's pane id for GPUI's parts, the browser
host's for a page), never by anything in them: no character, no key code, nothing a key could be
told from, as passwords go the same way. Refused pages, anything healed, menus, dialogs and the
keyboard leaving the window are warnings, the rest is information. Nothing is written for a key
that goes where it should. A release GPUI never heard and is told of is one line at
`koyo::input`.

## 7. Not yet

The browser host still moves the first responder as GPUI's focus comes to a page or leaves it,
over its socket (`docs/native-pages.md` 6), and before a page's views leave a window. It agrees
with the holders, which follow the same focus, and anything it does that they don't want is
put back.
