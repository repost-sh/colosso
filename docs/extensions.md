# Extensions

Written 2026-11-15 on branch `schulzfel/browser-extensions`. Drafted with Claude. The owner
asked for browser extensions in Koyo's browser "like it must be, like it is in Chrome, Arc
and Zen". This is what a person gets, how it works, what was decided for the owner while
they were away, and what is checked. Keep it in step with
`koyo-rs/koyo-browser-host/src/extensions/`, `koyo-rs/koyo/src/browser/extensions/` and
`script/cef-patches/`.

Status marks: `[proven]` (run on this machine, evidence named), `[read]` (taken from source,
not run), `[decided]`, `[not built]`, `[owner]` (the owner's to decide).

---

## 1. What a person gets

What the three browsers have, and what Koyo does about each. Chrome is the engine's own
behaviour; Arc is Chromium with its own window, as Koyo is; Zen is Firefox, whose add-ons
are another format, so from Zen comes only where things sit.

| What | Chrome | Arc | Zen | Koyo |
| --- | --- | --- | --- | --- |
| Install from the Chrome Web Store | "Add to Chrome", Chrome's dialog | the same store, the same dialog | its own store | the store's own page and Chrome's own dialog, section 4 |
| Content scripts, service workers, storage, alarms, `declarativeNetRequest`, messaging | yes | yes | yes | the engine's own, untouched |
| Toolbar icon, with its badge, title and icon as the extension sets them, for a tab or for all | toolbar, behind the puzzle piece unless pinned | in the site control center; on the toolbar when it's shown | toolbar and the unified panel | the site controls' row in both layouts; pinned ones on the page's toolbar in the Orca layout, section 5 |
| A click on the icon: the popup, or `action.onClicked` with `activeTab` | yes | yes | yes | Chrome's own popup, hung from the icon, section 6 |
| Pin and unpin | puzzle menu | "Pin to toolbar" | drag | the icon's menu; `action.getUserSettings()` says the same |
| The icon's menu: options, remove, manage | yes | yes | yes | Options, Pin, Remove, Manage Extensions, section 5 |
| A page of all extensions: turn off, remove, details, shortcuts, load unpacked, errors | `chrome://extensions` | the same page | `about:addons` | `chrome://extensions` in a tab, Chrome's own page |
| Options page | a tab or a dialog in `chrome://extensions` | the same | the same | a tab, or Chrome's dialog in `chrome://extensions` |
| Context-menu entries (`contextMenus`) | in the page's menu | the same | the same | in Koyo's page menu, section 7 |
| The pages are tabs of a window: `tabs.query`, the active tab, `tabs.create`, `tabs.update`, `tabs.remove` | yes | yes | yes | each page a `normal` window with one tab, the page the person is on the current one, section 3 |
| An extension opens a tab (`tabs.create`, its options, its welcome page) | a tab | a tab | a tab | a Koyo tab, section 3 |
| An extension opens a window (`windows.create`, a wallet's prompt, a sign-in) | a window | a window | a window | Chrome's own window, as Chrome draws it, section 3 |
| Keyboard shortcuts (`commands`) | yes | yes | yes | `[not checked]`, section 9 |
| A profile's own extensions | per profile | per profile ("Space") | per profile | per profile, which in Koyo is per project, section 8 |
| Extensions' DevTools panels | yes | yes | yes | the engine's own, in Koyo's developer tools `[not checked]` |
| Updates from the store | automatic | automatic | automatic | the engine's own updater `[read]` |
| Side panels (`sidePanel`) | Chrome's side panel | none | sidebar | `[not built]`, section 9 |
| Manifest V2 | gone | gone | n/a | gone, as in Chrome, section 9 |
| Sync of extensions across machines | with a Google account | no | Firefox account | no, as Arc |
| Incognito | per extension | per extension | per extension | Koyo has no incognito |

## 2. What it stands on

### 2.1 Native pages [decided]

Extensions stand on native pages (`docs/native-pages.md`), which every page of Koyo's is
since 2026-11-15. An off-screen page was "Alloy style": extensions got no tabs, no windows,
no toolbar and no popups, `chrome://extensions` was blocked, and the Web Store couldn't
install (`.superpowers/research/run/*.out`).

### 2.2 A CEF of Koyo's own, patched [proven it's needed]

`docs/native-pages.md` 11.6 said the extension UI would need a patched CEF. It does:

- On CEF's own 156.0.14, in a native page, the Web Store's "Add to Chrome" shows Chrome's
  install dialog, and accepting it crashes Koyo: Chrome goes on to show its "installed"
  bubble on a toolbar the page hasn't got, makes a browser window of its own for it, and
  falls over (`.superpowers/ext/evidence/stock-install-crash.ips`). `[proven]` 2026-11-15
- Each page is a window of type `popup` to an extension, which password managers take for
  a sign-in popup, and the "current window" is the first page made, never the one the
  person is on, as none of the pages' windows is ever key. `[proven]`
- Nothing in CEF or the DevTools protocol says what an extension set its icon, badge or
  title to, lists the extensions of a profile that isn't the default one, or removes one
  that came from the store. `[read]`, `extensions_handler.cc` of Chromium 156

So the framework is built with patches, by the script that already builds it with the
codecs (`docs/browser.md` 11): **`script/cef`** applies every `script/cef-patches/*.patch`
to Chromium's sources after CEF's own, and builds. Section 10 lists them. Most are Cra's
(`github.com/VictorKildahl/cra`, MIT), a macOS browser that embeds Chrome-style CEF pages
the way Koyo does; the protocol patch is Koyo's.

- **A framework says which patches it was built with**, in its
  `Resources/koyo-patches.txt`, which `script/cef` writes and which goes wherever the
  framework does. The host reads it before it starts the engine.
- **On CEF's own framework** (a checkout where `script/cef` hasn't run) there are no
  extension controls: no pipe is asked for, no page carries Chrome's toolbar, the row and
  the icons aren't drawn, and Koyo is as it was. An "Add to Chrome" in the Web Store still
  ends Koyo there, as above; that is the engine's, and the patched one is the cure.

### 2.3 Chrome's own UI, driven over the DevTools protocol [decided]

Koyo draws the icons and the list. Everything behind them is Chrome's: the popup is
Chrome's own popup (so it has every API a popup has, and `activeTab` is granted as a click
grants it), the install dialog is Chrome's, `chrome://extensions` is Chrome's page. The
other way, a popup's page loaded in a pane of Koyo's, was probed and is a tab to the
extension, not a popup: `tabs.query({active: true, currentWindow: true})` answers with the
popup itself (`.superpowers/research/run/03-popup-as-page.out`).

