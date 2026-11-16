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
| click | a press on a page, heard before the window hands it on, and the page's view taking the first responder after it (section 5) | the page |
| ⌘L, ⌥⌘I, new tab, ⌘⇧T, link opened beside, ⌃⇥, launcher, `pane_focus` | what the person asked for, told as it's asked, before GPUI's focus follows (section 3.1) | what they asked for |
| focus moved | GPUI's focus came to a page's view, or left it for another part (`browser/view.rs`, `focus_moved`) | the page, or GPUI's view |
| tab shown | the shell handing the keyboard to the tab shown (`shell/keys.rs`, `tab_takes_keys`) | the page the tab had it in as it was left, else what the shell picks |
| window key, window resigned | the window becoming the key window, or not (`shell.rs`) | the surface, or away |
| app active, app inactive | Koyo in front, or another app (`AppKit`'s notifications) | the surface, or away |
| menu opened, menu closed | a native menu tracking the keys, Koyo's or Chromium's (`AppKit`'s notifications) | a menu, then the surface |
| dialog opened, dialog closed | a dialog of Koyo's over the window, from open to closed (`popup.rs`) | away, then the surface once the window is key |
| window closed | another window of Koyo's that had the keys (a dialog, the tab switcher's panel) or one of Chromium's closing | the surface of the window the keyboard is kept for, made the key window |
| page closed | the page that had it closed (`browser/view.rs`) | the window's own keys |

GPUI's focus moves heard as a window stops being key aren't moves: GPUI's focus stays where it
was, though its views hear it leave. Two pages' focus moves are heard in no set order, so a page
left after another took the keyboard changes nothing. The mouse's side buttons only step through
a page's history, and give it no keyboard. Whether the window is key is read again as each frame
of the shell is drawn, so a change the holders missed is made up.

### 3.1 What the person asks for

| Asked | Where it's told | The keyboard |
| --- | --- | --- |
| ⌘L | `BrowserView::focus_address` | the page's address (in Arc, the launcher, a window of its own) |
| ⌥⌘I | `BrowserView::toggle_devtools` | hidden with the keyboard in them, `DevTools` give it back to their page; shown, it stays where it is and `DevTools` asking for it are refused |
| a new tab | `Shell::keys_to_new_tab`, `open_browser_tab` | the new tab, as the shell hands it (a new page, its page; a start page, its address) |
| ⌘⇧T | `shell/reopen.rs` | the tab brought back |
| a link opened beside | `open_browser_tab` | the new tab |
| ⌃⇥ | `shell/switcher/show.rs` | the tab picked; the switcher's panel never has it |
| the launcher's pick | `launcher/act.rs`, `Shell::focus_pane` | the pane picked |
| `pane_focus` | `control/exec/pane.rs`, `Shell::focus_pane` | the pane named, as a person moving to it would |

A tab shown again has the page it had the keyboard in as it was left, its own or its `DevTools`'
(docked or in a pane of their own): GPUI's focus alone is in the same pane either way. A tab whose
keyboard was in GPUI's view as it was left is handed it as the shell always has (`shell/keys.rs`):
the pane the person was in, a chat's prompt never, unless it's just been opened to type in.

## 4. The reconciler

`keyboard/reconcile.rs` is the one place that moves `AppKit`'s first responder, on the main
thread, as it's asked: to what the holder wants (section 2), after each change of the holder,
when the window becomes key or Koyo comes to the front, both of which are changes, and as a
page's views move. The browser host tells the keyboard before a page's views leave a window
(to be let go, hidden, parked or closed), and the first responder, if it's one of them, goes to
GPUI's view first; and once they're in a window, where a holder that wants the page has its
keys' view take it (`native::set_keyboard`). `AppKit`
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
- **Every press, before it's handed on**, from the hit test the window makes to route it. Over a
  page, it's the person's click: the page's view taking the first responder in the second after
  it, as `AppKit` hands it the press or Chromium asks after a trip to the renderer, is theirs,
  and makes the page the holder; so is a press on a page that has it already. A press GPUI
  keeps for itself never reaches the page, which then takes nothing, and a press anywhere else
  ends the click. Over GPUI's own content, GPUI's view is made the first responder first if the
  holder is GPUI's and it isn't, so a press on the composer always gives it the keys; with a
  page holding, the press is GPUI's to judge: one that moves GPUI's focus moves the holder,
  and a button leaves the keyboard in the page.
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

## 7. Every surface

| Surface | Holder | How it gets the keyboard, and loses it |
| --- | --- | --- |
| A chat's composer, a terminal, an editor, a device, the agent desktop | `gpui (pane N)` | GPUI's focus, as before; a press on GPUI's content gives GPUI's view the first responder if a page took it |
| A pinned, loose or worktree page | `page N` | a press on it, the shell handing it its tab, GPUI's focus coming to its view; out when GPUI's focus leaves it |
| `DevTools`, docked or in a pane | `page N` (their own) | the same; ⌥⌘I out of them gives it to their page; a tab shown again gives it back to them if they had it |
| The page's address and find bars | `gpui (a bar of page N)` | GPUI's focus; ⌘L |
| Native menus, Koyo's and Chromium's (a `<select>`, a page's menu) | `a menu` | while they track; then what the person had |
| Koyo's dialogs (a page's alert, confirm, prompt; Koyo's own) and the launcher | `away` in the window behind | their window is key; it closing with the keys gives them back to the window behind, made key |
| The tab switcher's panel | none: it never has the keyboard | ⌃ let go is heard through the modifiers, which GPUI's view always hears |
| Chromium's own windows and sheets (an extension's install dialog, the passkey sheet) | `away` | their window is key; one closing with the keys gives them back |
| Another app | `away` | Koyo in front again gives back what the person had |
| Agents' input to a page | the person's holder, unchanged | an agent types and clicks in a page through the engine, past `AppKit`, so its input needs no first responder; a page asking for the keyboard because of it is refused, and the person's keys stay where they are |

## 8. Limits

- A press on GPUI's content while a page holds leaves the first responder in the page until
  GPUI's focus says otherwise, heard at the end of the frame: a key typed within that frame of a
  click on the composer goes to the page. Making GPUI's view the first responder at every such
  press would take it from the page at a press on a button too, which the page hears as losing
  the focus and getting it back.
- What GPUI's focus is in, in GPUI's view, is known only as the app tells it (a page, a page's
  bar, a pane handed the keyboard); elsewhere it's "another part". GPUI's focus ids aren't
  public in gpui-fast.
- `DevTools` shown with ⌥⌘I don't take the keyboard, as before; Chrome's do.
- A page that asks for the keyboard more than 20 times in a second is let have it until the
  second is up; a key in the meantime still goes where it should, as the router moves it first.
- A key that comes while a dead key or an input method's text is being composed goes where the
  composition is, even if that's off what the holder wants; the next key after it is routed.
- None of this has been run on screen yet (section 9): the reconciler, the hooks and the router
  are checked against a fake `AppKit`, the holders with GPUI's test windows.

## 9. To check on screen

Each to be run on the owner's machine with his go, Koyo's log open at `koyo::keyboard`. Each
says where the text must land and what the log must say; nothing typed may appear in the log.

1. A pinned tab: click into its page, type; click the chat's composer, type. Text in the page,
   then in the composer; lines `… → page N (click)`, `page N → gpui (…) (focus moved)`.
2. With the composer holding, open a pinned page (one that opens and loads by itself); type in
   the composer. Text in the composer; a `page N asked for the keyboard … refused` line if
   Chromium asked, and no `healed` line for the keys.
3. Open a page's `DevTools` (⌥⌘I), click into them, type; ⌥⌘I; click the composer, type. Text in
   `DevTools`, then in the composer; `… (⌥⌘I)` puts it back in the page.
4. Right-click a page, pick Inspect, then click the composer and type. Lines `a menu` and back;
   text in the composer.
5. ⌃⇥ from a page to a chat and back, typing after each (in the page; in the composer after a
   click). Text where it was typed; the switcher closes as ⌃ is let go.
6. A page's JavaScript `alert` and `confirm`, answered; an extension's install dialog,
   dismissed; then type. Lines `away (dialog opened)` and back; text where the keyboard was.
7. A page's `DevTools` in a pane of their own, and back docked; type in each.
8. The agent desktop's tab, then the composer: type in each.
9. Another app and back (⌘⇥ twice); type. `away (app inactive)`, then back to what it had.
10. A dead key (´ then e), the emoji picker (⌃⌘Space) and Japanese input, each in a page and in
    the composer. One é, the emoji, the composed text, each where it was typed; no `healed`
    line in the middle of a composition.
11. An agent typing into its page (`browser_type`) while the person types in the composer.
    Neither's text lands in the other; any `asked for the keyboard` line is refused.
