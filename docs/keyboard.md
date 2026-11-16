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

| Holder | Has the keys | Its first responder |
| --- | --- | --- |
| `gpui (part)` | GPUI's view; the part is what GPUI's focus is in, by kind and pane | GPUI's view |
| `page N` | a page's own views: pinned, loose or worktree page, `DevTools` | the page's key view |
| `away` | nothing in the window: another window (a dialog, a sheet, one of Chromium's) or another app | left as it is |

Under it is what the person put the keyboard in (the **surface**: GPUI's view or a page), which
has it again whenever nothing has taken it away. A window is first heard of with its own keys.

## 3. What moves it

Each move has a cause, and the log names it:

| Cause | From | Moves to |
| --- | --- | --- |
| focus moved | GPUI's focus came to a page's view, or left it for another part (`browser/view.rs`, `focus_moved`) | the page, or GPUI's view |
| tab shown | the shell handing the keyboard to the tab shown (`shell/keys.rs`, `hand_keys_over`) | what the tab hands it to |
| window key, window resigned | the window becoming the key window, or not (`shell.rs`) | the surface, or away |
| dialog opened, dialog closed | a dialog of Koyo's over the window, from open to closed (`popup.rs`) | away, then the surface once the window is key |
| page closed | the page that had it closed (`browser/view.rs`) | the window's own keys |

GPUI's focus moves heard as a window stops being key aren't moves: GPUI's focus stays where it
was, though its views hear it leave. Two pages' focus moves are heard in no set order, so a page
left after another took the keyboard changes nothing.

## 4. The log

One line each time a window's holder changes, at target `koyo::keyboard` in Koyo's log:

```
keyboard: gpui (pane 7) → page 12 (focus moved)
keyboard: page 12 → away (window resigned)
```

Surfaces are named by kind and by pane (the shell's pane id for GPUI's parts, the browser
host's for a page), never by anything in them: no character, no key code, nothing a key could be
told from, as passwords go the same way. Anything to do with dialogs or the keyboard leaving the
window is a warning, the rest is information. Nothing is written for a key.

## 5. Not yet

The holders are told what happens and logged; nothing moves the first responder from them yet,
which the host still does as GPUI's focus moves (`docs/native-pages.md` 6).