- Each page carries Chrome's toolbar, hidden (`ChromeToolbarType::NORMAL`, the view not
  visible): a popup anchors to its extensions container, and there is no action to trigger
  without one.
- The host talks to Chrome over a **browser-level DevTools pipe** inside Koyo's own
  process: `--remote-debugging-pipe`, with the two descriptors named by
  `--remote-debugging-io-pipes` (a patch: Chromium takes 3 and 4 on macOS, which Koyo has
  long given away by the time the engine starts). No port is opened; nothing outside the
  process can reach it.
- **The pipe mustn't make Koyo an automation client** (`devtools-pipe-embedder`). Started
  with no window of its own (`no-startup-window`) and a DevTools pipe, stock Chrome takes
  whoever holds the pipe for one: it holds itself alive until the pipe hangs up, and when
  it does, quits by itself (`ChromeDevToolsManagerDelegate`, `CloseBrowserSoon`). That
  exit ran beside the host's own shutdown and ended the process before the profiles had
  written theirs: the host's test of a sign-in across the engine quitting lost its cookies
  every time. `[proven]` 2026-11-16, the engine as a process of its own; in the app's
  process the sign-in was kept all the same, twice of twice. Leaving the pipe open instead
  left an engine that never quit. With the patch Chrome does neither for a pipe whose ends
  are named (`--remote-debugging-io-pipes`), the host hangs up as it stops, and the pipe
  isn't made on a framework without the patch.
