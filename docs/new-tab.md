# Koyo's new-tab launcher and history

Written 2026-11-05 on branch `schulzfel/feat-new-tab-is-generic`. Drafted with Claude.

⌘T used to open a chat. It now opens a launcher: a floating bar over the window, modelled
on Zen's floating address bar, that opens any kind of tab. This note says what it does,
where its ranking comes from, and what was left for later. Keep it in step with the code
in `koyo-rs/koyo/src/launcher/` and `koyo-rs/koyo/src/history/`.

---

## 1. Behaviour

- **⌘T**, or the **+** in a tab strip (Orca and Arc layouts), opens the launcher over the
  window, for a tab on the selected checkout. ⌘T again, Escape, or a click outside it
  closes it.
- **⌘L**, from anywhere in the window, or a click on the Arc sidebar's address bar, opens it
  **on the page shown** (`launcher::OpenAddress`, `Shell::open_address`), as Arc's ⌘L does.
  It changes a page's address, so it needs a page:
  - The page is the one of the pane the person is in, or the shown tab's first page when
    they're in a pane of another kind (`docs/arc-sidebar.md` 2).
  - It opens on Browser, the page's whole address in the field, all of it selected, the rows
    for that address under it.
  - Enter on a Visit or a Search row loads in that page, as an address typed in its own field
    would: a typed visit, a search the history keeps, and the page takes the keyboard after.
    No tab opens. With the field emptied, Enter does nothing, as in any address bar.
  - Every other row does what it does after ⌘T: a Switch to Tab row switches, and Tab goes on
    to a chat, a terminal or a worktree, which open as usual.
  - ⌘L in the open launcher selects the field's text again.
  - With no page in the tab shown, a chat's, a terminal's, a file's, or no tab at all, ⌘L
    opens nothing and no tab: the key goes on to whatever has the keyboard, as one the
    window doesn't take, and a click on the bar does nothing. A new tab is ⌘T's.
  - In the Orca layout a page has its own toolbar, so ⌘L puts the keyboard in that page's
    address field instead, from anywhere in the tab.
- A pill on the left of the field names the **kind** of tab Enter opens: **Browser**, **Chat**
  or **Terminal**, in that order on purpose, then **Desktop**, the agent desktop's one tab
  (`docs/agent-desktop.md` 9), **Device**, the tab of an iPhone or iPad simulator, or of the
  person's own iPhone or iPad plugged in (`docs/devices.md` 8), and last **Worktree**, which opens no tab
  (`docs/arc-sidebar.md` 8). Each kind has its own colour (blue, violet, green, teal, pink,
  orange),
  so a glance tells them apart. **Tab** goes to the next kind and **⇧Tab** to
  the one before. Clicking the pill, or the hint on the right, also moves to the next kind.
  There is no editor tab any more: ⌘P opens files.
- **Nothing typed, Enter** opens an empty tab of the kind: the browser's start page with the
  address focused, a new chat, or a new shell.
- **Typed, Enter** picks the selected row. The first row does what's typed:
  - Browser: visits the address, or searches Google for the words (`browser::address_to_url`
    decides, as the address bar always has).
  - Chat: opens a new chat and sends the text through its composer, exactly as if it had
    been typed there and sent.
  - Terminal: opens a new login shell named after the command, then types the command into
    it with Enter (section 2).
  - Desktop: Enter shows the agent desktop's tab, the one there is or a new one, whatever
    is typed. Nothing is listed under the field.
  - Device: the rows are the device tabs open, then Koyo's own simulators no tab shows, then
    the iPhones and iPads plugged in no tab shows (the step's label beside one that misses a
    step: "Needs update"), those whose names hold every word typed (the simulators are read again each time
    the kind is picked; the devices as the plug watcher keeps them). A device picked opens in
    its own tab and starts; an open one is switched to.
    Enter with no row picked opens a device tab on its picker.
  - Worktree: what's typed is the new worktree's name. Enter opens the New Worktree dialog
    on the selected project with that name filled in, and the branch is picked there.
    Nothing is listed under the field, and Enter with nothing typed opens the dialog empty.
