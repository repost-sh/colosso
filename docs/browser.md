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
`script/dev` and the ad-hoc `script/bundle` are such builds, and stay so: an ad-hoc build
never claims the entitlement, since macOS won't start an app that claims a managed
entitlement without the provisioning profile granting it, and `script/bundle` stops if an
ad-hoc signature claims any entitlement at all.

### 9.4 Getting the entitlement [not done]

What the person who owns Koyo's Apple developer account does, once:

1. An Apple Developer Program membership as an organization. The entitlement is requested
   by its Account Holder, at
   <https://developer.apple.com/contact/request/macos-browsers-passkeys/>, for the App ID
   `dev.koyo.desktop`; Apple reviews the request.
2. Once granted: in Certificates, Identifiers & Profiles, the explicit App ID
   `dev.koyo.desktop` with the granted capability turned on; a Developer ID Application
   certificate in the keychain of the Mac that builds; and a Developer ID provisioning profile
   for that App ID, downloaded as a `.provisionprofile`.
3. The build, signed and checked:

   ```sh
   script/bundle --sign "Developer ID Application: <Organization> (<TEAMID>)" \
                 --profile ~/Downloads/Koyo.provisionprofile
   ```

   `script/bundle` checks the profile before building (it names the team, is for
   `<TEAMID>.dev.koyo.desktop` and carries the entitlement), writes the app's entitlements
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
