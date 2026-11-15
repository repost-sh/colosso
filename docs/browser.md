# Koyo's window and its browser tabs across a restart

Written 2026-11-06 on branch `schulzfel/browser-tab-restore`, and rewritten 2026-11-10 on
`schulzfel/url-bar-panes-and-spaces`, when the owner asked for the window back as it was:
splits, tab order, the tab shown and the worktree selected, not only the pages. Drafted with
Claude. Source of truth for what comes back after the window closes or Koyo quits, and how;
update it when a decision changes, and keep it in step with `koyo-rs/koyo/src/shell/kept.rs`.

The window comes back as it was after it's closed and opened again from the Dock, and after
quitting and relaunching Koyo: each checkout's tabs in their order, split as they were, the
tab shown shown, each project on the worktree that was selected. None of a tab's content is
made until the tab is shown. What a pane showed outlives the window somewhere else: a
terminal in the daemon (`docs/terminals.md` 5.1), a chat in the agent host (`docs/chat.md`
C13), a file on disk. No browser process outlives the app, so a page is kept as its address.
The app keeps the shape, and a reference to each pane's content, in one file.

Status marks: `[decided]`, `[not built]`.

---

## 1. What comes back [decided]

- Each checkout's tabs, in the order they had, whatever their kinds, and the tab that was
  shown on each.
- Each tab's panes, split as they were: side by side or one over the other, each split's
  share of the room as it was dragged.
- Each project on the checkout that was selected in it. Which project and space are shown is
  in `settings.json` already (`docs/arc-sidebar.md` 9).
- A pane's content, by kind:
  - **Page:** at the address it showed, titled as it was until its page says otherwise, with
    its site's icon when one is kept (`history-icons/`, `docs/new-tab.md` 4.7). A new tab
    still on its start page comes back as one.
  - **Terminal:** the terminal it showed, by its handle in the daemon, where its tab was,
    even on another worktree's list than its own (`docs/arc-sidebar.md` 5).
  - **Chat:** the thread it showed. A chat with nothing sent yet comes back as its tab, with
    the draft its folder keeps (`chat/drafts.rs`), so text typed and not sent is where it
    was typed.
  - **File:** the file it showed, as it is on disk, kept rather than a preview. Unsaved
    edits are asked about before the window closes, so there are none to bring back.
  - **Changes:** the checkout's diffs.
- A pane whose content is gone isn't brought back, and its split keeps the other side: a
  terminal that ended or was closed, a chat that was deleted, a file that's no longer there.
- Terminals and chats made while the window was closed, which no pane of the file shows,
  get a tab of their own after the others, as they did before the window kept its shape.
- The browser host runs in the app's own process (`docs/native-pages.md` 3.1), so it never
  outlives it: the app stops it on quit (`browser/host.rs`,
  `koyo-browser-ipc/src/client.rs`). It starts at launch with no page (`browser::warm_up`),
  once while Koyo runs.

## 2. Where it's kept [decided]

`window.json` in the app's data folder (`settings::data_dir()`; `KOYO_DATA_DIR` moves it),
beside `history.json`:

```json
{
  "projects": [
    { "project": "/Users/me/src/app", "checkout": "/Users/me/koyo/workspaces/app/feature" }
  ],
  "checkouts": [
    {
      "project": "/Users/me/src/app",
      "checkout": "/Users/me/koyo/workspaces/app/feature",
      "shown": 1,
      "tabs": [
        { "kind": "page", "url": "https://docs.rs/serde", "title": "serde - Rust" },
        {
          "axis": "row",
          "ratio": 0.4,
          "first": { "kind": "chat", "thread": "01JC…", "title": "Plan the release" },
          "second": { "kind": "page", "url": "http://localhost:3000/", "title": "App" }
        },
        { "kind": "terminal", "handle": "term_01JC…", "name": "zsh" },
        { "kind": "newChat" },
        { "kind": "file", "path": "/Users/me/koyo/workspaces/app/feature/README.md" },
        { "kind": "changes" }
      ]
    }
  ]
}
```

- `projects`: the checkout selected in each project, by the project's main checkout.
- `checkouts`: one entry per checkout with tabs: its folder, its project's main checkout,
  its tabs left to right, and `shown`, the place of the tab shown.