- Over it, Koyo's additions to the `Extensions` domain: the installed extensions of every
  profile, an action run on a page, turning one on or off, removing, pinning, which page
  is the active one, and events when an action or the list changes (section 10). Pages are
  named there by CEF's own number for a browser, which the host has from the moment a page
  is made, and profiles by their folder's name.

## 3. Pages as tabs

- **One window per page.** CEF gives each Chrome-style page a browser of its own, so to an
  extension every Koyo page is a window with one tab, always its active tab. A patch has
  those windows say `normal` instead of `popup`. One window with all the pages as its tabs
  would need Chrome's tab strip model to hold pages CEF made separately; Cra ships the
  one-window-per-page shape and extensions work in it. `[decided]`
- **The current window is the page the person is on.** The app tells the host which page
  has the keyboard, or was shown last (`AppRequest::Active`); the host tells Chrome each
  time (`koyoSetActivePage`), whose "last active browser" it becomes, the others ceasing
  to be. Each time, as a window of Chrome's own may have been the active one in between,
  which the host doesn't hear of. So `tabs.query({active: true, currentWindow: true})`
  from a service worker, a context-menu click or a shortcut is that page.
- **`windows.getCurrent()` too**, by a patch. Chrome answers it with the focused window,
  else the caller's own, else the window made last; a page of Koyo's is in a window that's
  never key, and a service worker has none, so it named the newest page, were that a tab
  opened in the background. With no window focused it's now the one active last
  (`extension-current-window-last-active`). `[proven]` 2026-11-16
- **The page the person is on is told as they come to see it**, not as the engine comes
  to count it on screen: a page an agent opens is on screen for the engine before its pane
  draws it (`sight.rs`), and was never told, so extensions went on taking the page before
  for the current one.
- **`tabs.create`** opens a Koyo tab: Chrome offers the new tab to the page's client as it
  offers a `window.open` (`OnBeforePopup`, marked `koyo_extension_tab`, which no page can
  set). The host takes it: the page gets a pane number of the host's own (from 2^40 up,
  where the app's count from one), Chrome makes its browser in a holder as any page's, and
  the app is told to give it a tab beside its opener's (`HostEvent::TabOpened`), in front
  when the extension asked. The tab the extension gets back is that page, so what it does
  with the tab's id afterwards (a sign-in it watches, a page it closes) works.
- **`tabs.update({active: true})`** selects the Koyo tab (`HostEvent::TabActivated`).
- **`tabs.remove`** closes the Koyo tab: a page that closes without the app having asked
  is told apart from one the app closed (`HostEvent::TabClosed`), and its tab goes; a
  pinned page's page closes and stays pinned. A page a script opened that closes itself is
  the same case. Before this a page the engine closed left its tab standing over nothing.
