# Koyo agent desktop: computer use on a desktop of the person's own

Written 2026-11-09 on branch `schulzfel/agent-desktop`, built and checked on a real Mac
(macOS 27.2) on 2026-11-10. Author of the vision: Felipe Schulz. Drafted with Claude. Source of
truth for the agent desktop; update it when a decision changes.

Koyo's agents drive real Mac apps (click, drag, scroll, type, pinch) on a desktop of their own:
one extra macOS desktop (a Space) in the person's own login session, which the person isn't
looking at, shown live in one Koyo tab that the person watches and can take over. Agents use the
person's installed apps and files. Nothing an agent does moves the person's pointer, takes their
keyboard focus, changes the desktop they're on, or draws a window there.

Status marks: `[decided]`, `[built]` (in the tree and checked on a real Mac), `[proposed]`
(designed here, not built), `[open]`.

How it got here. Two designs were built or measured first and dropped; their notes stay out of the
repository in `.superpowers/agent-desktop-research/` (`FINDINGS.md`, `PRIOR-ART.md`,
`INSESSION-27.md`, `VM-SPIKE.md`) and `.superpowers/agent-desktop/investigation/apple-rules.md`:

- **A hidden account logged in through Screen Sharing** (the first design, built to its first
  milestone; commits `3603f0c67`..`ac3075fbb`). On macOS 27.2 every such login makes the system
  take the agent for the console user: the person's AirPods are unpaired, AirDrop and Handoff
  stop, a system audio daemon spins once the session ends, and this Mac froze twice. Not fixable
  from outside Apple.
- **A macOS guest under Virtualization.framework.** Isolated by construction and set up by
  itself on macOS 27, but its memory is reserved up front (a 12 GB guest costs 12 GB at once).

---

## 1. What the person gets [built]