- A tab is a pane, or a split: `axis` (`row` side by side, `column` one over the other),
  `ratio` (the first side's share of the room), `first` and `second`, each a tab again. The
  axis is the model's own `SplitAxis`, kept by these names.
- A pane is what it showed, by `kind`: `page` (`url`, empty for a start page, and `title`),
  `terminal` (`handle`, `name`), `chat` (`thread`, `title`), `newChat`, `file` (`path`),
  `changes`. A kind this build doesn't know is read and brings nothing back, so a file a
  later build wrote still opens.
- Not kept: a page's `DevTools`, which belong to a page that isn't made yet when the window
  comes back; a terminal the daemon hasn't opened yet, which has no handle to find it by
  (a tab opened out of sight and never looked at).
- Written 500 ms after a change, with whatever else changed meanwhile; and on quit and when
  the window closes, if a write is due. A file that can't be read is no tabs. `kept_file`
  does the reading and writing, as it does for the chat drafts and the history.
- **`browser-tabs.json`**, which the builds before this kept the page tabs in on their own,
  is read the first time there's no `window.json`: each checkout's pages come back, a tab
  each, as they did then. From the first change on, `window.json` has them. The old file is
  never written again and is left where it is, for a build that still reads it.

- **Loose pages** (`docs/arc-sidebar.md` 11), the pages open in a space that are no
  project's, are in this file too, under `spaces`, left out while there are none:

  ```json
  "spaces": [
    { "space": 2, "shown": 0, "tabs": [
      { "url": "https://mail.example.com/", "title": "Inbox", "project": "no-project" }
    ] }
  ]
  ```

  `space` is the space's `id` in `settings.json`, which stays its own whatever its place
  among the spaces; `shown` the place of the loose page the space showed, left out when it
  showed its project's tab; each of `tabs` a page's address, its title, and the project
  whose browsing profile it keeps, `no-project` for a page opened outside any. They come
  back as the window opens, waiting for no checkout: `Shell::restore_pages`. Those of a
  space the settings no longer have are the space shown's.
- **Pinned pages** (`docs/arc-sidebar.md` 11) aren't in this file: they're no checkout's and
  no space's. They have a file of their own beside it, section 10.

## 3. One rule for what's kept [decided]

After each change of the shell, as `Terminals::retain` and `ChatTabs::retain` run,
`Shell::keep_window` has a write due. When it's made, 500 ms on or on the way out,
`Shell::window_due` builds what's kept from the panes as they are then, and writes it if it
differs from what the file holds: a run of changes, a divider dragged or a row fading, is
looked at once, and a window that didn't change isn't written. There's no hook at each of the
ways a tab comes, moves or goes (opened, navigated, retitled, reordered, split, a split
resized, moved to another worktree, closed by its ×, a split's ×, ⌘W or app control, ⌘⇧T, a
worktree gone, a project removed): every one of them changes the shell.

- A checkout's tabs read from the file wait until git lists the checkout, and every write
  carries them meanwhile, so the first write after launch doesn't drop them. So does a
  project's selected checkout, until git lists the project's worktrees.
- They go once their project is removed, or once their project's worktrees are listed without
  the checkout.

## 4. Restoring [decided]

- `Shell::restore_window` runs in `show_worktrees`, each time git lists a project's
  worktrees, **before** `restore_terminals` and `follow_chats`. A checkout's waiting tabs
  come back the first time it's listed, once a window: a window closed and opened again
  from the Dock reads the file again, as a launch does.
- The shape first: the tabs are built from the file as it is, each pane holding the
  reference to its content, and the tab that was shown is shown. Nothing else is done for a
  pane then. Tabs opened on the checkout before it was listed, if any, come after them and
  stay shown.
- Then the daemon's and the host's own lists, which arrive a moment later, settle what's
  still there:
  - **Terminals** (`terminals/restore.rs`): a pane brought back for a terminal the daemon
    lists as running keeps it; for a person's shell a daemon restart ended, it runs the
    shell again under its name, as that restore always did; for one the list doesn't have,
    or has as ended, the pane goes. With no daemon running and none remembered, no terminal
    outlived the window, and they all go. If the list can't be had, the panes stay as they
    were brought back. A terminal the list has that no pane shows gets a tab at the end.
  - **Chats** (`chat/tabs.rs`): a pane brought back for a thread the host's list doesn't
    have goes; the others take their titles from it. A thread the host says has a tab
    (`tabOpen`) that no pane shows gets a tab at the end.
- A file that isn't on disk any more isn't brought back at all.

## 5. Content made when shown [decided]

- A restored page pane holds its address and the site's icon (`Unloaded`) and no
  `BrowserView`; the model keeps a pane from holding both (`Pane::set_view` lets the address
  go). `Shell::make_shown_browsers` makes the shown tab's missing views with
  `BrowserView::new(address, …)`, from `Shell::workspace_content` beside
  `make_shown_terminals` and `make_shown_chats`, and from `focus_pane`, so a page focused
  through the launcher or app control takes the keyboard. The page opens once it's laid out
  (`browser/view/lifecycle.rs`, `ensure_open`). Restoring makes no view and opens no page.
- A restored file or Changes pane holds what it opens (`Pane::unopened`,
  `model/unopened.rs`) and no file or diffs: those are made over their checkout's panels,
  which only the selected checkout has. `Shell::make_shown_editors` opens them once their
  tab is the one shown, after each change of the shell, the way ⌘⇧T reopens a file or the
  diffs (`shell/reopen.rs`).
- A terminal's view and a chat's are made when first drawn, as they always were, so a
  restored tab attaches to nothing and opens no thread until it's looked at.
- What reads a pane copes with one whose content isn't made: the tab's icon and title
  (`Pane::favicon`, `Face::of`), the address (`Pane::address`) that the launcher's Switch to
  Tab rows and `tab_list` show, the file and the `changes` kind `tab_list` gives an editor
  pane (`Pane::unopened_file`, `Pane::shows_changes`), and ⌘⇧T, which keeps a closed unshown
  tab's address, file or diffs and opens it again.
- What looks for the tab that already shows something finds one brought back for it, rather
  than opening a second: a file opened again from the tree, ⌘P, a link or `tab_open` shows
  its tab and opens it there (`Shell::open_brought_back`), and the diffs asked for again show
  the Changes tab that came back. A pane stops waiting the moment it's given its file or the
  diffs (`Pane::set_file`, `Pane::set_changes`), as a page's does with `set_view`.
- A restored page's first load is no new visit in `history.json`, as Firefox counts none for
  a restored session; it updates the entry's title (`docs/new-tab.md` 4.1).

## 6. Agents [decided]

A page an agent opens (`tab_open`, `pane_split`) starts in responsive mode at the person's
size for agents' pages, 1440 × 900 by default, or at the `viewport` its call asks for
(`docs/responsive.md` 4.1, 9). A restored page isn't one an agent opens: it fills its pane
once shown, whoever opened it before.

`browser_list` lists a restored page whose tab wasn't shown yet, with its kept address and
title, `loading: false` and a 0 × 0 viewport. Every browser tool that names it, or means it
for lack of another, answers `browser_unavailable` until its tab is shown, not retryable: its
message says to open a page of the agent's own or ask the person to show this one, since
trying again alone never loads it and what the window shows is the person's to decide
(2026-11-12; an engine still starting is the retryable case, `docs/app-control.md` 11).
`browser_list` gives it `open: false`. Showing the tab is what makes the page, so the tools
don't make one of their own.

## 7. What doesn't come back [decided]

- A page's back and forward history, its scroll position and what was typed in its forms:
  CEF offers no way to restore them.
- Docked or detached `DevTools`: the page comes back without them.
- Responsive mode (`docs/responsive.md` 1; a page an agent opened at 1440 × 900 comes back
  filling its pane), zoom, and who held a page's lease.
- Which pane of a split the person was in (`docs/arc-sidebar.md` 2): the first, until
  they're in one again.
- A file's unsaved edits, its scroll position and whether it was a preview; the Changes
  pane's place in the diffs.
- The tabs closed lately, which ⌘⇧T brings back while the window lives.
- [not built] Unloading tabs in the background during a session: a page made stays until its
  tab closes.

## 8. Where it lives

| Path | What |
| --- | --- |
| `koyo/src/kept_file.rs` | Reading the file, the write a moment after a change, and the write on quit |
| `koyo/src/shell/kept.rs` | The file, the rule (`keep_window`, `window_due`) and restoring (`restore_window`) |
| `koyo/src/shell/kept/shape.rs` | A tab as it's kept, and both ways between that and the tab |
| `koyo/src/shell/kept/before.rs` | Reading `browser-tabs.json`, the file the builds before kept |
| `koyo/src/shell/kept/pinned.rs`, `tests/pinned.rs` | `pinned-tabs.json`: the pinned pages kept, read tolerantly, and brought back |
| `koyo/src/shell/kept/loose.rs`, `tests/loose.rs` | Each space's loose pages in `window.json`, and `restore_pages` |
| `koyo/src/shell/kept/tests.rs`, `tests/back.rs` | The file, the rule, what waits for a checkout, the old file; the window coming back, content made when shown, terminals and chats gone meanwhile |
| `koyo/src/browser/tabs.rs` | A page as it's brought back, and `make_shown_browsers` |
| `koyo/src/terminals/restore.rs` | The daemon's list settling the terminal panes brought back, and tabs for the rest |
| `koyo/src/chat/tabs.rs` | The host's list settling the chat panes brought back, and tabs for the rest |
| `koyo/src/model/tab.rs` | A pane's page or its kept address, never both (`Page`, `view`, `set_view`, `unloaded_address`), `Pane::address`, `Pane::favicon` |
| `koyo/src/model/unopened.rs` | An editor pane that opens its file or the diffs once shown, and what reads one (`file_path`, `shows_changes`) |
| `koyo/src/shell/reopen.rs` | `make_shown_editors`, beside ⌘⇧T |
| `koyo/src/projects.rs` | `show_worktrees`, where restoring runs, in order |
| `koyo/src/shell.rs` | The shell's observer, where the rule runs, and `workspace_content` |
| `koyo/src/shell/closing.rs` | Closing a tab, its unshown pages' addresses kept for ⌘⇧T |
| `koyo/src/control/exec/browser/page.rs` | Unshown pages in `browser_list` and the tools' refusal |
| `koyo/src/browser/view/navigation.rs` | `mark_restored`: a restored page's first load is no visit |

---

## 9. Passkeys

Written 2026-11-08 on branch `schulzfel/arc-like-sidebar`. Drafted with Claude. Pages in
Koyo's browser sign in with passkeys as they do in Safari: the standard macOS passkey sheet
comes up over the Koyo window and offers the person's passkeys in iCloud Keychain (Apple
Passwords) and in their other credential providers (1Password and the like, as turned on in
System Settings → General → AutoFill & Passwords), a phone by QR code, and security keys,
with their PIN. macOS shows that sheet for any site only to a browser signed with Apple's
managed entitlement `com.apple.developer.web-browser.public-key-credential`, which Koyo
isn't yet (9.4); until it is, every passkey request is turned down at once (9.3).

