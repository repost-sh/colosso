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
  The host shuts its simulator down as it goes: when the app says quit, when the app's end of
  the link closes, and on a terminate, interrupt or hang-up signal. It clears its signal mask
  as it starts [built]: Koyo's thread that starts it has every signal blocked, the host came
  with them blocked, and a `kill` did nothing (measured: not gone in 60 s; 3.3 s with the mask
  cleared).
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
  `currentOrientation`, after a turn and every two seconds while the picture is wanted, over one
  connection kept to that service [built]: a second connection made from the same looked-up
  port is refused as invalid, so with one made a read every other read failed (measured), and
  in a first boot's first minutes most did. Unread, the picture turns the way the device was
  told, as the Simulator app shows it [built]. An iPhone's Home Screen doesn't turn, so a turn
  there shows nothing until an app that turns is in front.
- Home is the device's own button (the consumer page's Menu usage, held 60 ms); the app
  switcher is Home pressed twice 100 ms apart, as the Simulator app's App Switcher [built].
  Measured on an iPhone 18 Pro: two presses 100, 150 and 250 ms apart each brought it, where the
  swipe from the bottom edge held a second took 1.6 s before it showed.
- A contact's last move is folded into its lift unless the lift comes more than a frame later
  and the last place is sent once more (measured: a drag 17 steps of 18, two fingers 30 of 31,
  before the fix) [built].
- Apps by `simctl --set <set> install | launch | terminate | listapps | openurl`.

## 5. A real device [decided]

