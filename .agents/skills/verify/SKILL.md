---
name: verify
description: Launch and drive a test instance of the Koyo desktop app (the shell, chats, terminals, native browser pages, settings) on Koyo's agent desktop, apart from the owner's Koyo, and capture evidence. Use it to prove a Koyo change in the running app, including scripted keyboard-focus sequences (pinned tab, click in page, type in the chat composer), window and page screenshots for visual changes, and on-screen browser checks.
---

# Verify Koyo in the running app

You get one test Koyo per checkout: the debug build in a bundle of its own (`test.koyo.verify`),
with its own data, settings, logs, temporary folder and probe folder under
`<checkout>/.superpowers/v/`. It runs on the agent desktop, a macOS desktop of its own that the
owner isn't looking at. You drive it through one helper, `koyo-verify`, which can reach only
the test instance. Evidence goes to `.superpowers/v/evidence/<stamp>-<label>/` and survives
cleanup.

```sh
B=.agents/skills/verify/bin/koyo-verify   # from the checkout root; every command below uses it
```

## Rules

Read these before every run. Each one protects the owner's machine.

1. **Never quit, drive or read the data of the owner's Koyo** (`/Applications/Koyo.app`, data in
   `~/Library/Application Support/codes.koyo.desktop`). Your `mcp__koyo__*` tools talk to the
   owner's Koyo. Use only its `desktop_*` tools, and only on the test app's pid and windows.
   Do the rest with `$B app`, `$B mcp` and `$B probe`.
2. **One thread at a time uses the agent desktop.** The owner has allowed it once and for all
   (2026-11-18): don't ask before `desktop_open`. Within one run, only the thread that opened the
   test app drives it; subagents of a coordinator take it in turn. When you finish, run `$B stop`.
3. **Never start the bundle from a shell** (`open`, running its binary, or `script/dev-bundle`
   without `--no-run`). Its window would land on the owner's desktop. Only `desktop_open`
   starts it.
4. **Never send `probe activate`, and never call `desktop_take_focus`** unless the owner has
   agreed in so many words. Both bring the test app in front, which can switch the owner's
   screen to the agent desktop.
5. **Treat Koyo's floating panels like native menus.** These are the address panel (⌘L), the
   launcher (⌘T), the tab switcher (⌃⇥) and popovers. GPUI makes them join every Space before
   Koyo pins them to one. In the proving run, an address panel opened on the test app wasn't on
   the agent desktop's window list, so it may have shown on the owner's screen. Open one only
   when the journey is about it, check `probe windows` for it, and close it at once with
   `$B probe escape`.