### 9.1 What a page gets [decided]

- `navigator.credentials.create` and `get` with `publicKey` go through a bridge every frame
  runs before its own scripts; every other kind (`password`, `federated`) goes to Chromium as
  before. Nothing reaches Chromium's own WebAuthn, which has no window to show its dialogs
  in, can't reach the person's passkeys, and reaches for Bluetooth.
- The answer is a `PublicKeyCredential` (`instanceof` holds) with `id`, `rawId`, `type`,
  `authenticatorAttachment`, `getClientExtensionResults()` and `toJSON()`; its response has
  `clientDataJSON` and `attestationObject`, `getAuthenticatorData()`, `getPublicKey()`,
  `getPublicKeyAlgorithm()` and `getTransports()` for a new passkey, or `authenticatorData`,
  `signature` and `userHandle` for a sign-in. `AbortSignal` aborts a request: the page's
  promise rejects with the signal's reason at once and the sheet goes.
- `isUserVerifyingPlatformAuthenticatorAvailable()` is true once Koyo may ask for passkeys;
  `isConditionalMediationAvailable()` is false, as Koyo has no autofill to offer passkeys in,
  and a conditional request waits until the page aborts it, as in a browser with no passkeys
  to offer. `getClientCapabilities()` says the same. The `signal…` methods do nothing.
- One request at a time per page; another while one waits is `NotAllowedError`, as in
  Chrome. The sheet comes up once the page's pane is drawn, over the window that shows it.
- The extension answered is `credProps`. [not built] `prf`, `largeBlob`, `appid` and the
  others go unanswered.

### 9.2 The checks [decided]

