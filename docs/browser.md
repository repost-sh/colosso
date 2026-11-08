# Koyo browser tabs across a restart

Written 2026-11-06 on branch `schulzfel/browser-tab-restore`. Drafted with Claude. Source of
truth for which browser tabs come back after Koyo quits and how; update it when a decision
changes, and keep it in step with `koyo-rs/koyo/src/browser/tabs.rs`.

Browser tabs come back after quitting and relaunching Koyo, on the checkouts they were on,
and none of their pages loads until its tab is shown. Chats come back by the agent host's
`tabOpen` (`docs/chat.md` C13), terminals by the daemon that keeps them
(`docs/terminals.md` 5.1). No browser process outlives the app, so the app keeps the list
of browser tabs itself.

Status marks: `[decided]`, `[not built]`.

---

## 1. What comes back [decided]

- Each checkout's page tabs, in the order they had, each at the address it showed, titled
  as it was until its page says otherwise, with its site's icon when one is kept
  (`history-icons/`, `docs/new-tab.md` 4.7).
- They come after the tabs already on the checkout, without taking the focus: the tab shown
  stays shown and the checkout selected stays selected.
- No page loads until its tab is shown. A restored tab is a pane with an address and no page;
  showing it makes the page, which opens that address.
- The browser host never outlives the app, as before: the app stops it on quit
  (`browser/host.rs`, `koyo-browser-ipc/src/client.rs`), and the host stops itself when the
  app's socket closes (`koyo-browser-host/src/host.rs`). The host starts at launch with no
  page (`browser::warm_up`).

## 2. Where the list lives [decided]

`browser-tabs.json` in the app's data folder (`settings::data_dir()`; `KOYO_DATA_DIR` moves
it), beside `history.json`:

```json
{
  "checkouts": [
    {
      "project": "/Users/me/src/app",
      "checkout": "/Users/me/koyo/workspaces/app/feature",
      "tabs": [
        { "url": "http://localhost:3000/", "title": "App" },
        { "url": "https://docs.rs/serde", "title": "serde - Rust" }
      ]
    }
  ]
}
```

- One entry per checkout with page tabs: its folder, its project's main checkout, and its
  tabs left to right. A page in a split counts by its place in the tab, left to right and top
  to bottom.
- Kept: page panes with an address. Not `DevTools`, and not a tab with no address yet (a new
  tab's start page): an empty browser tab leaves nothing behind, as an empty chat tab doesn't.
- Written 500 ms after a change, with whatever else changed meanwhile; and on quit and when
  the window closes, if a write is due. A file that can't be read is no tabs. `kept_file`
  does the reading and writing, as it does for the chat drafts and the history.

## 3. One rule for what's kept [decided]

After each change of the shell, as `Terminals::retain` and `ChatTabs::retain` run,
`Shell::keep_browser_tabs` builds the list from the panes. When it differs from what's kept, a
write is due. There's no hook at each of the ways a tab comes or goes (opened, navigated,
retitled, moved, closed by its ×, a split's ×, ⌘W or app control, ⌘⇧T, a worktree gone, a
project removed): every one of them changes the shell.

- A checkout's tabs read from the file wait until git lists the checkout, and every write
  carries them meanwhile, so the first write after launch doesn't drop them.
- They go once their project is removed, or once their project's worktrees are listed without
  the checkout.

## 4. Restoring [decided]

- `Shell::restore_browser_tabs` runs in `show_worktrees` beside `restore_terminals` and
  `follow_chats`, each time git lists a project's worktrees. A checkout's waiting tabs come
  back the first time it's listed, once a window: a window closed and opened again from the
  Dock reads the file again, as its chats and terminals come back too.
- A checkout with no tabs before has the first one brought back as its shown tab. It loads
  when that checkout is selected and the window draws it, as any shown tab does.

## 5. Pages made when shown [decided]

- A restored pane holds its address and the site's icon (`Unloaded`) and no `BrowserView`;
  the model keeps a pane from holding both (`Pane::set_view` lets the address go).
  `Shell::make_shown_browsers` makes the shown tab's missing views with
  `BrowserView::new(address, …)`, from `Shell::workspace_content` beside
  `make_shown_terminals` and `make_shown_chats`, and from `focus_pane`, so a page focused
  through the launcher or app control takes the keyboard. The page opens once it's laid out
  (`browser/view/lifecycle.rs`, `ensure_open`). Restoring makes no view and opens no page.
- What reads a browser pane copes with one that has no view: the tab's icon
  (`Pane::favicon`), the address (`Pane::address`) that the launcher's Switch to Tab rows and
  `tab_list` show, and ⌘⇧T, which keeps a closed unshown tab's address and opens it again at
  that address.
- A restored page's first load is no new visit in `history.json`, as Firefox counts none for
  a restored session; it updates the entry's title (`docs/new-tab.md` 4.1).

## 6. Agents [decided]

A page an agent opens (`tab_open`, `pane_split`) starts in responsive mode at the person's
size for agents' pages, 1440 × 900 by default, or at the `viewport` its call asks for
(`docs/responsive.md` 4.1, 9). A restored page isn't one an agent opens: it fills its pane
once shown, whoever opened it before.

`browser_list` lists a restored page whose tab wasn't shown yet, with its kept address and
title, `loading: false` and a 0 × 0 viewport. Every browser tool that names it, or means it
for lack of another, answers `browser_unavailable` until its tab is shown (`tab_select`,
`pane_focus`), as a page the host hasn't opened yet does (`docs/app-control.md` 11). Showing
the tab is what makes the page, so the tools don't make one of their own.

## 7. What doesn't come back [decided]

- A page's back and forward history, its scroll position and what was typed in its forms:
  CEF offers no way to restore them.
- Splits and docked or detached `DevTools`: each page comes back as a tab of its own.
- Which tab was selected, responsive mode (`docs/responsive.md` 1; a page an agent opened at
  1440 × 900 comes back filling its pane), zoom, and who held a page's lease.
- [not built] Unloading tabs in the background during a session: a page made stays until its
  tab closes.

## 8. Where it lives

| Path | What |
| --- | --- |
| `koyo/src/kept_file.rs` | Reading the file, the write a moment after a change, and the write on quit |
| `koyo/src/browser/tabs.rs` | The file, the rule (`keep_browser_tabs`), restoring and `make_shown_browsers` |
| `koyo/src/browser/tabs/tests.rs` | The file, the rule, restore order and focus, pages made when shown, ⌘⇧T, quit |
| `koyo/src/model/tab.rs` | A pane's page or its kept address, never both (`Page`, `view`, `set_view`, `unloaded_address`), `Pane::address`, `Pane::favicon` |
| `koyo/src/model/browsers.rs` | `Workspace::add_unloaded_tab` |
| `koyo/src/projects.rs` | `show_worktrees`, where restoring runs |
| `koyo/src/shell.rs` | The shell's observer, where the rule runs, and `workspace_content` |
| `koyo/src/shell/browser_tabs.rs` | Closing a tab, its unshown pages' addresses kept for ⌘⇧T |
| `koyo/src/control/exec/browser/page.rs` | Unshown pages in `browser_list` and the tools' refusal |
| `koyo/src/browser/view/navigation.rs` | `mark_restored`: a restored page's first load is no visit |