The host's phone backend (`koyo-device-host/src/phone/`), started as `--phone <udid> --name
<name>`. idevice (MIT) is a crates.io release pinned exactly (`=0.1.68`, its `ring` backend) and
reached only from `phone/`: its API is unstable below 0.2.

- Over the cable through the developer tunnel (usbmuxd, `CoreDeviceProxy`, a software tunnel, the
  RSD handshake), on a thread of the host's own with its own runtime; the stream is read by a
  task of its own, so nothing the session waits on holds it up. No network interface is made;
  nothing is installed on the device. Ready in 0.5-1.5 s [measured].
- What must be true first, each told apart so the setup shows the one step missing: connected by
  cable; trusted; Developer Mode on; iOS or iPadOS 27 or later; the developer image there. When
  connecting fails the host asks `devicectl list devices` once for which (`phone/checks.rs`).
- The device's own screen stream (sound first, then video on the same session), decoded by the
  Mac's hardware decoder in its own format and made BGRA in one surface of the host's, which the
  ring turns upright by the LCD's own turn. The feedback the device is owed: every picture
  acknowledged, a report every 50 ms, a word every second, a keyframe asked for when a decode
  fails (twice a second at most). Unwanted, pictures are dropped undecoded; wanted again, decoding
  starts from a keyframe asked for (about a second's pictures skipped).
- How it's turned and held, from the device's own `deviceinfo` service (`getdisplayinfo`): the
  LCD's `currentOrientation` turns the picture; how it's held turns places, and lying flat
  (`faceUp`) the last way it was held (`currentDeviceNonFlatOrientation`) holds. Read at connect
  and every two seconds while the picture is wanted (asked at each tick: as the loop's condition
  it was read only when something else woke the loop, and turns went unseen); a failed read is
  logged and the service asked anew. Its point scale is the device's own (`pointScale`: 2 on an
  iPad). Measured on the iPad, turned four ways by `devicectl device orientation set` (the
  owner's word, for want of his hands): followed upright 0.15-2.0 s after each, and Control
  Centre opened from the top right corner in each.
- **It doesn't lock while its stream is held open** (PoC: unlocked through 8 and 12 minutes;
  locked in 5-6 with only a power assertion; M6: unlocked at 6 and 11.7 minutes held). That is the
  whole mechanism: nothing is set on the device. The tab says "While this tab is open, your iPad
  stays awake and its camera and microphone are off." **The stream is stopped when it's let go**
  (`stopmediastream`, and the host waits up to 7 s for its session to do it before it exits; a
  picture that won't start after the sound did is stopped too): left
  to the tunnel's end, the device kept the stream's session, and itself awake, long after the
  host was gone (M6: still lit 20 minutes after the tab closed, two sessions found standing).
  Stopped, the iPad's screen went dark by its own Auto-Lock (QR6). Its camera, meanwhile, showed a
  still and dark preview (U14, by numbers).
- Input through its universal HID service (up to five fingers) and Indigo (buttons, keys);
  places turned to the device's own upright axes by how it is held. A connection the device
  closed is made again and the act sent once more. **Never turned by Koyo** [decided]: the only
  way found leaves a simulated orientation on the device. The `lock` button is refused: an agent
  that locks it has locked itself out.
- Locked: the device's own lock notifications (`com.apple.springboard.lockstate`,
  `hasBlankedScreen`) say when it changes, watched by a task of their own; notices that come
  together are one change, and `devicectl device info lockState` (bounded, off the stream) says
  which way: the device's `getlockstate` answers "not implemented" on iPadOS 27.0.1, with or
  without its action. The tab says "Unlock
  your iPad to continue." and agents' acts are refused at once: one under way when it locks is
  cut short and those queued refused, and at a lock notice acts wait (5 s at most) until it's
  known which way it went, since a key sent to a locked device goes to its passcode field. Locked
  means its passcode is needed now **or its screen is dark**: just locked by its side button, a
  device whose passcode is asked only after a while says none is needed, and keys sent to it
  went on to the app behind (M7: the person locked it 5 s into a line of letters; the act was
  refused at 171 of 2000, and 171 reached the page). The notices are watched again a moment
  after they stop. Koyo never types, asks for or
  stores a passcode.
- Looking: a real device has no accessibility tree, so a snapshot is the text read off its picture.
  Its stream is encoded afresh, so a still screen's pictures differ a little: a point counts as
  changed past 20 in a colour, and up to 16 sampled points moving on their own (an iPad's Clock
  icon and clock widget, measured at 1 to 11 of 88,236) don't keep a wait from being still.
- A stream that stalls (keyframes asked for two seconds bring nothing decodable) or ends while
  the device is still plugged in is started again on the same tunnel: at once, then no sooner
  than a minute after the last, three times in all; then the tab says so with Start Again.
  Never a loop: sessions restarted often are reported to wedge the device's daemon.
- Unplugged mid-session: the host says it's gone ("{name} was unplugged. Plug it back in to
  continue.") and that stays its last word (it had told its standing over it, and the app took
  the quit for a crash); a stream that stalls, or won't start again, asks first whether the cable
  went. Not connected at the start, it's the setup's cable step. Plugged back in, it came back by
  itself 20 s after (M6).
- One remote view of a device at a time: Xcode or Device Hub taking it pauses Koyo's.
- **In the app** (`devices/connected.rs`): what's plugged in is listed by `devicectl list devices`
  off the main thread when a Devices surface first shows, iPhones and iPads by cable only, each
  with the first step it misses (one reading of that JSON for the host and the app,
  `koyo_device_wire::real`, whose `devicectl` call is bounded at 15 s and the host's too). From
  then a watcher, the host's program run as `--watch` (usbmuxd's plug events by cable; it ends
  when Koyo does), has it listed again at each plug, a burst as one, and once more 3 s later;
  should the watcher go (usbmuxd gone, as after a sleep), a look a minute on starts it again.
  `device_list` reads that list while the watcher keeps it. Nothing else polls but a device's
  setup, and only for the steps no plug says. A real device is no simulator: no limit, no share
  of the simulators running at once. A tab that ended for a step the list sees (unplugged, the
  cable, trust, Developer Mode, the system, the developer services) starts again by itself once a
  listing begun after it ended says the device is ready: three times at most until it stands
  ready or the person starts it, so a host that disagrees with the list is never started in a
  loop; what the list can't see (another app holding the device) waits for Start Again.
- Measured on an iPad Pro 12.9-inch on iPadOS 27.0.1 (`tests/real_phone.rs`): ready in 0.5-1.5 s,
  2736 x 2064 at 2x, 32.6 pictures a second on a still screen (none lost: the ring copied every
  one); a tap aimed at an icon opened it; the volume buttons, Home and five fingers; a wait on a
  still screen still in 0.5 s and an act on one answered in 0.4 s; on the touch-lab page in its
  Safari, a pinch to 2, 1.5 and 0.5 read 2.00, 1.50 and 0.50, a rotation 90.00, and typed ASCII
  arrived whole. The apps tools refuse on a real device in this build, pointing at `devicectl`.

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
  framework (`source: text`) [built], which is all a real device has.
- **Accessibility is on in a simulator only while its tree is read** [built]. An app has a tree
  only if `ApplicationAccessibilityEnabled` is on in the simulator's own settings, and every app
  started while it is on loads its accessibility bundles: Settings 109 of them, 4.7 s of CPU for
  a launch against 3.4. The simulator's service switches it on at its first start after a boot
  and nothing switches it off, across boots, so a simulator an agent had read once paid for it
  ever after. The host switches it on before a read and before an agent's launch (the setting
  written, two notices posted: an app that runs loads its bundles then; the first read 0.8-1.9
  s, the next 15-30 ms), off a minute after the last read, off as it goes, and off a minute
  after its start if an earlier host left it on. A simulator only the person uses never has it
  on. The first read of a new
  build of the host makes macOS compile its text model, 30-40 s: the host does it as it starts,
  while the simulator boots, and a snapshot asked before then is told to ask again. A `text`
  ref is where its words are: on the home screen that is an app's name, under its icon, where a
  tap doesn't open the app [measured, M5]; the guide sends agents to `device_launch` for apps.
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
  pixel, on the tab's own card (no root background of its own), with at least 48 points of room
  at its sides and above it [built, the owner's], and the bar's room under it.
- A simulator's picture has its model's own screen corners [built]. Xcode records them: each
  device type's `capabilities.plist` has `DeviceCornerRadius` in points (MobileGestalt's own
  key) and the same on its displays (`cornerRadiusUL`…): 62 for an iPhone 17 Pro and 18 Pro, 55
  for an iPhone 16, 47.33 for a 17e, 30 for an iPad Pro, 21.5 for an iPad mini, 0 for an iPhone
  SE. The host reads it from the simulator's device type bundle and says it in `Screen.corner`;
  the tab clips the frame to that radius times the fit (a rounded box round the surface, the
  renderer's own clip: no mask texture, no pass more). Xcode also ships each model's exact
  outline (`framebufferMask` in `profile.plist`, a PDF of the screen in pixels with Apple's
  continuous corners, which the Simulator app clips by); a circle of the recorded radius meets
  it at the diagonal to a tenth of a point (62.1 against 62), so the outline isn't parsed. A
  real device's corners aren't read yet: its picture stays square.
- One bar, the pane's own (28 points, a hairline under it) [built]: the device's glyph and name,
  as any pane's; then a dot and a word for how it stands (green Ready, amber Starting, red what
  needs doing, grey Not running); then icon buttons at its right, each named by its tooltip with
  its key: Home (⇧⌘H) and App Switcher, then Rotate Left (⌘←), Rotate Right (⌘→) and Lock for
  simulators; for a real device the sentence of 5, while its stream is held (ready or locked),
  and the lend's button. Dimmed until the device stands ready. In a split the same bar has the
  pane's grip, "Open in a tab of its own" and close round them; alone in its tab a device still
  has it, where other panes have none. A view of its own (`devices/bar.rs`), drawn again when
  the standing changes, not with each picture. It was a second bar under the pane's (40
  points): the owner's "a huge waste of space".
- The picker lists Koyo's simulators (or what they miss, with `Set Up…`), then under "Connected"
  the iPhones and iPads by cable, each with its system and the step it misses.
- A real device's states: locked ("Unlock your iPad to continue.", the lock screen under it),
  moving on by itself; unplugged ("{name} was unplugged. Plug it back in to continue."; the
  bar says Unplugged), moving on by itself, with Start Again should the plug go unheard; the
  cable, trust and Developer Mode, each in a sentence with
  `Set Up…`: the cable moves on by itself at the plug; trust and Developer Mode once a listing
  sees them done (the setup's, which looks every two seconds while open, or the next plug's);
  below 27 ("{name} runs iOS 26.6.1. Koyo drives
  devices on iOS and iPadOS 27 or later: update it in Settings → General → Software Update."),
  nothing to press; another app holding it, or its developer services not up, with Start Again.
- The control bar is the browser's own pill (`held::bar`). An agent's touches show as marks.
- After a restart a tab comes back as "{name} isn't running." with `Start`: nothing starts by
  itself. A device no longer in Koyo's set says "{name} is no longer among Koyo's devices." with
  `Choose Another…`, which turns the tab into the picker. Closing the last tab of a device shuts
  its simulator down or lets the device go; two tabs of one device share its host.
- The launcher's **Device** kind lists the device tabs open, then Koyo's simulators no tab shows,
  then the iPhones and iPads plugged in no tab shows ("Needs setup" beside one that misses a
  step), filtered by what's typed; Enter with nothing picked opens a tab on the picker
  (`docs/new-tab.md`).
- The device counts as drawn while its picture is painted in a window (a keyed state of the
  picture's element): only then, or while an agent works on it, does the host copy frames. A
  hidden tab costs no copies.
- Every state has one way out or moves on by itself: no device chosen (the picker), not running,
  starting (the app's loading wheel over "Starting {name}", or "Getting input ready", and on a
  first boot "The first start takes about a minute." under it), ready, locked, unplugged, needs
  setup, below iOS 27, failed.
- Stopped (its tab closed, Settings' Stop, `device_close`, Koyo quitting), a device is
  stopping until its host has shut the simulator down and closed; it still counts against the
  limit until then, and the set is listed again once it's down.
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
  A call naming no device never finds a real one.
- `device_list` lists Koyo's simulators and the iPhones and iPads plugged in (`kind: device`, no
  `set`; `needsPerson` with the step a device misses until its tab says more).
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
  `user_declined` and raises no new sheet for that agent and device for ten minutes (the control
  socket's memory of a refusal). One sheet a device at a time; it stays until answered (a sheet
  can't be taken down), and a call meanwhile is told so, another agent's that the person is
  being asked for someone else. What a real device refuses anyway (its app tools, turning,
  locking) is refused before the person is asked. The decision goes to the activity log
  (`device_lend`). The lend is kept in the app by device and agent, an agent being its caller's
  key and the name it goes by together (`devices/lend.rs`), so a host that stopped and was started
  again by itself keeps it (one stopped, or one that gave up and was started afresh, doesn't); it
  ends when the device is stopped (its tab closed, `device_close`, Koyo quitting) or leaves the
  list of what's plugged in, and at the tab's `Take It Back`, which also calls off what the
  agent had under way; its next call asks the person again. The tab shows "{name} is
  yours, lent to {agent} until you take it back." with `Take It Back`, in the agent desktop's
  own notice for an app it lends (`held::notice`), over the top of the picture, which doesn't
  move for it; the person can lend it
  ahead from the bar ("Let Agents Use This iPad"), to every agent. Lent, `device_open` opens
  and starts it as a simulator (no limit). The app tools (`device_apps`, `device_launch`, …) are
  refused on it, pointing at `devicectl`; the gate is `control/exec/device/lend.rs`.
- Nothing unverified is shipped as working: a tool path that couldn't be shown to work on the real
  thing refuses with what to use instead.

## 10. Setup and Settings → Devices [decided]

**The setup for simulators** is one dialog (`devices/setup.rs`, its words in `setup/words.rs`),
built as the agent desktop's: numbered rows, each ticked the moment its state is there, and what
to do at the step in hand under them. What the Mac has is read off the main thread
(`devices/needs.rs`), with Koyo's set, whenever a Devices surface first shows.

| Step | How Koyo knows | What it says and does |
| --- | --- | --- |
| Xcode 27 or later, the one in use | `DEVELOPER_DIR`, else `xcode-select -p`; each Xcode in `/Applications` by its `Contents/version.plist` | Missing: install it, with `Open the App Store`. Older: "You have Xcode {version}", the same. Another in use (or the Command Line Tools): Xcode → Settings → Locations → Command Line Tools in the newest. `Check Again` |
| Xcode's components | `xcodebuild -checkFirstLaunchStatus` exits 0 (R6) [measured: 0 here, with them installed; the other answer read, not seen] | Continue makes the first `simctl` call, which installs them; if they're still missing, its last line and "Open Xcode once" |
| An iOS system | `simctl runtime list -j`: the ordinary iOS systems ready, newest first, with sizes | Xcode → Settings → Components, and `Check Again`. No download from Koyo: approval C wasn't given, so nothing of it is built |
| A device | Koyo's set | The newest iPhone on the newest system (iPhone 18 Pro on iOS 27.2), or a model chosen from that system's list; Continue makes it, then opens and starts it in a tab |
| First start | the device's link | "Starting {name} for the first time … about a minute", with the time so far; then ready, and Koyo comes forward |

- Nothing runs before Continue. `simctl` isn't asked for the list until Xcode and its components
  are there: its first call would install them, and may ask for the person's password. Nor is
  an agent's `device_list` or `device_open` that first call: while Koyo doesn't know Xcode and
  its components are there it looks first, and with either missing the tool is refused with
  `device_needs_person` and the same sentence.
- What the Mac has is read again only while it isn't known or something was missing; a list
  after that is `simctl`'s alone. While the setup waits for a system, only the systems are read.
- While the dialog is open with Koyo in front, it looks again every two seconds and the moment
  Koyo comes forward; never behind it.
- It comes up by itself once per run, from the picker or from a tab the person opened that can't
  start for want of Xcode; never from a tab an agent opened, never because a tool failed, and only
  while Koyo's window is active. `Not Now` doesn't bring it back. The picker and such a tab say the
  one thing missing in a sentence, with `Set Up…`.
- Walked for real off screen from a set that held nothing (`--visual-check-devices` with
  `KOYO_VISUAL_SETUP`): the Mac read in 0.6-1.1 s, only a device missing; Continue made the iPhone
  18 Pro in 0.6 s (one listing of the newest system and its models, then `simctl create`); it
  stood ready 49-60 s later; the person's first tap, on Settings where the
  accessibility tree placed it, opened it.

**The setup for a real device** (`devices/setup/device.rs`): its own dialog, opened by `Set Up…` in
a device's tab or Settings → Devices, for that device or the first plugged in that misses a step.
Five rows, each ticked the moment `devicectl` lists it so: connected by cable; trusted ("Unlock
it, tap Trust and enter its passcode"); Developer Mode (Settings → Privacy & Security → Developer
Mode; what it is, that it lowers the device's guard, the restart and Turn On, and that the switch
shows only once a Mac with Xcode has seen the device); iOS or iPadOS 27 or later (Settings →
General → Software Update); its developer services (nothing to do: they start once it's
unlocked). It lists again at each plug, and at the steps no plug says (trust, Developer Mode, the
services) every two seconds while open with Koyo in front and at once when Koyo comes forward;
`Check Again` for the impatient. Koyo changes no setting on the
device: each step is the person's. A picture of each Settings screen beside its step isn't there:
it would be a picture of a device.

**Settings → Devices** (`settings_view/devices.rs`; `settings_open` with `devices`):

- Requirements, each row marked at its right [built]: a green tick and a word once it's there
  ("Installed"), an amber warning, a word ("Update needed", "Not selected", "Not installed",
  "Setup unfinished") and `Set Up…` while it isn't. Xcode with its version; iOS versions, the
  ones installed with their sizes. No `Delete` for a version: they're Xcode's, and the row says
  to add or remove them in Xcode → Settings → Components.
- iPhones and iPads plugged in, each marked the same way: "Ready", or "Needs setup" with the
  step it misses and `Set Up…`; none plugged in, how to plug one in, with `Set Up…`.
- Koyo's simulators, each with its system and whether it runs: `Stop` while it does (through its
  host, else as `simctl shutdown`), "Stopping…" until it's down, `Delete` while it doesn't,
  asked first. `New…` lists the newest system's models and makes one.
- Simulators running at once: 1 to 4, 2 unless the person changes it, with what runs now. An
  agent's `device_open` past it is refused naming what runs; the person's own Start past it (the
  tab's Start, a pick in the picker, the launcher) asks "Start {name} too?" with `Start Anyway`.
  One rule (`devices::past_limit`) over one count: what Koyo's set has booted and what Koyo holds,
  starting, running or still shutting down. Opening a tab starts nothing: whoever opens it starts
  it, the person through the question. No figure for memory: none was measured well enough to
  show.
- No section of what Koyo doesn't touch [decided, the owner's]: rows that change nothing read
  as options. Each of those facts is said where it applies: Delete's question says Xcode's
  simulators stay as they are, and a real device's bar says it stays awake only while its tab
  is open, its camera and microphone off.
- Nothing here is reachable by `settings_set`.

Measured off screen at the end of the walk: the page showed the device running; its `Stop` had
it down and the page saying so in 3.6 s (once its host had shut it down and closed); `Delete`
took it out of the set in 0.3 s.

## 11. Milestones

| | What | State |
| --- | --- | --- |
| M0 | Premises re-measured with the PoC's hosts alone; the QA harness; this document | done |
| M1 | The simulator's host, headless, and its real-simulator suite | done: every row in debug and release |
| M2 | The tab | done: the hand's and the states' tests, the off-screen pictures with two real simulators, QH1-QH4; the window at 60 and 120 Hz waits on the owner (QP8) |
| M3 | Agents on a simulator | done: QT1-QT8 and QL1-QL3 through the real MCP client |
| M4 | The simulator half reviewed and hardened | done: the review's findings fixed, the rows re-run |
| M5 | Setup and Settings | the setup for simulators, the picker's and the tab's missing states, Settings → Devices and the limit: off screen; the walk through the release bundle in a window waits on the owner |
| M6 | The real device | done: on the iPad QR1-QR6, QR12, QR13 and U14 (QR4 by `devicectl`, QR6 with Auto-Lock set short, by the owner's word); in the app: listed, its states, its setup, Settings' list, the launcher, the lend by the person's sheet (off screen and through the pipeline's tests). In M7, with the owner's hands, in release: QR7 (his lock stops an agent's typing), QR8 ("was unplugged", back by itself in 9 s), QR9 (his clicks on the sheet: Don't Allow, Allow, Take It Back). Not run: QR10 (the iOS 26 iPhone isn't plugged in), QR14 (no iPhone on iOS 27 here) |
| M7 | Finish | done: `script/check` green but for the endpoint socket tests (this checkout's path); the second review's findings fixed (an agent's act stopped by a lock among them); the simulator rows in debug and release (R1's 300 ms and QI14 failing at a load average of 50-110 from other work; R1 on the iPad simulator finding Settings on another page); the iPad's rows in release all PASS |

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

**A real device** (iPad Pro 12.9-inch, iPadOS 27.0.1, by cable; M6):

| What | PoC | Built (M6) |
| --- | --- | --- |
| Ready from the host's start; through the app's tab | 1-2 s | 0.5-1.5 s; 0.3-0.6 s |
| Picture | 2736 x 2064 HEVC | 2736 x 2064 at 2x, upright by the LCD's own turn |
| Pictures a second, a still screen | about 30 | 32.6-33.4 (none lost); 26-28 drawn by the off-screen tab |
| A turn followed (four ways, by `devicectl`) | | 0.15-2.0 s, upright; Control Centre from the corner in each |
| Pinch to 2, 1.5, 0.5; rotation 90°; typed ASCII | | 2.00, 1.50, 0.50; 90.00; whole |
| A wait on a still screen; an act answered there | | still in 0.5 s; 0.4 s |
| Pictures back after a moment of the stream lost | | 0.95 s, by a keyframe |
| Awake with the tab open and hidden, nothing sent | 8 and 12 minutes | 12 minutes |
| Dark by its own Auto-Lock once the stream is let go | 2 min 20 s | within 210 s (Auto-Lock 2 minutes), no session left |
| Back after an unplug and a replug | | 20 s after the replug |
| Its camera while viewed | off [read] | a still, dark preview (0 ± 0 of 255) |

M0's numbers were taken on macOS 27.2, Xcode 27.2 (27B52), CoreSimulator 1174.21, iOS 27.2
(24B8117), with the PoC's `simhost` alone and no window.

What an app's start costs in a simulator (2026-11-12, an M4 Max; Settings and Safari started
cold by `simctl launch` five and four times, the median; busy is until the app's own process
rests, CPU its own; `.superpowers/devices/probes/launchcost`):

| Simulator | Settings: busy, CPU | Safari: busy, CPU |
| --- | --- | --- |
| iOS 26.5, alone (the PoC's system) | 2.0 s, 2.6 s | 1.0 s, 1.1 s |
| iOS 27.2 iPhone 18 Pro, alone | 3.2 s, 3.7 s | 2.9 s, 2.5 s |
| the same with Koyo's host attached | 3.0 s, 3.4 s | 2.7 s, 2.3 s |
| the same with accessibility left on (before the fix) | 3.6 s, 4.7 s | |

The host costs a start nothing. Through it on iOS 27.2 a page animates at 60.0 frames a second
at 1.9% of a core, as in the PoC.

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
- iOS 27.2's simulator starts apps slower than iOS 26.5's, with or without Koyo: its loader
  (`dyld_sim`, in the runtime) takes 1.4 s of Settings' 3.2 to load the app's dependencies,
  where 26.5's takes under 0.3 s. Both find their libraries in a shared cache and both make
  their loaders anew at each start (`DYLD_PRINT_LOADERS`: 461 and 494 for `defaults`); 27.2's
  does it slowly (`JustInTimeLoader::matchesPath` parsing each image's header again). Nothing
  on the Mac switches it: `xcrun simctl runtime dyld_shared_cache update` says it is obsolete
  for 27.2, whose runtime has its cache prebuilt, builds nothing, and leaves an empty folder
  that the next boots take for the cache (tried with the owner's yes, and removed with
  `... remove` at once). A simulator of iOS 26.5 is the faster one, if speed matters more
  than the newest system.
- Pinch, rotation and typed text on a real device.