6. **The engine suites are `script/check`'s** (`koyo-browser-host` `tests/smoke` and
   `tests/embedded`, `host_tests`). Their hosts never come forward, so they don't take the
   owner's keyboard focus. If the app in front ever turns into `Koyo Browser Host` or `Koyo
   Engine Test` while one runs, stop running them and say so.
7. **Everything stays in the checkout.** Build with `nice -n 10`. Don't write to `~`, `/tmp` or
   global state. If a tool insists on writing outside the checkout, stop and ask. There is one
   exception, which the owner allowed on 2026-11-16: macOS gives the app the per-user temporary
   folder whatever the bundle says. The engine keeps its singleton socket there
   (`codes.koyo.desktop.browser-host.XXXXXX/`) and removes it when the app quits. `$B stop`
   removes the instance's folder if a crash left it.

## Launch

1. **Build** the debug app, the browser engine and the chat agent host into
   `koyo-rs/target/dev-bundle/Koyo Dev.app`. Pages need this bundle: a bare `cargo run` has no
   pages.

   ```sh
   nice -n 10 script/dev-bundle --no-run
   ```

   The build directory is shared by every checkout on this Mac (`~/.cargo/config.toml`
   `build-dir`). The line `Blocking waiting for file lock on build directory` means another
   worktree is building, and yours waits its turn.

2. **Prepare** the bundle and a fresh instance:

   ```sh
   $B prepare                                   # Koyo's default layout, no pins
   $B prepare --tabs Sidebar --pin "file://$PWD/.superpowers/v/fixture/focus.html"   # tabs in the sidebar, a pinned page
   ```

   This clones the dev bundle to `.superpowers/v/KoyoVerify.app`. The clone gets its own id and
   an `LSEnvironment` that sets `KOYO_DATA_DIR`, `KOYO_CONFIG_DIR`, `KOYO_LOG_DIR`,
   `KOYO_NATIVE_PROBE` and `KOYO_AGENT_PROVIDER=fake`. `TMPDIR` isn't set, because macOS
   replaces it (rule 7). The agent desktop starts apps with no
   environment of yours, so the bundle carries its own. The command then empties
   `d c l p t s fixture downloads` and seeds `c/settings.json`:
   - the fixture repository `.superpowers/v/fixture/repo` as the only project;
   - `agent_desktop: false` and `accounts.manage: false`;
   - `on_quit: "End"`;
   - `worktrees_dir` and `downloads_dir` inside `.superpowers/v`, so a page's download never
     lands in the owner's Downloads folder;
   - app control on for the client `koyo-verify` alone, with no `desktop` or `devices` scope, and
     its MCP server on port 0, so it never takes the owner's 6272;
   - `tabs` (`--tabs TopRail|Sidebar`) and `address_bar` (`--address-bar Sidebar|Page`), only when
     given: left out, Koyo's defaults apply (docs/layout.md).

   It refuses while an instance runs, and when the dev bundle is older than
   `koyo-rs/target/debug/koyo`. Its last line is the `desktop_open` input.

3. **Open it on the agent desktop** with the Koyo MCP tool. Use an absolute path. The `args`
   keep GPUI from writing `~/Library/Preferences/test.koyo.verify.plist`:

   ```json
   desktop_open {"app": "<checkout>/.superpowers/v/KoyoVerify.app",
                 "args": ["-NSAutoFillHeuristicControllerEnabled", "NO"]}
   ```

   The answer carries `pid` and the windows so far.

4. **Confirm** isolation and readiness:

   ```sh
   $B confirm <pid>
   ```

   First it checks the process's environment (`ps eww`) for this instance's three folders. If
   any is missing it kills the process and fails, and you drive nothing. It records the pid and
   start time. Then it waits up to 60 s for the control socket, for the log line
   `app control listens on`, and for `app_status` to answer. Last, it sends the probe one
   `tick`, which keeps the window drawing on a desktop that isn't shown. When the instance is
   ready, it prints
   `test Koyo pid N: isolated (KOYO_DATA_DIR=…), control socket up, probe ticking`.

5. **Start the evidence folder**:

   ```sh
   $B run <label>
   ```

   From here on, `app`, `mcp`, `probe`, `shot`, `pageshot` and `note` append to its
   `transcript.log`.

Teardown is `$B stop` (Cleanup).

## Doctor

```sh
$B doctor
```

The doctor is read-only. Run it first whenever anything looks off. Each line starts with
`ok`, `warn`, `FAIL` or `info`, and the command exits 1 on any `FAIL`. It checks:

- the socket path's length (at most 100 bytes);
- the bundle's stamp: the commit it was assembled at, and whether `target/debug/koyo` or any
  `.rs` source is newer than the bundle's binary (a stale build proves nothing about a change);
- that the recorded pid runs this bundle's binary with this instance's folders in its
  environment;
- that `app_status` answers with `dataDir` equal to `.superpowers/v/d`;
- that the probe answers `state`;
- the test settings: `agent_desktop` off, the port not 6272;
- the panics in `Koyo.log`, and the MCP server's URL;
- whether the owner's Koyo runs (info only: leave it alone);
- any `~/Library` entry for `test.koyo.verify` that shouldn't exist.

## Drive

Pick the channel by what the journey has to prove.

| Channel | Reaches | Use it for |
| --- | --- | --- |
| `$B app <operation> ['{json}']` | the app-control catalog over the test instance's socket (`$B app help`, `$B app help <operation>`) | setting state up and reading it back: projects, tabs, panes, chats, settings, and pages over CDP |
| `$B mcp <tool> ['{json}']` | the tools only `koyo mcp` serves: `terminal_*` (`$B mcp --list`) | terminals |
| `$B probe '<command>' …` | `NSEvent`s handed to the test window, and the window's own state and pictures, in the app's process | what a hand does: clicks, keys, menus, focus. Also who has the keyboard, and screenshots |
| `desktop_*` (Koyo MCP) | the agent desktop: real events posted to the test app's pid, and a picture of the desktop | shortcuts and clicks through the window server (`desktop_key`, `desktop_click`), on-screen pictures with pages in them (`desktop_screenshot`) |

`app` and `mcp` run the bundle's own `koyo app` / `koyo mcp` in a clean environment (`env -i`).
Only `KOYO_DATA_DIR` of the test instance is set, so no `KOYO_CHAT` or `KOYO_TERMINAL` of your
own Koyo leaks into the hello. Failures print `{"error":{code,message,retryable}}` and exit 1.
They run in the fixture's repo, the test project, so `$B mcp terminal_open` with no worktree
opens there.

`$B app` names no chat or terminal, so it has no worktree of its own, and `browser_upload`,
which gives a page only the caller's worktree's files, is refused. Call as a terminal of the
test project's: open one with `$B mcp terminal_open '{"name":"verify"}'`, then pass its handle
in `VERIFY_TERMINAL`, which `app` and `mcp` hand on as `KOYO_TERMINAL`:

```sh
VERIFY_TERMINAL=term_01… $B app browser_upload '{"pane":3,"paths":["<fixture>/notes.txt"]}'
```

`<fixture>` is the fixture repo `$B paths` names.

Changing operations take `select`. Left out, nothing is shown (`showWhatAgentsOpen` is off),
so pass `"select": true` when the journey needs the window to switch.

### The probe

The probe is in debug builds only, enabled by `KOYO_NATIVE_PROBE`, and implemented in
`koyo-rs/koyo/src/browser/native/probe.rs` and `probe/look.rs`. Each command prints
`<command> => <what it found>`. You can pass several commands in one call; they run in order,
and `wait <ms>` pauses between them.

| Command | Prints | Notes |
| --- | --- | --- |
| `state` | `first responder <Class>, key <bool>, main <bool>, seen <bool>, app active <bool>, number <n>` | `GPUIView` means GPUI has the keyboard. `RenderWidgetHostViewCocoa` means a page has it |
| `windows` | one line per window: `<number> <class> “<title>” x,y wxh visible key seen alpha content through level order` | Frames are in screen points from the bottom left. Page holders are the lines with `through true` |
| `pages` | one line per native page: `page <hostPane>: views in <class> visible …, drawn by a pane …, on screen for the engine …` | `<hostPane>` is the engine's id, not `tab_list`'s pane |
| `click x y [window#]`, `rclick x y`, `rdown x y`, `move x y`, `hover x y` | `clicked`, …; `hover` prints `over <ViewClass>` | Points are window points from the top left. Use `hover` to check what is under a point before you click it |
| `type <text>` | `typed` | Key down and up per character, to the first responder, every one with key code 0 |
| `key <code> <chars> [cmd shift ctrl alt]` | `taken by the window as a key equivalent`, `taken by the menu as a key equivalent`, or `to the first responder` | `<chars>` as written, or in double quotes with `\r`, `\t`, `\e`, `\n`, `\\`. Common codes are below |
| `shot <name> [window#]` | `saved <probe>/<name>.png` | Use `$B shot`, which moves the picture into the run |
| `eval <hostPane> <script>` | the CDP `Runtime.evaluate` answer | Works on any page, a pinned one too |
| `views`, `layers`, `cursor` | the view tree, the layers, the cursor | |
| `escape` | `escape posted` | Closes a native menu |
| `drag <ms> x y x y …`, `scroll <dy>` | | |
| `quit` | | `$B stop` sends it |
| `tick`, `front`, `frame`, `draw` | | `confirm` already sends `tick`, and a second `tick` turns it off |

Key codes (macOS virtual keys): Return 36, Tab 48, Escape 53, Delete 51, Space 49, A 0, L 37,
T 17, W 13, = 24, 0 29, ← 123, → 124, ↓ 125, ↑ 126. For example, ⌘L is `key 37 l cmd`, ⌃Tab is
`key 48 "\t" ctrl` (the chars are a tab), and Return is `key 36 "\r"`. Before 2026-11-20 the
probe took the quotes and the backslash as characters and typed them into the page; `key 36`
with no characters submits nothing. A Return that has to submit a page's form is proven only
through `desktop_key` (key `return`), which the window server delivers as a keyboard's.

### Finding a point to click

GPUI exposes no element bounds from outside. Inspector (⌘⌥I) shows them on screen only, and
`debug_selector` names exist only in test builds. So:

1. Run `$B shot look`. Open the PNG. It is drawn at a point a pixel, so a pixel in the picture
   is a window point. Pages are blank in it: place them by their frames, below.
2. Pick the point and run `$B probe 'hover x y'`. Make sure it prints the view you mean:
   `GPUIView` for Koyo's own UI, `RenderWidgetHostViewCocoa` or `WebContentsViewCocoa` inside a
   page.
3. Click it, then confirm the result with `state`, a read through `$B app`, or another shot.

To click inside a page, the probe's `windows` gives the frames. Take Koyo's window (class
`GPUI…`) and the page's holder (`through true`), both in screen points from the bottom left:
`x = holder.x − koyo.x + holder.w/2` and `y = (koyo.y + koyo.h) − (holder.y + holder.h) + holder.h/2`.
In the proving run, Koyo's window was at `90,55 1620x1018` and the holder at `358,63 1344x970`,
which gives (940, 525), and `hover 940 525` printed `over RenderWidgetHostViewCocoa`.

### Keyboard focus

Observed in the proving run (2026-11-16):

- Right after `desktop_open` the desk host makes the window key, main and active: `state`
  printed `key true, main true, app active true`.
- Once something takes key away, such as a panel opened by ⌘L, the window doesn't get it back
  on the agent desktop. `desktop_focus` refuses Koyo's window ("doesn't take being raised"), and
  neither a `desktop_click` on the window nor a second `desktop_open` made it key again. The app
  ends up `key false, main false, app active false`.
- A press on a page is a first click (Chromium's view doesn't take `acceptsFirstMouse`), so on a
  window that isn't key it proves nothing.

So:

1. Run each focus journey on a fresh launch (`$B stop`, `$B prepare …`, `desktop_open`,
   `$B confirm`), before anything else touches the window.
2. Run `$B probe state` before every step. It must print `key true`. If it prints `key false`,
   the rest of the journey proves nothing: record that, then stop and relaunch.
3. Keep panels (⌘L, ⌘T, ⌃⇥) to the end of a journey (rule 5). Check a panel with
   `probe windows` (a `GPUIPanel` line with `key true`), not with `state`, which describes only
   the main window.
4. The probe and the desktop deliver shortcuts differently. `probe 'key …'` tries the window's
   and the menu's key equivalents first, so a page never sees the shortcut. `desktop_key` sends
   real events: ⌘L from it reached the page as `Meta` and `l` keydowns and opened the address
   panel. Prove "a shortcut works while a page has the keyboard" with `desktop_key`, and use the
   probe for repeatable sequences.

The scripted journeys are in [features/keyboard-focus.md](features/keyboard-focus.md).

## Evidence

Everything for a proof goes in the run folder that `$B run <label>` printed,
`.superpowers/v/evidence/<stamp>-<label>/`:

- `transcript.log`: every `app`, `mcp` and `probe` call with its output, timestamped, after a
  header with the checkout's commit and the bundle's stamp. `$B note '<text>'` adds what a
  `desktop_*` call did, since those calls don't pass through the helper.
- `<name>.png`, from three sources:
  - `$B shot <name>`: the window as the app draws it, taken in the process with no
    screen-recording grant. It covers Koyo's own UI only: a page's area comes out blank,
    because pages are composited on the GPU. A panel's shot fails (`no picture of the window`).
  - `$B pageshot <name> [pane]`: a page as the engine renders it (`browser_screenshot`).
  - `desktop_screenshot {"window": <id>}`: the window as it is on screen, pages included. The
    tool's result names a PNG file. Copy it in with `$B keep <that path> <name>`. Use this for
    on-screen checks of pages and of anything visual over them.
- `logs/`: `Koyo.log`, the agent host's and the terminal daemon's logs, copied by `$B stop`.

Proof standards:

- Exercise the user's path. Keys, clicks and menus go through the probe or `desktop_*`.
  App-control calls are for setting state up and reading it back, unless app control itself is
  the feature.
- Capture the action and the state it leads to. Run `state` before and after a focus change,
  and take a shot before and after a visual change.
- Check the side effect too: `window.json` and `pinned-tabs.json` under `.superpowers/v/d`, a
  chat's messages through `chat_read`, a page's own record of keys through `browser_eval`.
- The fake chat provider is the production boundary to Claude. It answers `You said: <text>`.
  A real Claude session would write the owner's `~/.claude`, so it isn't part of this skill.
- A probe event is handed to the window directly and skips the window server, so it can't
  prove which window is in front or that a real pointer reaches a view. Use `desktop_click` or
  `desktop_key` when that is what the change is about.

## Cleanup

```sh
$B stop            # or: $B stop --keep   (keeps data and settings, for a relaunch journey)
```

`stop` works through these steps:

1. It sends the probe `quit` and waits 15 s. If the recorded process (same pid and start time)
   is still there, it gets `TERM`, then `KILL`.
2. It stops every process of yours whose environment carries `KOYO_DATA_DIR=<this instance>`:
   the chat agent host and the terminal daemon outlive the app. Nothing else is touched, and
   nothing is matched by name.
3. It copies the logs into the run's `logs/`.
4. It removes `d c l p t s fixture worktrees downloads`.

The bundle and `.superpowers/v/evidence/` stay. `stop` prints the evidence folder and its
contents. `desktop_close {"pid": …}` is
the desktop's own way to quit the test app. Run `$B stop` after it anyway, for the leftovers
and the logs.

Run `$B stop` after every failed attempt too, so no process or socket outlives it.

## Gotchas

- **A click that should make the window key may be ignored.** On the agent desktop the test app
  is the active app only right after launch. Once the window loses key, nothing an agent can do
  there brings it back (Keyboard focus). Check `key true` in `state` before each focus step.
- **Probe input counts as the person using the page.** For a few seconds after a probe click
  or key, `browser_*` calls on that page answer `person_has_control`. Read the page with
  `$B probe 'eval <hostPane> <script>'` instead, or wait about 5 s.
- **A page opened with `"select": true` takes the keyboard.** `state` prints
  `RenderWidgetHostViewCocoa` straight away.
- **A chat turn that ends while the window isn't main posts a macOS notification.** The first
  one makes macOS ask the owner for permission on their own screen. `$B app chat_create` and
  `chat_send` refuse unless `probe state` prints `main true`, and you check it yourself before
  sending with a key ([features/chats.md](features/chats.md)).
- **Shells in test terminals don't get `KOYO_DATA_DIR`.** A `koyo …` typed in one reaches the
  owner's Koyo, and a default login shell reads the owner's rc files and writes the owner's
  history. Open terminals with a command, `{"command": ["/bin/zsh", "-f"]}`, and never run
  `koyo`, `claude` or `git push` in them.
- **Pinned and loose pages aren't in `tab_list` or `browser_list`.** App control can't drive
  them. Seed them with `$B prepare --pin <url>` (`--tabs Sidebar` shows their tiles, the default
  rail a pill of buttons), click them through the probe, and read them with
  `probe eval <hostPane>`.
- **The probe's `type` sends key code 0 for every character.** A page sees the right
  `event.key`, but `event.code` is `KeyA` for every character. Assert on `key`.
- **A second `tick` stops the window drawing.** If shots look frozen, run `$B doctor` before
  you touch `tick`.
- **The dev bundle goes stale.** `prepare` refuses a dev bundle older than `target/debug/koyo`,
  and `doctor` warns about sources newer than the bundle. Rebuild before proving a change.
- **A worktree path can be too long.** The control socket must stay at or under 100 bytes, and
  `.superpowers/v/d/control/koyo-control.sock` adds 42 bytes to the checkout's path, so the
  checkout's path can be 58 bytes at most. `prepare` refuses longer ones.
- **An unknown client gets an Allow/Deny prompt that activates the window.** `prepare`
  pre-allows `koyo-verify`, so never pass another `--client`.
- **A settings file that doesn't parse is dropped whole.** The app then runs on defaults:
  external control off, accounts managed. If `$B app` answers `control_disabled`, the seeded
  settings didn't load. Run `$B stop`, then `$B prepare` again.
- **Worktrees.** Creating one in the test instance runs git on the fixture repository and puts
  the worktree under `.superpowers/v/worktrees`. Never point the test instance at the
  checkout you work in.

## Other harnesses

None of these is on the default path, but each is worth knowing:

- **GPUI test support.** These are headless `Shell` tests in `koyo-rs/koyo/src/test_support.rs`
  (`VisualTestContext`, `debug_bounds` for `debug_selector` names such as `chat-composer`). They
  are the cheapest proof of focus and layout logic. Run them with
  `nice -n 10 script/cargo test -p koyo --bin koyo <filter>`.
- **Visual checks.** `script/cargo run -p koyo --features visual-check -- --visual-check-<name>`
  takes one of `finder lsp settings chat chat-theme chat-markdown native-controls accounts
  accent sidebar terminal switcher devices`, or plain `--visual-check` for files. Each opens real
  windows off screen and saves PNGs under `.superpowers/qa/<name>/`. They need
  `KOYO_CONFIG_DIR`, `KOYO_DATA_DIR`, `TMPDIR` and `KOYO_LOG_DIR` inside the checkout, and the
  chat ones need `KOYO_AGENT_PROVIDER=fake` (`docs/tab-switcher.md` section 7 has the full
  command). They have no pages, and their windows open off screen, not on the agent
  desktop.

## Gaps worth closing in the app

These need Koyo code, not this skill. They come from the 2026-11-16 run and from rune's
verification (github.com/anomalyco/rune: scripted taps by element key, focus asserted by
element key):

- **Bounds by name.** A probe command that lists named GPUI elements (the `debug_selector`
  names, such as `chat-composer`) with their window bounds, so clicks aim at a name, not at
  coordinates read off a shot.
- **Focus by name.** A probe answer naming GPUI's focused element, beside AppKit's first
  responder.
- **Key on demand.** A probe command that makes the main window key without activating the app,
  as the desk host does at launch, so a journey can recover from a panel.

## Helpers

- `bin/koyo-verify`: every command above. `$B help` lists them, and `$B paths` prints the
  folders, the socket and the `desktop_open` input.
- `bin/mcp-call.py`: the stdio MCP client behind `$B mcp`, run with `python3 -I`.

## Feature map

[features/README.md](features/README.md) lists the features and how to drive each one. A proof
that drives one convenient entry point is incomplete when the map lists others.
`/maintain-verification-skill` keeps the map in line with the app.
