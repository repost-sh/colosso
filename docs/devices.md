# Koyo devices: iPhones and iPads, simulated and real, live in a tab

Written 2026-11-11 on branch `schulzfel/devices`, from a proof of concept run on a real Mac
(macOS 27.2, Xcode 27.2) on 2026-11-10 and 11. Author of the vision: Felipe Schulz. Drafted with
Claude. Source of truth for devices in Koyo; update it when a decision changes.

An iOS or iPadOS simulator, or an iPhone or iPad connected by cable, shown live and full size in a
Koyo tab. The person clicks to tap, drags, pinches and rotates on the trackpad, and types. Agents
get a family of `device_*` tools: list and open devices, look (screenshot, a snapshot with element
refs, wait until the screen settles), act (tap, swipe, scroll, pinch, type, keys, buttons, rotate),
and install, launch and stop apps. The person always comes first: their touch in the tab pre-empts
an agent, and their own device is used only after they allowed it in Koyo's window.

Status marks: `[decided]` (the owner's word), `[built]` (in the tree and checked on a real Mac),
`[proposed]` (designed here, not built; the owner's to overturn), `[open]`, `[not built]`.

How it got here. The study and the proof of concept stay out of the repository, in
`.superpowers/mobile-device-research/` (`STUDY.md`, `POC.md`, `XCODE27.md`, `SPEC.md`). The PoC
showed, on this Mac: a booted simulator's screen taken as the `IOSurface` it renders into, with no
capture and no privacy grant, sent to another process by mach port and drawn by Koyo's renderer;
touches, keys and buttons into the simulator through `dtuhidd`; and a real iPad on iPadOS 27 seen
through its own HEVC screen stream and driven through its developer services over the cable, with
nothing installed on it. Its Objective-C is the recipe; the product is Rust.

---

## 1. What the person gets [decided]

- A new kind of tab, one per device. It shows an iPhone or iPad simulator of Koyo's own, or an
  iPhone or iPad on iOS or iPadOS 27 or later connected by cable, live and full size.
- In the tab: click to tap, press and hold, drag; pinch and rotate on the trackpad; scroll; type.
  A press that begins at the very edge is that edge's gesture (Home, Notification Centre, Control
  Centre, back). Home and the app switcher are buttons on the tab's bar; a simulator also turns.
- Settings → Devices installs simulators with their progress shown and walks the person through
  making a real device ready, one step at a time.
- A real device doesn't lock while its tab is open, and locks by its own setting once it closes.
- **Xcode 27 is the minimum** [decided 2026-11-11]: for simulators and real devices alike (a
  device's developer image and `devicectl` come with Xcode). With an older Xcode or none, every
  surface of the feature says "Koyo's devices need Xcode 27 or later" with what to do, and offers
  nothing else.

Not in this build: Android, Apple Watch, Apple TV, Vision Pro [decided]; recipes kept in a project
[decided]; Wi-Fi [proposed]; driving or watching a device below iOS 27 (it is listed, with what to
update) [decided]; Xcode 26's input path [decided]; the iPhone Duo's second display and hinge
[proposed]; Xcode's own agent tools (`xcrun mcpbridge`) [proposed]; bezels [proposed]; unlocking a
device, ever [decided]; a marker on the device itself [proposed].

## 2. What reaches the person, and what doesn't [decided]

Each line is a rule Koyo keeps. The measured proof of each goes into 12 as it is built.

| The person's | What happens | Kept by |
| --- | --- | --- |
| Pointer, keyboard, front app, desktops, displays | Nothing | The device host has no window and posts no Mac events. Koyo's own dialogs open only while Koyo's window is active |
| Privacy grants | None is asked for | The simulator's surface and a device's stream need none (measured from a terminal in the PoC; `[open]` from the installed app) |
| Xcode's own simulators, Simulator and Device Hub | Never listed, started or changed | Koyo's simulators live in a device set of its own, `<data>/devices/set` |
| Memory | Each running simulator takes gigabytes (the PoC summed 3.5 GB on iOS 26.5 and 10-13 GB on iOS 27.2 from `top`, not looked into) | Shown in Settings with `Stop`; at most two running at once unless the person raises it; shut down when its tab closes and when Koyo quits or is found to have crashed |
| Disk | About 2 GB a simulator in Koyo's data folder. A simulator system is about 8 GB in the system's own folder, shared with Xcode. macOS keeps its compiled text model for the host in `~/Library/Caches/koyo-device-host` (12 MB). A simulator's own services that crash leave reports in `~/Library/Logs/DiagnosticReports`, as under Xcode (about a hundred, 2 MB, in a day of testing) and CoreSimulator keeps a log per simulator in `~/Library/Logs/CoreSimulator` | Downloaded only on the person's click, never deleted by Koyo; the reports and logs are macOS's |
| Their iPhone or iPad | Seen and driven only after they allowed it in Koyo's window; kept awake while its tab is open; no setting changed; never turned, never unlocked | The lend (9.6) and the device's own stream (5) |
| What is on that device | Its screen goes to the agent that was allowed, as pictures and text | The lend, its notice in the tab and `Take It Back` |
| The device's own state | Developer Mode must be on, which lowers its guard; the developer image is mounted | The setup says so before the person turns it on |
| The device's camera and microphone | `[open]` reported off while its screen is viewed | The tab says so; to be checked by the owner once |

## 3. Architecture [decided]

```
Koyo (the app)                                      one host per open device
  devices/  registry, links, views, setup     ──►   koyo-device-host  (a child process)
  control/exec/device.rs  tool executors              simulator: CoreSimulator, dtuhidd, simctl
  settings_view/devices.rs                             device:    idevice tunnel, VideoToolbox
       ▲            │  socket (fd 3): CBOR messages          │
       │            └──────────────────────────────────────►│
       └──── mach port: a ring of three IOSurfaces ◄────────┘
```

- **A host process per open device.** A crash in private API takes one device's tab, not the app
  and not the other devices. A lock file per device keeps a second host off the same device.
- **Started as Koyo's child**, the way the browser's host is: a socket pair inherited as fd 3, a
  one-time token. It holds no privacy grant, so it needs no loader, no app bundle and no Launch
  Services start (the agent desktop's host has those only for its grants).
- **The host goes when Koyo goes.** It ends when its socket closes and shuts its simulator down
  itself; Koyo's quit path doesn't wait on a device. What a killed host left booted is shut down at
  the next start from a record (`<pid> <start time> <udid>`). Everything in Koyo's device set is
  Koyo's to shut down; nothing of the person's is ever recorded as something to end.
- **Restarts**: the browser's policy. One retry of a failed start; three restarts in sixty
  seconds, then failed with `Start Again`.
- **A watcher** [proposed]: the same binary run as `koyo-device-host --watch` while a Devices
  surface is open, listening on usbmuxd for a device plugged in or pulled, so the app never polls.

Crates:

| Crate | Owns |
| --- | --- |
| `koyo-device-wire` | Every message between app and host; no Apple frameworks; every message round-tripped in tests |
| `koyo-device-gestures` | What the Mac's pointer, trackpad and keys do over a touch screen; no dependencies |
| `koyo-device-host` | The host binary: the simulator backend, the real-device backend, the ring, acting and looking |
| `koyo` | `src/devices/` (registry, link, view, setup), `control/exec/device.rs`, `settings_view/devices.rs` |
| `koyo-app-control` | `ops/device.rs`, the `devices` scope, two error codes |

### 3.1 The picture

1. The host owns a ring of three BGRA `IOSurface`s at the picture's upright size. Each frame goes
   into a slot that isn't the one last announced and that nobody holds (`IOSurfaceIsInUse`). With
   no slot free the frame waits; only an older waiting frame is dropped for a newer one. The newest
   frame is never the one lost: a still simulator sends no other.
2. Simulator: a GPU copy from the simulator's surface into the slot, started in the frame callback
   and not waited for; the frame is announced when the copy completes. Drawing the simulator's own
   surface tears a few reads in ten thousand; the ring's copy didn't in 35,625 (PoC). Waiting for
   the copy brought an iPad simulator from 60 to 46 frames a second (PoC).
3. Real device: its HEVC stream depacketized, decoded by VideoToolbox, then one
   `VTPixelRotationSession` pass turns it upright and makes it BGRA in the slot.
4. The ring goes to the app once per generation as one mach message (`koyo_browser_ipc::mach`);
   a new generation only when the picture's size changes. Each frame is a socket message
   `Frame { generation, slot }`; the app holds one slot at a time, as the browser does.
5. Kept current only while somebody needs it: a tab draws the device in a window that can be seen,
   or an agent asked something in the last ten seconds. Otherwise a simulator's frames aren't
   copied, and a device's stream is kept open (it keeps the device awake) and not decoded.

## 4. The simulator [decided]

- Koyo's own device set at `<data>/devices/set`. The person's own Xcode simulators are never
  listed, booted or touched.
- Boot and shutdown by `xcrun simctl --set <set> boot | bootstatus -b | shutdown`, one-shot.
- The picture through CoreSimulator (`SimServiceContext`, the device's `io` ports, the main
  display's `framebufferSurface` and its screen callbacks). Every private call can raise an
  Objective-C exception; each is caught and answered as a failure.
- Input to `dtuhidd` in the simulator over XPC: one and two fingers with edge flags, keys by HID
  usage, buttons. Readiness is a barrier key event that gets a reply, tried five times four seconds
  apart; input is held until it answers. Three to five fingers through the simulator's
  `universalhidservice` with the 58-byte report a real device takes.
- Turning by a GSEvent to the simulator's workspace port [built]. The picture is drawn turned
  inside the same surface, so the host turns it upright in its copy (a small kernel; 0 torn of
  2.9 million reads) and maps places. **Which way is the interface's, not the device's**: told
  upside down, an iPhone's Safari stays on its side. The host reads the simulator's own
  `getdisplayinfo` (the CoreDevice action a real device answers too) for the primary display's
  `currentOrientation`, after a turn and every two seconds while the picture is wanted.
- A contact's last move is folded into its lift unless the lift comes more than a frame later
  and the last place is sent once more (measured: a drag 17 steps of 18, two fingers 30 of 31,
  before the fix) [built].
- Apps by `simctl --set <set> install | launch | terminate | listapps | openurl`.

## 5. A real device [decided]

- Over the cable through the developer tunnel (usbmuxd, `CoreDeviceProxy`, a software tunnel, the
  RSD handshake), with idevice (MIT). No network interface is made; nothing is installed on it.
- What must be true first, each told apart so the setup shows the one step missing: connected;
  trusted; unlocked; Developer Mode on; the developer image mounted; iOS or iPadOS 27 or later.
- The device's own screen stream, decoded on the Mac, with the feedback the device is owed (every
  picture acknowledged, a report every 50 ms, a keyframe asked for when a decode fails).
- **It doesn't lock while its stream is held open** (PoC: unlocked through 8 and 12 minutes;
  locked in 5-6 with only a power assertion). That is the whole mechanism: nothing is set on the
  device. The tab says "While this tab is open, your iPad stays awake and its camera and
  microphone are off."
- Input through its HID services; places turned to the device's own upright axes by how it is
  held. **Never turned by Koyo** [decided]: the only way found leaves a simulated orientation on
  the device. The `lock` button is refused: an agent that locks it has locked itself out.
- Locked: the tab says "Unlock your iPad to continue." and agents are refused at once. Koyo never
  types, asks for or stores a passcode.
- Unplugged: the tab says so and recovers by itself when it's plugged back in. Restarts are
  bounded: sessions restarted often are reported to wedge the device's daemon.
- One remote view of a device at a time: Xcode or Device Hub taking it pauses Koyo's.

## 6. Acting and looking [decided]

- Acts become timed touches in the host, in slices on its loop, never one blocking call: a long
  type goes about twelve characters a turn, a swipe step by step. The person's input in the tab
  ends a running act at once and lifts its fingers; the agent is told `person_has_control`.
- **Every act says what happened**, because these input paths fail silently: the picture is
  sampled before, the act sent, and the answer waits until the picture has changed and stood still
  for 300 ms (2 s at most). The answer is `{ change, region, still, millis }`: `change` is `none`,
  `small` or `large`. On a real device `small` can be the stream's own noise.
- Text is typed as characters: ASCII as HID usages with Shift where needed. Text outside ASCII is
  refused with the characters named, never dropped (`[open]` a simulator's pasteboard as a way).
- Screenshot: the ring's latest picture as PNG, at one pixel a point by default.
- Snapshot: indented text with `[ref=eN] @x,y`, from the simulator's accessibility tree where it
  can be had (`source: accessibility`), else text read off the picture with macOS's Vision
  framework (`source: text`) [built], which is all a real device has. The first read of a new
  build of the host makes macOS compile its text model, 30-40 s: the host does it as it starts,
  while the simulator boots, and a snapshot asked before then is told to ask again.
- An act's answer waits until the picture has changed and then stood still for 300 ms, 2 s at
  most [built]: a press whose effect is a moment coming isn't taken for one that did nothing.

## 7. The link [decided]

Length-prefixed CBOR both ways (`koyo_desk_wire::{read_frame, write_frame}`), one `PROTOCOL`
number both sides refuse to differ on, each side's hello first. Places are points of the upright
picture from its top left; nothing above the host knows about orientation.

App to host: `Hello { protocol, token, surfaces }`, `Visible`, `Input(Input)` (the person's, no
answer), `Request { request, ask }`, `Cancel`, `Quit`. Host to app: `Hello { protocol, pid }`,
`State` (`Starting`, `Ready { screen }`, `Locked`, `Gone`, `Unavailable { code, why }`),
`Surfaces`, `Frame`, `Touched`, `Answer`, `Log`.

## 8. The tab [decided]

- `PaneKind::Device`, one view per pane, a lease per view, as the browser's pages. The title is the
  device's name.
- The picture fitted to the pane and centred, never larger than one device pixel to one screen
  pixel, on the tab's own card (no root background of its own).
- A toolbar as the browser's over a page (40 points, a hairline under it): the device's glyph,
  its name and a dot and a word for how it stands (green Ready, amber Starting, red what needs
  doing, grey Not running); then icon buttons, each named by its tooltip with its key: Home
  (⇧⌘H) and App Switcher, then Rotate Left (⌘←), Rotate Right (⌘→) and Lock for simulators;
  for a real device the sentence of 5. Dimmed until the device stands ready.
- The control bar is the browser's own pill (`held::bar`). An agent's touches show as marks.
- After a restart a tab comes back as "{name} isn't running." with `Start`: nothing starts by
  itself. A device no longer in Koyo's set says "{name} is no longer among Koyo's devices." with
  `Choose Another…`, which turns the tab into the picker. Closing the last tab of a device shuts
  its simulator down or lets the device go; two tabs of one device share its host.
- The launcher's **Device** kind lists the device tabs open, then Koyo's simulators no tab shows,
  filtered by what's typed; Enter with nothing picked opens a tab on the picker
  (`docs/new-tab.md`).
- The device counts as drawn while its picture is painted in a window (a keyed state of the
  picture's element): only then, or while an agent works on it, does the host copy frames. A
  hidden tab costs no copies.
- Every state has one way out or moves on by itself: no device chosen (the picker), not running,
  starting (for the first time: "about a minute"), ready, locked, unplugged, needs setup, below
  iOS 27, failed.
- How it stands is read in one place (`devices/standing.rs`): the host's last word first. A host
  that says it can't go on (what it needs is missing, its input never answered, the simulator
  went) is told to go and isn't started again by itself; its word stays on the tab, and Start
  Again, or an agent's `device_open`, starts afresh. A simulator shut down from outside stays
  down.

## 9. Agent tools [decided]

`device_list`, `device_open`, `device_close`, `device_screenshot`, `device_snapshot`,
`device_wait`, `device_tap`, `device_swipe`, `device_scroll`, `device_pinch`, `device_type`,
`device_key`, `device_button`, `device_rotate`, `device_apps`, `device_install`, `device_launch`,
`device_terminate`, `device_open_url`. Reads need the `read` scope; the rest a scope of its own,
`devices`, which Koyo's own agents have and an outside client is given in Settings → MCP server.

- Which device: `device` or `pane`; with neither, the one this caller acted on last while a tab
  still shows it, else the shown tab's, else the only one open, else an error that lists them.
- `device_snapshot` reads a simulator's own accessibility tree (`source: accessibility`), asked of
  its CoreSimulatorBridge as idb asks it: roles, labels, values, ids, a ref touching the
  element's activation point (a switch's row on its switch). A system alert is an app of its
  own. An element another process draws is marked `remote`: `source: "text"` reads the
  picture's words instead, as a real device always does. `device_launch` answers once the
  launched app is in front by the tree (its bundle id, 10 s at most) and the screen changed and
  went still; `device_open_url` once the screen changed and went still. A read right after a
  boot asks again for up to 5 s while nothing but the status bar is there.
- At most two simulators run at once, counted from what Koyo's set has booted (an agent's own
  `simctl boot` included) and what Koyo is starting.
- Errors say what to do. New codes: `device_unavailable` (retryable: starting, reconnecting) and
  `device_needs_person` (not: locked, not trusted, Developer Mode off, below iOS 27, Xcode missing,
  or the person is being asked whether to lend their device).
- **The person's own device** [decided 2026-11-11]: the first call of an agent on a real device
  that isn't lent to it, reads included, puts a sheet in Koyo's window ("Let “{agent}” use your
  {iPad} “{name}”?", `Allow` / `Don't Allow`) and returns at once with `device_needs_person`. After
  `Allow` the device is that agent's until the person takes it back. `Don't Allow` answers
  `user_declined` and raises no new sheet for that agent and device for ten minutes.
- Nothing unverified is shipped as working: a tool path that couldn't be shown to work on the real
  thing refuses with what to use instead.

## 10. Setup and Settings → Devices [decided]

One guided dialog, a step at a time, moving on by itself as a state changes, offered once per run
when a device tab or the picker first shows with something missing, never because an agent's tool
failed, and only while Koyo's window is active. For simulators: Xcode 27; Xcode's components; a
simulator system (downloaded with its progress, on the person's click); a device; the first start.
For a real device: connect, trust, unlock, Developer Mode, the system version, each with the exact
path on the device.

Settings → Devices: Xcode's state; the systems installed (no `Delete`: they're Xcode's too); Koyo's
simulators with `New…` and `Delete`; what is running, with `Stop`; the limit of simulators running
at once (2); connected devices with `Set Up…`; and in words what Koyo doesn't touch.

## 11. Milestones

| | What | State |
| --- | --- | --- |
| M0 | Premises re-measured with the PoC's hosts alone; the QA harness; this document | done |
| M1 | The simulator's host, headless, and its real-simulator suite | done: every row in debug and release |
| M2 | The tab | done: the hand's and the states' tests, the off-screen pictures with two real simulators, QH1-QH4; the window at 60 and 120 Hz waits on the owner (QP8) |
| M3 | Agents on a simulator | |
| M4 | The simulator half reviewed and hardened | |
| M5 | Setup and Settings | |
| M6 | The real device | |
| M7 | Finish | |

## 12. Measurements

| What | PoC (Xcode 27.2) | M0, re-measured | Built (M1, the host) |
| --- | --- | --- | --- |
| Simulator surface (iPhone 17 Pro, iOS 27.2) | 1206 x 2622 BGRA | 1206 x 2622 BGRA | the same; iPad Pro 13 2064 x 2752 |
| Frames a second while a page animates | 60 | 60-61 | 60.0 (iPad 60.0-61.7), copy not waited for |
| First boot, iPhone 17 Pro on iOS 27.2; a later boot | 49 s; 8-9 s | 48.4 s; 7.8 s | |
| Input ready, first boot; later boot | up to five tries 4 s apart | try 2, 12.8 s; try 1, 2.5 s | a tap asked meanwhile held 5.4 s, then landed |
| Taps landing at five places | 5 of 5 | 5 of 5 | 5 of 5 to the point, iPhone and iPad |
| Tearing, the simulator's own surface | 7 of 33,830 | 2 of 17,797 | |
| Tearing, what the app draws (the slot it holds) | 0 of 35,686 | 0 of 51,528 | 0 of 976,384 to 1,543,844; turned 0 of 2.9 million |
| Copying a frame into the ring | 0.36-1.4 ms | 0.96 ms | |
| A finger's step to the next frame | 25.7 ms (median) | 24.9 ms (median of 10) | |
| Host CPU at 60 frames a second; a still screen | 2.1-2.4%; 0% | | 2.0-2.7%; 0.2-0.5%, 1 frame in 3 s |
| Colours (U1) | not checked | | sRGB read back exact (0 off): no conversion |
| Unseen, then seen again: the first frame | | | 8-9 ms (none copied while unseen) |
| Every slot held by the app: the owed frame once let go | | | 9-15 ms |
| Drag 30% of the height; pinch to 3 and to a third; rotation 90° | 262 points; 3.03, 0.33; 90 | | 262 of 262; 3.017, 0.329; 90 |
| Text read off the picture: the first read of a new build | 39.6 s once | | 30-40 s (macOS compiles its model): warmed up as the host starts |
| The tab off screen, the 3 s after ready (M2, debug) | | | 33 frames shown, 26 paints; a still screen 0 and 0; the iPad 51 and 42 |
| The host killed: noticed; ready again (M2) | | | 133-450 ms; 1.7-4.6 s, the simulator kept booted |
| Koyo quits: the quit's own time; the simulator down (M2) | | | 1.7 ms; 2.9 s |
| A second device beside the first (M2) | | | ready in 30-39 s from cold; the hidden first sent 0 frames meanwhile |

M0's numbers were taken on macOS 27.2, Xcode 27.2 (27B52), CoreSimulator 1174.21, iOS 27.2
(24B8117), with the PoC's `simhost` alone and no window.

## 13. Where it lives

| What | Where |
| --- | --- |
| This document | `docs/devices.md` |
| The research, the PoC, the spec | `.superpowers/mobile-device-research/` (not in the repository) |
| QA log, matrix, recipes | `.superpowers/devices/qa/` (not in the repository) |
| The wire; the hand's mapping; the host | `koyo-rs/koyo-device-wire`, `koyo-device-gestures`, `koyo-device-host` |
| The registry, link, tab and its pictures | `koyo-rs/koyo/src/devices/` (`visual.rs`: `--visual-check-devices`) |
| A device's pane in the shell; the launcher's rows | `koyo/src/shell/device_tabs.rs`; `koyo/src/launcher/rows.rs` |

## 14. Open

- An iPhone on iOS 27 was never run: everything real was measured on one iPad.
- Whether no prompt appears when the host is started by the installed Koyo, from an identity that
  holds nothing.
- The simulator's accessibility tree on iOS 27.2.
- Pinch, rotation and typed text on a real device.