- **↑/↓** (and ⌃P/⌃N) move the selection. **→** at the end of the field takes the host
  completion, or the selected row's text, into the field. **⇧⌫** (or ⇧⌦) forgets the
  selected row if it comes from history; so does the × on that row.
- Text typed and then dismissed is offered again if the launcher reopens within 45 seconds,
  with its kind. This matches Zen's `zen.urlbar.wait-to-clear`. Picking a row clears it.
  Opened on a page, the field holds the page's address whatever was dismissed, and that text
  goes on waiting for the next ⌘T. A page's address dismissed as it was isn't text anyone
  typed, and isn't offered; one typed over is.
- The window grows and shrinks with its rows, keeping its top edge where it is
  (`native/window.rs`: AppKit's `setContentSize` would keep the bottom edge instead).
- Over a web page, its glass takes the opposite tone to the page: dark over a light page,
  light over a dark one, with its text, rows and field in that tone's palette. It reads the
  page's own frame under it, from the surface the browser host shares with the app, as it
  opens and every 400 ms while it's open. A page near mid-grey doesn't make it flicker
  (`theme/glass.rs`). Over anything else, or over less than half a page, it keeps the theme's
  tone, as dialogs do. The bar over a page an agent is using does the same.

## 2. Terminal commands are typed into a shell

The owner chose "typed into a new interactive shell" over running the command as the
terminal's program. As a result, the command shows at the prompt, goes into the shell's
own history, ⌃C works as usual, and the shell stays open once the command ends.

The command is sent with the daemon's `Send` request, as text with `enter`. The app's
connection counts as the person, so it needs no claim.

The command waits for the shell's prompt: it's typed once the daemon reports the terminal
`Idle`, whether when it opens or on a later status change (`terminals/typing.rs`). With a
shell's marks, `Idle` means at its prompt; without them, as for bash, it means quiet, the
most the daemon can tell. `NeedsInput` is a bell or a notification, not a prompt. If the
command were written before the shell switched the tty to raw mode, the line discipline would
echo it above the first prompt. Closing the tab first drops the command.

## 3. Rows

At most ten rows. With nothing typed, nothing is selected, so Enter opens an empty tab.
Once something is typed, the first row is selected.

| Kind | Nothing typed | Typed |
| --- | --- | --- |
| Browser | Open browser tabs but the one shown ("Switch to Tab", up to 4), then top sites no tab shows (up to 8 rows in all) | Host autofill, or the address typed ("Visit"); the search ("Search with Google") unless what's typed has a scheme; past searches (≤2, clock icon) and Google's suggestions; then pages from history, but the one the first row visits. A page an open tab shows becomes "Switch to Tab", and open tabs that match are added |
| Chat | Chats on this checkout with no tab ("Open Chat", up to 4), then messages sent lately | "Ask in a New Chat"; chat tabs and tabless chats that match; then past messages |
| Terminal | Commands run lately | "Run in a New Terminal"; terminal tabs that match; then past commands |

Of the browser rows left after the first, searches take about two in three and pages the
rest; either kind takes the room the other leaves. Firefox shares rows the same way when
suggestions are shown first.

Matching follows Firefox's tokenizer, in one pass over the entries; ASCII text, as most
titles and addresses are, is compared as it is, without lowercased copies. What's typed is split into words, and every word has
to appear in the title or in the address without its scheme. Matches at the start of a
word come first: a position starts a word unless it is a lowercase letter right after
another letter, so "hub" matches the "Hub" in "GitHub". When fewer than 15 rows match that
way, matches anywhere are added. Matched characters are drawn in the accent colour.

Addresses are shown the way Firefox shows them: `https://` is dropped, then `www.`, and a
lone trailing `/` goes. `http://` stays, so an insecure page reads as one.

## 4. History

### 4.1 What's recorded

One store, `history.json` in the app's data folder, holds four kinds of entry:

| Kind | Recorded when |
| --- | --- |
| Page | A browser tab finishes loading a page (`shell/visits.rs`). Only http, https and file pages are kept |
| Search | Words are searched from the launcher or from a tab's address bar |
| Prompt | A message is sent from any chat composer |
| Command | A command is run from the launcher |

Each entry keeps up to 10 recent uses, each with a timestamp and whether it was typed or
followed, plus the total count and the last 4 checkouts and projects it was used in.

The store is global. Rows used on the same checkout rank ×3, and rows used in the same
project ×2. That was the owner's choice: "global, this project first".

How a visit is marked (`browser/view/navigation.rs`):

- **Typed** when it came from the address bar or the launcher.
- **Followed** for links and agents.
- **Not counted** for back, forward or a pick from the history list, since Firefox counts
  no new visit for session history; nor for the first load of a tab brought back at launch
  (`docs/browser.md` 5), as Firefox counts none for a restored session.

A tab tells its visit once, when the page finishes loading or moves to another address
(`BrowserEvent::Visited`), so an icon or a title arriving meanwhile can't use up how the
next page came. The same page again in the same tab, a reload, only updates the title, as
does a title that changes later. A typed visit is marked by the app, because CEF stamps
every `LoadURL`, including popups, as `TT_EXPLICIT | TT_DIRECT_LOAD_FLAG` (section 5).

**Clear browsing data** (Settings › Browser) also forgets pages, searches and site icons.

### 4.2 Frecency, as Firefox 159 computes it

Firefox 147 and later replaced the bucketed frecency with exponential decay
(`toolkit/components/places/PlacesFrecencyRecalculator.sys.mjs`, `SQLFunctions.cpp`):

```
score = Σ weight(visit) · e^(−λ·age) / n × max(count, n)    λ = ln 2 / 30 days
weight: typed 100, followed 50 (redirects 20, unused here)
n = sampled visits, at most 10
```

`history/rank.rs::strength` implements exactly this, and the tests pin it: one typed visit
today scores 100, a followed one 50, a typed one 30 days ago 50, and three followed visits
today 150. When an entry kind is full (5000 pages, 1000 other entries), the weakest entry
goes.

### 4.3 Input history ("adaptive")

Firefox's `moz_inputhistory`: when a row is picked after typing something, the store
remembers that input for that entry.

- Each pick sets `use = use × 0.9 + 1`.
- The value decays ×0.975 per day, and is dropped below 0.1.
- Lookup is a prefix match on the stored input, ranked by `use`, doubled for an exact match.
- Up to three such rows come first.

Switching to a tab teaches its address in the same way.

### 4.4 Top sites

With nothing typed, the browser lists top sites. Pages are grouped by host and their
strength summed. A host qualifies at 87.5 or more, which one followed visit does not
reach. Each qualifying host shows its strongest page, at most eight, matching
Firefox's top-sites threshold and count.

### 4.5 Autofill

Firefox's origin autofill (`UrlbarProviderAutofill.sys.mjs`):

- An origin's score is the average strength of its pages × the number of distinct days in
  the last 90 on which it was typed.
- The threshold is the median score: the highest score of the lower half.
- The origin needs a typed visit within those 90 days.
- Autofill applies only when the text grew at its end, with the caret at the end.
- It never applies to text with a space or a `/`.

The completion is selected in the field, so typing on replaces it. → accepts it, and the
first row visits the origin. The hosts' scores and the threshold don't depend on what's
typed, so the launcher works them out once, the first time it autofills
(`History::origins`).

### 4.6 Search suggestions

Google's endpoint in Firefox's format:
`https://www.google.com/complete/search?client=firefox&q=…`. The answer is
`["query", ["suggestion", …]]`, and is used only if element 0 matches what was asked.

- Requests start 50 ms after typing pauses, with a 500 ms timeout (Firefox's
  `browser.search.suggest.timeout`).
- They are sent without cookies and only for 2 to 100 characters.
- They are not sent for anything that looks like an address.
- They are fetched with the system `curl`, as koyo-lsp fetches language servers, so the app
  links no TLS stack of its own for them. A request that's no longer wanted, because the
  person typed on, is killed rather than waited for.

Settings › Browser › **Search suggestions** turns them off (on by default, as the owner
chose).

### 4.7 Icons

The icon a tab fetches for a page (`browser/favicon.rs`) is kept as a PNG per host in
`history-icons/` beside the history. The launcher shows it next to that host's rows, as
tabs do.

## 5. Why Koyo keeps its own history, not Chromium's

Findings from CEF at `4fa4ab51` and Chromium 156:

- CEF has no history API. `CefBrowserHost::GetNavigationEntries` (`cef_browser.h`) lists one
  browser's back/forward list only (`cef_navigation_entry.h`).
- Panes render windowless, which means the Alloy runtime style. Chromium still writes its
  own `History` SQLite database in the profile, but:
  - the database is exclusively locked while the engine runs;
  - its schema is internal and changes between releases;
  - Koyo runs one profile per project, while the launcher wants one global, project-ranked
    history.
- Typed versus link comes from the transition type. `LoadURL` stamps
  `TT_EXPLICIT | TT_DIRECT_LOAD_FLAG` on every navigation, popups included
  (`frame_host_impl.cc`), so only the app knows what the person typed.

Koyo therefore records visits itself, on the app side, from the page state the host already
sends. No host or IPC change was needed.

**Follow-ups**, if visits ever need to be more exact:

- Have the host forward `OnLoadStart` and the transition type, and `OnLoadEnd` with its HTTP
  status (`cef_load_handler.h`), so error pages and redirects can be told apart the way
  Firefox does: redirect sources at weight 20, and no visit for 4xx/5xx.
- Have the host forward `OnBeforeBrowse`'s `user_gesture` (`cef_request_handler.h`), to tell
  links from scripted navigations.

## 6. Where it lives

| Path | What |
| --- | --- |
| `koyo/src/launcher/mod.rs` | The actions and keys, the kinds, opening it from the shell |
| `koyo/src/launcher/view.rs` | Its state: what's typed, autofill, rows, picking |
| `koyo/src/launcher/render.rs` | How it looks |
| `koyo/src/launcher/tone.rs`, `koyo/src/theme/glass.rs`, `koyo/src/browser/lightness.rs` | Its glass's tone over a page, and how light the page under it is |
| `koyo/src/launcher/rows.rs` | What it lists for what's typed (section 3) |
| `koyo/src/launcher/here.rs` | The checkout it opened on, its open tabs, and its chats with no tab |
| `koyo/src/launcher/act.rs` | What a pick does in the window behind, or in the page it opened on |
| `koyo/src/shell/shown_page.rs` | The page ⌘L and the sidebar's controls act on |
| `koyo/src/launcher/suggest.rs` | Google's suggestions |
| `koyo/src/history/` | The store, ranking (`rank.rs`) and site icons (`icons.rs`) |
| `koyo/src/browser/view/navigation.rs` | How a page came to load, for its visit |
| `koyo/src/shell/visits.rs` | Visits and searches recorded from browser tabs |
| `koyo/src/terminals/typing.rs` | A command typed into a new shell at its prompt |
| `koyo/src/modal.rs` (`open_floating`, `fit_height`), `koyo/src/native/window.rs` | The floating window that keeps its top edge |

Sources:

- Firefox at `FIREFOX_159_0_1_RELEASE`:
  - `browser/components/urlbar/` (`UrlbarProviderPlaces`, `UrlbarProviderAutofill`,
    `UrlbarProviderInputHistory`, `UrlbarTokenizer`, `UrlbarUtils`, `UrlbarPrefs`)
  - `toolkit/components/places/` (`PlacesFrecencyRecalculator.sys.mjs`,
    `SQLFunctions.cpp`, `nsNavHistory.cpp`, `History.cpp`)
- Firefox's urlbar docs: <https://firefox-source-docs.mozilla.org/browser/urlbar/overview.html>
  and <https://firefox-source-docs.mozilla.org/browser/urlbar/ranking.html>
- Zen (<https://github.com/zen-browser/desktop>, `e5f9d1e6`): floating urlbar, open-tab
  rows, `zen.urlbar.wait-to-clear`
- CEF `include/` at `4fa4ab51` (<https://github.com/chromiumembedded/cef>)
