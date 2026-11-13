# Koyo's Claude accounts: switching the login and watching weekly usage

Written 2026-11-11 on branch `main`. Drafted with Claude. Author of the vision: Felipe Schulz.

Koyo keeps several Claude Code logins and makes one of them the Mac's login, the way
claude-swap (`cswap`) does, so every Claude Code on the Mac follows: Koyo's chats, Koyo's
terminals, other terminals, the VS Code extension. A button at the left of the sidebar's foot,
or on the top rail in the Orca layout, opens a glass panel with each account's weekly usage and switches with a click. Auto-switch
moves to another account at a threshold the person sets, or when an account hits its limit.

The account logic is a port of claude-swap at commit `0fe22a8` (2026-11-11), not a redesign:
same rules, same numbers, same order of steps, and its tests come along. Where this page says
"as cswap", the cswap source at that commit is the spec. Usage polling follows Orca 1.4.256,
with two of cswap's rules on top (section 4). The study behind this page is in
`.superpowers/research/claude-accounts-study.md` (gitignored).

---

## 1. Decisions [decided]

- **Koyo's own store.** Koyo users never need cswap. Koyo keeps its accounts in its data folder
  and its own Keychain service, and never reads, writes or looks for cswap's.
- **One switch for all of it.** Settings → Accounts → **Manage Claude accounts**, on by default.
  Off, Koyo never touches the login, polls nothing and runs no auto-switch: the setting for a
  person who manages accounts with cswap or anything else. Two managers on one login fight over
  its refresh token, so the person picks one; Koyo doesn't check.
- **A port, not a redesign.** What Koyo needs from cswap is ported as it is (section 3). A rule
  that seems odd stays: each one is there for a failure cswap measured.
- **macOS only**, as Koyo is. Credentials live in the Keychain; the file fallbacks cswap keeps for
  Linux and Windows are not ported.

## 2. What it touches

