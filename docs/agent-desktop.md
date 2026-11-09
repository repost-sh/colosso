# Koyo agent desktops: computer use in a hidden account

Written 2026-11-09 on branch `schulzfel/agent-desktop`. Author of the vision: Felipe Schulz.
Drafted with Claude. Source of truth for agent desktops; update it when a decision changes.

Built from the proof of concept of 2026-11-08 and 09, whose notes stay out of the repository in
`.superpowers/agent-desktop-research/`: `DECISIONS.md` (the model), `FINDINGS.md` (every
experiment, measurement and dead end), `PRIOR-ART.md` (what each project it drew on taught) and
`poc/` (the sources). Section numbers there are quoted
as FINDINGS "…" by their headings.

Koyo's agents drive real Mac apps (click, drag, scroll, type, hover, pinch) on a desktop of their
own: an area of 1680 × 1050 points on one screen of a hidden macOS account, shown in a Koyo tab as
a browser page is, which the person watches and takes over. Nothing an agent does reaches the
person's pointer, keyboard focus, displays, windows, Spaces or files.

The PoC ran on macOS 26.4. This Mac now runs macOS 27.2 (26B123, 2026-11-09): what the build
relies on is checked again there before it's built on, and marked "27.2:" where it held or
moved (16 lists what's still to check).

Status marks: `[decided]`, `[proposed]` (designed here, confirmed by implementation),
`[not built]`.

---

## 1. What the person gets [decided]

- An **agent desktop** is a tab (or a split pane) of the kind `desktop`. It shows one agent's
  area live, with that agent's pointer drawn where it acts and a ring where it clicks. The
  person clicks, types, scrolls and drags in it as on their own screen, and the agent waits.
- Agents get it through Koyo's MCP server (10): `desktop_add` makes one in the agent's
  worktree, and the `desktop_*` tools send files from that worktree, open apps, look, act, wait
  for the screen to settle, run commands and take results back.
- Agents act through accessibility first: a snapshot of their apps' accessibility trees with a
  ref per element, and presses and values set on refs, which need neither the session's one
  pointer nor its one keyboard focus, so agents never wait for each other there. Points, the
  pointer and keys are the fallback for what accessibility can't reach (7.7).
- Up to four agents at a time, each in its own area of one agent screen (6). A fifth is refused
  until a second session exists (`[not built]`).
- One background picture on every area: the person's own wallpaper, followed as it changes
  (7.10).
- Set up once, with one administrator approval, from Settings → Agent desktop (5); removed from
  the same page.

## 2. What never reaches the person [decided]

Each line is a promise and the rule that keeps it; FINDINGS has the incident behind each rule.

| Never | Kept by |
| --- | --- |
| Their pointer moves or their keyboard focus changes | Agents act in another login session. Events are posted at `kCGSessionEventTap` of that session only, never at `kCGHIDEventTap`: from a background session the HID tap drives the person's pointer ("voxl … in a Koyo browser tab"). |
| A display appears, moves or becomes main | The agent screen is the virtual display Screen Sharing gives the agent session. No process ever calls `CGVirtualDisplay`: one made from a background session attached to the console and became the person's main display ("One session, several displays"). The host refuses to start on a display that isn't Screen Sharing's (vendor `aapl`). |
| A window, alert or prompt on their screen | Agent apps run in the agent session, whose windows, notifications and system prompts stay there ("PoC: separate agent session"). Koyo's own windows are the only ones the feature draws on the person's screen. |
| Their files are read or changed | The agent account has a primary group of its own, not `staff`, so `/Users/<person>` (`drwxr-x--- staff`) is closed to it; its home is closed to the person the same way ("The concept fixes" 1). Files go over only when an agent sends them, as copy-on-write clones (7.8). |
| Their Mac is reachable from the network | Screen Sharing is enabled for the agent account only, and a `pf` anchor drops ports 5900 and 3283 on every interface but `lo0` (5.2). |
| Commands run as them | Commands an agent runs (`desktop_run`) run as the agent account, in its work folder. Anything run against the agent account's processes is scoped to its user id (`pkill -u`, `launchctl … gui/<uid>`), never by name. |

### 2.1 Why one shared session [decided]

Measured in the PoC (PRIOR-ART "Learned the hard way", "Virtual machines"): one hidden session
costs ~2.7 GB idle (312 processes, mostly per-user services) and each agent added to it ~110 MB
plus its apps; a session per agent costs another ~2.7 GB each, and a macOS VM ~4 GB with no
memory balloon, one display, at most two running (the licence, enforced by
`VZErrorVirtualMachineLimitExceeded`), and a person clicking through Setup Assistant, two
permissions and a login item in every new guest. So: one account, one session, an area per
agent. What it costs: agents share one pointer and one keyboard (hence accessibility first, 7.7)
and can read each other's work folders (4).

## 3. Architecture [decided]

```
 person's session                                              agent session (account koyo-agents)
 ┌──────────────────────────────┐   RFB 003.889, 127.0.0.1:5900   ┌────────────────────────────────┐
 │ koyo-desk-holder (person)    │ ──────────────────────────────▶ │ screensharingd: the session,   │
 │  logs in, keeps it open,     │                                 │ its one virtual display        │
 │  resizes the agent screen    │                                 │                                │
 ├──────────────────────────────┤   link socket (CBOR frames)     │ Koyo Agent Desktop (host)      │
 │ Koyo app                     │ ◀────────────────────────────── │  areas, windows, views (H.264),│
 │  agent_desktop: link, panes, │                                 │  input, files, commands,       │
 │  decoder, tools, wallpaper   │                                 │  the account's look            │
 └──────────────┬───────────────┘                                 └────────────────────────────────┘
                │ helper socket
                ▼
 koyo-desk-helper (root, launchd daemon): setup and removal, Screen Sharing on and off, the firewall
 anchor, the host's install, ending a stuck session
```

Four programs, four new crates and a module of the app:

- `koyo-rs/koyo-desk-wire`: no UI, no Apple frameworks. The link protocol between the app and the
  host (8), the holder's control lines, the area layout (6), key names to key codes, and the
  helper's requests. Everything both sides must agree on, with its tests.
- `koyo-rs/koyo-desk-helper`: the root helper (5). One binary: `setup`, `remove` and `status` run
  under the person's administrator approval; `serve` is the daemon launchd keeps, which answers
  the app on a socket only the person's user id may use.
- `koyo-rs/koyo-desk-host`: the host inside the agent session (7), a `cdylib`
  (`libkoyo_desk_host.dylib`) loaded by a stable loader app, `Koyo Agent Desktop.app` (7.1).
- `koyo-rs/koyo-desk-holder`: the session holder (6): the RFB client and the daemon round it,
  run as `koyo desk-holder` from the app's own executable, as `koyo termd` is, and started
  detached through `koyo-daemon`, as the agent host is (`docs/chat.md` 4.1).
- `koyo-rs/koyo/src/agent_desktop/`: the app's side: the link listener, the desktop pane, the
  H.264 decoder, the drawn pointer, the person's input, visibility, the tools' executors, the
  wallpaper follower and Settings → Agent desktop.

Native code is Rust over the `objc2` framework crates the app already uses (0.3:
`objc2-screen-capture-kit`, `objc2-video-toolbox`, `objc2-core-media`, `objc2-core-video`,
`objc2-application-services` for accessibility, `objc2-core-graphics` for events), so the lints,
the 500-line rule and the tests apply to it as to the rest. The only C is the loader's dozen
lines (7.1).

## 4. The agent account [decided]

- Short name `koyo-agents`, full name "Koyo Agents", hidden (`IsHidden 1`), a standard user (never
  admin), primary group `koyo-agents` of its own, out of `staff` and every
  `com.apple.sharepoint.group.*`; home `/Users/koyo-agents`, owner `koyo-agents:koyo-agents`,
  mode 750. One account for all agents (not one per agent: a session costs ~2.7 GB, an area
  ~30-110 MB, FINDINGS "Four agents in ONE agent session").
- Its password is 32 random characters, handed to `sysadminctl -addUser … -password -` on a pty,
  never on a command line, and checked with `dscl . -authonly` the same way (Rocuronium's
  `provision-agent-account.sh` explains both). The helper keeps it in a root-only file (5.3) and
  hands it to the person's holder on each login; it never lands in the person's home.
- First login skipped: every `com.apple.SetupAssistant` `DidSee…` key and `LastSeen…` version
  pre-seeded, Siri and suggestions off, no screen saver and no idle lock (an idle lock would
  curtain the session), written as the account (`sudo -u`).
- Trimmed: the per-user launch agents in `trim-labels.txt` (Siri, Apple Intelligence, Biome,
  Spotlight's UI, widgets, Notification Center, media and photo analysis, iCloud, Home, Wallet…)
  disabled with `launchctl disable gui/<uid>/<label>`. Kept: Dock, Finder, WindowManager,
  ControlCenter, SystemUIServer, text input, spelling, icon services, `replayd`, `trustd`, fonts,
  the pasteboard. Set through the account's `user/<uid>` domain, which writes its own overrides
  file (`/var/db/com.apple.xpc.launchd/disabled.<uid>.plist`): `launchctl disable gui/<uid>/…`
  fails (`125: Domain does not support specified action`) while the account isn't logged in.
  Measured at a login on 27.2: the session had 230 processes instead of 303 and 4.3 GB of RSS
  instead of 5.6 GB, but `chronod`, `ContextStoreAgent`, `assistantd` and `suggestd` still ran:
  they're started on demand over XPC, come straight back when ended, and SIP refuses
  `bootout` for them. The two domains share the overrides file and each writes only its own
  view, so a `gui/<uid>` disable during a session drops the `user` entries: setup writes the list
  again each time it runs.
- Clean logins: `TALLogoutSavesState` and `LoginwindowLaunchesRelaunchApps` false, so nothing
  comes back after a forced logout ("The concept fixes" 8).
- The look, set by the host at each start (7.10): Dock auto-hidden with a 1000 s delay, menu bar
  hidden, no recent apps in the Dock.
- Work folders `~/Work.noindex/agent-<n>` (Spotlight skips `.noindex`, "The concept fixes" 3).
- `[not built]` Reduced motion and transparency (a configuration profile, which needs its own
  approval in System Settings); wiping the home from a template between runs.
- Accepted: agents in the one account can read each other's work folders (a folder picker lists
  them). Areas are a convention there, not a boundary.

## 5. Setup and removal: the helper [decided]

### 5.1 One approval

Settings → Agent desktop → Set up… explains what it will do (the list below) and asks for an
administrator's approval once, with the system's own dialog: the app runs `koyo-desk-helper setup`
as root through `osascript … with administrator privileges` (the one route that works for an
ad-hoc build; `SMAppService` daemons need a signed app, 5.6). `setup` is idempotent and does, in
order, stopping at the first failure with what it did so far:

1. Refuses a macOS older than 26.6.1, where `screensharingd` has a pre-authentication bypass
   exploited in the wild (CVE-2026-65400), unless `--accept-screensharing-risk` is given
   (development only; the page never passes it).
2. The `pf` anchor `com.apple/900.Koyo`:
   `block drop in quick on ! lo0 proto { tcp udp } from any to any port { 5900, 3283 }`,
   pf enabled with a reference of the helper's (`pfctl -E`, its token kept to release it).
3. The group `koyo-agents` (the first free group id from 600), the account (4), its preferences
   and trimmed launch agents.
4. The account in `com.apple.access_screensharing`, and no one else added. A person who already
   lets others in keeps them.
5. The shared folder (5.3) and the host's copy in it (7.1), and the account's launch agent for
   the host (7.1).
6. The daemon: the helper copied to `/Library/PrivilegedHelperTools/dev.koyo.agent-desktop.helper`
   (root, 0755) and `/Library/LaunchDaemons/dev.koyo.agent-desktop.helper.plist` (`KeepAlive`,
   `RunAtLoad`), bootstrapped into `system`.
7. Writes `state.json` (5.3): the person's user id (the only one the daemon serves), the
   account's uid, gid and password, the token, the macOS version it was set up on. It's first
   written the moment the account exists, so a setup stopped after that finds it its own.

Screen Sharing itself is not left on: the daemon turns it on just before a login and off once no
session needs it (5.4).

### 5.2 The firewall

pf's rules don't survive a restart, so the daemon applies the anchor at each load (`RunAtLoad`),
before it ever turns Screen Sharing on, and refuses to turn it on when the anchor isn't in place
(`pfctl -a com.apple/900.Koyo -sr`). The anchor sits under `com.apple`, which the stock
`/etc/pf.conf` loads; a Mac whose `pf.conf` doesn't is refused at setup.

### 5.3 What lives where

| Path | Owner, mode | What |
| --- | --- | --- |
| `/Library/Application Support/Koyo/Agent Desktop/` | root:wheel 0755 | The shared folder |
| `…/state.json` | root 0600 | 5.1 step 7, and the account's password |
| `…/host/Koyo Agent Desktop.app` | root:wheel, read-only to all | The host's loader the account runs (7.1) |
| `…/host/libkoyo_desk_host.dylib` | root:wheel, read-only to all | The host itself, beside the loader's bundle (7.1) |
| `…/link/` | person:koyo-agents 0750 | The app's `desk.sock` (0666 in a folder only the two reach) |
| `…/staging/` | root:koyo-agents 1770, ACL: the person may add and remove | Copy-on-write sends both ways (7.8) |
| `/var/run/dev.koyo.agent-desktop.helper.sock` | root 0666 | The daemon's socket; it answers only the person's uid (`getpeereid`) |
| `/Users/koyo-agents/Library/LaunchAgents/dev.koyo.agent-desktop.host.plist` | koyo-agents | Starts the host in the session (7.1) |

### 5.4 What the daemon does

At each load, before anything else, it turns Screen Sharing off (`launchctl bootout` and
`disable system/com.apple.screensharing`): launchd keeps a service enabled across a restart while
pf forgets its rules, and after the PoC's restart this Mac showed `"com.apple.screensharing" =>
enabled` with no anchor in place. Then it loads the anchor, and only then serves. Screen Sharing
is turned on only with the anchor loaded (`prepare-login` checks), and setup and removal both end
with it disabled, so no path leaves it enabled across a restart.

It also ends the account's session on its own (6.1): when the holder's lease connection drops,
and when the person is no longer the console user.

Requests are one JSON line each way, from the person's uid only:

- `status` → set up or not, the account, Screen Sharing on or off, the anchor in place, whether
  the account has a session (its `loginwindow`), the host installed and its version.
- `prepare-login` → anchor checked, Screen Sharing on (`launchctl enable` and `bootstrap
  system/com.apple.screensharing`), answers the account's name and password for the holder.
- `end-session` → `launchctl bootout gui/<uid>`, then after 5 s `pkill -9 -u <uid>`; Screen
  Sharing off once no session is left (`bootout` and `disable`).
- `install-host <bundle>` → copies a host bundle into `…/host/`. The bundle must be a Koyo Agent
  Desktop bundle the person could read; a signed build also checks it carries the helper's own
  team (`[not built]` until Koyo is signed).
- `restart-host` → `launchctl kickstart -k gui/<uid>/dev.koyo.agent-desktop.host`.
- `hold` → the holder's lease: a connection kept open for as long as it holds the session. Its
  end, however it ends (the holder ended, crashed, or killed at the person's logout), ends the
  session within seconds (6.1).

`[proposed]` once the holder has logged in, `prepare-login`'s `enable` is taken back
(`launchctl disable`) while the service runs, so the override is off at the next boot whatever
happens. Whether `screensharingd` keeps the held connection, and takes the next login, after
`disable` is checked in M1; if it doesn't, Screen Sharing stays enabled while a session lives,
and the daemon's load-time rule above still turns it off after any restart.

### 5.5 Removal

On macOS 26.4 root can't delete a user record (`eDSPermissionError`, FINDINGS "Second login for
agents"): it takes an administrator's own name and password (`dscl -u <admin> . -delete …`). So
Settings → Agent desktop → Remove… asks for them in a sheet of its own (the name filled in, the
password in a secure field), and sends both to the daemon, which is root already: no second
system dialog. The daemon checks the name is an administrator's (`dseditgroup -o checkmember -m
<name> admin`) and the password is theirs (`dscl . -authonly`, on a pty) before it touches
anything, uses the password only on `dscl`'s pty, and keeps it nowhere. Then, in order, stopping
at the first failure with what it did so far:

1. The session ended and every process of the account's uid gone. A survivor refuses the removal
   and is named: an account deleted under its running session once wedged the WindowServer all
   sessions share (Rocuronium's `AccountLifecycle.swift`).
2. The account out of the Screen Sharing list; Screen Sharing off.
3. The account's `Public` share point and its `com.apple.sharepoint.group.*`; the account and its
   home (`sysadminctl -deleteUser`, then `dscl -u` for the record); the group.
4. The anchor flushed and pf's reference released.
5. The shared folder, the helper's copy and its plist; last, the daemon boots itself out.

A daemon that can't be reached (a broken install) leaves `koyo-desk-helper remove` run as root
through the system's dialog, which does all but the record and says the command that finishes it.

### 5.6 Signed Koyo `[not built]`

With a Developer ID, the daemon becomes an `SMAppService` daemon inside `Koyo.app`
(`Contents/Library/LaunchDaemons`), approved in System Settings → Login Items instead of a
password, the host's copy is checked against the team, and the loader trick of 7.1 is no longer
needed (a Developer ID grant names the team, not a hash).

## 6. The session and the agent screen [decided]

- `koyo desk-holder` (the person's, a daemon beside the app) logs the account in through Apple's RFB
  003.889 on `127.0.0.1:5900`, as `poc/src/rfbpoc.m` does: security type 30 (Diffie-Hellman
  through Security's `SecDH…`, the credentials AES-128 encrypted under the MD5 of the shared
  secret), ClientInit `0xC1`, session select command 2 ("connect to virtual display"), then
  `SetDisplayConfiguration` (`0x1d`) with the dynamic flag. It never asks for pixels after the
  first update: the session's own picture over RFB is black ("PoC: separate agent session").
- It holds the connection open as long as the agent session should live. A dropped connection
  curtains the session for good (Rocuronium, measured): a holder that finds the account with a
  session and no connection of its own asks the daemon to end it, then logs in afresh.
- It outlives a crash or a hot reload of the app, as `koyo termd` does terminals, so the agents'
  apps keep running through those. A normal quit of Koyo ends the session (6.1); so does Koyo
  being gone for 2 minutes, and no area for 10. Its folder is `koyo-daemon`'s,
  `<data>/desk-holder/` (socket 0600, lock, log); the app sends it JSON lines (`size`, `status`,
  `end`). Settings → Agent desktop → End session ends it.
- The agent screen is the session's one virtual display, 60 Hz, at 1x. Areas are 1680 × 1050
  points (16:10) at fixed places that never move: slot 0 at 0,0, slot 1 at 1680,0, slot 2 at
  0,1050, slot 3 at 1680,1050. The screen grows 1680 × 1050 → 3360 × 1050 → 3360 × 2100 as slots
  fill, and shrinks when the last slot of a row or column frees. 3840 × 2160 is the most Screen
  Sharing gives one display ("Agent screen ceiling"), so four areas is the most per session.
- `[not built]` A second session for agents five to eight; Retina areas (1680 × 1050 at 2x fits
  one area only).

### 6.1 Never in the way of the person's restart, shutdown or logout [proposed, tested in M1]

An incident (2026-11-09): with the PoC's account logged in, the person's Restart showed "Unsaved
changes may be lost when restarting. To avoid losing unsaved changes, log out all users before
restarting. Enter an administrator name and password to restart without saving changes." That is
loginwindow evaluating the `system.restart` right (`system.shutdown` for Shut Down), "checked if
the foreground console user tries to restart the system while other users are logged in via
fast-user switching" (`/System/Library/Security/authorization.plist`); the agent session counts
as such a user. The person must never see it because of Koyo. Changing the right is out: it would
weaken the policy for every user of the Mac. So the session has to be gone before loginwindow
evaluates it, and the signal that a restart is coming has to come before that. In layers, the
earliest first:

1. **loginwindow's own notice.** The holder listens for the distributed notifications loginwindow
   posts as the person asks (`com.apple.restartInitiated`, `com.apple.shutdownInitiated`,
   `com.apple.logoutInitiated`, its binary's strings) and for
   `NSWorkspaceWillPowerOffNotification`, asks the daemon to end the session at once, stops
   holding, and logs in again only after `com.apple.logoutCancelled` and an agent's next need.
2. **Koyo's quit.** Every quit of the app (the person's ⌘Q, or loginwindow quitting it for a
   restart) ends the session first: the app holds its quit (`applicationShouldTerminate` →
   later) until the holder says the session is gone, at most 5 s.
3. **The lease.** The daemon ends the session within seconds when the holder's lease connection
   drops (5.4), as it does when launchd kills the person's processes at their logout, so the login
   window never finds another user logged in.
4. **The console.** The daemon ends the session when the person's session is no longer the one on
   the console. It reads the kernel's console users (`IOConsoleUsers`, each session's
   `kCGSSessionOnConsoleKey`), never `/dev/console`'s owner nor `State:/Users/ConsoleUser`'s
   `Name`/`UID`: an agent login takes both over while the person stays at the console (27.2,
   2026-11-09, as Rocuronium measured), and a first build that read `/dev/console` ended its own
   session two seconds after logging in.
5. **Idle.** The holder ends a session nobody uses (6).

How a session ends matters as much as when. Killed (`launchctl bootout gui/<uid>`), it leaves
its traces behind: on 27.2 the kernel's console users were right at once, but `who` and `last`
still listed the account "still logged in", `State:/Users/ConsoleUser` still named it, and it
still owned `/dev/console` (measured 2026-11-09). So the daemon ends a session through its own
loginwindow first, as a logout does (`«event aevtrlgo»` sent from inside the session, as the
account, and only to a session the kernel lists off the console, so it can never reach the
person's), and boots it out only if it's still there 3 s later. `[proposed]` which of those
traces loginwindow's restart check reads is what the restart test below settles.

**The freeze of 2026-11-09, 18:51.** The third agent login within three minutes hung the whole
Mac about three seconds after it logged in (18:51:27): every process's log, the kernel's
included, stops at 18:51:30.4-.7; there was no panic, and the person forced it off
(`ResetCounter`: `btn_rst, btn_seq_reset`). The two sessions before it had been ended by
`launchctl bootout` while the holder's Screen Sharing connection still held their virtual
display, the last a minute before. In its last seconds the new session was starting a dozen
widget extensions (`chronod`, untrimmed, above), `ContextStoreAgent` was failing keystore calls,
`ospredictiond` logged 9,540 lines, and the session's `ScreensharingAgent` kept retrying a screen
capture it isn't allowed (its picture over RFB is black). A second hang (19:10, after a 60 s gap had
been added, and with no new login) came 15 s after a session was ended by `bootout`. The two
logs show one pattern: a system daemon that follows the console user went into a tight XPC
reconnect loop against a per-user service the moment the agent session changed. Before the
second hang `audiomxd` logged 229,000 lines in 15 s, 25,500 rounds of "TargetUserSession
koyo-agents", "failed to do a bootstrap look-up: No such process", "audioaccessoryd died",
"Syncing with AudioAccessoryServices as they just recovered", from 19:10:04.65, as the session
was killed. Before the first, `ospredictiond` reconnected to `com.apple.MobileTimer.alarmserver`
731 times in one second, three seconds after a new agent login. Then the whole Mac stopped.
`[open]` The likely root: on 27.2 an agent's Screen Sharing session takes over the console user
record (6.1, item 4), so daemons that route to "the console user" (audio, alarms, Bluetooth)
switch to the agent's session, and a killed or new agent session leaves them chasing services
that aren't there. Until it's understood: no agent login without the person there to reset the Mac; a login waits until the
kernel lists no session of the account and 60 s have passed since the last one ended (the
helper's rule: it answers `notYet` to `prepareLogin` until then, whoever asks, and writes the
trim again before each login); and a
session is ended by its own logout while its connection is still held, the connection closed
only after it, never booted out while its display lives unless the logout fails. `[open]` which
of these, if any, is the cure; Rocuronium saw the shared WindowServer wedge after an account was
deleted under a running session.

What M1 proves on this Mac, with timestamps from the holder's and the daemon's logs: Apple menu →
Restart with an agent session up shows no dialog and restarts; the same for Log Out and Shut
Down; and how long the session took to end against when loginwindow's notice came. If layer 1
comes too late on 27.2, this section changes before anything is built on it.

## 7. The host [decided]

### 7.1 How it ships and starts

- `Koyo.app/Contents/Helpers/Koyo Agent Desktop.app`: bundle id `dev.koyo.agent-desktop.host`,
  `LSUIElement`, whose executable is a few dozen lines of C that `dlopen`s
  `libkoyo_desk_host.dylib` from the folder the bundle is in and calls `koyo_desk_host_main`.
- The loader never changes, so the privacy grants given to it (Accessibility, Screen Recording)
  survive every new build of the host, whose code is the library (`poc/src/labhost.c`, the PoC's
  `KoyoLab.app`). An ad-hoc signature's grant is tied to the binary's hash, and the hash covers
  the bundle's seal: so the library lives beside the bundle, never in it.
- The grants go with the loader wherever a copy of it runs, so it loads nothing but the library
  in its own folder, and only when root owns that library and nobody else can write to it or its
  folder: the shared folder's installed copy. A process of the person's can't run the loader
  with a library of its own, and so can't borrow its grants. The host itself refuses to start as
  anyone but the agent account, or on a display that isn't Screen Sharing's, except to ask for
  the grants (`--ask`).
- The account can't read the person's home, nor so a `Koyo.app` in `~/Applications` or a
  checkout, so the helper copies the host into the shared folder (5.3), root-owned, and the app
  asks for `install-host` when its own copy is newer.
- The account's launch agent runs the copy's executable directly (`KeepAlive`,
  `LimitLoadToSessionType Aqua`), with `--link /Library/Application Support/Koyo/Agent
  Desktop/link/desk.sock`.
- Privacy: once, Settings → Agent desktop → Allow… runs the copy in the person's session with
  `--ask` only (it asks for Accessibility and Screen Recording and quits; it never starts its work
  there), and the person turns both on in System Settings. Grants made in the person's session
  hold in the agent session on macOS 26.4 ("PoC: separate agent session"). The page shows whether
  the host reports both granted.
- `[proposed]` macOS 15 and later re-ask a screen recorder every month; in the agent session nobody
  sees that question. What happens when it's due is watched for; Apple's managed entitlement for
  remote-desktop apps (`com.apple.developer.persistent-content-capture`) is the fix once signed.

### 7.2 Clean start

At start the host refuses unless its display is Screen Sharing's, sets the account's look (7.10),
quits the apps a previous session left (normally, then forced after 2 s; Finder stays), removes
their saved state, and records every window present as nobody's.

### 7.3 Areas and windows

- `AddArea { id, slot, owner }` from the app makes an area at its slot, creates its work folder,
  and keeps the owner (the app's opaque name for the agent and its worktree) so the app can give
  it back after a restart (9.6).
- A window belongs to an area when the app it belongs to was opened by that area's agent
  (`desktop_open`). A window of no agent's app first seen within 10 s of an agent's action is
  that agent's (system dialogs: CoreServicesUIAgent, the open and save panel service); windows
  present at start are nobody's.
- Kept inside: accessibility observers on each owned app (window created, moved, resized) put a
  window back when it leaves its area, sizes clamped to the area; a sweep every second does the
  same for every owned window (an unsandboxed app's own `NSOpenPanel` opens centred on the whole
  screen and sends nothing).
- Full screen: a streamed window has no traffic lights (macOS 26 shows the capture badge, whose
  menu offers Full Screen and tiling). A window that goes into native full screen is taken out
  after its animation (`AXFullScreen` false, else ⌃⌘F) and fills its area instead; a green-button
  click on an unstreamed window fills the area; tiling from the badge is a move, put back.
- `RemoveArea` closes the area's windows, quits the apps its agent opened, frees the slot.

### 7.4 Views

- One ScreenCaptureKit stream per area: a display filter including the area's windows and the
  desktop picture's windows (macOS 26: the Dock's at the desktop level − 1, the wallpaper
  agent's at − 2, an ownerless one at − 3; Finder's icons at + 20 and widgets at + 22 stay out;
  27.2, the person's session: the same levels, − 1 now WindowManager's, and a WindowServer window
  at + 21 that stays out; chosen by level, never by owner),
  `sourceRect` the area, 1680 × 1050, 60 fps at most, no cursor (the app draws the agent's),
  NV12 in full range with the BT.601 matrix, the format and matrix GPUI's renderer draws
  (`vendor/gpui-fast-apple/src/shaders.metal`), so the app's decoder hands GPUI its buffers as
  they are (9.1). The
  filter is updated when the area's window set changes (checked each second).
- Zero copy: the capture's IOSurface-backed NV12 buffer goes straight to VideoToolbox, never
  through the CPU (no BGRA, no `CGImage`): the PoC's four views went from 78-87% of a core with
  JPEG to 9-12% with this (moonshine, PRIOR-ART "Streaming"). Frames whose status isn't
  `Complete` are dropped.
- Encoded in hardware: VideoToolbox H.264, low-latency rate control, real time, no frame
  reordering, Constrained High, ~24 Mbps average, quality 0.9, frame QP never above 24, key
  frame at least every 10 s. At most three frames in flight: a new capture is dropped before the
  encoder, never after, so the P-frame chain stays whole (moonshine). No damage, no encode: a
  frame whose damage (7.11) misses the area isn't encoded. Paint-over: 400 ms after the last
  change the last frame goes again as a forced key frame, so a still view is sharp (voxl).
- Parameter sets go with each key frame; the app's decoder starts at a key frame and asks for one
  (`WantKeyFrame`) when it starts or loses one.
- Expected (PoC, six areas visible): 58-65 fps each, 35-75 kB/s each while still, host 2-19% of a
  core.

### 7.5 Visibility

The app reports which areas can be seen (`Visible { id, visible }`). A hidden area's capture is
stopped; shown again, it restarts with a key frame. Six areas hidden took WindowServer from 70% to
35% of a core. An agent waiting on a hidden area (7.11) has its capture run, unencoded, for as
long as it waits.

### 7.6 The person's input

`Input` records from the app, in area points, already clamped to the area, are posted as real
events at `kCGSessionEventTap` of the agent session with a `CombinedSessionState` source: moves,
buttons with click counts, drags, scrolls (continuous, with phases), keys with their key code,
flags and character, and pinch. A green-button press fills the area instead (7.3). While the
person has given input in the last 2 s, agents' actions wait.

### 7.7 Agents' actions

Accessibility first (vercel-labs/agent-desktop, PRIOR-ART "Acting on apps"); the pointer and the
keyboard for the rest.

- **Snapshot.** The accessibility trees of the windows in the agent's area (`AXUIElement`, the
  apps the area's windows belong to): role, subrole, title, value, description, enabled,
  focused, frame in area points, and the actions each element offers. Interactive elements get a
  ref (`e12`), valid until the area's next snapshot; `interactive` leaves out the rest, as
  `browser_snapshot` does. Depth and count are capped (a web view's tree can be huge) and the
  answer says when it was cut.
- **On a ref**, nothing of the session's shared input is used, so agents act at once, also in
  different windows of one app: press (`AXPress`, else `AXConfirm`, `AXPick`, `AXOpen`), set a
  value (`AXValue`; a text field gets its text without keyboard focus), raise its window
  (`AXRaise`), show its menu (`AXShowMenu`), scroll it into view (`AXScrollToVisible`). The
  agent's pointer is drawn at the element's centre, so the person sees what it acted on. An
  element that offers none of these is acted on at its centre by the pointer, below.
- **By points and keys**, for what accessibility can't reach (canvases, web content without a
  tree, hover, drags, gestures, shortcuts): the session has one pointer and one keyboard. Agents
  act on them in turns (one lock), and wait while the person is active. An action posts session
  events: the pointer glides to the point in eight steps, then the click, drag (held 700 ms
  before moving, for drag and drop), scroll or keys.
- Typing and keys are refused when keyboard focus is outside the agent's area: the focused app's
  focused window, or with a menu open (no focused app) the frontmost app's windows ("keyboard
  focus is in Koyo ("Koyo") outside agent 1's area; click into your window first"). Typing into a
  ref sets its value instead and needs no focus.
- Every point an agent acts on is sent as its pointer (`Pointer { id, at, pressed, click }`), so
  the app draws it.
- Never the session's own end: keys that log out, restart, shut down, lock or sleep (⇧⌘Q, ⌥⇧⌘Q,
  ⌃⌘Q, ⌃⌥⌘⏏ and the like) are refused, and so are presses on the Apple menu's Restart, Shut Down,
  Log Out and Lock Screen (Rocuronium's hazard rail: every app's menu bar carries the Apple
  menu).
- `[not built]` Events posted to an app's process (`CGEventPostToPid` with window stamps,
  FINDINGS "The input recipe"), which let agents in different apps type at once.

### 7.8 Files

Copy-on-write, with no root in the loop:

- Send: the app checks the path is inside the calling agent's worktree, `clonefile(2)`s it whole
  into `staging/<uuid>/` (one call for a tree: 54k files in 1.6 s), opens read access on the
  clone only (`chmod -R a+rX`, 0.84 s; the originals keep their modes), and asks the host to
  `clonefile` it into its work folder (1.1 s), owned by the account. The app removes the staging
  copy once the host answers. 5.2 GB sent costs ~0 disk until changed.
- Fetch: the host clones from its work folder into staging (`a+rX`), the app clones it into the
  calling agent's worktree, owned by the person, and the host removes its staging copy.
- Another volume (no clones): copied instead, said in the answer.
- Considered and left: the root helper cloning across accounts in one step. Root can read
  anything, so it would need to check, file by file, that the person could read each file it
  clones; the staging route has the kernel check it.

### 7.9 Commands

`Run { request, area, command, cwd, timeout }`: `zsh -lc` as the account in the area's work
folder (a `cwd` inside it), stdin `/dev/null`, stdout and stderr merged and streamed back, in a
process group of its own (`posix_spawn` with `SETPGROUP`), killed (`SIGTERM`, then `SIGKILL`
after 5 s) on timeout, when the request is cancelled, and when the app's link drops: an orphaned
build once raced its successor ("The concept fixes" 7).

### 7.10 The account's look and the wallpaper

- Dock and menu bar hidden for the account, as 4 says.
- The desktop picture is one 1680 × 1050 tile repeated over the whole agent screen, areas on tile
  boundaries, set with `NSImageScaleNone`, rebuilt when the screen grows or shrinks, under a new
  file name each time (macOS keeps the picture of a path it already showed), in
  `~/Library/Application Support/Koyo Agent Desktop/`.
- The tile is the person's wallpaper, sent by the app (`Wallpaper { png }`): the app reads the
  picture of the person's main screen (`desktopImageURLForScreen`, checked every 2 s), crops its
  middle 16:10 and scales it to 1680 × 1050. A wallpaper with no single file (dynamic, extension)
  is captured from the person's desktop picture windows only when Koyo already holds Screen
  Recording (never asked for this), and fingerprinted every 60 s to follow time-of-day changes;
  else its file's first image, else Koyo's gradient.

### 7.11 Waiting for the screen

An agent waits instead of sleeping and guessing (voxl's `/wait`): until its area settles (no
damage for `quietMs`, 400 by default), or until a region of it changes, at most `timeoutMs`. The
damage is ScreenCaptureKit's dirty rects for each frame, checked against a 16-point grid sampled
from the frame's luma plane on the host (6.6k samples a frame), because the PoC saw filtered
captures report 55-65% of an area dirty with nothing changing (FINDINGS "Frame rate of the four
views"). The answer says whether it was met, how long it took, and the rect that changed.

## 8. The link [decided]

- The app listens on `link/desk.sock`; the host connects (and retries every half second while the
  app isn't running). The app accepts only the account's uid (`getpeereid`); the host only the
  person's uid from `state.json`.
- Frames: a 4-byte little-endian length, then CBOR (`ciborium`, byte strings as `serde_bytes`), at
  most 16 MB; bigger transfers are chunked. As the terminals' daemon does (`docs/terminals.md`
  5.4).
- `Hello { protocol, version }` both ways; a mismatch closes the link and the app offers to
  install its host (`install-host`, `restart-host`).
- Every request carries a `u32` request id, answered with the same id, so one agent has several
  in flight (a screenshot answered during a 20 s build, FINDINGS "The concept fixes" 7).
- App → host: `AddArea`, `RemoveArea`, `Visible`, `WantKeyFrame`, `Input`, and requests: `Act`
  (click, move, drag, scroll, type, key, by point or by ref), `Snapshot`, `Wait`, `Open`,
  `Screenshot`, `CloneIn`, `CloneOut`, `StagingDone`, `Run`; `Cancel`, `Wallpaper`.
- Host → app: `Hello`, `Screen { width, height }`, `Areas` (what it keeps, after a hello),
  `Video { area, key, pts, parameterSets?, data }`, `Pointer`, `Answer { request, result }`,
  `Output { request, data }`, `Log`, `Stats { area, fps, bytes, dropped, … }` every 5 s.

## 9. The agent desktop in Koyo [decided]

### 9.1 The pane

- Kind `desktop`, in `tab_list` as `kind: "desktop"` with `desktop` (the area id) and `owner`.
  Its tab reads "Desktop · <agent>" (the caller's name: the terminal's title, the chat's title, an
  external client's name), with a display icon.
- The view: the area's video decoded by VideoToolbox into IOSurface-backed `CVPixelBuffer`s in
  `420YpCbCr8BiPlanarFullRange`, the one YUV format GPUI's renderer draws (any other asserts,
  `vendor/gpui-fast-apple/src/metal_renderer.rs`), drawn with GPUI's `surface` element, scaled to
  fit the pane (letterboxed on the pane's background), never larger than 1:1 in points. Until the
  first frame: the agent's name and "Starting…".
- The agent's pointer: the arrow drawn over the view where the host last reported it, travelling
  between reports as `browser/pointer.rs`'s does for pages, with the ring on a click; it fades 6 s
  after the agent's last act.
- A strip along the top while an agent holds it, as a page's glow: "<agent> is working · Take
  control"; while the person holds it, "You have control · Give back".

### 9.2 Taking over

- The person's pointer and keys in the pane go to the area (positions mapped through the scale
  and clamped to the area). Koyo's own shortcuts (⌘T, ⌘W, ⌘1…⌘9, ⌘B, ⌘, and the app's menu
  equivalents) stay Koyo's; every other key goes to the agent desktop while the pane has the
  keyboard.
- As pages (`docs/app-control.md` 5, Browser): input in the pane holds the desktop for the person
  until 5 s after their last act; Take control holds it until Give back. Meanwhile an agent's act
  fails `person_has_control`, its message saying which.

### 9.3 Visibility

A desktop pane is visible when its view was drawn in the window's last frame (as a page is:
its element's keyed state, `browser/frames.rs`) and the window is open, not minimised and not
fully covered (`NSWindow.occlusionState`, which nothing in the app follows yet: added here). The
app tells the host on every change, within one frame; nothing is decoded for a hidden pane.

### 9.4 Opening one

- Agents: `desktop_add` (10). Out of sight unless the person's setting or the call's `select`
  says otherwise, as every agent's tab (`docs/app-control.md` 5).
- The person: ⌘T's launcher lists "Agent desktop" once set up, which opens one of their own
  (owner "you"); useful to try an app by hand in the agent account.

### 9.5 Closing one

Closing the tab (or `desktop_remove`) removes the area: its windows close, its agent's apps quit,
its slot frees and the agent screen shrinks when it can. Its work folder stays until the account
is reset. ⌘⇧T doesn't bring a desktop back.

### 9.6 Across a restart of Koyo

The holder and the host outlive the app (6). When the link comes back, the host lists its areas
with their owners and the app gives each its tab again, in the owner's worktree, out of sight, as
terminals come back from their daemon. An area whose worktree is gone is removed.

## 10. Agent tools [decided; schemas proposed]

On Koyo's MCP server beside the browser's, under `docs/app-control.md`'s conventions. A new scope,
`desktop`: agents in Koyo's terminals and chats have it (`Scope::AGENT`); an external client has
it once the person switches it on for that client in Settings → MCP server. Absent `desktop`
means the calling agent's latest desktop. An agent acts only on desktops it added; another's is
`control_required`.

```
desktop_add    [desktop] C { worktree?, select?: bool } → { desktop, pane, area: { x, y, width,
                 height }, workFolder }; `invalid_input` when the agent desktop isn't set up,
                 `busy` (retryable) when four areas are in use
desktop_list   [read] R {} → { screen: { width, height }, desktops: [{ desktop, pane, owner,
                 worktree, area, visible, controller? }], setUp, sessionRunning }
desktop_remove [desktop] I { desktop? }
desktop_screenshot [read] R { desktop?, format?: "png"|"jpeg" } → image block of the area at
                 1680 × 1050, with the agent's pointer drawn in; { width, height, pointer }
desktop_open   [desktop] C { desktop?, path, args?: [string], env?: {string: string}, newInstance?:
                 bool } → { app, pid, windows }; `path` an app or a file, absolute or in the work
                 folder; its windows placed in the area within 15 s
desktop_snapshot [read] R { desktop?, interactive?: bool, app?: string } → { tree: text with
                 `[ref=eN]` markers, frames in area points, truncated }; refs valid until the
                 area's next snapshot
desktop_wait   [read] R { desktop?, for?: "settled"|"change" (default settled), region?: { x, y,
                 width, height }, quietMs? (default 400), timeoutMs? (default 10000, at most
                 60000) } → { met, elapsedMs, changed?: { x, y, width, height } }
desktop_click  [desktop] C { desktop?, ref? | x?, y?, button?: "left"|"right", count?: 1..3 }
                 a ref is pressed through accessibility; a point is clicked with the pointer
desktop_move   [desktop] C { desktop?, x, y }
desktop_drag   [desktop] C { desktop?, x, y, toX, toY }
desktop_scroll [desktop] C { desktop?, ref? | x?, y?, dx?, dy? } a ref is scrolled into view
desktop_type   [desktop] C { desktop?, ref?, text, clear?: bool } with a ref its value is set (no
                 focus needed); without, keys are typed where the keyboard focus is
desktop_key    [desktop] C { desktop?, key: "cmd+s" | "return" | "shift+cmd+[" … }
               every act answers { desktop, by: "accessibility"|"pointer"|"keys", pointer? }
desktop_send   [desktop] C { desktop?, path (in the caller's worktree), to?: path in the work folder }
                 → { desktop, path, files, bytes, elapsedMs, cloned: bool }
desktop_fetch  [desktop] C { desktop?, path (in the work folder), to?: path in the caller's
                 worktree, replace?: bool } → { desktop, path, files, bytes, elapsedMs, cloned }
desktop_run    [desktop] C { desktop?, command, cwd?, timeoutMs? (default 600000, at most
                 3600000) } → { exitCode, output (the last 64 kB), truncated, elapsedMs }
```

Points are the area's, top left 0,0, 1680 × 1050. The PoC's `desk` verbs map as add, list, send,
fetch, open, shot → `desktop_screenshot`, click, move, drag, scroll, type, key, run; snapshot and
wait are new. The server's instructions and the `koyo_guide` prompt say: add a desktop, send what
it needs, open the app, snapshot it and act on refs, fall back to a screenshot and points for
what the snapshot doesn't show, wait for the screen to settle after an act instead of sleeping,
and type by keys only after clicking into its window.

## 11. Settings → Agent desktop [decided]

A page with the state and one action at a time:

- Not set up: what setting up does (5.1, in plain words), the macOS check, Set up…
- Set up: the account; Screen Sharing (off, on for a session); the firewall anchor; the host
  (installed version, Accessibility and Screen Recording as it reports them, Allow…); the session
  (running since, memory, areas in use); End session; Remove…
- No setting reachable by `settings_set`: an agent can't set up, remove or widen this.

## 12. Implementation plan

Each milestone ends with its checks green (`script/check`) and a real-surface run on this Mac,
measured beside FINDINGS, its numbers written into 13.

**M1. The account and the session.** `koyo-desk-wire` (layout, holder lines, helper requests),
`koyo-desk-helper` (`setup`, `remove`, `status`, `serve`), `koyo-desk-holder` (login, hold,
resize, the dead-session rule), Settings → Agent desktop with Set up…, End session, Remove….
Checks: the account as 4 says (`id`, home mode, `ls /Users/<person>` refused as the account and
the reverse), Screen Sharing for the account only, the anchor loaded; a login (PoC: ~4 s first,
<0.4 s again) and the person's display list unchanged over the run; the screen resized 1680 ×
1050 → 3360 × 1050 → 3360 × 2100 and back; Screen Sharing off when the session ends; removal
leaves nothing but what 5.5 says. Then the restart rule (6.1), with the person: an agent session
up, Apple menu → Restart shows no dialog and the Mac restarts; after it, `launchctl
print-disabled system` says Screen Sharing is disabled and the anchor is back before anything
listens on 5900; Log Out and Shut Down the same. And on 27.2 again: the RFB login, session select
and resize, the 3840 × 2160 ceiling (3840 × 2160 taken, 4096 × 2160 refused).

**M2. The host.** Re-checked on 27.2 first: privacy grants made in the person's session holding
in the agent session, the desktop picture's window levels there, and the capture badge in place
of a streamed window's traffic lights. Then `koyo-desk-host` and its loader, installed by the helper, started by the
account's launch agent: clean start, the look, areas, zero-copy views with visibility and no
encode without damage, the damage map and waits, accessibility snapshots and acts on refs, the
person's input, the pointer and keys in turns, the window rules, files in and out, commands, the
tiled wallpaper. Checked through an ignored integration test that plays the app's side of the
link: fps and bytes per view still and moving, host CPU, key frames on show, a wait that settles
after typing and one that times out on a still screen, TextEdit's text set by ref while another
area types by keys, input landing in TextEdit, a stray dialog claimed, native full screen undone,
a 54k-file tree cloned in and a folder out, a command's tree killed on cancel.

**M3. The desktop pane.** The link in the app, the pane kind (tab, `tab_list`, launcher, close,
restart), decoding and drawing, the agent's pointer, takeover and the person's hold, visibility,
and the first tools: `desktop_add`, `desktop_list`, `desktop_open`, `desktop_screenshot`,
`desktop_snapshot`, `desktop_wait`, `desktop_remove`. Checks: in a running Koyo, a desktop opened by the person and one by an agent;
TextEdit opened by the tool, typed in by the person; the pane hidden and shown (host capture stops
and resumes with a key frame); Koyo restarted with the desktop coming back; Koyo's decoding CPU
against the PoC viewer's 4-5% for four views.

**M4. Agents at work.** The acting tools (by ref first, by point), `desktop_send`,
`desktop_fetch`, `desktop_run`, the person's wallpaper followed, scopes and holds. Checks: from a
real MCP client (`koyo mcp`), an agent sends a repo, runs a build, opens the result, drives it by
snapshot refs and, where those don't reach, by screenshots and points, types, drags and scrolls,
waits instead of sleeping, and fetches a file back; two agents act by ref at once; the person
takes over and hands back mid-task; the wallpaper changed in System Settings reaches the agent
screen.

**M5. Four agents, and the record.** Four desktops at once, all visible and all hidden (fps,
bytes, host and WindowServer CPU, the session's memory against FINDINGS "Hybrid agent host PoC"),
pointer turns measured with real agents (an open question of the PoC), the trim measured, the
tests and docs finished ("As implemented"), and everything outside the checkout listed and
removed.

## 13. Measurements [filled in per milestone]

| What | PoC (FINDINGS) | Built |
| --- | --- | --- |
| First login of the account | ~4 s | |
| Login again | <0.4 s | |
| Session memory, one agent, idle | 2.7 GB | |
| Each added area | 30-110 MB | |
| View rate, six visible | 58-65 fps | |
| View bytes, still | 35-75 kB/s | |
| Host CPU | 2-19% of a core | |
| WindowServer, six visible → none visible | 70% → 35% | |
| App decoding, four views | 4-5% (AVSampleBufferDisplayLayer) | |
| Clone in, 54k files | 1.6 s + 0.84 s + 1.1 s | |

## 14. Tests

- `koyo-desk-wire`: every message round-trips; frames over the limit refused; the layout (slots,
  screen sizes as areas come and go, nothing moves); key names to codes; holder lines; helper
  requests.
- `koyo-desk-host`: the window rules as pure functions over window lists (ownership, strays,
  clamping, full screen), the paint-over, in-flight and no-damage rules over a fake encoder, the
  damage grid and waits over made-up frames, snapshots and refs over a made-up accessibility
  tree; an ignored integration test on a real session (M2).
- `koyo-desk-holder`: the RFB login and session select against a scripted server, the display
  configuration bytes against the PoC's, the dead-session rule.
- `koyo-desk-helper`: `setup`'s and `remove`'s plans as lists of commands (run against a stand-in
  runner), the peer check, the macOS version check.
- `koyo`: the pane on a headless shell (kind, tab, `tab_list`, restore), the tools' executors
  against a stand-in host (scopes, ownership, holds, worktree checks on send and fetch), input
  mapping and clamping, visibility.
- MCP: the catalog (names, schemas, `requestId`), as for every operation.

## 15. Where it lives

| Path | What |
| --- | --- |
| `koyo-rs/koyo-desk-wire/` | The link's messages and frames, the layout, keys, holder lines, helper requests |
| `koyo-rs/koyo-desk-helper/` | The root helper: setup, removal, status, the daemon |
| `koyo-rs/koyo-desk-host/` | The host library and its loader (`loader/main.c`) |
| `koyo-rs/koyo-desk-holder/` | The RFB 003.889 client and the holder daemon (`koyo desk-holder`) |
| `koyo-rs/koyo/src/agent_desktop/` | The app's side: link, pane, decoder, pointer, input, visibility, wallpaper |
| `koyo-rs/koyo/src/settings_view/agent_desktop.rs` | Settings → Agent desktop |
| `koyo-rs/koyo-app-control/src/ops/desktop.rs` | The tools' inputs and outputs |
| `koyo-rs/koyo/src/control/exec/desktop.rs` | The tools' executors |
| `koyo-rs/koyo/src/model.rs`, `model/tab.rs` | `PaneKind::Desktop`, its pane |
| `koyo-rs/koyo-app-control/src/scope.rs` | The `desktop` scope |
| `koyo-rs/koyo-term-mcp/src/guide/koyo.md` | The tools in the guide |
| `koyo-js/koyo-agent-host/src/providers/claude/options.ts` | `desktop_list` and `desktop_screenshot` among the reads a chat may make unasked |
| `script/bundle` | `Contents/Helpers/Koyo Agent Desktop.app` and `Contents/MacOS/koyo-desk-helper` in `Koyo.app` |

## 16. Open

- The two freezes of 2026-11-09 (6.1): system daemons following the agent session as the
  console user, in reconnect storms after it changed. Whether a real logout (not a kill), or
  keeping the console user record the person's, avoids them; an Apple Feedback report with
  both logs.
- What changed in macOS 27.2 for what's built on 26.4 measurements: checked in M1 (the RFB login
  and session flow, `SetDisplayConfiguration`'s modes, the 3840 × 2160 ceiling, deleting a user
  record as root) and M2 (privacy grants across sessions, the agent session's desktop picture
  levels, the capture badge, ScreenCaptureKit's dirty rects).

- Pointer contention: agents' turns on the one pointer wait for each other and for the person;
  measured with real agents in M5. Accessibility first should leave few acts needing a turn.
- How much accessibility reaches: Chromium and Electron apps build their tree only when asked
  (`AXManualAccessibility`, `AXEnhancedUserInterface`), which the host sets on each app it
  opens; canvases and games have none.
- Whether dirty rects are usable for waits, or the luma grid carries them (7.11).
- `[not built]` Viewing on the same Mac with no encode at all: the capture's IOSurface handed to
  the app through a mach-port broker (sessions can't pass mach ports over a Unix socket;
  PRIOR-ART "Streaming").
- Per-app light or dark: the account in dark mode with `-NSRequiresAquaSystemAppearance YES`
  opting an app out to light was not read back when the PoC stopped.
- Privacy grants on macOS versions other than 26.4.
- Notarization and the helper's signing (5.6).