- **A window Chrome makes on its own** (`native/strays.rs`). `windows.create` with type
  `popup` or `panel` (a wallet's confirmation, a sign-in window, a popped-out popup) stays
  a window of Chrome's, drawn as Chrome draws it: that is what the extension asked for,
  and Arc does the same. A tabbed window Chrome makes (a tab it has nowhere to put, as an
  extension's options or the page it opens once removed; `windows.create` with type
  `normal`) has no place in Koyo: the address it goes to opens as a Koyo tab beside the
  page the person is on when that's of the window's profile, else beside another page of
  that profile, and the window is closed before it shows. Such windows close as Koyo
  quits. On CEF's own framework, where extensions have no controls, every such browser is
  closed at once, as all were before. `[decided]`
- **How the two are told apart.** Chrome tells a client nothing of a browser's kind, and a
  browser just made has no window to look at. By its first page it has one, and the
  window's style says: a tabbed window's content takes the whole window, its tabs being in
  the title bar (`FullSizeContentView`); a popup's has a title bar of its own. `[proven]`
  2026-11-16, both made by an extension. Before, the kind was read as the browser was
  made, found no window, and took every one for tabbed: a popup became a tab.

## 4. Installing

- The Chrome Web Store opens as any page. Its banner says "Switch to Chrome to install
  extensions and themes"; the "Add to Chrome" button beside it works all the same.
- **The banner is Google's server's word, not anything the page reads of Koyo.** The
  store's script (its module `o2G9me`, read 2026-11-16) shows it when a value the server
  writes into the page is false, in any Chrome from 142 on, and then looks at neither the
  user agent nor the brands. The server takes four request headers, `X-Browser-Validation`,
  `-Channel`, `-Year` and `-Copyright`, which Google's own Chrome sends and Chromium's
  sources haven't got. `[proven]` by fetching the page: with "Google Chrome" among the
  `Sec-CH-UA` brands the value is still false, and the four headers come back in the page.
  That a true validation header turns it is `[read]`, not tried: only Google's build makes
  one. So Koyo, which says `Chrome/156.0.0.0` and the brands `Chromium` and `Not:A-Brand`
  as CEF does, leaves the banner: nothing it may honestly send takes it away. `[decided]`
- The button brings Chrome's own dialog over the page, with the permissions asked for; "Add
  extension" installs it in the page's profile. A patch drops Chrome's "installed" bubble,
  which points at a toolbar button Koyo doesn't show.
- **The dialog is a window of its own, not a sheet.** Chrome shows it as a sheet on the
  page's window, and a sheet needs that window to be one of Chromium's; the page is drawn in
  Koyo's. Shown as a window instead, it kept the title bar a sheet never draws, over its
  heading (the owner's build, 2026-11-16). `dialog-without-parent-titlebar` keeps that bar
  clear and untitled. `[read]` in `native_widget_mac.mm`, whose own note says as much; the
  dialog hasn't been seen with it.
- The extension is in the row and the list at once (`koyoInstalledChanged`), and its
  welcome page, if it opens one, is a Koyo tab.
- **"+"** at the end of the row opens the Web Store's extensions page in a new tab.
- Unpacked extensions: `chrome://extensions`, developer mode, "Load unpacked", Chrome's own.

## 5. The icons

- **The site controls' popover** (`docs/browser.md` 12) gets the row Arc has: between the
  actions and the settings, the icons of the profile's enabled extensions, pinned ones
  first, then **"+"**. An icon carries its badge. A press runs the action and closes the
  popover; the popup, if the extension has one, hangs from the sliders button. In both
  layouts.
- **On the page's toolbar**, in the Orca layout, the pinned extensions' icons sit between
  the address field and the responsive button, as Chrome's sit right of its address. The
  Arc layout's sidebar bar has no room for them, and Arc itself keeps them in the control
  center unless its toolbar is shown.
- **An icon is** the extension's own: what it set for the tab, else what it set for all,
  else its manifest's, drawn at 16 points; with none, the first letter of its name on a
  round. Greyed when the action is disabled for the tab. The badge is its text on its
  colour at the icon's lower right, as Chrome draws it. The tooltip is the action's title.
- **The icon's menu** (right press), Koyo's native menu: the extension's name (its home
  page, the store's for one from there), Options when it has them, Pin to Toolbar or Unpin
  from Toolbar, Manage Extension (its page in `chrome://extensions`), and Remove from
  Koyo, whose only row, "Remove “name”", is the asking once more. From the popover the menu
  opens under the sliders button, the popover having closed.
- What the menu, the "+" and Chrome itself open comes in front: an extension's page, its
  options, Chrome's page about it, the store, and the tab that stands in for a tabbed
  window Chrome made (`BrowserEvent::GoToTab`, `HostEvent::OpenUrl`'s `front`). A link
  opened in a new tab still opens behind. They opened behind at first, as links do.
- Pinning is Chrome's own (`extensions.pinned_extensions` in the profile), so an extension
  that asks `action.getUserSettings()` is told the truth, and `chrome://extensions` agrees.
- A page with no site (a start page, `chrome://` pages) has no site controls, and so no
  row; the toolbar's pinned icons are there on every page.

## 6. Popups

- A press on an icon asks the host to trigger the action on that pane's page
  (`Extensions.koyoTriggerAction`): Chrome grants `activeTab`, then opens the popup or fires
  `action.onClicked`.
- Chrome opens the popup as a bubble of its own under its hidden toolbar. The host finds
  the new window and puts it where the app said: a top corner of it under the icon,
  kept on the screen, and put back there whenever Chrome anchors it to its toolbar again,
  which it does as the popup's page resizes. Until it's placed it's clear, so it doesn't
  flash at the page's top. The window is known for the popup by what it is: a new bubble
  of Chromium's that is a child of the page's holder, or, where Chromium has given it no
  parent yet, lies over the page.
- **Which corner** (`Corner`) is the app's to say: the one that keeps the popup over
  Koyo's window. From an icon in the window's left half, the site controls of the
  sidebar's address field, it hangs by its top left and opens to the right; from one in
  the right half, a toolbar's end, by its top right, as under Chrome's toolbar. By its top
  right from the sidebar it hung out over the desktop to the window's left (seen
  2026-11-16).
- **A press on the icon of the popup that's open closes it** and opens no other, and a
  popup goes when its page stops being the one the person is on (another tab chosen, its
  own closed). Chrome does both through its toolbar, which is hidden here, so the host
  does them (`popup.rs`): left to Chrome, a second press opened a second popup.
- A press that opens no popup (`action.onClicked`) is waited on for five seconds, a new
  bubble kept clear meanwhile, then let be.
- **What it rests on.** Nothing of Chrome's says which window a popup is: the host takes
  the first new bubble over the page within those five seconds, a sheet excepted (the
  dialog Chrome asks an extension's permissions in). A bubble of Chrome's own that an
  action brings up instead of a popup would be taken for one and put under the icon. The
  engine saying which window it opened would end the guessing; that is a patch not written.
  `[open]`
- The popup is the extension's `popup` view: it sizes itself, takes the keyboard, and
  closes when it loses it, as in Chrome. Two patches take out a two-second wait before its
  page loads and a quarter second's animation of the hidden toolbar.
- The popup is Chrome's bubble as Chrome draws it. Arc puts it in a small titled window
  that can be dragged away; that is its own, and isn't copied. `[decided]`

## 7. Context menus

Koyo shows a page's context menu itself, from the model CEF hands it, which in a
Chrome-style page is Chrome's own menu: an extension's entries are in it, under the
extension's name when it has several, and picking one is Chrome's command. Their icons
aren't carried: Koyo's menu rows have none.

## 8. Profiles

- Extensions are a profile's, as everything Chromium keeps is: they live in the profile's
  folder. A project's pages are of the profile the project is on (`docs/browser.md` 15,
  `browser::profile`), so an extension installed from a page is there for every project on
  that profile, and for none on another. A project put on another profile has that one's
  extensions from then on.
- "Clear browsing data" moves every profile to a fresh folder
  (`docs/native-pages.md` 7), and the extensions go with the rest. `[owner]`: Chrome keeps
  extensions through "Clear browsing data"; keeping them here means carrying
  `Extensions/`, `Local Extension Settings/` and the extension entries of the preferences
  into the new folder, which isn't built.

## 9. What isn't there

- **Popups nobody pressed for**: `action.openPopup()` and the `_execute_action` shortcut
  open the popup where Chrome puts it, at the page's top right corner, not under an icon.
  `[not built]`

- **Side panels** (`chrome.sidePanel`): Chrome's side panel isn't shown in a page of
  Koyo's, and Arc has none either. Cra shows them in a split; its patch is not taken.
  `[not built]`
- **Manifest V2**: Chromium 156 no longer loads them, and Chrome's store no longer offers
  them; uBlock Origin's successor there is uBlock Origin Lite. Cra patches them back in;
  not taken, as that is a fork to carry on every engine update. `[decided]`
- **Themes** from the store: Chrome's theme colours have nothing of Koyo's to paint.

## 10. The patches

`script/cef-patches/`, applied by `script/cef` in name order.

| Patch | From | What |
| --- | --- | --- |
| `extensions-normal-windows` | Cra | A page's window says `normal`, and `action.openPopup()` doesn't ask for a visible toolbar |
| `extension-tabs-create` | Cra | `tabs.create` is offered to the page's client (`OnBeforePopup`, `koyo_extension_tab`) |
| `extension-tab-activate` | Cra | `tabs.update({active: true})` tells the client (`Koyo.activate`) |
| `extension-popup-browser-info` | Cra | A popup's page doesn't wait two seconds for a CEF browser it never gets |
| `extension-popup-no-popout-wait` | Cra | No animation of the hidden toolbar before the popup |
| `extension-install-ui-null-tab` | Cra | No "installed" bubble; an install with no active tab doesn't crash |
| `koyo-extensions-protocol` | Koyo | The `Extensions` DevTools domain: the list (`koyoGetInstalled`), an action run (`koyoTriggerAction`), on and off, remove, pin, the active page, and events for an action and for the list; `triggerAction` answers instead of crashing (from Cra); a page found by CEF's number for its browser |
| `devtools-pipe-descriptors` | Koyo | `--remote-debugging-io-pipes=<read>,<write>` on macOS and Linux |
| `devtools-pipe-embedder` | Koyo | With those descriptors named, Chrome doesn't wait on the pipe's client before it quits, and doesn't quit by itself when the client hangs up |
| `dialog-without-parent-titlebar` | Koyo | A window-modal dialog whose parent isn't one of Chromium's windows, Chrome's install dialog over a page in Koyo's, has no title bar drawn over its heading |
| `extension-current-window-last-active` | Koyo | With no window focused, an extension's current window is the one active last, not the one made last |

Each is a patch to carry: an engine update applies them again, and one that no longer
applies stops `script/cef` with its name. `script/cef-patches/README.md` says which files
each touches and carries Cra's licence.

Building them costs what `docs/browser.md` 11.3 says a build costs, once; after that a
change to a patch is the files it touches and the framework's link.

## 11. Where it lives

| Path | What |
| --- | --- |
| `script/cef`, `script/cef-patches/` | The framework built with the patches; the patches, what each touches, Cra's licence |
| `koyo-browser-ipc/src/extensions.rs` | An extension, an action's state and a change, as the protocol carries them (version 14) |
| `koyo-browser-host/src/extensions.rs` | The host's part: the pipe's switches, what's asked of the engine and heard from it, the list and the actions told to the app, an action run, the page the person is on |
| `koyo-browser-host/src/extensions/pipe.rs` | The browser-level DevTools pipe inside the process |
| `koyo-browser-host/src/extensions/popup.rs` | Chrome's popup found and put under the icon |
| `koyo-browser-host/src/native/adopt.rs` | A tab an extension opens, taken in as a pane |
| `koyo-browser-host/src/native/strays.rs` | Browsers Chrome makes on its own: a window that stays, a tabbed one whose page becomes a tab |
| `koyo-browser-host/src/native/parts.rs` | A page's hidden Chrome toolbar; the browser view of a page an extension opens |
| `koyo-browser-host/src/pane.rs` | An extension's tab offered as a popup; the engine's word that a tab is asked for in front |
| `koyo-browser-host/tests/smoke/extensions.rs`, `tests/smoke/extension/` | The smoke test and the extension it loads |
| `koyo/src/browser/extensions.rs` | What the app keeps: each profile's extensions and their actions, an icon resolved for a page |
| `koyo/src/browser/extensions/icon.rs` | An icon drawn, with its badge |
| `koyo/src/browser/extensions/menu.rs` | An icon's press and its menu, and what a pick does |
| `koyo/src/browser/site/panel.rs`, `toolbar.rs` | The popover's row; the toolbar's pinned icons |
| `koyo/src/browser/view/extensions.rs` | A view for a page the host made already; telling which page the person is on |
| `koyo/src/shell/browser_tabs.rs` | The tab given to a page an extension opened, beside its opener's wherever that is (`open_beside`); a tab brought in front |
| `koyo/src/native/window.rs` | A point of a window as a point of the screen, for where a popup hangs |
| `koyo/src/browser/native/probe/look.rs` | The harness's `tick` keeps menus and popovers shown on a desktop where Koyo is never in front |

## 12. Checked

By tests that need no engine: the protocol's wire names (`koyo-browser-ipc` `tests.rs`);
the pipe's framing, a list entry read, a popup's place on the screen by either corner, a
tabbed window told from a popup by its style (`koyo-browser-host`); the store's order and
what a page shows, a badge's colours, an icon's letter and picture, an icon's menu, which
corner a popup hangs by, the popover's height with the row, a tab asked for shown and a
link's tab not (`koyo`).

In the running debug build on Koyo's agent desktop, 2026-11-15 and 16, a test bundle with
folders of its own (`.superpowers/ext/desk/`), the pointer's own events through the
desktop's tools, on the patched framework. The extensions: Bitwarden and uBlock Origin
Lite from the Chrome Web Store, and one loaded from a folder
(`.superpowers/research/ext/probe`), whose service worker was asked over DevTools what it
sees and told what to do. Where a row says an extension did something, that is how.

| What | State |
| --- | --- |
| On CEF's own framework the build runs as before, and says extensions' controls are off | checked 11-15: a page opened and drew; the log line; no row in the popover |
| The patched framework starts in Koyo's process with the pipe on named descriptors | checked: the host heard the engine's list over it |
| Chrome's toolbar, carried hidden, takes no room and isn't seen | checked: the page fills its pane, by eye |
| An install from the Chrome Web Store | checked, twice: "Add to Chrome", Chrome's dialog, "Add extension"; no crash; the icon in the row at once; Bitwarden's welcome page a Koyo tab; the store's button turned to "Remove from Chrome" |
| The install dialog's look | checked: its heading clear, no band over it (`dialog-without-parent-titlebar`); over the page, near its top, above Koyo's window |
| The store's "Switch to Chrome" banner | there, as section 4 says it would be |
| Across a restart of the test build | checked: both extensions listed again, their icons drawn, the pinned one still on the toolbar |
| A page is a `normal` window with one active tab | checked: `windows.getAll` |
| The current window and its active tab are the page shown | checked from a worker, `windows.getCurrent`, `windows.getLastFocused`, `tabs.query({active, currentWindow})`: after a tab was chosen, after an agent opened one in front, and with a tab an extension opened behind. The last two were wrong before two fixes (section 3) |
| The popover's row: icons and "+" | checked, by eye: an extension's own icon, a letter for one with none, "+" opens the store in front |
| The toolbar's pinned icons (Orca layout) | checked, by eye |
| A press on an icon opens Chrome's own popup under it | checked: from the sidebar's site controls it hangs by its top left and from the toolbar's end by its top right, over Koyo's window both times. It hung out to the left of the window before `Corner` (section 6) |
| A second press on the icon closes the popup | checked. It opened a second popup before |
| The popup goes when another tab is chosen | checked. It stayed before |
| A press inside the popup | **not checked**: a click on Bitwarden's popup changed nothing. `[read]` why: a page of Chromium's doesn't take the click that would make its window key (`acceptsFirstMouse`, `render_widget_host_view_cocoa.mm`), and on the agent desktop the test build is never the active app, so no window of its becomes key and every click is that one. On a real desktop the host makes the popup key as it places it. Nor is "it closes when it loses the keyboard" seen there. Both need the app in front on a real desktop `[open]` |
| A badge and an icon an extension sets | checked, by eye: an icon set from image data; a badge for every page and another, with its own colour, for one tab; the icon greyed where the action is off for the tab; uBlock Origin Lite's own count of what it blocked |
| Pin and unpin | checked: the extension's `action.getUserSettings()` says so each time, `chrome://extensions` agrees, the toolbar's icon comes and goes |
| Options, Manage Extension | checked: each opens its page in a tab in front. They opened behind before |
| Turning off, in `chrome://extensions` | checked: Koyo's icon went at once, and came back with it on |
| Remove | checked: gone from the row, the engine's list and the profile's folder |
| The icon's menu itself | **not opened**: it's a native menu, which shows on the owner's own desktop from there. Its rows were picked through the harness (`pick`), which runs what a row runs |
| `tabs.create`, in front and behind | checked: a Koyo tab beside its opener, shown or not as asked |
| `tabs.update({active: true})` | checked: Koyo shows that tab |
| `tabs.remove` | checked: Koyo's tab closes |
| `windows.create`, type `popup` | checked: a window of Chrome's own with its title bar, on the same desktop as Koyo's. It became a tab before the kind was read from the window's style (section 3) |
| `windows.create`, type `normal` | checked: its page opens as a Koyo tab; no window shows |
| A content script, a service worker, blocking rules | checked: the folder extension's script marked the page; with uBlock Origin Lite, a page's fetch of an ad script and of an analytics script failed and of its own site loaded |
| An extension's entries in a page's context menu | **not checked**: the menu is native and would show on the owner's desktop `[open]` |
| Koyo quits cleanly with extensions, a popup and a window of Chrome's open | checked: "the browser engine stopped in" 90 to 270 ms each time |
| A sign-in across Koyo quitting, in the app | checked: a page's session cookie, its cookie with an age and its local storage were all there after a quit within a second of setting them and a start again, 2 of 2 before `devtools-pipe-embedder` and 2 of 2 with it (the host then not yet hanging up, as it does now) |
| A sign-in across the engine quitting, the engine a process of its own (`host_tests`, `keeping`) | lost 3 of 3 before `devtools-pipe-embedder`; kept 15 of 15 with it, and the engine as quick to quit as with no pipe (section 2.3) |
| The host's smoke test for extensions (`tests/smoke/extensions.rs`) | passed, its first run, before the patch; not run since |
| `script/cef` on a built folder | checked: patches found applied, only what a new patch touches built, the framework and its list of patches in place |

Still to do: the three rows marked; both of the engine's suites whole on the framework
with `devtools-pipe-embedder`, on a machine not busy with other builds (they allow an
engine five seconds to quit, and on 2026-11-16, with other checkouts building, an engine
with no pipe at all missed that in 20 tests of 20); and a from-scratch `script/cef`
(`docs/browser.md` 11.3). The suites run only while nobody is at the machine: they take
the keyboard from whoever is.

## 13. For the owner

Decisions made while the owner was away, each the nearest to Arc:

1. Extensions' controls need the patched engine; on CEF's own there are none (2.2).
2. A patched CEF, built by `script/cef` with the codecs (2.2). Where a built framework is
   kept for other Macs is still open (`docs/browser.md` 11.3).
3. One window per page, `normal` (section 3).
4. Windows an extension opens stay Chrome's own windows (section 3).
5. Icons in the site controls in both layouts; pinned ones on the Orca toolbar (section 5).
6. No side panels, no Manifest V2 (section 9).
7. "Clear browsing data" takes the extensions too (section 8).
8. Koyo doesn't pass for Google's Chrome to the Web Store, so the store's "Switch to
   Chrome" banner stays above a button that works (section 4).