Made by the browser host, from what the engine knows of the frame (its address, and its
parents'), never from what the page's script says:

- The frame's origin is a secure context: `https`, or `http` on `localhost`, `*.localhost` or
  a loopback address. Opaque origins (`about:blank`, `data:`) have none (`NotAllowedError`).
- Every frame above it has the same origin. Chrome lets a cross-origin frame ask when its
  embedder delegates `publickey-credentials-get`/`-create` by Permissions Policy; the host
  can't read that policy, so such frames are refused (`NotAllowedError`).
- The relying party's id is the origin's domain, or a suffix of it that's registrable by the
  Public Suffix List (the `psl` crate, built in): never `co.uk` or `github.io`, never an IP
  address (`SecurityError`). A page naming none gets its domain.
- Malformed options (no challenge, a user id of no bytes or more than 64) are a `TypeError`.

### 9.3 Until Koyo is signed for passkeys [decided]

At start Koyo reads its own signature (`SecTaskCopyValueForEntitlement` on its own task) and
tells the browser host whether it carries the entitlement (`Hello.passkeys`). Without it the
host answers every `create` and `get` with `NotAllowedError` as soon as it has checked it,
within a few milliseconds, and the availability checks say no; so a site goes straight to its
other ways of signing in ("Try another way") instead of waiting. Builds made with
`script/dev` and `script/bundle` without `--sign` are such builds, and stay so: signed ad hoc
or by this Mac's own identity (12.7), a build never claims the entitlement, since macOS won't
start an app that claims a managed entitlement without the provisioning profile granting it,
and `script/bundle` stops if such a build claims any entitlement at all.

### 9.4 Getting the entitlement [not done]

What the person who owns Koyo's Apple developer account does, once:

1. An Apple Developer Program membership as an organization. The entitlement is requested
   by its Account Holder, at
   <https://developer.apple.com/contact/request/macos-browsers-passkeys/>, for the App ID
   `codes.koyo.desktop`; Apple reviews the request.
2. Once granted: in Certificates, Identifiers & Profiles, the explicit App ID
   `codes.koyo.desktop` with the granted capability turned on; a Developer ID Application
   certificate in the keychain of the Mac that builds; and a Developer ID provisioning profile
   for that App ID, downloaded as a `.provisionprofile`.
3. The build, signed and checked:

   ```sh
   script/bundle --sign "Developer ID Application: <Organization> (<TEAMID>)" \
                 --profile ~/Downloads/Koyo.provisionprofile
   ```

   `script/bundle` checks the profile before building (it names the team, is for
   `<TEAMID>.codes.koyo.desktop` and carries the entitlement), writes the app's entitlements
   from it, embeds it as `Contents/embedded.provisionprofile`, signs the engine
   (`script/browser-engine --sign`), the agent host and the app with the hardened runtime,
   and says the signature back.
4. Notarized and stapled, so other Macs open it:

   ```sh
   ditto -c -k --keepParent koyo-rs/target/bundle/Koyo.app Koyo.zip
   xcrun notarytool submit Koyo.zip --keychain-profile <profile> --wait
   xcrun stapler staple koyo-rs/target/bundle/Koyo.app
   ```

5. Checked: `codesign -d --entitlements - koyo-rs/target/bundle/Koyo.app` lists the
   entitlement; Koyo's log no longer says "passkeys are turned down"; on
   <https://webauthn.io> Register brings up the sheet over the window.

None of this has been run: there's no Developer ID identity or profile on the machine it was
written on. Only the checks before the build were tried (with a stand-in for `security`), and
the ad-hoc path. Still unknown until it runs: whether the agent host and the engine's helper
apps need entitlements of their own under the hardened runtime (JIT for the renderer helper,
as Chrome's has); and the `-67030 errSecCSInfoPlistFailed` signature check the host's log
has shown.

### 9.5 What Koyo does once signed [decided]

- The request, as the host checked it, goes to the app (`HostEvent::Passkey`, protocol 9),
  which asks macOS with `ASAuthorizationController`: a platform request
  (`ASAuthorizationPlatformPublicKeyCredentialProvider`, the person's synced passkeys and
  their providers', with `shouldShowHybridTransport` for a phone) and a security-key request
  (`ASAuthorizationSecurityKeyPublicKeyCredentialProvider`), both made with the
  `…WithClientData:` initialisers from the page's challenge and the checked origin, the sheet
  anchored to the page's window. The platform request needs macOS 13.5 (earlier is
  `NotAllowedError`), the security-key one macOS 14.4, where the browser's security-key
  requests came in. A site asking for a `platform` authenticator gets the platform request
  alone, without the phone; otherwise both, with the phone. Platform passkeys are ES256 only:
  a site that doesn't take ES256 gets the security key alone, and one taking none of ES256,
  `EdDSA` and RS256 gets `NotSupportedError`. A sign-in's allowed passkeys go to the request
  their transports name (both when they name none).
- macOS makes the client data and signs it, and the app answers `AppRequest::Passkey` with
  what the authenticator made. The host reads a new key's authenticator data and public key
  out of its attestation object for `getAuthenticatorData()` and `getPublicKey()`, as it
  shapes every answer into `WebAuthn`'s JSON forms.
- The person cancelling, or the system failing, is `NotAllowedError`; a passkey the site
  excluded is `InvalidStateError`. A page that aborts, navigates or closes withdraws its
  request (`HostEvent::PasskeyWithdrawn`) and the sheet goes.
- Known risk: reports of `ASAuthorizationController` failing with error 1004 on macOS 15.4,
  even when entitled. The app logs the system's error.

### 9.6 Tests

- `koyo-browser-host/src/passkeys/origin/tests.rs`: origins, suffixes and public suffixes.
  `passkeys/asks.rs`, `passkeys/answers.rs`: options read, answers shaped.
  `passkeys/attestation/tests.rs`: attestation objects read, keys as DER, broken ones.
- `koyo-browser-host/tests/smoke/passkeys.rs`, the host alone against the packaged engine:
  frames of the page's origin and of another, requests asked, answered and withdrawn.
- `koyo/src/browser/passkeys/sheet/tests.rs`: which requests an ask becomes, the system's
  objects for them, and how the sheet's endings read.
- `koyo-browser-ipc/src/passkeys/tests.rs`: the messages round-trip.
- `koyo/src/browser/host_tests/passkeys.rs`, against the packaged engine
  (`script/cargo test -p koyo browser::host_tests::passkeys -- --ignored`): without the
  entitlement `create` and `get` reject with `NotAllowedError` within half a second; and, with
  a stand-in for the sheet, its answer reaches the page as a `PublicKeyCredential` and an
  aborted request is withdrawn from the app.
- The sheet itself can't be tested until Koyo is signed for it.

### 9.7 Where it lives

| Path | What |
| --- | --- |
| `koyo-browser-host/src/passkeys/bridge.js` | The page's side: `navigator.credentials`, the credentials built from the host's answers |
| `koyo-browser-host/src/passkeys/renderer.rs` | The renderer's side: the bridge run as each frame's context is made, messages to and from the browser process |
| `koyo-browser-host/src/passkeys.rs` | The browser process: requests checked, sent to the app, answered, withdrawn |
| `koyo-browser-host/src/passkeys/origin.rs` | Secure contexts, frames above, relying party ids and public suffixes |
| `koyo-browser-host/src/passkeys/answers.rs`, `attestation.rs` | Answers in `WebAuthn`'s JSON forms; a new passkey's authenticator data and public key |
| `koyo-browser-ipc/src/passkeys.rs` | The request and answer on the wire |
| `koyo/src/browser/passkeys/mod.rs` | The entitlement check, one request per page, the sheet over its window |
| `koyo/src/browser/passkeys/sheet.rs` | `ASAuthorizationController`: requests, the sheet, answers and errors |
| `script/bundle` | `--sign`/`--profile`: the signed build that claims the entitlement |

---

## 10. Pinned pages [decided]

Added 2026-11-13 on `schulzfel/pinned-browser-tabs`. The pages pinned at the head of the Arc
sidebar (`docs/arc-sidebar.md` 11) are no checkout's and common to every space, so the window's
file doesn't hold them. They're kept in `pinned-tabs.json`, beside `window.json` in the data
folder (`settings::data_dir()`; `KOYO_DATA_DIR` moves it):

```json
{
  "version": 1,
  "shown": 0,
  "tabs": [
    {
      "url": "https://mail.example.com/inbox",
      "title": "Inbox (3)",
      "pinnedUrl": "https://mail.example.com/",
      "project": "/Users/me/src/app"
    }
  ]
}
```

- `tabs`: the pinned pages in the order of their tiles. `url` is the address the page last
  showed, empty for a start page; `title` its title then; `pinnedUrl` the address it's pinned
  at, where it goes back once closed; `project` the main checkout of the project whose
  browsing profile it keeps, for its sign-ins.
- `shown`: the place among `tabs` of the pinned page that was the tab shown, left out when
  none was.
- `version`: 1. It's there for a later build to tell files apart; no build refuses a file for
  its version.
- **What comes back:** each page, in order, at `url`, titled as it was, with its site's icon
  when one is kept (`history-icons/`), pinned at `pinnedUrl`, of `project`'s profile; and
  the one `shown` is shown, in place of the space's own tab. As with any page (section 5),
  none is made until it's shown: the others are tiles until picked. `Shell::restore_pages`
  brings them back once as the window opens, with the loose pages (section 2), before any
  project's worktrees are listed, as they wait for none.
- **The rule** is the window's (section 3): after each change of the shell a write is due,
  made 500 ms on or on the way out from the pinned pages as they are then, through
  `atomic_file`, and only if it differs from what the file holds.
- **Read tolerantly**, by hand rather than by a shape (`shell/kept/pinned.rs`), so that no
  build's file drops a pinned page:
  - A field this build doesn't know, in the file or in a page's entry, is carried with what
    it was read with and written back as it was.
  - A field left out, or of another type than this build writes, has its default: no title,
    no project, pinned where it showed, none shown.
  - An entry with no `url`, which this build can't read as a page, stays in the file as it
    was, after the pages it could read.
  - A `version` later than this build's is kept, not taken back to 1.
  - A file that isn't this at all (not JSON, not an object, `tabs` no list) is renamed
    `pinned-tabs.unreadable.json` beside it and read as no pages, so the next write doesn't
    lose what it held.
- **Apart from the window's file:** neither is read through the other, so a `window.json`
  that can't be read brings back no tabs and every pinned page.
- Tests and visual checks keep no file, as with the window's.
- `shell/kept/tests/pinned.rs` has the round trip, the file exactly as this build writes it
  (`TODAY`, which a later build has to go on reading), a later and an earlier build's files
  read and written again with nothing lost, the file set aside, the pages back in a new
  window, and a window file that can't be read.

---

## 11. Video and sound

Written 2026-11-13 on branch `schulzfel/fix-video-playback`, when the owner found that
WhatsApp's videos don't play. Drafted with Claude. What a page's `<video>` and `<audio>` play
is decided when the engine is built, not by anything Koyo sets.

### 11.1 What CEF's own build plays [checked 2026-11-13]

The framework `cef-dll-sys` fetches on the first build is CEF's own
(`cef-builds.spotifycdn.com`). Asked in a page of the app as installed, and of the engine
packaged from this checkout (CEF 156.0.14, Chromium 156.0.8078.48):

- **Plays:** VP8, VP9 and AV1 video; Opus, Vorbis, FLAC, MP3 and WAV sound (by
  `canPlayType`; a clip of VP9 with Opus was played). Calls go another way: WebRTC lists
  H.264 beside VP8, VP9 and AV1 among what it receives (`RTCRtpReceiver.getCapabilities`).
- **Doesn't:** H.264, AAC, HEVC and HLS playlists. For each, `canPlayType` is `""` and
  `MediaSource.isTypeSupported` is false; `WebCodecs` has none of the three codecs. A
  `<video>` given H.264 with AAC in MP4 ends in `MEDIA_ERR_SRC_NOT_SUPPORTED`,
  `DEMUXER_ERROR_NO_SUPPORTED_STREAMS: FFmpegDemuxer: no supported streams`, while the same
  picture as VP9 in WebM plays beside it.
- **So:** WhatsApp's videos don't play: they are MP4 with H.264 and AAC (Meta's media
  reference for the platform: "Only H.264 video codec and AAC audio codec supported"). That
  is read off the format, not seen in WhatsApp Web itself, which takes the owner's sign-in.
  The same goes for whatever else comes as MP4 or HLS and nothing else; a site that offers
  VP9 or AV1 as well can still play.

### 11.2 Why, and what brings them back [decided]

- H.264 and AAC are patent-licensed, and CEF's builds leave them out for that
  (`chromiumembedded/cef` issue 3559). In Chromium they are compile-time switches,
  `proprietary_codecs` and `ffmpeg_branding`, and FFmpeg is linked into the framework: no
  setting, command-line switch or feature flag turns them on in a framework built without.
- So the framework is built again, from source, with
  `proprietary_codecs=true ffmpeg_branding=Chrome` over CEF's official configuration, at the
  commit CEF's own was built from: **`script/cef`**, which says how. The headers, the C++
  wrapper and the `cef` crate's bindings stay what they were; only the framework differs.
- The same switch compiles in HEVC through the Mac's own decoder and Chromium's HLS demuxer
  (`media/media_options.gni` of Chromium 156), and on the build both play (11.5).

### 11.3 What it costs

- Xcode with its Metal toolchain (`xcodebuild -downloadComponent MetalToolchain`, a
  component for the whole machine), 90 GB free to start with, and some hours.
  **[checked 2026-11-13]** On an M4 Max that was building other things too it ran to its
  end in about three hours, twenty minutes of fetching and two and a half hours of
  compiling its 76,000 steps, and its folder then held 46 GB.
- It all goes into one folder, in the checkout unless `--dir` names another. On the owner's
  Mac that's `~/.cache/cef-build`, beside the shared cargo build folder, so that it outlives
  a worktree: `script/cef --dir ~/.cache/cef-build`. Another worktree gets the framework by
  running the same there, which builds nothing again and takes twenty seconds
  **[checked 2026-11-14]**.
- The framework an app carries is its worktree's: bundled where `script/cef` hasn't run, it
  carries CEF's own, and installing that app takes the codecs away again. So in a worktree
  whose app is to be installed, `script/cef` runs before `script/bundle`.
- A later CEF version is built from scratch, from an emptied folder (the script's header
  says why), so keeping up with Chromium's security updates costs those hours each time.
- **[not decided]** Where a built framework is kept for other Macs and for CI.
  `download-cef` fetches from `CEF_DOWNLOAD_URL` when it's set, an address that serves an
  `index.json` shaped as CEF's and an archive under CEF's name for it; then `cef-dll-sys`
  would fetch the build with the codecs itself, in every worktree, and `script/cef` would
  only make it.

### 11.4 Licences [not decided]

A build with the codecs carries FFmpeg's H.264 and AAC decoders, and both codecs are
patent-licensed (Via LA administers the two pools). What that asks of Koyo before such a
build is handed to anyone else is the owner's to settle, and isn't settled here.

### 11.5 The check

`koyo-browser-host/tests/smoke/media.rs`: a page plays a second of H.264 with AAC, in a
packaged engine, and the clip's picture is in the frame the app is handed.
**[checked 2026-11-13]** It fails on CEF's own build, with the engine's reason, and passes
on the build `script/cef` made.

```sh
KOYO_BROWSER_ENGINE=$PWD/koyo-rs/target/browser-engine/<version> \
  script/cargo test -p koyo-browser-host --test smoke -- --ignored a_page_plays
```

**[checked 2026-11-14]** In the app bundled with that build, run as a second instance and
used as a person would, a page at a time:

- **Plays now:** H.264 with AAC from a file, handed to the player as a blob (how a page
  plays a video it fetched itself) and through Media Source Extensions; AAC alone; HEVC
  with AAC; an HLS playlist by its own address and through hls.js. `canPlayType` says
  `"probably"` and `MediaSource.isTypeSupported` true for H.264, AAC and HEVC.
- **On the web:** MDN's and w3schools' MP4 clips and Mux's HLS stream play, and Vimeo's
  player, which showed "Player error" on CEF's own build. YouTube's plays as it did, and
  WhatsApp Web opens to its sign-in.
- **As before:** VP9, AV1, Opus and MP3; seeking, pausing, a reload, full screen, two clips
  at once, a clip whose tab is out of sight; a format no browser plays still says so. The
  host's smoke suite and the app's host tests fare as on CEF's own build.
- **Not seen:** a video in a WhatsApp chat, which takes the owner's sign-in.

## 12. Site controls

Written 2026-11-14 on branch `schulzfel/browser-extensions`. Drafted with Claude. The owner
asked for a page's permissions where Arc and Zen have them: a popover off the address field,
with the same look. This is that popover, what it holds, and what of Arc's and Zen's it
leaves out. Zen's is `ZenSiteDataPanel.sys.mjs` and `zen-panels/site-data.inc`
(`.superpowers/research/zen/files/`), whose sizes these are. Keep it in step with
`koyo-rs/koyo/src/browser/site.rs`.

### 12.1 In the address field [decided]

- Two buttons at the field's right end, with a page shown, in both layouts (the Arc
  sidebar's bar, `arc/top.rs`; the page's toolbar in Orca's, `browser/toolbar.rs`):
  - **Copy link:** the page's whole address to the clipboard. The link turns to a check for
    a second and a half.
  - **Site controls** (sliders): opens the popover under it, and closes it again. It stays
    lit while the popover is up.
- They're there while the pointer is over the field, fading in and out over 150 ms as a
  tab's close button does over its row (`theme::hover::eased`), and whatever the pointer
  does while the popover is up or the address was just copied.
- Neither is there while the field is being typed in, nor on a start page, which has no
  site. In the Arc layout a press on either is theirs alone: the launcher, which a press on
  the rest of the bar opens, stays shut.

### 12.2 The popover [decided]

A menu's window (`menu::Trigger::toggle_panel`), 232 points wide, as a callout
(`MenuBody::callout`): centred under the sliders, kept within the window, an arrow on its
top edge pointing at the button's middle, its corners 16 points round. It lies over the
sidebar and the page both, so it's on glass of its own (`MenuBody::backdrop`), thicker than
a menu's: dark with a dark theme and light with a light one, what's behind tinting it as in
Arc, and what's on it reading over a white page. The owner turned down a menu's own thin
glass (white text washed out over a light page) and a nearly opaque dark one (too dark),
2026-11-14. A press anywhere else in the window closes it, and so do Escape, the window
showing something else, and Koyo no longer being the app in front. Top to bottom:

- **Actions:** a row of four buttons of one width, 32 points high, an icon each:
  - **Share:** macOS's share sheet for the page's address.
  - **Screenshot:** a picture of the page as its pane shows it, on the clipboard.
  - **Find:** the page's find bar (⌘F).
  - **Developer tools:** opens or closes them (⌥⌘I).

  Each closes the popover as it acts.
- **Settings:** what the site has been allowed or blocked from, one row each: a round icon,
  the permission's name, and under it "Allowed" or "Blocked". The round is filled while it's
  allowed. A press on a row turns it to the other, there and then, and the page is told the
  next time it asks: no reload. Only what was decided is listed, as in Zen: a permission the
  site never asked for, or was never answered about, isn't there, and with none the section
  isn't either.
- **Foot:** under a hairline, the connection: "Secure" with a lock for `https`, "Not
  Secure" with an open one for anything else.

What Arc and Zen have here that Koyo doesn't, so it isn't shown `[not built]`: extensions
(the row of their icons and the "+"), boosts, reader mode, bookmarks; a certificate's
details behind "Secure"; the "…" at the foot's right, a menu of the page's other commands. "Developer Mode" and
"Automatic Picture-in-Picture", rows of Arc's own, have no counterpart: the developer tools
are an action here.

### 12.3 Where a site's permissions are kept [decided]

Chromium keeps them, in the profile, as the site's content settings; Koyo keeps none of its
own. So they're a profile's (a project's pages share them, another project's don't), they go
with "Clear browsing data".

- The app asks the host for a page's (`AppRequest::SiteSettings`, protocol 11) as the
  popover opens and after each change; the host answers with, for each
  permission it can name and has a decision for, whether it's allowed or blocked
  (`HostEvent::SiteSettings`). A row pressed is `AppRequest::SetSiteSetting`.
- The permissions that can be listed are those Chromium asks the person about and keeps as
  a content setting: the camera, the microphone, the location, notifications, the
  clipboard, MIDI devices, installed fonts, window placement, several downloads.
- **The camera and the microphone** weren't kept before: CEF hands every such request to
  the host, which asked the person every time. Now the host keeps an Allow as the site's
  setting, and answers by the setting without asking while it's allowed or blocked
  (`pane/prompts.rs`). The screen is asked for every time, as in every browser.
- The location is `GEOLOCATION`, the content setting Chromium keeps today; the newer
  `GEOLOCATION_WITH_OPTIONS` is a website setting, and asked for as a content setting stops
  the host (`location.rs`).

### 12.4 A page asks for a permission [decided]

The owner asked for Arc's way, not a dialog in the middle of the window (2026-11-14): the
question comes out of the sliders button, a callout as the popover is (`site/prompt.rs`).

- A round icon for what's asked, the question ("Allow example.com to access your camera,
  microphone?", not bold), and two buttons of one width: **Not Now** and **Allow**. Return
  is Allow.
- **Allow** is kept for the site, by Chromium for what it asks itself and by the host for
  the camera and the microphone (12.3), and shows in the popover's Settings from then on.
- **Not Now** keeps nothing: the page is told no this once and may ask again
  (`PermissionRequestResult::DISMISS`, where the dialog's Block was a kept `DENY`). Put
  away any other way, by a press elsewhere, Escape, or Koyo no longer in front, it's Not
  Now. A site is blocked from the popover: allowed once, its row turns to Blocked at a
  press. One never allowed has no row to block; it's asked each time.
- The button is lit and there while the question is up. Where it isn't drawn for the page
  that asks (its address field has never been laid out), the question is a dialog as
  before, with the same two answers.
- The question is still the person's: `browser_dialog` answers none of this kind
  (`docs/app-control.md`), and `browser_list` shows it waiting.

### 12.5 The callout's window [decided]

`native::window::callout`, asked for by the menu's surface each time its window's size
changes (`menu/view.rs`).

- macOS frames a window as a rectangle, whatever is in it, so a callout's window gives up
  the frame: the bits of its style that make one go (titled, closable, resizable, content
  under the title bar), the rest of what it is stays, and it's clear and not opaque.
- Its blur is cut to the callout's shape by a mask on its effect view, a point inside the
  outline the app draws, stretched only in the straight between its corners so the shape
  holds while the window fits what it lists.
- The app draws the rest: the body as a bordered, rounded surface, as any panel's, so its
  edge is a sharp one-point line, in three strips side by side with the one under the arrow
  left open at the top; and the arrow over it, its fill and its two slants. A first go drew
  the whole frame as stroked outlines, which came out soft; the owner turned it down, and
  found this one acceptable, 2026-11-14.

### 12.6 Checked

On Koyo's agent desktop, on a native page, the debug build, 2026-11-14
(`.superpowers/native/desk/`): the two buttons come with the pointer over the bar; the
popover opens centred under the sliders and lists a site's blocked notifications; a press
on the row turned the page's `Notification.permission` from `denied` to `granted` with no
reload; Find closed the popover and brought the find bar up; Share brought macOS's share
sheet up from the button, with the page's address; the developer tools opened from it; copy
link put the address on the clipboard, and the owner saw its check. A camera request came
out of the button as a question; Not Now told the page no, and the next request asked
again. The owner allowed a location on it and the page had its position.

Not tried: the picture, which writes the person's clipboard; the camera and the microphone
allowed, kept and answered by their setting (the owner's camera request ended in
`NotAllowedError`, not looked into); the Orca layout's field. There is no test of the
popover or the question in a window: `site/panel.rs` has its height and its names,
`site/prompt.rs` its wording, `host/site.rs` a site's address and a media request's devices.

### 12.7 What macOS allows Koyo: the screen, the camera, the microphone [decided]

The owner reported, 2026-11-15: a page asked to share the screen, macOS opened System
Settings on Screen Recording, and Koyo wasn't in the list to switch on. Read from the log of
macOS's privacy service (`log show --predicate 'process == "tccd"'`) that day:

- **Whose request it is.** Chromium's browser process is Koyo's (`docs/native-pages.md` 3.1),
  and the screen is taken there, through ScreenCaptureKit: macOS asks about Koyo itself,
  `codes.koyo.desktop` at `/Applications/Koyo.app/Contents/MacOS/koyo`, never a helper. A
  first request is refused (Screen Recording is never asked about) and macOS shows its own
  alert, which offers System Settings and puts the app in the list; switched on, the same
  Koyo process shares the screen with no restart (pid 72316 refused at 12:03, sharing at
  12:06).
- **Why Koyo wasn't listed.** macOS keeps an app's grants by its bundle id and its designated
  requirement. Two things stood in the way, both of this Mac:
  - The bundle id had moved from `dev.koyo.desktop` that morning (`2e193a503`). The agent
    that shows macOS's alert and puts the app in the list, `universalAccessAuthWarn`, had run
    since days before and still named `/Applications/Koyo.app` by the old id, where the
    signature, the Info.plist and Launch Services gave the new one. So it listed the app as
    `dev.koyo.desktop`, and System Settings dropped that entry as it opened: at 12:04, and
    again at 18:17 with a build signed by the identity below. Added with "+", Koyo is listed
    by the app on disk, and shares. Someone new to Koyo never meets this; an update across
    the rename does, until the agent starts again, at a logout.
  - Signed ad hoc, the designated requirement is the build's hash, so every build is another
    app to macOS. The entry made for one build stays in the list, switched on, and the next
    build is refused against it ("Failed to match existing code requirement"), with nothing
    of its own to switch on. The camera's and the microphone's grants are tied to the build
    the same way. The build installed at 16:22 (`8662b8e0…`) doesn't match the entry of the
    one that shared the screen at noon (`c50aaec1…`).
- **Someone new to Koyo** has no entry from an older build or an older id: the first
  request puts Koyo in the list. They'd lose the grants with every update of a build signed
  ad hoc; a release is signed with a Developer ID (9.4), whose designated requirement names
  the team, and keeps them.
- **So a build is signed by one identity** (`script/bundle`): one made on this Mac, once, by
  `script/signing-identity` (a certificate for code signing, signed by itself, its key in
  the login keychain for codesign alone). Signed by it, the designated requirement is
  `identifier "codes.koyo.desktop" and certificate leaf = H"…"`, the same for every build,
  and only code signed with that key meets it. Until it exists a build is signed ad hoc, and
  says so. Only the app's own signature counts: the engine's helpers run as Koyo's, and keep
  theirs ad hoc, so the engine still packs `KoyoMockKeychain` and the profiles' cookie key
  stays as it was.
- **Not taken:** an ad-hoc signature with a designated requirement of the bundle id alone.
  It needs no identity, but any program signing itself `codes.koyo.desktop` would get what
  Koyo was allowed: the screen, the camera, the microphone, Accessibility.
- **Left as they are:** `script/dev-bundle`'s `Koyo Dev.app` isn't sealed at all, so its
  grants still go with each debug build; signing it too would put the keychain in every
  checkout's debug loop. The agent desktop's loader keeps its grants already, being the same
  binary in every build (`docs/agent-desktop.md` 3: 2c96b1ec… through the 15:08 and 16:22
  installs, its Screen Recording allowed after both).
- **Once, by the owner:** entries made for older builds and the old id stay in the list
  and still don't match. They're removed with "−" in System Settings, or
  `tccutil reset ScreenCapture codes.koyo.desktop` (and `dev.koyo.desktop`, and `Camera`
  and `Microphone` alike), and the first build signed by the identity is allowed once.
- **codesign uses the key unasked** only once the key lets in codesign's partition,
  `apple:`: one the security tool imports lets in `apple-tool:` alone, and codesign then asks
  for the login keychain's password in a dialog, which waits for as long as nobody answers
  (a build left running waited two hours, 2026-11-15). `script/signing-identity` lets it
  in, asking the password once in the terminal. `script/bundle` checks, without a dialog,
  that the keychain is unlocked (`SecKeychainGetStatus`) and the key lets codesign in (its
  access list, read by `security dump-keychain -a`): before building, and again before
  signing. When either fails it stops and says so, rather than sign ad hoc.

Checked 2026-11-15: the identity made; a build signed by it installed (`019344328a8e…`,
`certificate leaf = H"6b376c3f…"`); the camera and the microphone asked for and kept under
it; Screen Recording added with "+", and the screen shared. Not seen yet: a grant holding
across two builds, which the next builds signed by the identity show.

## 13. Developer tools

Written 2026-11-14 on branch `schulzfel/browser-extensions`. Drafted with Claude. The owner
asked for the developer tools as Arc and Chrome have them: no bar of Koyo's over them, the
tools' own controls instead. Until now they ran as a plain page of the tools' frontend with a
bar of Koyo's on top ("DevTools · title", dock left, bottom, right, detach, close). Keep it
in step with `koyo-rs/koyo/src/browser/dock.rs` and
`koyo-rs/koyo-browser-host/src/devtools/`.

### 13.1 What the person gets [decided]

- The tools have no bar of Koyo's. Their own toolbar has what Chrome's has: at its right end
  the "⋮" menu with **Dock side** (undock, left, bottom, right) and the **×** that closes
  them; at its left the element picker and the **device toolbar** toggle.
- Picking a side in the tools moves them there, as Koyo's buttons did, and the side is kept
  as before (`settings.devtools.dock`). ⇧⌘D in the tools goes back to the side before.
- **Undock into separate window** is Koyo's detached place: a pane of their own beside the
  page, not a window of macOS's. Undocked, the tools show no × of their own (Chrome's
  don't either): the pane's × closes them, or they're docked back from "⋮".
- **×** closes them, as ⌥⌘I does, with the keys in the page or in the tools themselves.
- **The device toolbar toggle** turns Koyo's responsive mode on and off for the page
  (`docs/responsive.md`). The tools' own device toolbar isn't shown: see 13.3.
- Between the page and the tools is Koyo's divider, as Arc has it: a band of its own, nine
  points, with a grip in its middle, picked up anywhere along it (`theme::card::divider`). It
  has room of its own rather than lying over the two, either of which may be a native view
  that takes a press before Koyo's own content does; from the press until the button is let
  go the pointer is Koyo's all over the window, so it's picked up however fast the hand
  moves (`docs/native-pages.md` 6). Dragged, the pointer holds its middle.
  The size it sets is kept as before, in whole points, as a native page's is
  (`docs/native-pages.md` 6), and leaves the page and the tools 120 points each at the
  least.

### 13.2 How [decided]

The tools' frontend decides what it shows by how it's loaded and by what its embedder
answers. At a valid frontend address Chromium attaches its own embedder to the page, as it
does to any tab showing a `devtools://` page (`DevToolsUIBindings`: `DevToolsHost` and
`devtools_compatibility.js`'s `DevToolsAPI` in the page, seen on screen with
`typeof DevToolsHost` `"object"` and `isHostedMode()` false). For a page that isn't one of
Chrome's own DevTools windows that embedder's delegate does nothing with docking or closing:
the frontend's × and dock menu would go nowhere.

- **Loaded with `can_dock=true`**, which is all that turns on the dock menu, the × and the
  device toggle (the frontend of Chromium 156, read from the engine's resources). The
  address is `devtools_app.html?ws=…&can_dock=true` and nothing else: Chromium loads a
  frontend only at an address its own check (`SanitizeFrontendURL`) leaves as it is, and
  that check drops every parameter it doesn't know, so an address with one of Koyo's was
  refused (`ERR_INVALID_URL`, seen on screen with `koyo_dock=right`). Both are kept as they
  are: the bridge as given, `can_dock` as `true`, which Chrome's own docked tools are
  loaded with.
- **A shim in the tools' page** (`devtools/embedder.js`) takes what the frontend sends its
  embedder, at `DevToolsAPI.sendMessageToEmbedder`, which every `InspectorFrontendHost`
  method goes through, the × button's bound one too:
  - `setIsDocked`: the frontend has written the new side to its preferences just before;
    the shim tells the host the side, and calls back at once, which the frontend waits for
    (Chromium's embedder isn't asked).
  - `closeWindow`: tells the host.
  - `setPreference` for `emulation.show-device-mode`: tells the host, and passes it on.
- **The host puts the shim in** over the tools' page's own `DevTools` session (the one
  the browser tools use), each time the page has loaded at the frontend's address: the
  page's runtime on, a binding (`Runtime.addBinding`, `koyoDevToolsAsk`), and the shim,
  evaluated with the side and the binding's name. The shim tells the host by calling the
  binding; the host hears `Runtime.bindingCalled` for it in that session only. An error page
  in the frontend's place isn't at the frontend's address and gets none. The first try, a
  shim put in by the renderer as the page's script context was made, wasn't in the page on
  screen (why wasn't found: no hook of the renderer is needed now, and none is left).
- **Its first side.** The frontend boots after its page's load, with the side its
  preferences say: Chromium keeps them in the profile the tools' browser is of
  (`devtools.preferences`), which for `DevTools` panes, made with no profile of Koyo's, is
  the engine's own (`profiles/default`). The frontend writes the side there itself as it
  moves, which is where the app moves them too, so it mostly boots where Koyo has the tools.
  As it boots it docks itself there (`setIsDocked`), and shows its device toolbar if its
  preferences say so. None of that is the person asking: the shim tells the host of a side
  or of the device mode only once the frontend is up and the shim has set it as Koyo has
  it, at Koyo's side through its own setting (`currentDockState`) and its device toolbar
  off (`emulation.show-device-mode`), as responsive mode is off for a page until it's
  turned on (`docs/responsive.md`). In a frontend whose insides it can't find (a later
  Chromium's, say) the shim gives up after 20 s and passes asks on from then, the boot long
  over, so the tools' controls still work, with the placeholder showing
  (`koyoDevTools.unsettled`). Told of the boot, as it first was, Koyo followed the
  frontend: tools opened after Koyo's side was changed from Settings went back to the side
  the frontend remembered and wrote it over the setting (seen: set to the right, opened at
  the bottom, the setting the bottom again).
- **The host tells the app** (`HostEvent::DevTools`, protocol 12): `Dock { side }`
  (undocked is the side `undocked`), `Close`, `DeviceMode { on }`. The side the frontend
  asks for is kept with the page's open tools in the host, and is the side the shim puts
  the frontend at after a reload. The app does what its bar's buttons did
  (`BrowserView::dock_devtools`, `close_devtools`), and for device mode what its own toggle
  does (`toggle_responsive`); a side the tools are at already, or a mode the page is in
  already, changes nothing.
- **Undocked, the frontend opens a window** for its device toolbar
  (`device_mode_emulation_frame.html`). The host turns down every `devtools://` popup of a
  `DevTools` pane.
- **A side Koyo changes itself** (Settings, an agent's `settings_set`) is told to open
  tools the next time they open: open ones keep theirs until moved from their menu.

### 13.3 What stays Koyo's, and why [decided]

In Chrome the docked tools take the tab's whole area and leave a hole the page is put in
(`setInspectedPageBounds`): the frontend draws the divider, and the device toolbar round the
hole. In Koyo the tools are a pane of their own beside the page's, each a page with its own
picture and input. Following Chrome there means layering two pages'
pictures and routing every press to the right one: a far larger change, for a
divider and a toolbar. So:

- The frontend's page placeholder, which it shows beside itself when docked, is hidden
  (its split's `hideMain`): the tools fill their pane. Once the frontend is up the shim
  hides it on the frontend's own `AdvancedApp` (`Emulation.AdvancedApp.instance()`), and
  wraps that instance's `updateForDocked` to hide it again after each docked layout.
- The divider is Koyo's.
- The device toolbar is Koyo's responsive mode. The frontend's own emulation commands for
  it are answered and not passed on, so only one of the two sizes the page: the host
  answers `Emulation.setDeviceMetricsOverride` and `Emulation.clearDeviceMetricsOverride`
  in the frontend's own session with an empty result (`devtools/session.rs`). The same
  commands in a session under the frontend's (Lighthouse's) still go to the page, as do the
  frontend's other emulation (touch, user agent, sensors).
- Koyo's responsive mode doesn't tell the frontend: turned off from Koyo's device bar or
  ⌥⌘R, the frontend's toggle stays on, and its next click (off) changes nothing; a page in
  responsive mode when its tools open shows the toggle off. **[not built]**

### 13.4 Where it lives, and what's checked

- `koyo-rs/koyo-browser-ipc/src/devtools.rs`: `DevToolsDock`, `DevToolsAsk`.
- `koyo-rs/koyo-browser-host/src/devtools/embedder.rs` and `embedder.js`: the address, the
  shim, what puts it in and what its binding's calls become; `devtools/session.rs`: the
  emulation answered, and the side kept with the page's open tools (`Inspected::side`);
  `host/inspect.rs`: the shim put in after each load (`frontend_loaded`, from
  `host/panes.rs`'s `loading_changed`), its calls heard (`from_page`); `pane.rs`: the
  popups turned down.
- `koyo-rs/koyo/src/browser/dock.rs` (`devtools_asked`, `devtools_side`) and
  `host/events.rs`: what the app does.
- Unit tests: the address (nothing in it Chromium's check drops or changes), each side's
  name, what puts the shim in, its binding's calls heard and others not, what the shim's
  messages become, the wire names, the emulation answered, and a page following what its
  tools ask (`browser/view/tests.rs`). The shim was run under Node against a stand-in of
  the frontend with Chromium's embedder (a bound × button, settings that keep
  `setPreference` unbound, the frontend up after the shim), not in the frontend.
- On screen (2026-11-14, native): with the side in the address, the tools' pane showed
  Chromium's error page (`ERR_INVALID_URL`); without it and with no shim, the frontend's own
  toolbar and no bar of Koyo's, but the placeholder shown and the × doing nothing. As built,
  seen: the tools' own toolbar; docked at the bottom and undocked into a pane of their own
  and back from "⋮", and at every side and undocked by the frontend's own setting, which is
  what "⋮" sets; the × closing them; the
  device toggle turning responsive mode on and off; the placeholder hidden
  (`koyoDevTools` in the tools' page: the side, `ready`, `OnlySidebar`). Opened at each side
  set from Koyo's (an agent's `settings_set`) while the frontend remembered another: at
  Koyo's side, the setting left as it was. ⌥⌘I with the keys in the tools closed them. The
  divider dragged at each side with every page fitting its pane after (`docs/native-pages.md`
  6). Not tried: a link followed from inside the tools.