- **Claude Code's login**, as cswap does: the Keychain item `Claude Code-credentials` (account
  `$USER`, else the OS user, else `claude-code-user`) and the `oauthAccount` block of the global
  config (`~/.claude.json`, or `~/.claude/.config.json` under Claude Code 2.1.299's rule). Paths
  resolve as cswap's `paths.py` does, `CLAUDE_CONFIG_DIR` and `CLAUDE_SECURESTORAGE_CONFIG_DIR`
  included.
- **Koyo's store**, cswap's layout under `<data dir>/claude-accounts/`: `sequence.json` (slots,
  identity, active slot), `configs/` (each slot's config), `cache/usage.json` (usage per account),
  `cache/subscription_plans.json` (each account's plan), `autoswitch_state.json`, the lock files,
  and `credentials/`, cswap's stash of refreshed tokens that couldn't be stored yet
  (`.unclaimed-<id>.enc`, base64, 0600, with a manifest): files on purpose, outside the
  Keychain, so a refresh the server accepted is never lost. Backed-up credentials go to the
  Keychain under service `Koyo Claude accounts`, accounts `account-<n>-<email>` and
  `account-<n>-<email>.prev`, written and read through `/usr/bin/security` exactly as cswap's
  `macos_keychain.py` does.
- **Claude Code's locks**, as cswap's `claude_locks.py`: `<config home>/.oauth_refresh.lock`, then
  `~/.claude.lock`, then `~/.claude.json.lock`; directories, stale after 60 s, 60 s and 10 s.

## 3. What is ported, and where

The logic goes in a new crate, `koyo-rs/koyo-accounts`, with no UI in it. The app wires it in
`koyo-rs/koyo/src/accounts/`. One Rust file per cswap module or less, each under the 500-line
limit, each opening with the cswap module and commit it ports.

| cswap | Ported | Left out |
|---|---|---|
| `paths.py`, `session.py` | config home, global config (2.1.299 rule), credentials path; the Keychain service names (`keychain_service_name`, `_keychain_services`) | XDG, legacy-root migration; the rest of `session.py` |
| `macos_keychain.py` | all of it | |
| `credentials.py` | active read with its verdicts (unavailable, degraded), backups, shared MCP fields, display cache, failure cooldown, the unclaimed-token stash (its `.enc` files) | file backend, `.enc` slot backups |
| `claude_locks.py` | all of it, takeover directory included | |
| `locking.py`, `fsutil.py` | Koyo's lock and atomic writer, to cswap's rules (0600 from creation, fsync, rename, write through symlinks) | Windows retries, junctions |
| `switcher.py` | add the current login, remove, switch (`_perform_switch`, `_classify_outgoing_credential` and its verdicts, rollback, foreign-successor rescue), the refresh gate (`consume_backup_grant`), usage collection, `accounts_snapshot` | aliases, swap/move, priority tiers, API-key and setup-token accounts, session-profile checks |
| `oauth.py` | refresh, profile, usage endpoint, result parsing, error kinds | the header probe |
| `usage_store.py`, `poll_policy.py` | the store, leases, last-good, backoff and Retry-After rules, the per-account poll plans | nothing; the app's cadence (section 4) runs on top for the active account and the open panel |
| `autoswitch.py`, `lines.py`, `settings.py` | the engine with the `weekly-first` strategy (section 5), its settings and their range | other strategies, `--once`, per-account thresholds, the weekly line and the per-model windows (`weekly_threshold`, `model`: every window's line is the threshold) |
| `subscription.py`, `pace.py` | plan label, pace marker | |
| `exceptions.py`, `models.py`, `json_output.py` | the errors, the switch result and snapshot types, the usage sentinels | JSON output |
| `transfer.py`, `menubar.py`, `tui/`, `migrations.py`, `update_check.py` | | all |

- **HTTP** goes through the system `curl`, as Koyo's other requests do, with headers passed on
  stdin (`--config -`), never in argv, so a token never shows in `ps`. The system `curl` trusts
  the Keychain's certificates, which covers TLS-inspecting proxies.
- **Keychain writes** go through `security -i`'s stdin, so a token stays out of a command's
  arguments; a credential too long for its 4,096-byte line (a login with MCP tokens, about 3.4 KB,
  twice that as hex) goes in the arguments as hex instead, as cswap and Claude Code write it:
  visible in `ps` to the person's own processes while `security` runs, rather than an item cut
  short (the owner's choice, after a real login didn't fit).
- **Tokens never reach a log.**
- **Running Claude Code picks up a switch** within its 30 s Keychain cache, or at once when a
  `.credentials.json` already exists and its mtime is bumped, as cswap does.

## 4. Usage polling [decided]

Orca's cadence, with cswap's two rules where they differ:

- **The active account:** every 15 min while Koyo is visible and focused; on focus when the last
  reading is 5 min old or more; on every switch; 1 s after Koyo starts.
- **Live numbers in between:** each chat reply's `rate_limit_event` (5 h, 7 d, per-model window,
  `utilization`, `resetsAt`) updates the active account's reading for free. The agent host passes
  them on; it reads only `rejected` today.
- **Other accounts:** one at a time, at most one request a minute, while the panel is open. While
  auto-switch is on they're also read in the background on cswap's candidate schedule (every
  5–10 min, exhausted accounts every 10 min), since the engine ranks them (cswap).
- **Failures:** back off from 30 s, doubling to 15 min. A 429 waits out its `Retry-After`, parsed
  as a bounded integer, plus cswap's 900 s margin when the ask is over 600 s, at most 4,500 s
  (cswap: retrying at the deadline was blocked again for an hour 20 times in 35).
- **Never a token refresh to read usage**, as Orca; renewal happens only through the refresh gate
  (section 3), as cswap.
- **What's shown:** the last good reading and its age; countdowns computed when drawn; a weekly
  window past its reset shows 0%; no 5 h window (Ultra) is no bar, `utilization: null` is "?",
  never 0%; per-model windows (Fable) from `limits[]`; error kinds named with their remedy
  (rate-limited until …, plan lapsed, certificate refused, sign in again).

## 5. Auto-switch [decided]

cswap's engine with its `weekly-first` strategy:

- **When:** the active account reaches the threshold on any window, or hits its limit.
- **Where to:** among accounts under the threshold, the one whose weekly window resets soonest
  ("the one that expires first"), most quota left breaking ties. When none is under the line, the
  one with the most quota left, as cswap.
- **Guards, as cswap:** 5 min cooldown; a landing needs a 10-point margin under every line; no
  bouncing straight back; the target's token is freshened through the refresh gate before it's
  activated; disabled and quarantined accounts are skipped; an expired token on an idle machine
  holds rather than fails over.
- **Settings → Accounts:** Auto-switch on or off (off by default, as cswap), and the threshold,
  50–99.9 %, default 90 %. The other numbers keep cswap's defaults and aren't shown.
- It runs while Koyo runs. A switch it makes is said in the panel and as a notice.

## 6. Where the person sees it

- **The button:** the 24 px spacer at the left of the Arc sidebar's foot
  (`koyo-rs/koyo/src/arc/foot.rs`, `bar`), so the space dots stay centred; in the Orca layout,
  on the top rail beside the sidebar's toggle (`koyo-rs/koyo/src/shell/rails.rs`). It shows the
  active account's weekly usage as a small ring, amber near auto-switch's threshold, red at it.
- **The panel:** a glass panel anchored to the button, opening away from it
  (`menu::Trigger::toggle_panel` with a `MenuBody`, as `chat/composer/context.rs`), and fitting
  what it holds as that changes. The accounts are grouped by provider, each group headed once
  by its provider's mark: a monochrome vector 14 pt high in the small print's grey, its left
  edge where the emails start, as far from the panel's edge over it as from the first email's
  letters under it (16 pt each; a hovered account's tint stays 4 pt clear of it), counted in
  the panel's height like every part. Anthropic's "A\\"
  heads the Claude logins, the only ones the accounts crate keeps; another provider's accounts
  would form a group of their own under its mark (`Provider` in `accounts/model.rs`, its mark
  in `accounts/mark.rs`). One block per account: email, plan badge, organisation, the
  active mark, the 7 d bar and each per-model bar with a tick where even use would have it,
  percentage, and reset countdown; then when the week resets and how old the reading is, or what
  keeps it from being read. A click switches. It goes as every floating window does
  (`koyo-rs/koyo/src/popup.rs`).
  Last in Anthropic's group, since it's Claude Code's: the Mac's login when it isn't one of
  Koyo's accounts yet, with Add in Settings, which opens Settings → Accounts, where it's added
  and the rest is set. Under the groups, auto-switch's state with what it last did.
- **Settings → Accounts:** a new page (`settings_view/accounts.rs`): Manage Claude accounts, the
  account list with Hold Out / Put Back (out of auto-switch) and Remove, Add Current Login with
  what the Mac's login is to Koyo (not one of its accounts yet, or which account it is),
  Auto-switch and its threshold.
- **App control:** `claude_accounts_list` (read) through `koyo-app-control`, for agents. No agent
  tool changes the accounts: switching, adding, removing, holding one out, auto-switch and
  Manage Claude accounts stay with the person.

## 7. Proof

- cswap's tests for the ported modules are translated to Rust beside the code, so each rule is
  checked against cswap's own cases.
- Tests can't reach the real Keychain, `~/.claude` or `~/.claude.json`: `security` runs through a
  trait with a fake in tests, every path comes from one root the tests set, and a guard refuses
  a write under the real roots.
- One real-Keychain test on a temporary keychain, run on its own.
- Real-surface QA: two real accounts; switch while a chat runs and see its next reply on the new
  account; auto-switch at a low threshold; a locked keychain reads as unavailable, never signed
  out; sleep and wake.

## 8. Order of work [proposed]

1. `koyo-accounts`: paths, Keychain, locks, atomic writer, store; their tests.
2. Usage: endpoint, parsing, store and polling; the button and panel showing the current login.
3. Accounts: add the current login, remove, switch with its rollback; the panel switches.
4. Refresh gate and usage for every account.
5. Auto-switch and Settings → Accounts.
6. App control, the notices, QA.