- Agents (Claude in a Koyo chat or terminal, any MCP client of Koyo's) open apps, look at them
  and act in them, on the agent desktop. The person keeps working on their own desktops.
- One tab, "Agent desktop", shows that desktop live: its wallpaper, every window on it at its
  place, the agent's pointer drawn where it acts. Clicking or typing in the tab takes over: the
  person's input goes to the window under it, and a bar at the tab's bottom says who has the
  desktop, the same bar a browser page has.
- The agent's apps are instances of their own: not in the Dock, not in ⌘-Tab.
- It needs two privacy grants, given once to Koyo's agent desktop host: Accessibility and Screen
  Recording. No account, no administrator, nothing installed outside Koyo.

## 2. What reaches the person, and what doesn't [built]

Each line is a rule the host keeps, checked on macOS 27.2 (13 has the numbers).

| The person's | What happens | Kept by |
| --- | --- | --- |
| Pointer, keyboard focus, front app | Unchanged by every act, but for the keyboard while a menu is up (next line) | Events are posted to one process (`CGEventPostToPid`) with the target window stamped in; never at the HID or session tap. The app is told it's active by an AppKit event of its own, and that the window is its key window by the pair of records the window server itself sends (`SLPSPostEventRecordTo`); neither changes what the window server has in front. |
| Keyboard, while a menu is up in an agent's app | Goes to that menu: a right-click's, a pop-up button's. The host closes the menu at the person's first key press, which is lost to it; what they type next is theirs. One left up for a minute is closed too | macOS sends every key to a menu that is open, whichever app is in front (its "key thief"), and a menu that loses the keyboard closes: none can be kept up without it. The host watches for a menu of the agent's apps and, on a key of the keyboard's own since it opened, hands the keyboard to the person's front app (`keyboard.rs`). A menu the person opened in the tab is left alone. `[open]` no menu up at all: read, closed at once and shown by Koyo, an item pressed through accessibility (15). |
| Desktop they're on | No agent window is drawn there | An agent's app is started hidden, assigned to the agent desktop, then shown; a window made later is made there. Sampled every 0.2-0.5 ms while six apps opened: never seen. |
| Front app, when an agent's app comes to the front (by itself, or brought forward by macOS) | Back within ~5 ms. Their desktop stays on their screen, but for a third of a second now and then when an agent's app starts (Chrome: two starts of 36) | A thread of the host's own asks the window server which app is in front, every 8 ms and every millisecond for 1.5 s after an agent started or acted on an app, and sets the front there itself (`_SLPSSetFrontProcessWithOptions`, no window raised) to an app with a window on the desktop the person is looking at: the one they had, when it has one there, else the one whose window is frontmost there, else the Finder, as on an empty desktop (`front.rs`). An agent's app in front after an answer is answered again at every look, for 3 s, and four times a second after that: an app takes the front back within a look of losing it (Chrome at its start, 17 ms after), and left for 250 ms as it once was, it kept it that long. Measured over thirteen starts of Chrome: the person's app in front again 2-10 ms, most often 4, after each of 39 times Chrome took it. That is all a guard can do, and it doesn't keep the person's desktop where it is. macOS gives the front to any app with a window that asks for it (asked by an app of our own with a window of 4 points: given while it launched, 1.5 s after, while hidden, started as background-only, and with Launch Services told it isn't allowed to become frontmost; refused only with no window at all). And an app that orders a window forward in that moment itself asks for the desktop the window is on, which the Dock shows about a tenth of a second later, whoever is in front by then; its own code skips that only while Mission Control is up or another switch runs. So Chrome, which takes the front three or four times as it starts, took the person to the agent's desktop with every one of those answered in 5-10 ms (2026-11-11, one start of three), as it had the day before with one answered 240 ms late (one start of about twelve), when nothing brought them back for 50 s. What holds is the way back: on the agent's desktop within 700 ms of the front being given back (it took 354-363 ms, three times of three), the person was taken there, and the host puts the desktop they were on back on their screen by its button in Mission Control, again every 200 ms until it is there. Seen six times: back 92-144 ms after they arrived, a third of a second of sliding there and back in all. How often macOS follows goes with how long the agent's app has the front: with a look every 8 ms Chrome was followed at 3 starts of 16, with one every millisecond at 2 of 36 (too few to say more than fewer). Their app brought forward with its windows, as it was before, wasn't enough: the Dock dropped it while its own switch still ran, and it does nothing for an app that is in front already. `[open]` A person who goes to the agent desktop themselves within those 700 ms is taken back too: nothing the host can see tells the two apart (the window server tells the Dock alone of the request: an app of our own that asked for the same notice got none while the Dock switched). macOS's own "When switching to an application, switch to a Space with open windows for the application" (Desktop & Dock) doesn't rule the following out: with it turned off by the person (it sets `AppleSpacesSwitchOnActivate` to 0 for every app), the Dock followed Chrome all the same, at one start of 23. The Dock has a setting of its own for it, `workspaces-auto-swoosh`, which that switch leaves alone: read once, as the Dock starts (`-[Spaces activate]` registers for the notice unless it is off, the one place in the Dock that does). `[open]` Neither tried with the Dock started again or after a new login. Not on the agent desktop itself: there, an agent's app in front is the person's doing. |
| An app they open while the agent runs its only instance | Nothing shows on their desktop; the Agent desktop tab says the one running is the agent's and offers Open Yours | Launch Services activates the agent's hidden instance: the host gives the front back, and tells Koyo of an activation no act of the agent's explains (1.5 s). An instance for the person is started only when they press the button, never on a guess. |
| An app of their own that they run, when an agent asks for it | Stays theirs, and where it is, until they tell the agent yes in so many words. Then every window of it is on the agent desktop until it's given back: by the agent, or by Take It Back in the tab, which brings it back to the desktop it came from. Its coming to the front (its Dock icon, ⌘-Tab, a link, the app itself) doesn't bring it back: the front returns to their own app. It's never quit for them | The host refuses first and tells the agent to stop, say what it wants and ask (5, lending). Their yes counts once. A lent app is on record, so a host that crashed is followed by one that gives it back. |
| The desktop a lent app left | Theirs to go to as before: with nothing else on it, it shows the Finder in front, as an empty desktop does | The Dock keeps each desktop's front app and brings it forward as the person arrives; brought forward, the lent app shows its window, and the Dock follows the window to the desktop it's on. That took the person to the agent desktop each time they went to the desktop their Arc had left (2026-11-10). Telling the window server another front app for that desktop (`SLSSpaceSetFrontPSN`, which the Dock itself calls) changed nothing. What holds is the line above: the front taken from the app again within ~20 ms, here given to the Finder (four arrivals of four stayed; the one answered a second late didn't). |
| Dock and ⌘-Tab | Agents' apps aren't listed | Launched as UI elements (Launch Services' own option). |
| Clipboard | Theirs, but for the moment between an agent's ⌘C or ⌘X and the host putting theirs back | `desktop_key` keeps every item, presses, reads what was copied for the agent's answer, and writes theirs back (a file promise among them isn't called for, and so isn't put back). `[open]` a menu's Copy pressed by an agent isn't guarded; nor is a copy that takes an app longer than half a second; and a copy of the person's own made in that same moment is lost to the put-back. |
| Displays, Arrange Displays | Unchanged | No virtual display is made. |
| Mission Control | One more desktop in the strip while the agent desktop is on; Mission Control shows for ~150 ms when Koyo makes it and when it removes it | Made when the host starts and kept; removed when Koyo quits or the person turns it off. With Reduce Motion on, Mission Control fades, and the Dock watches that setting as it changes; but the host can't hold it on for those moments: macOS refuses the write (`com.apple.universalaccess`) to a process with the host's two grants, and takes it only from one with Full Disk Access. `[open]` The desktop kept between runs of Koyo, so Mission Control shows at neither end. |
| Apps they have open | Never driven | Only processes the host launched are acted on. |
| Unsaved documents | `[open]` An agent's instance of a document app opens with the person's unsaved Untitled documents, and can change or delete them | The app reopens them at every launch, whoever launches it. The launch is "fresh" (as `open -F`), so nothing else of their last session is restored. |
| What an app remembers | `[open]` Shared with the agent's instance: Open Recent, the folder a panel opens in, preferences, what the app keeps between launches (Calculator's last sum) | It's the same app run by the same person. |

What an agent can reach: everything the person can. It runs as them, with their files and their
signed-in apps (its Safari is their Safari), so the `desktop` scope (10) is in effect every other
scope too. Koyo itself is the one app it can't open. `[open]` a per-app allow list.

## 3. Architecture [built]

```
 Koyo app                                          Koyo Agent Desktop (the host)
 ┌────────────────────────────────┐  link socket    ┌──────────────────────────────────────────┐
 │ agent_desktop                  │ ◀─────────────▶ │ the agent desktop: made at start, removed │
 │  starts the host, the link,    │  CBOR frames    │   at quit (Mission Control's own buttons) │
 │  the desktop tab (surfaces,    │                 │ apps: launched hidden as UI elements,     │
 │  pointer, taking over),        │  mach messages  │   assigned to the desktop                 │
 │  the tools' executors,         │ ◀────────────── │ windows: listed, one ScreenCaptureKit     │
 │  Settings → Agent desktop      │  one IOSurface  │   stream each                             │
 └────────────────────────────────┘  per frame      │ input, screenshots, accessibility         │
                                                    └──────────────────────────────────────────┘
```

- `koyo-rs/koyo-desk-wire`: what both sides agree on, no Apple frameworks: the link's messages
  and frames, shapes, key names. Tested by itself.
- `koyo-rs/koyo-desk-host`: the host, a library loaded by a small loader app, `Koyo Agent
  Desktop.app` (bundle id `dev.koyo.agent-desktop.host`, `LSUIElement`; `loader/main.c`, packaged
  by `script/desk-host`). The loader never changes, so the privacy grants given to it survive
  every new build of the host: an ad-hoc signature's grant is tied to the binary's hash, and the
  library lies beside the loader's bundle, outside its signature. It loads only that library.
  `[open]` On a build signed ad hoc that also means whoever can write the folder can run code
  with the loader's grants; a release signs both with a Developer ID and the hardened runtime,
  and should have the loader check the library's signature.
- The loader runs standing by itself. In Koyo's bundle it lies in `Contents/Frameworks`, and
  from there macOS files what it asks for under the app it's inside: the host's Screen
  Recording came up as Koyo's, and where another build of Koyo already had an answer on record
  (a different signature) it was refused without a prompt, with no entry to switch on. So
  before starting it, Koyo copies the loader and the library out to `desk/host/` in its data
  folder (the loader once, the library whenever it differs) and starts the copy. One grant
  then holds for every build: the copy `script/dev` runs and the bundle's are the same binary
  (the same signature hash).
- `koyo-rs/koyo/src/agent_desktop/`: the app's side.

The host runs as the person, in their session. Why a process of its own: the grants. It's started
through Launch Services (`open`), not as a child, so macOS holds it, not Koyo, responsible for
them; Koyo itself asks for neither. It goes when Koyo says quit, and on its own when Koyo just
goes (the link closes): either way it closes the agent's apps and removes the desktop first.
One host at a time works on a state folder (a lock, waited for 15 s and then given up on), so a
Koyo started again at once waits for the last one's host to finish.

## 4. The agent desktop [built]

- A desktop is made only by Mission Control. The host opens Mission Control (by launching
  `Mission Control.app`, not activated), presses its add button through accessibility and closes
  it: 145 ms as built. On macOS 27 its accessibility tree hangs from WindowManager (`mc.display`
  → `mc.spaces` → `mc.spaces.add`, `mc.spaces.list`), on earlier systems from the Dock. The window
  server's own calls make only a private space, which no other app's windows can be put on.
- Made when the host starts, kept, removed when it goes (the desktop's own `AXRemoveDesktop`
  action, after the agent's apps are closed). A desktop is pressed by its place in the strip of
  its own display, read with Mission Control up right before the press, and only when the strip
  matches the window server's list. With Mission Control already open (the person is in it) the
  host doesn't touch it: it says so and tries again.
- The host records the desktop (id and name) and the apps it started (pid and start time). A run
  that crashed is cleaned up by the next, once, as its host starts: its apps ended if they are
  still the same processes, its desktop removed if no app keeps a window on it alone (every
  desktop also holds the menu bar, the wallpaper and all-desktop windows, which don't count).
- Under a locked screen Mission Control takes nothing, so the host tries nothing: a desktop
  is neither made nor removed. One that was to be made is made once the screen is unlocked
  (looked for every 2 s; until then the tab says the screen is locked). One that was to be
  removed stays, and on record, for the next host: the record holds every desktop that is
  the host's or was an earlier one's, a line each, and one leaves it only when it's gone. A
  host that started under a locked screen forgot the desktop of the one before it, which
  then stayed among the person's (2026-11-11). `[open]` Not run under a real lock since:
  the rules are unit-tested, the lock itself only read.
- The person can remove the agent desktop in Mission Control: its windows land on theirs, and
  the host makes another and puts the agent's apps, still running, there.
- One desktop for all agents, on the main display; its size and scale are that display's, and
  everything the app is told counts from its top left. `[open]` several displays: written for
  (a strip a display), not checked with more than one.

## 5. Apps on it [built]

- `desktop_open` starts an instance of its own (`NSWorkspace`: a new instance, not activated,
  hidden, as a UI element through `_kLSOpenOptionUIElementLaunchKey`, and fresh through the open
  event's `rsto` parameter, which is what `open -F` sends), assigns its process to the agent
  desktop (`SLSProcessAssignToSpace`, what the Dock's "Assign To: This Desktop" does) and shows
  it. An app that hides itself again as it finishes starting is shown again; one that shows no
  window is told it's active after 0.8 s and opened once more through Launch Services after
  1.8 s (Calculator shows its window only then).
- An app is found by path, bundle id, or name: in the Applications folders, CoreServices, then
  wherever Launch Services knows it. Koyo itself (`dev.koyo.*`) is refused: a second Koyo on the
  person's data could grant an agent what the person didn't. A file is a full path or a URL.
- An app assigns whole, and some hand a second launch to the instance that runs. Chromium and
  Electron apps (told by their frameworks) get a data folder of their own under the host's state
  folder, so the agent's instance is apart from the person's and never hands over (a
  `--user-data-dir` an agent passes is dropped). Finder, System Settings, Arc and Firefox take
  no second instance: while the person runs one, theirs is all there is, and it's lent or
  refused (next). So is an instance Launch Services answers with that isn't one the host
  started.
- Lending: the person's own running instance comes to the agent desktop only on their word
  (`consent.rs`, shared with taking the focus below), the way Prisma's CLI guards a database
  reset from an agent. Asked for one (an app of the line above, or any app with
  `personsOwn`), the host refuses (`person_has_control`) with a
  message that tells the agent to stop; to tell the person what it wants to bring over, why,
  and what it will do there, and that the app's windows leave their desktop meanwhile; to
  ask; that an unclear answer isn't consent and nothing said before the refusal counts; and
  to call again with `consent` set to the exact text of the message in which they agreed.
  Words count once and only after a refusal: asked for again after it went back, the app is
  refused again, and the refusal says the earlier words are used up.
  With those words the app is lent (`lending.rs`): assigned to the agent desktop as it runs,
  every window of it, listed among the agent's apps so that acts and the guard of the front
  treat it alike, and the words go to the host's log. The desktop that held most of its
  windows is kept as its home. Should its windows still be on a
  desktop of the person's (an assignment made in the first tenths of a second of an app's
  life answers yes and doesn't hold), it's assigned again, and after 3 s given back and the
  agent told. What an agent opens in it goes to the instance as it runs, with nothing
  hidden and no arguments: those are for an instance that starts. The host can't tell the person's words
  from an agent's: what guards is the instruction, the log and the tab's notice (9).
- A lent app is given back, never quit: by `desktop_close` with its pid, by Take It Back in
  the tab (in front of the person), at the host's end, and by the next host after one that
  crashed (the record of apps marks it `lent`, with its home). Not by its coming to the
  front: macOS and the app bring it forward for reasons of their own (the desktop it left,
  a page it was asked for, a click in Mission Control), and a lent app that went back each
  time was of no use; the front is given to the person's app instead (2). Giving back
  assigns it to its home (the desktop the person is on, if that one is gone) and then to
  none, as the Dock's Assign To: None does (the Dock's own code calls
  `CGSProcessAssignToSpace` with 0 for an app without a binding): its windows all land on
  that one desktop, wherever they were, and what it opens later opens where the person is.
  With it goes what an agent held of it: the last snapshot's elements, text on its way to
  it, a window held by its bar.
- An app's pages on a desktop that isn't showing. AppKit calls every window on the agent
  desktop not visible while that desktop isn't on screen, and a window that starts there
  is never told of a change (a window of our own logged it: not visible from its first
  moment, no notice after, its display link at 120 a second all the same). What the host
  starts draws regardless: AppKit's own apps; an Electron app as Electron makes one by
  default (its page called itself visible, ran at 126 frames a second, was read and
  clicked) as well as VS Code, which turns throttling off; and a Chromium browser of the
  agent's own (Chrome for Testing 152: visible, 60 frames a second, drawn). The person's
  own Arc, lent, did not: its own chrome was drawn, the page's place stayed one flat
  colour for as long as it was looked at (40 s, 76 s), the tree held the tab's name and
  nothing of the page, and nothing asked of it through accessibility changed that
  (`AXManualAccessibility`, `AXEnhancedUserInterface`, an observer). The page was drawn,
  and read, from the moment the person looked at the agent desktop itself. `[open]` Why
  Arc and not Chrome: most likely Chromium stops a page when its window is told it is no
  longer visible, which Arc's was on the person's desktop before it was lent and a window
  that starts on the agent desktop never is. Not verified, and with it whether an agent's
  own browser stops once the agent desktop has been on screen and left again.
- An app's pages in its tree. Chromium builds a page's accessibility tree only when
  asked. An Electron app is asked by an attribute of its own (`AXManualAccessibility`); a
  Chromium browser hasn't that one and builds them for a screen reader, so the host sets
  what a screen reader sets (`AXEnhancedUserInterface`) on a Chromium app that refuses
  the first (`app.rs`). Chrome for Testing had its toolbar in the tree and nothing of the
  page before; with it, the page was in `desktop_snapshot` 3 s after the open was asked,
  and its button took a click, all unseen. Chrome answers that it took neither. No other
  app is set it: it changes how an app behaves. A lent app has it turned off again when
  it goes back, unless it was on when it came, as for a person who uses a screen reader
  (not seen run here: this Mac's only browser, Arc, takes the first attribute). `[open]` A
  host that crashed leaves it on in a lent app until that app quits.
- Taking the focus (`showing.rs`): for an app that won't work unseen, an agent may put the
  agent desktop on the person's screen, its window in front, in place of what they are
  doing. It's the one thing here that changes what the person looks at, so it's theirs to
  allow each time, asked as lending is: `desktop_take_focus` refuses (`person_has_control`)
  with what to tell them (that their screen goes to the agent desktop, why nothing less
  will do, for how long, and that their desktop comes back) and takes only the exact text
  in which they agreed, once. The desktops are switched by Mission Control's own buttons,
  pressed through accessibility (`mission::go_to`): no key or pointer event, and the
  desktop meant, whatever macOS is set to do when an app comes forward. The guard of the
  front stands down for the switch, and on the agent desktop an agent's app in front is
  left there (2). The focus goes back with `desktop_return_focus`, when a lent app it was
  taken for is closed, and at the host's end: to the desktop the person was on. Going to a
  desktop of their own takes it back at once. Never lent: the Finder, which holds the
  person's desktop itself; Koyo; and an app with a window in full screen, which is a space
  of its own that an assignment doesn't move (the agent is told to ask the person to take it
  out of full screen).
- Asked again for an app it runs, the answer is that instance (shown, and asked for a window, if
  it had none); a file goes to it when the person doesn't run the app too. An instance that was
  just asked to quit isn't opened in: a new one starts.
- A window made off the desktop is moved onto it. An app that makes itself the front app is
  answered by giving the front back (2).
- When an agent closes one, and at quit, the app is asked to quit, then stopped after 3 s: only
  processes the host launched. macOS ends an idle app with no window by itself; it's then gone
  from the list.

## 6. The picture [built]

- A display-wide capture delivers nothing for a desktop that isn't showing; a capture of one
  window does. So each window on the agent desktop (`SLSCopyWindowsWithOptionsAndTags`, in the
  window server's order) has a ScreenCaptureKit stream of its own, at the display's scale, 60
  frames a second at most.
- The desktop's own picture is shown behind the windows: the wallpaper window macOS keeps on
  the agent desktop (WindowManager's, far below every app's, the size of the display), listed
  last and captured at a pixel a point, two frames a second. A desktop with no app on it shows
  as itself. The menu bar's window comes out blank, and isn't shown.
- Frames are asked for in sRGB (`colorSpaceName`), which is what the app draws them as. Left
  alone they come in the display's own colours, Display P3 on a MacBook's screen, and shown as
  sRGB those look washed out (a wallpaper yellow of 245/197/32 came out 238/198/76).
- Each window is captured alone (`includeChildWindows` off). Left on, a window with a sheet and
  the sheet itself both come as the two together, scaled into whichever one's size, and a
  screenshot of either isn't made at all (`SCStreamError` -3811).
- No codec. A frame's `IOSurface` goes to the app as a mach port (one message a frame, on the
  channel the browser's pages use, `koyo-browser-ipc`'s `mach`), and the app draws it as it is at
  the window's place on a canvas the shape of the desktop, back to front as the host lists them.
- A window that isn't showing is delivered whole about 57 times a second whether or not it
  changed. The host compares each frame with the last (every second point's row) and drops equal
  ones, so a still desktop costs the app nothing; a still window's frame is sent again every 2 s,
  in case one was lost.
- Every window is captured for as long as it's there: 60 frames a second at most while a tab
  draws the desktop or a `desktop_wait` watches, two a second otherwise, and then nothing is sent
  to the app; the frame that came last is sent once somebody looks again (a window that stays as
  it is, as a wallpaper does, sends no other). A window being resized keeps its stream, its
  frames stretched to it, until its size has stood 150 ms. A window on a desktop that isn't showing, captured for no one, is hidden to its
  app, which stops short of its work: every save panel kept its Save, New Folder and New
  Document buttons off (TextEdit and VS Code alike) until a stream ran on the window. So macOS
  shows its screen-recording mark for as long as an agent has a window open.
- Windows are listed five times a second (sheets, menus and popovers are windows of their own),
  and sixty while the person drags in the tab, so a window follows their pointer.
  A listing ScreenCaptureKit never answers is asked for again after 3 s.
- Screenshots are captures of their own, a window each, put together on the desktop's shape, PNG.

## 7. Acting [built]

The recipe: an event from a private source at the global point, the target window's number in
fields 51, 91 and 92, the window-local point set last (`CGEventSetWindowLocation`), posted to the
process. Once per app, an AppKit "application activated" event; and whenever the target window
changes, the window server's make-key records for it. Without those records an Electron app took
no click and no key; with them it takes both.

A window another process draws. An Open or Save panel of an app in a sandbox (TextEdit, Preview)
is the app's window, but its content is a macOS service's
(`com.apple.appkit.xpc.openAndSavePanelService`), and events posted to the app never reach it.
The host looks through every window in the session five times a second for one that belongs to a
process macOS holds an agent's app responsible for (`responsibility_get_pid_responsible_for_pid`)
and has exactly the frame of one of that app's windows; events for that window, pointer and keys
alike, are posted to the service with its own window's number. `[open]` a view of another
process's inside part of a window gets the app's events only.

Where keys go. To the window an act names, which has them from then on; else to the one last
clicked or focused; and from there to where the app moved them by itself, as it would with the
person at it: its own key window (`AXFocusedWindow`: the document it just made, the panel it
opened) and the sheet on that window (the window in front that the window server keeps together
with it, `SLSCopyAssociatedWindows`). Once a panel has closed, an app is left with no key window
at all and hears no key, and the make-key records don't give it one: the host asks it through
accessibility to make the window its main one (`AXMain`).

| Act | On the agent desktop |
| --- | --- |
| Click, right-click, double and triple click | yes |
| Typing (any text: characters as they are, a line break as Return, a tab as Tab) | yes, twelve characters a turn of the host's loop, so the person acting in the tab stops it within a fifth of a second: the agent is told, and part of the text is in |
| Shortcuts | yes: the modifiers go down and up as keys round the key. A key with only a flag set left the app taking the modifier for held, and what was typed next for shortcuts. A character's key is looked up in the person's keyboard layout, which is how the app reads it |
| Open and Save panels, sheets | yes: clicks by point, keys and typed text (a file saved through the sheet, a file picked in the panel) |
| Drag, drag-select | yes |
| Scroll | yes, as a trackpad gesture with phases (one without doesn't move a window that isn't showing). AppKit carries a gesture on at the speed it ended with, so it ends slow and its momentum is said over: 12-25% further than asked, still after ~0.4 s. "About" in the tool's words |
| Chromium and Electron apps | yes, the same way (VS Code: click, shortcut, typing) |
| Bringing a window in front | yes, through accessibility (`AXRaise`); an act can name its window, which is raised first when another covers the point |
| Moving a window by its title bar | yes, through accessibility (`AXPosition`). macOS moves a dragged window itself, after the real pointer: an agent's drag of a bar moved nothing, and the person's in the tab moved the window by what their own pointer did on their screen, off from the picture. A press on the bar (the top 56 points, on nothing that takes a press itself; found by going down the window's own tree, as an app doesn't answer what's under a point while its desktop isn't showing) is the host's to follow. Resizing by an edge goes by the events |
| Hover (tracking areas) | `[open]` no |
| Shift-click to extend | `[open]` not on 27.2 |

Accessibility first, where it reaches: a snapshot is the accessibility tree of the desktop's
windows, a ref per element and its middle in desktop points; a plain click on a ref presses it
with no event at all (12-54 ms), and a field's whole value can be set. With `menus`, the apps'
menu bars too: a menu item is pressed by its ref without the menu opening. Refs count on from
one snapshot to the next and are never used twice: one from an older snapshot is refused, not
taken for another element. An app doesn't list
its windows while their desktop isn't showing, so the host reaches a window through the notices
of windows the app makes, its main and focused window, and the numbers it gives its elements.
A page's elements come into the tree, and to their right places, a moment after the page loads
(Safari had a field 240 points off until its sidebar had settled): wait, then read.

A wait is over when no window changed more than a text cursor does (200 square points) for the
quiet time: a cursor never rests.

## 8. The link [built]

- The app listens on a Unix socket in its data folder (`desk/link.sock`, the person's alone, gone
  once the host connected); the host connects. Only the loader's own program is let on: a
  connection from any other is closed, as what the person types in the tab goes down the link.
  Frames: a 4-byte little-endian length, then CBOR, at most 16 MB. The app takes frames'
  surfaces only from the process on the socket's other end.
- `Hello` both ways with the protocol's number; the app's hello names the mach service for frames.
- App → host: `Visible`, `Input` (the person's, in desktop points: pointer, scroll, keys, typed
  text), `Request { request, ask }`, `Cancel`, `OpenForPerson` (the person pressed Open Yours),
  `TakeBack` (they pressed Take It Back on an app of theirs that is lent), `Quit`. Host → app: `Hello` (with the grants it holds), `Grants`, `Desktop`,
  `Windows` (the agent's windows, and the desktop's own picture apart from them), `Wanted`
  (an agent's app came to the front unasked), `Lent` (the person's apps that are lent, on
  every change of the apps), `Pointer`,
  `Answer { request, result }`, `Log`.
- Asks: `Open` (with whether the person's own is asked for, and their words of consent),
  `TakeFocus` (with their words) and `ReturnFocus`, `Windows`, `Screenshot`, `Snapshot`, `Act` (click, move, drag, scroll, raise,
  type, set a value, key), `Wait`, `Close` (a window, or an app the host started; a lent one is given back). Refusals carry a code: no desktop, not permitted, not
  found, the person's app, invalid, failed, timed out, the person acting. An answer too large for
  a frame is refused as failed; the link stays up.
- `[open]` The host's end is told by its program's path, not by its audit token.

## 9. The tab [built]

- Pane kind `desktop`, one in the window, titled "Agent desktop". Opened by the person from the
  new-tab launcher (Desktop), by `tab_open`, or by an agent's `desktop_open`: out of sight, as
  every agent's tab, and where the agent is, never where the person happens to be
  (`control/exec/beside.rs`, 2026-11-14). That's right after the agent's own tab (its
  chat's, its terminal's), the tab that checkout shows left shown; for an agent no tab
  shows, its worktree, after the tab it shows (the desktop's is the one shown on a checkout
  with no tab); for a client outside every checkout, the selected one. The tab is put there
  directly (`Shell::one_pane` at a `Place`, `shell/one_tab.rs`): no checkout is selected on
  the way, so the window isn't touched. It goes to the tab only when the person set Switch
  to what agents open. Before, it opened on the checkout the person was on, after the tab
  they were at: an agent in one project put it in whichever project the person was working
  in. Open already, it stays where it is.
- The canvas keeps the desktop's shape, fitted to the pane and never blown up. Each window's
  latest surface is drawn at its place.
- The agent's pointer is drawn where the host says it acted, the arrow a page's agent has.
- Round the desktop the tab shows the window's own card, as a chat does: it paints none.
- Taking over: the person's pointer, scroll and keys in the pane go to the host as input, typed
  text as their keyboard layout and input method made it. Koyo's own shortcuts stay Koyo's; the
  edit commands, Tab, Escape and Return go to the agent's app. A key comes from the app by
  its US code, which stands for the character meant, and the host posts the key that types
  that character on the person's layout (`layout::placed`), as it does for an agent's
  shortcut: sent on as it came, ⌘Z on AZERTY reached the app as the key that types W there. A modifier alone, or the pointer
  just moving, takes nothing over.
- Who has the desktop is a browser page's lease (`browser::control`), for the one desktop. An
  agent's act makes it that agent's (from another agent too: they share it) until it has rested
  10 s. The person acting in the tab makes it theirs while they're at it and 5 s after; Take
  control makes it theirs until they give it back. While it's theirs an agent's act fails
  `person_has_control`, and text an agent was typing stops where it is. The bar at the tab's
  bottom is the page's own (`held.rs`): "Claude Code is using the agent desktop" with Take
  control; "Claude Code waits while you use the agent desktop" or "You took over the agent
  desktop from Claude Code" with Give back.
- What is in the way is said in the tab with what to do: off (Turn On), the host stopped
  (Start Again), a grant missing (Set Up…, the setup of 11, which also comes up by itself the
  first time).
- When an agent's app comes to the front with no act of an agent's behind it, the tab says for a
  minute "Calculator came to the front: if you opened it, the one running is the agent's." with
  Open Yours, which starts an instance for the person on their desktop (2).
- While an app of the person's is lent, the tab says "Arc is yours, lent to agents: its
  windows are here until it's given back." with Take It Back (`notices.rs`).

## 10. Agent tools [built]

Scope `desktop` for the acts, off for an external client until the person allows it, as
`settings` and `destructive`; Koyo's own agents hold it. The reads (`desktop_windows`,
`desktop_screenshot`, `desktop_snapshot`, `desktop_wait`) need `read`, as every read of the
catalog does (a chat, a page, a terminal's screen): a client that may read Koyo sees what
agents have open on the desktop, and can open nothing there itself. Inputs and outputs:
`koyo-rs/koyo-app-control/src/ops/desktop.rs`; executors:
`koyo-rs/koyo/src/control/exec/desktop.rs`.

| Tool | Scope | Does |
| --- | --- | --- |
| `desktop_open` | `desktop` | Starts an app (name, bundle id or path), optionally with a file or URL, on the agent desktop; opens the tab out of sight, after the caller's own tab (9). With `personsOwn`, or for an app that takes no second instance, the person's own running one, once `consent` carries their words (5); the answer then says `lent` |
| `desktop_windows` | `read` | The windows there, front to back: id, process, app, title, frame |
| `desktop_screenshot` | `read` | A window, or the whole desktop, as PNG with its place on the desktop; points equal pixels at scale 1 |
| `desktop_snapshot` | `read` | The accessibility tree with refs and each one's middle, optionally the menu bars |
| `desktop_click`, `desktop_move`, `desktop_drag`, `desktop_scroll` | `desktop` | By point (desktop points) or by ref; `window` raises the window meant first |
| `desktop_type`, `desktop_key` | `desktop` | Text as keys (at most 4000), or a field's whole value by ref; a combo such as `cmd+s`, in the window that has the keys or in `window`, which has them from then on (7). A copy or cut answers with what it copied (2) |
| `desktop_focus` | `desktop` | Brings a window in front on the agent desktop; keys go to it from then on |
| `desktop_take_focus`, `desktop_return_focus` | `desktop` | Puts the agent desktop on the person's screen with a window in front, once `consent` carries their words (5); and gives their own desktop back |
| `desktop_wait` | `read` | Until the picture is still for `quietMs`, at most `timeoutMs` (60 s) |
| `desktop_close` | `desktop` | A window, or quits an app the agent opened; a lent one is given back as it is |

Acts and `desktop_close` fail `person_has_control` while the person acts in the tab and for
5 s after (9), and so does text being typed when the person acts; the host's refusals map to `desktop_unavailable` (no desktop, a grant
missing), `not_found`, `person_has_control` (the person's app, or theirs to allow first),
`invalid_input`, `timeout`
and `internal`, their message as the host wrote it.

## 11. Settings → Agent desktop, and the setup [built]

Settings → Agent desktop: on or off (off until the person turns it on); the host's two grants as
it reports them, each with what it's for and a mark at its right (a tick and "Allowed", or a
warning and "Not allowed"), and Set Up… while one is missing; the desktop (ready, and that it
is the display's size); the apps on it. Nothing here is reachable by `settings_set`.

The setup (`agent_desktop/setup.rs`) is a dialog that takes the person through the two grants
one at a time: Accessibility, then Screen Recording. It comes up by itself the first time the
Agent desktop tab shows with a grant missing (once since Koyo started) and the person is at
Koyo's window: opened behind another app, a dialog takes the keyboard from the app they're
typing in. It also comes up from Set Up… in the tab and in Settings. Each step's button has macOS list the host for that grant and opens its
pane of System Settings. The dialog is out of the way while Koyo isn't in front, shows each
grant as given the moment it is, goes on to the next by itself, and brings Koyo back in front
once the desktop is made. Nothing is asked of macOS before the person presses.

How a grant is asked for and seen (`agent_desktop/granting.rs`, the host's `grants.rs`):

- The asking is a process of the loader started for that alone (`--ask accessibility`,
  `--ask screen-recording`), a new one each time the person presses. macOS hears a process ask
  once. And its helper that lists an app for Screen Recording (`universalAccessAuthWarn`, which
  also shows the alerts) drops what it's asked without a word while it has any alert up, of any
  app: no entry is made, so the app isn't in the list to be switched on. Asked by the host
  that runs, a request dropped like that could never be made again; asked afresh, the next
  press gets through once that alert is answered.
- Accessibility takes no asking: the first time a process checks whether it's trusted, macOS
  lists its app, also after the person took it off the list. macOS's own alert for
  Accessibility is never raised: with it up, the Screen Recording request that followed as the
  person switched Accessibility on was dropped, and the host never came to be listed. Screen
  Recording has to be asked for, and macOS answers with an alert of its own beside the pane
  Koyo opens.
- The host reads what it holds a few times a second while one is missing, so Accessibility
  shows as given at once and the desktop is made. Screen Recording a running process never
  learns of: while it's the one waited for (for as long as Koyo is in front with the dialog
  up, and 5 minutes on from then or from the last press), Koyo starts a process of the
  loader every second that only writes what it holds (`--grants <file>`), and
  once that says yes starts the host again by itself. A host that closes meanwhile (macOS
  offers Quit & Reopen as the grant is switched on) is started again too.
- If the host isn't in the list all the same, the dialog's last line shows the loader in
  Finder, to be dragged into the list.

## 12. Milestones

1. `[built]` The host: the desktop's life, apps, windows, capture, input, screenshots, the link.
   Checked by `koyo-desk-host/tests/real_mac.rs` on a real Mac (ignored by default: it needs a
   session and a loader that holds the grants): a document opened, shown, typed in, copied
   from, driven by ref, an Open panel driven, a new document saved through its sheet with
   nobody watching, everything closed and the desktop removed.
2. `[built]` The tab: host start, the picture, the pointer, taking over, Settings.
3. `[built]` The tools, driven from an MCP client over HTTP.
4. `[built]` Accessibility snapshots and acts by ref, waits. Chromium and Electron apps take
   events once their window is told it's key, so no DevTools port is needed.

## 13. Measurements

macOS 27.2, MacBook Pro M4 Max, one 2560 × 1440 display at 2× (the last rows on its own
1800 × 1169 display); release builds for the frames and the CPU, debug builds for the rest. The record, with every check and finding:
`.superpowers/agent-desktop/qa/QA.md`; `qa/smoke` there runs the whole tool set against a
running Koyo with a check after each step (24 of 24 with the tab out of sight and with it
showing, on the debug and the release host).

| What | PoC on 27.2 | Built |
| --- | --- | --- |
| Desktop made / removed | 86-109 ms / 87-113 ms, Mission Control up ~150 ms | made in 140-145 ms |
| New window on the person's desktop | 0 ms | never seen in 12,000-32,000 samples each over 7 s, six apps |
| Frames reaching the tab | 57-60 a second for one window that changes | ~55 a second while an agent types; 9-10 a second idle (two cursors), of ~57 delivered a window |
| CPU, tab showing, TextEdit and VS Code idle / an agent typing | 5% of a core for one changing window | Koyo 4.7% / 14.0%, host 5.3% / 7.6-8.5% of a core |
| CPU, nobody watching, TextEdit and VS Code idle (two frames a second a window) | | Koyo 0.4%, host 1.2% of a core; no frame sent |
| A save sheet's Save button with nobody watching | off for good with no capture of the window | on; a file saved through Go to Folder, name, Return |
| A window dragged by its bar, by an agent / by the person in the tab | 0, 0 for the agent's; off the pointer for the person's | exactly what was asked / +196, +118 for +200, +120 at a tab scale of 1.23 |
| Frames macOS delivers for a watched window, on battery in Low Power Mode | | 53-57 a second |
| The wallpaper's yellow, itself / in the tab | 245 197 32 / 238 198 76 | 245 197 32 / 247 200 35 |
| The person's input stops an agent's typing | | refused within 1 s (`real_mac.rs`), part of the text typed |
| Front given back after a theft | 12-15 ms | given back (logged); the person stays on their desktop |
| The person's clipboard after an agent's copy | 2 ms holding the agent's | unchanged when read right after |
| `desktop_open` | | TextEdit 0.8-1.1 s, Calculator 1.4-1.5 s, VS Code 0.9-3.0 s, Safari 2.3 s |
| `desktop_click` by point / by ref | | 110 ms / 12-54 ms |
| `desktop_key` / `desktop_type` | | 30-45 ms / 16 ms a character |
| `desktop_snapshot` / `desktop_screenshot` (a window) | | 45-60 ms / 69 ms |
| `desktop_screenshot` with a save sheet up: the sheet, its window, the whole desktop | | 75 ms, 70 ms, 136 ms |
| An Open panel on the person's desktop while an agent opened and drove one | | never seen in 19,844 samples over 7 s |
| `desktop_scroll` asked 52, 104, 208, 416, 832 pt | 2× and more, gliding | 65, 117, 234, 494, 949 pt |
| Koyo killed → host, apps and desktop gone | | 0.2 s |
| Turned off → apps quit, desktop removed | | 3.7 s |

## 14. Where it lives

| Path | What |
| --- | --- |
| `koyo-rs/koyo-desk-wire/` | The link's messages and frames, shapes, key names |
| `koyo-rs/koyo-desk-host/` | The host library, its loader (`loader/main.c`) and the real-Mac test |
| `script/desk-host` | Packages the loader app and the library; `script/dev` and `script/bundle` run it |
| `koyo-rs/koyo/src/agent_desktop/` | The app's side: the desk, the host's start and link, the tab, who has the desktop (`control.rs`) |
| `koyo-rs/koyo/src/held.rs` | The bar that says who has a pane, for a browser page and the desktop alike |
| `koyo-rs/koyo/src/settings_view/agent_desktop.rs` | Settings → Agent desktop |
| `koyo-rs/koyo-app-control/src/ops/desktop.rs`, `ops/desktop/` | The tools' inputs and outputs |
| `koyo-rs/koyo/src/control/exec/desktop.rs` | The tools' executors |
| `koyo-rs/koyo/src/control/exec/beside.rs` | Where the tab an agent's `desktop_open` opens goes: beside the caller |
| `koyo-rs/koyo/src/shell/one_tab.rs` | The one pane there is, or a new one in a tab at a place; a device's tab opens through it too |

Running it from a checkout: `script/dev` packages the host in `koyo-rs/target/desk-host`; its
loader has no grants until the person gives them in Settings → Agent desktop. In a debug build
only, `KOYO_DESK_HOST` names another loader app and `KOYO_DESK_HOST_ARGS` arguments put before
the host's own; `KOYO_DESK_STATS` has the host log the frames it sends.

## 15. Open

- Menus. One that is up in an agent's app holds the person's keyboard until their first key
  press closes it (2). To be built: no menu left up at all. The host reads a menu as it
  opens, closes it at once (handing the keyboard on closes it in 13 ms) and keeps its
  picture and items; Koyo shows that, and a pick is the item pressed through accessibility,
  the menu opened again for the moment that takes where its items don't outlive it (a
  right-click's). The menu bar is the same thing: macOS draws none for a desktop that
  isn't showing, and a title pressed through accessibility opens a real menu, with the
  keyboard. Until then an agent presses a menu bar's items by ref, with no menu opening.
  The Apple menu is the person's session's (Log Out, Shut Down, Sleep) and should be left
  out of what an agent is shown.
- A menu's Copy pressed by an agent reaches the person's clipboard; a copy slower than half a
  second isn't caught; the person's own copy in that moment is lost (2).
- The person's unsaved Untitled documents open in an agent's instance of the app, which can
  change or delete them; what an app remembers is shared with the agent's instance (2).
- A view another process draws inside part of a window (7). Panels of other kinds that a
  service draws (sharing, printing to PDF) aren't checked.
- Hover; shift-click to extend (7). Scroll is about, not exactly, what's asked (7).
- A window that an app moves by a drag of its own making (a bar the host doesn't take for one:
  lower than 56 points, or under a control) still goes by the real pointer (7).
- The screen locked, or the display asleep: untested; grant prompts wait for the unlock.
- Lending (5): the person's words of consent are taken from the agent as given; Koyo could
  check them against the chat for its own agents. A lent app's windows come back to one
  desktop (its home), not to each one they were on: only the Dock moves a single window.
  A browser's pages aren't drawn or read on a desktop that isn't showing (5): lending the
  person's Arc gives an agent a window it can't see into, until the person looks at the
  agent desktop. The person asking for a lent app at its Dock icon gets nothing but the
  tab's notice. A window of the person's own that they drag onto the agent desktop in
  Mission Control is theirs still, unseen in the tab, and takes them there whenever its
  app comes forward: nothing is done about it. The Dock passing over the apps' bindings
  (it sets every app without one to none), as when it restarts, would undo the host's
  assignments: not seen, not tested. Whether the
  Dock's own pass over the apps' bindings (it sets every app without one to none) undoes the
  host's assignments, as when the Dock restarts, isn't tested. A per-app allow list.
- Audio and notifications of the agent's apps reach the person.
- System prompts an agent's app raises show on the person's screen.
- Several displays; a full-screen app on the agent desktop (4).
- Two Koyos on different data folders each run a host, and nothing keeps the two from driving
  Mission Control at once (3).
- The person's input in the tab waits behind an act of an agent's that blocks the host's loop
  (a window being raised, an app that answers accessibility slowly): up to a few seconds.
- The link's peer is told by path (8); the loader's library on an ad-hoc build (3).
- The loader's grants on a build signed ad hoc hold only while the loader stays the same binary:
  a new compiler can change it. A build signed with a Developer ID keeps them by identity.
