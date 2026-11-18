# Koyo's chat backdrop: the wallpaper behind a chat, dithered in its own colours

Written 2026-11-16 on branch `main`, and built on `schulzfel/chat-backdrop` in the order of
section 10, which says what of it is built and checked so far. Drafted with Claude, from
research in the code and an interview with the owner. Keep it in step with
`koyo-rs/koyo/src/chat_backdrop/`. A chat's background shows the desktop
picture that lies behind the window, dithered in its own colours, faint, and fading out
towards the bottom, as in the owner's reference: Monodrive's disk overview with its Dither
effect, whose top shows the picture as a lattice of lit and dark cells and whose bottom fades
into the window's plain blur. The picture is made once for each screen and only moved as the
window moves; the dither and the fade are the renderer's, as it draws, so a frame costs what
drawing one texture costs. What the owner decided is marked **[asked]**; what was proposed
while researching it, and is the owner's to change, **[proposed]**. Section 9 lists what can
only be settled once it's built.

**The look changed on 2026-11-16.** The first build put a grid of black dots over the
picture, dimmed it towards the card's colour and blurred its bottom, from the doc's
description of the reference; seen on screen, it looked nothing like it. The reference is
Monodrive's Dither effect ([donnes/monodrive](https://github.com/donnes/monodrive), MIT,
© 2026 Donald Silveira: `crates/monodrive/src/background.rs`, and the
`project_background` shader of its vendored renderer), and the owner asked for it, with the
settings mapped onto it. Koyo takes the technique, not the code (section 4).

**It shows behind more than chats since 2026-11-16**: the explorers, files, diffs, terminals,
and round a simulator's screen and the agent desktop, each with a switch of its own, all on by
default (sections 2 and 6). The name stays `chat_backdrop`, in the code and the settings file.

## 1. What it shows

- **The screen's wallpaper, where it lies behind the window.** Each chat shows the part of
  the picture its pane covers on the screen, laid out as macOS lays it out (section 3.2). Not
  the windows behind Koyo's: those would need the screen captured live (section 8).
  **[asked]**
- **Dithered in its own colours, as Monodrive's Dither.** The picture is cut into cells of
  2 × 2 screen pixels, each taking the picture's colour there. A 4 × 4 Bayer matrix of
  thresholds lies over the cells: a cell whose brightest channel passes its threshold is lit
  and shows its colour at full brightness; one that doesn't shows 8 % of it. On a mid-tone
  that's a regular lattice of dark cells two cells apart, in the picture's own colours.
  **[asked]**
  - The cells are the screen's, counted from its top left, so the pattern holds still as the
    window moves and two chats side by side show one. **[proposed]**
- **Faint, at an opacity over the card** it sits on (`theme/card.rs`), 24 % by default, as
  Monodrive's: the card and the window's glass under it show through. How faint is the
  Opacity setting. **[asked: the opacity; proposed: 24 %]**
- **Fading out towards the bottom**, by Monodrive's curve: whole down to 18 % of the pane's
  height, 61 % of itself at 32 %, 32 % at 47 %, 11 % at 62 %, gone by 72 %. Below it the card
  over the window's glass, which blurs the desktop behind the window: the plain blur at the
  bottom. Where the fade starts and ends are settings. **[asked]**
- **The composer and the floating cards are see-through** over it: the prompt, the details
  card and the floating prompt take a translucent fill, so the backdrop shows through. Their
  text's contrast is checked at every Opacity stop (section 9). **[asked]** Where macOS has
  Liquid Glass (26 on), the three are glass, as they were, and glass shows what's under it
  already. Where it hasn't, and in the visual check, which draws glass as the fill it falls
  back to, they're filled in the card's own colour, as see-through as the card is (60 % in
  the dark appearance, 65 % in the light), where they were the card's colour solid
  (`chat_backdrop::fill`, `native/liquid.rs`, `floating_filled`). **[proposed: the fill]** The
  scroll pill and the details card's buttons keep theirs.
- **With nothing to show, the plain card**, as today: when the setting is off, when the
  wallpaper is a moving one macOS gives no still of, or when its file can't be read.
  **[asked]**

## 2. Where it shows

- **Behind chats, and the rest but pages**: a chat and a subagent's chat; the right panel's
  file tree and list of changed files; an open file; a checkout's diffs; a terminal; round a
  simulator's screen and the agent desktop. Each has a switch of its own, all on by default
  (section 6). A page keeps the card, which its native view covers anyway. **[asked: chats,
  2026-11-16 the rest, each a switch, on by default]**
- **Where each draws it.** Every tab's pane, a chat's included, draws it as the first child of
  its part (`shell/splits.rs`, `pane_part`, by way of `chat_backdrop::behind`, which names the
  pane's surface and keys the backdrop by the pane's id), so it lies under a split pane's
  header too; the right panel as the first child of its part (`right_panel.rs`). What paints over it, partly: a diff's file
  cards (`p.sunken`, see-through), the simulator's screen and the agent desktop's own
  wallpaper, and a terminal's background where a program sets its own, or the terminal's
  pinned grid, which is painted solid.
- The chat view has no background of its own (`chat/view.rs`, `impl Render for ChatView`): it
  lies on the pane's card, which rounds its corners and clips what it holds
  (`theme/card.rs`, `island`). The backdrop is the pane part's first child, absolute and
  filling it, so the card's corners clip it too. Until 2026-11-16 it was the chat view's own
  first child. **[proposed]**
- **One picture a screen, cropped.** Every chat on a screen shows a part of the same picture,
  placed where the screen is relative to the window and clipped to the pane. Two panes side by
  side meet without a seam. **[proposed]**
- A window across two screens shows the picture of the screen holding most of it, as GPUI's
  own `Window::display` says. Where the pane reaches past that screen's edge, the card shows.
  **[proposed]**

## 3. The picture

### 3.1 Made once, kept compressed on the GPU

- **Made once for each screen** a chat shows the backdrop on, and again only when what it's
  made from changes: the wallpaper (its file, the file's time, how it's laid out, its fill
  colour), the screen's size and scale, and Quality. Off the main thread. The dither, the
  opacity and the fade are applied as it's drawn (section 4), so the appearance and every
  other setting change nothing of it. **[proposed]**
- **Kept on the GPU as an ASTC texture, the only copy.** Uncompressed, the picture for a
  2560 × 1440 pt screen at 2× is 5120 × 2880 pixels, 59 MB, and GPUI keeps an image twice,
  once in the app's memory to upload it again and once in the GPU's atlas: 118 MB for one
  element. **The owner found that unacceptable for a UI element.** **[asked]**
  1. The wallpaper file is decoded by the system's decoders, as the viewer decodes a picture
     (`viewer/picture.rs`): HEIC, JPEG, PNG and the rest, at no more pixels than the layout
     needs, upright, in sRGB.
  2. It's drawn into a bitmap the screen's size, laid out as macOS lays it out (3.2), with
     Quartz (`viewer/bitmap.rs`, `Canvas`).
  3. macOS's own encoder compresses it to ASTC: an `org.khronos.astc` image destination in
     ImageIO, its block size set by `kCGImagePropertyASTCBlockSize` (4 × 4 or 8 × 8, the only
     two ImageIO names). The result is a 16-byte header, then the blocks, 16 bytes each
     (`chat_backdrop/astc.rs`). The encoder writes the picture's bottom row of blocks first,
     as OpenGL counts rows, where the GPU reads the first row as the top one, so the picture
     is drawn into its bitmap upside down and reaches the GPU upright (`make.rs`,
     `upside_down`); a test reads the blocks back in the file's order and finds it upright.
  4. The blocks go straight into a Metal texture of the matching ASTC format; the bitmap and
     the encoded bytes are then dropped.
- **Why ASTC and why the system's encoder:** Koyo builds for Apple Silicon only, and every
  Apple Silicon GPU reads ASTC as it samples, at the cost of no extra pass. ImageIO writes it
  (and BC, through DDS) at no cost to the binary, as the viewer already counts on it to read.
  **[proposed]**
- **Memory**, on that screen:

  | | Full resolution (5120 × 2880) | Half (2560 × 1440) |
  |---|---|---|
  | Uncompressed, as GPUI keeps an image (twice) | 118 MB | 29 MB |
  | ASTC 4 × 4 (8 bits a pixel, near lossless) | 14.7 MB | 3.7 MB |
  | **ASTC 8 × 8 (2 bits a pixel), the default** | **3.7 MB** | 0.9 MB |

  While it's made, the bitmap takes 59 MB for the fifth of a second that takes (section 9),
  and is freed. A screen's texture is let go once no chat on that screen shows it, and made
  again when one does. Nothing else is kept: the dither and the fade take no memory of
  their own. **[asked: 8 × 8 at full resolution; proposed: letting go]**
- **Quality is the person's to pick** (section 6): three stops, always compressed, never the
  uncompressed picture. **[asked]**

  | Stop | Blocks | Resolution | On that screen |
  |---|---|---|---|
  | Smallest | 8 × 8 | half | 0.9 MB |
  | **Default** | 8 × 8 | full | 3.7 MB |
  | Sharpest | 4 × 4 | full | 14.7 MB |

  There were four: 4 × 4 at half resolution cost what 8 × 8 at full does, and the rule was
  that if the two looked the same once built, one would go. They did (section 9), so 4 × 4
  at half went, as 8 × 8 at full is the default the owner asked for and came closer to the
  picture (40.9 dB against 39.7). A `settings.json` that still names it reads as the default.

### 3.2 Reading the wallpaper

- `NSWorkspace.desktopImageURL(for:)` names each screen's wallpaper, and
  `desktopImageOptions(for:)` how it's laid out. On the owner's Mac, as researched: a
  downloaded picture, `~/Library/Application Support/com.apple.mobileAssetDesktop/Radial
  Yellow.heic`, scaled to fill a 2560 × 1440 pt screen at 2×.
- The screen is found by its display ID, which GPUI's display ID is on macOS and `NSScreen`
  gives as `NSScreenNumber` in its `deviceDescription`.
- The layout, from the options' `NSImageScaling` and whether clipping is allowed:

  | Scaling | Clipping | System Settings calls it | Laid out |
  |---|---|---|---|
  | `ProportionallyUpOrDown` | yes | Fill Screen | scaled to cover the screen, centred |
  | `ProportionallyUpOrDown` | no | Fit to Screen | scaled to fit, centred, the fill colour round it |
  | `AxesIndependently` | | Stretch to Fill Screen | stretched to the screen |
  | `None` | | Centre | at its own size in points, centred, the fill colour round it |
  | `ProportionallyDown` | | | as Fit, never enlarged |

  The layout is a function of the picture's size, the screen's and these options, with
  nothing of AppKit in it, and is unit-tested. Tiling isn't among these options and isn't
  handled. **[proposed]**
- Needs the `NSWorkspace` and `NSScreen` features of `objc2-app-kit`, and
  `CGImageDestination` of `objc2-image-io`, in `koyo-rs/koyo/Cargo.toml`.
- **A dynamic wallpaper shows the picture macOS shows** (`dynamic.rs`), asked for 2026-11-16,
  after the owner saw his solar wallpaper's day picture behind the chat while macOS showed its
  dark one. A dynamic wallpaper is one HEIC holding several pictures; its first image's
  metadata, in the `apple_desktop` namespace, says which to show: `apr` by the appearance
  (`{l, d}`, the light and the dark picture), `h24` by the time of day (`ti`, each picture from
  a fraction of the day on, with an `ap` like `apr`'s), or `solar` by the sun (`si`, each
  picture at a sun altitude `a` and azimuth `z`, with an `ap`), each value a binary property
  list in Base64. Before, the backdrop decoded the file's primary picture, index 0, whatever
  macOS showed. Now:
  - **Light (Still) and Dark (Still)** show `ap`'s light or dark picture; **Automatic** the one
    for the system's appearance (`AppleInterfaceStyle`, not Koyo's own theme); **Dynamic** the
    schedule's: for `h24` the last picture whose time has come (before the first, the day's
    last, still showing from the evening before), for `solar` the picture whose sun is nearest
    the sun now on the sky, by the angle between them. The sun is the Astronomical Almanac's
    low-precision position, good to about a hundredth of a degree.
  - **Where the sun is seen from**: Koyo has no location and doesn't ask for one. It takes the
    place the time zone database gives the system's time zone (`/etc/localtime` and
    `/usr/share/zoneinfo/zone.tab`, Brussels for Europe/Brussels): right to the time zone's
    city, not to the person. macOS with Location Services uses where the Mac is; without it,
    its own words are "This wallpaper changes throughout the day", and what it then uses
    wasn't checked. With no place at all, a solar wallpaper shows its picture for the
    appearance.
  - **The person's choice** is read from the store System Settings keeps,
    `~/Library/Application Support/com.apple.wallpaper/Store/Index.plist`, a private format,
    read as macOS 27.2 writes it: `AllSpacesAndDisplays.Desktop.Content.EncodedOptionValues`, a
    property list whose `values.style.picker._0.id` was `dark` for Dark (Still); `light`,
    `automatic` and `dynamic` are taken to name the other three, the names System Settings
    shows them by (not seen written). It's behind one function (`style`), read again only when
    the store's file changes; unreadable, or a name it doesn't know, it's Dynamic (which for an
    `apr` wallpaper is Automatic), said once in the log. A choice made for one display alone
    (the store's `Displays`) isn't read.
  - **When the picture is made again**: the chosen picture is part of the wallpaper as read, so
    a new one is a changed wallpaper (3.3). The system's appearance changing is heard at once
    (`AppleInterfaceThemeChangedNotification`); a time or a sun position coming is caught by
    the once-a-second ask the wallpaper already had, as the metadata is read once a file and
    the pick is arithmetic: no timer of its own, as one would add a wake without removing that
    ask.
  - Checked against The Cliffs (solar, 8 pictures, Dark (Still) chosen: picture 1), Sonoma
    (`apr`, 2 pictures) and a time-of-day file written for the check, as none ships with
    macOS 27.2 (`dynamic/tests.rs`, the ignored `dynamic_wallpapers_through_a_day` with
    `KOYO_DYNAMIC` naming the files).
- **When it draws nothing, the log says why**, once: a window on no screen `AppKit` names
  (`NSWindow.screen` nil, `element.rs`), or a screen with no wallpaper file (`screens.rs`).
  Added 2026-11-16, after two sittings on the agent desktop were spent guessing.
- **A debug build's stand-in**: `KOYO_WALLPAPER=<file>` gives every screen that file as its
  wallpaper, laid out to fill, so a test Koyo on a desktop with no wallpaper still has one,
  and no one's wallpaper is changed for it. Release builds ignore it (`debug_assertions`).
  It pins the picture, so a test Koyo given it doesn't follow a change of wallpaper; leave it
  out on a screen that has one (`koyo-verify prepare` passes it on only when it's set).

### 3.3 Noticing a change

- macOS has no public notice that the wallpaper changed. Koyo asks `NSWorkspace` for the
  wallpaper of each screen a chat shows the backdrop on once a second, a few calls that cost
  next to nothing, and again at once when the Space changes
  (`NSWorkspaceActiveSpaceDidChangeNotification`): each Space can have a picture of its own.
  A change in what's asked (3.1) makes the picture again. **[proposed]** Built so
  (`chat_backdrop/watch.rs`, `screens.rs`, `tick`): a changed wallpaper draws the windows
  again, and each chat on that screen asks for the picture of the new one. A screen no chat
  had shown is read again when one does. Nothing is asked while no chat shows the backdrop or
  nobody can see a window of Koyo's (covered, minimized, on another Space): the watch sleeps
  until a chat shows it, the Space or the appearance changes, or a window is seen again, which
  asks at once, and wakes on its own only to let go a picture no chat shows (below).
- The settings and the appearance are heard as the rest of the app hears them
  (`settings::observe`, `theme::follow_window_appearance`). Built: a change of the backdrop's
  settings draws again what draws the backdrop, which reads them, as the shell follows the
  settings; the appearance, a window moved to another screen and a screen's new size or scale
  draw the windows again already (GPUI's own refresh), and each chat then asks for the picture
  they call for.
- **Letting go** (3.1): the same tick lets a screen's picture go once no chat has shown it
  for 30 seconds, not at once, so a chat shown again after a terminal's tab shows its
  backdrop without the card in between. A chat holds its screen's picture through its
  element's state, which GPUI drops the first frame that doesn't draw the chat.
  **[proposed: the 30 seconds]**

## 4. Drawing it

- **Cropping is placing.** The picture is drawn as big as the screen, where the screen lies
  relative to the window, and clipped to the pane: only the pane's part reaches the GPU.
  Moving or resizing the window only moves where it's placed. GPUI tells of both through
  `observe_window_bounds`, and redraws the whole window as it does, the views it would
  otherwise keep from the last frame included (`Window::bounds_changed`, vendored
  `release_gpui/src/window.rs`). The window's bounds are its screen's, from that screen's top
  left, so the screen lies at the negative of the window's origin (`chat_backdrop/element.rs`,
  `Picture`). **[proposed]**
- **Through the renderer's surface path.** The vendored Metal renderer already draws a
  texture over one rectangle and clipped to another, with rounded clips, for a browser page's
  frame (`draw_page_surface` and `page_surface_fragment`, `vendor/README.md`). What it lacks is
  a way to be given a texture Koyo made: it draws only pixel buffers, which can't hold a
  compressed format. So one more mark, beside a picture's, a capture's and a pane's (`Mark`
  in `metal_renderer.rs`): a placeholder BGRA pixel buffer marked as standing for a texture the
  app registered with the renderer, drawn over the screen's rectangle, clipped to the pane. A
  `Koyo:` change, described in `vendor/README.md` as the others are. **[proposed]**
  `register_texture` makes the texture of the blocks from any thread, on the system's GPU, as
  `ASTC_4x4_LDR` or `ASTC_8x8_LDR` (section 9), and gives it a number; `release_texture` lets
  it go. A test draws a picture so through the real renderer, headless, and finds it where it
  was laid out, its colours as they were, and nothing past the pane (`chat_backdrop/tests.rs`).
- **The dither, the opacity and the fade are the shader's.** The mark says how the texture
  is drawn (`set_surface_backdrop`, `Mark::Backdrop`): its cells' size in screen pixels, how
  many are lit, its opacity, and where its fade starts and ends. Its own fragment shader
  (`backdrop_fragment`, `shaders.metal`) finds each pixel's cell from the screen's top left,
  samples the picture at the cell's middle, lights the cell or not by the Bayer threshold
  there, and gives it the opacity times the fade at its height in the pane, the pane being
  the surface's content mask. Monodrive bakes its dither into its picture and fades it in
  its shader. Here both are the shader's: the picture stays the smooth wallpaper, which ASTC
  compresses well, where 8 × 8 blocks would smear a pattern two pixels across; a cell size,
  density, opacity or fade changes without making the picture again; and the pattern lands
  on the screen's own pixels at either resolution. **[proposed]**
- **Taken from Monodrive, credited in the shader and here:** the 4 × 4 Bayer dither of a
  picture's colours in 2 × 2 cells, a lit cell at its colour divided by its brightest
  channel and an unlit one at 8 % of it, and the fade's stops. The Bayer matrix is the
  standard one. No code is copied: Monodrive dithers on the CPU and fades in a pass of its own
  over a captured layer; Koyo does both in the one draw.
- **Order, bottom up:** the card, the dithered picture, then the chat.
- **While dragging, the backdrop may slip by a frame.** Koyo's window is dragged by macOS
  (`start_window_move`, which is `performWindowDragWithEvent`), which moves it before Koyo
  hears of it, so the picture is a frame behind until it catches up. The first version
  accepts that, and it's measured on the real window (section 9). If it shows, Koyo drags the
  window itself from its mouse events, and draws that frame in the same Core Animation
  transaction as the move (`presentsWithTransaction`, only while dragging, as it costs frame
  rate). **[asked: accept it at first; proposed: the fix]**

## 5. What a frame costs

Once the picture is made, a frame draws one textured rectangle for each chat, clipped to its
pane, the dither and the fade worked out as it's sampled. Nothing is decoded, dithered or
compressed while the window moves, resizes or scrolls. The picture is made again only when
what it's made from changes (3.1), off the main thread. The card shows until a screen's first
picture is ready; after that the last one made shows until the next is. One picture of a
screen is made at a time, and the last one asked for next (`chat_backdrop/screens.rs`).
**[proposed]**

## 6. Settings

In Settings → Appearance, a section of its own, "Wallpaper background" (first "Chat
background", renamed once it showed behind more), under "Theme". **[asked:
on by default, customizable, in Appearance]**

| Row | Control | Default | Shown |
|---|---|---|---|
| Wallpaper background | switch | on | always |
| Chats, Explorers, Files, Diffs, Terminals, Simulators and agent desktop | a switch each: what it shows behind | on | when on |
| Opacity | slider, Less to More | 24 %, as Monodrive's | when on |
| Cell size | slider, Fine to Coarse: how many screen pixels a cell is a side | 2, as Monodrive's | when on |
| Density | slider, Sparse to Dense: how many cells are lit | as Monodrive's | when on |
| Fade | switch | on | when on |
| Fade starts | slider, High to Low | 18 % of the way down, as Monodrive's | when on and Fade is on |
| Fade ends | slider, High to Low | 72 %, as Monodrive's | when on and Fade is on |
| Quality | slider, Smallest to Sharpest, the memory the stop takes on the person's screens under it | 8 × 8, full | when on |

**[asked: mapped onto the dither, 2026-11-16]** The first build's rows were Dim, Dot size,
Density, Blur, Blur strength and Blur starts; Dim became Opacity, Dot size the cell's size,
Density how many cells are lit, and the blur's rows the fade's. The stops for now: Opacity
12, 18, 24, 35 and 50 %; Cell size 1, 2, 3 and 4 pixels; Density, the threshold scaled by
0.6, 0.8, 1, 1.25 and 1.5 (more lights more cells); Fade starts at 0, 9, 18, 30 or 45 % of the
way down, and ends at 50, 60, 72, 85 or 100 %, so it always ends below where it starts;
Quality its three stops (3.1). They're still to be tuned by eye (section 9).

- **Rows show only when they apply**, not greyed out: with the switch off, the section is the
  switch alone. **[asked]** The settings pages are built from the settings as they draw, so a
  row left out is simply not added; the MCP server page does the same with its port and
  clients (`settings_view/mcp_server.rs`). **[proposed: the fade's own rows following the
  same rule]**
- **Search still finds it.** Settings search looks at the rows shown, so with the switch off
  "opacity" or "fade" would find nothing. The switch's description names what it reveals:
  off, "Your wallpaper behind chats, files, diffs, terminals and devices, dithered in its own
  colours. Turn it on to choose what it shows behind (chats, explorers, files, diffs,
  terminals, simulators and agent desktop), and set its opacity, the cell size and density,
  the fade, where it starts and ends, and the quality."; on, its first sentence alone. The Fade switch does the same for its two rows while it's
  off ("Turn it on to set where it starts and ends."). A test searches the section on, with
  the fade off, and off, for each row's word. **[proposed]**
- **The Quality row's memory** is worked out from the sizes of the screens Koyo's windows are
  on, summed for more than one: "About 3.7 MB on this screen." **[asked: shown; proposed: how]**
  Built so, after "How sharp the wallpaper behind chats is kept, always compressed."; "on these
  2 screens" for two. The screens are `AppKit`'s, asked as the row is drawn: the screen of
  each of the app's visible windows, once each, at its size in pixels
  (`wallpaper.rs`, `screens_of_windows`).
- Sliders are `NativeSlider` over stops, switches `NativeSwitch` (`settings_view/`), each
  value kept in `settings.json` as the stop's value, as the other sliders keep theirs. The
  stops' values are tuned by eye against the reference once built. **[proposed]**
- In `settings.json` **[proposed]**:

  ```json
  "chat_backdrop": {
    "on": true,
    "surfaces": {
      "chat": true,
      "explorer": true,
      "files": true,
      "diff": true,
      "terminal": true,
      "devices": true
    },
    "opacity": 0.24,
    "cell": 2.0,
    "density": 1.0,
    "fade": true,
    "fade_start": 0.18,
    "fade_end": 0.72,
    "quality": "astc8_full"
  }
  ```

## 7. What it builds on

| Need | Already there |
|---|---|
| Rendering an image once on the CPU and drawing it as a GPUI image | `backdrop.rs` (the glass theme's gradient and grain) |
| Decoding any wallpaper format at a set size | `viewer/picture.rs` (ImageIO) |
| Drawing it laid out into a bitmap | `viewer/bitmap.rs` (`Canvas`, Quartz) |
| The look: a Bayer dither of a picture's colours, and its fade | Monodrive's Dither ([donnes/monodrive](https://github.com/donnes/monodrive), MIT) |
| Drawing a texture over one rectangle, clipped to another, rounded | vendored `metal_renderer.rs`, `draw_page_surface` |
| Hearing the window move and resize | `cx.observe_window_bounds`, used by `shell.rs` and `popup.rs` |
| Rounded clipping of the chat | `theme/card.rs`, `island` |
| A settings row and its controls | `settings_view/components.rs`, `native_slider.rs`, `native_switch.rs` |

Where it would go **[proposed]**: a module of its own, `koyo-rs/koyo/src/chat_backdrop/`, a
file for each job (reading the wallpaper, laying it out, making the picture, the element),
each under the 500 lines `script/hygiene` holds them to; the setting in
`settings/chat_backdrop.rs` and its rows in `settings_view/chat_backdrop.rs`. `settings.rs`
is 489 lines, so its new field and default may need something moved out of it first.

## 8. What was considered and dropped

- **macOS's own blur or Liquid Glass, dithered.** The window is blurred today
  (`WindowBackgroundAppearance::Blurred`, `main.rs`), and the switcher's glass is an
  `NSGlassEffectView` (`native/window_glass.rs`). Both are drawn by the window server, with
  Apple's filters, from pixels Koyo never sees, and take no shader of Koyo's.
- **The windows behind Koyo's, captured live** with ScreenCaptureKit, as the agent desktop
  captures its windows (`koyo-desk-host/src/capture.rs`), excluding Koyo's own. It would show
  what's really behind, but needs Screen Recording, shows macOS's recording indicator for as
  long as it runs, trails what it shows by a frame or two, and redraws the window whenever
  anything behind changes. The owner picked the wallpaper alone, for its cost and
  simplicity.
- **A dot screen over the picture**, dimmed towards the card's colour, with a blurred copy
  fading in towards the bottom under strips of a black dot tile. Built first, from the doc's
  description of the reference, and dropped on 2026-11-16 once the owner saw it: the
  reference is a dither of the picture's own colours, faded to the window's glass.
- **Halftone dots** whose size follows the picture's brightness, and Monodrive's other
  effects (ASCII, scanlines, and Haze, which blurs and darkens instead of dithering).
- **The dither baked into the picture**, as Monodrive bakes it: 8 × 8 blocks would smear a
  pattern two pixels across, and every change of the look would make the picture again.
- **The picture uncompressed** at full resolution (118 MB, as GPUI keeps an image twice), and
  **as a JPEG**: the GPU can't sample a JPEG, so it would be decoded back to the same 59 MB to
  be drawn; it would only save making it again, which is mostly decoding the wallpaper anyway.
- **An uncompressed stop in Quality.** Every stop stays compressed.
- **Two menus for Quality**, its blocks and its resolution apart: one slider of stops, with
  their memory, says it without the jargon.
- **The settings' rows greyed out** while the switch is off: they're left out instead.
- **Koyo dragging the window itself from the start**: the slip is measured first.

## 9. To settle once built

- How long ImageIO takes to encode a screen's picture, at each Quality stop. **Settled**:
  little. Making the picture of a 2560 × 1440 pt screen at 2× from the owner's wallpaper
  (a 6016 px square HEIC), the file warm, takes 80 ms to decode, 70 ms to lay out and dim
  and 33 ms to compress at 8 × 8, full: under a fifth of a second, 0.6 s the first time the
  file is read. At half resolution it's 67, 17 and 11 ms; 4 × 4 at full compresses in 30 ms.
  (`make.rs`, `making_a_screens_picture_takes`, run with `--ignored`.) In the bundled app on
  the agent desktop, 250 to 420 ms, and up to 4.7 s while the Mac ran at a load of 50 from
  other builds.
- Whether 8 × 8 blocks show under the dither, against 4 × 4 at half resolution, which costs
  the same, by screenshots side by side. If they look alike, one of the two middle stops
  goes. Measured against the picture uncompressed and dimmed 60 %, on the owner's wallpaper:
  8 × 8 full 40.9 dB, 4 × 4 half 39.7 dB, 4 × 4 full 47.0 dB, 8 × 8 half 36.7 dB. Under the
  first look the four stops looked alike at 2× (`.superpowers/v/evidence/20261116-141927-backdrop`,
  `quality-*-x2.png`), and again under the dither, 8 × 8 full and 4 × 4 half side by side
  (`.superpowers/v/evidence/20261116-153057-dither-light`, `crop-*.png`). **Settled**: they
  look alike, so 4 × 4 at half went (3.1).
- Whether the ASTC texture is sampled as sRGB or not: GPUI's atlas holds plain BGRA and
  blends its colours as they are, so the texture's format should match, or the picture comes
  out darker. **Settled**: not as sRGB. Drawn through the renderer headless, a picture
  dimmed to 128 comes out 128 as `ASTC_8x8_LDR`, and 54 as `ASTC_8x8_sRGB`, which takes the
  colours out of sRGB as it samples them (`the_gpu_draws_a_screens_picture_as_it_was_laid_out_and_only_where_its_pane_is`;
  the sRGB format tried once and put back).
- How far the backdrop slips while the window is dragged, recorded on the real window.
  **Not measured yet.** On the agent desktop `desktop_drag` answers only once the move is
  done, so every picture taken comes after it, and the agent desktop shows no wallpaper of
  its own around the window to compare with. After each move, at rest, the backdrop lay where
  the screen's picture is. Left to the owner's eye, or to a trace of the window's frame
  against the frame drawn, which needs code.
- The text's contrast over the see-through composer and cards, at each Opacity stop, in both
  appearances. Under the first look, the picture showing 40 % (Dim 60 %), text over the
  brightest and darkest bands was hard to read in both appearances; at 25 % and less it read
  everywhere. **Settled for the dither**: at its default 24 %, and at 35 %, the text reads
  cleanly over it in both appearances (`.superpowers/v/evidence/20261116-152949-dither` and
  `…-153057-dither-light`).
- What macOS names for a moving (aerial) wallpaper: a still the backdrop can use, or nothing,
  which shows the card. **Unverified**, and left so: no wallpaper is changed to find out,
  the agent desktop's included, as it's the owner's system. What the code does with it: a
  screen with no file NSWorkspace names, or a file ImageIO can't read (a movie, say), shows
  the card; why is logged, and that picture isn't tried again until something it's made from
  changes (`screens.rs`, `finished`). On
  the owner's QA list, as optional.
- That a downloaded wallpaper's file, under `com.apple.mobileAssetDesktop`, can be read by
  Koyo as it runs. **Settled**: the bundled app made screen 4's picture from the owner's
  `Radial Yellow.heic` (its log, 2026-11-16 14:19:37).
- The stops' values for Opacity, Cell size, Density and the fade, by eye against the
  reference (`.superpowers/ref/reference.jpg`, the owner's screenshot of Monodrive). The
  defaults are Monodrive's (24 %, 2 px, its density, 18 % to 72 %) and looked like the
  reference on screen; 35 % comes closer to its brightest parts. The owner's eye decides.

## 10. Order of work [proposed]

1. The layout of a wallpaper on a screen, as a function, with unit tests for each scaling.
   **Built**: `chat_backdrop/layout.rs`, `place` and `decode_longest`, tested for each
   layout of 3.2 and for the pixels a picture is decoded at.
2. Reading each screen's wallpaper and its options; making the picture: decoded, laid out,
   dimmed, compressed to ASTC, its encode time measured. **Built**: `wallpaper.rs` (read on
   the main thread, as AppKit asks of its screens), `make.rs` and `astc.rs`, tested by
   compressing a picture and reading its blocks back; times in section 9.
3. The renderer's texture mark, and the chat view drawing the picture cropped to its pane.
   Screenshots of the real app on this checkout. **Built**: the mark (section 4), the
   screens' pictures (`screens.rs`, `texture.rs`) and the chat's element (`element.rs`, the
   chat view's first child), the setting's values in `settings.json` (`settings/chat_backdrop.rs`).
   Shot in a test Koyo on the agent desktop on 2026-11-16, under the first look:
   `.superpowers/v/evidence/20261116-141927-backdrop` (dark) and `…-142530-backdrop-light`.
   Two chats side by side met without a seam, and where a pane reached past the screen's
   edge the card showed.
4. The blur's strips and the dots' tiles. **Built, then dropped for step 11.**
5. The see-through composer and cards. **Built** (section 1).
6. The settings, their rows and their search wording. **Built** (section 6), then mapped onto
   the dither in step 11. On screen the switches turned on and off by a click, and the rows
   hid and showed as section 6 says.
7. Noticing a change of wallpaper, Space, screen and settings. **Built** (3.3). On screen, a
   screen's picture was let go 30 s after no chat showed it, and made again in 353 ms when
   one did. A changed wallpaper and Space weren't tried, as no wallpaper is changed for it.
8. The drag measured; Koyo's own drag only if the slip shows. **Not measured** (section 9).
9. A visual check, as the tab switcher has (`docs/tab-switcher.md` 7): the shell in a real
   window off screen, over a wallpaper kept in the checkout rather than the person's, saving
   screenshots with the backdrop on and off, in both appearances, and at each Quality stop.
   **Wired** as the regression check (`chat_backdrop/visual.rs`): the wallpaper is painted into
   `.superpowers/qa/chat-backdrop/wallpaper.png` (a diagonal from amber to blue, soft discs, a
   grid of fine lines and a band of finer checks), and the window stands on a screen of the
   check's own, 2560 × 1440 points, at (480, 220), as a window off screen lies on no screen.
   It saves the backdrop light and dark, at each Quality stop with a crop of each where the
   picture is sharp, off in both appearances, and Appearance › Wallpaper background (Chat
   background before it showed behind more). Its windows
   open in the person's session, so it's run only while they're away (an idle time of 5
   minutes); this step's screenshots are taken in a test Koyo on the agent desktop instead.

   ```sh
   Q=$PWD/.superpowers/qa/chat-backdrop && mkdir -p $Q/tmp &&
     KOYO_AGENT_PROVIDER=fake KOYO_DATA_DIR=$Q/data TMPDIR=$Q/tmp KOYO_CONFIG_DIR=$Q/config \
     KOYO_LOG_DIR=$Q/logs script/cargo run -p koyo --features visual-check -- \
     --visual-check-chat-backdrop
   ```

   **Run** on 2026-11-17 at 00:00, on `fa5a15b26` (the surfaces and dynamic wallpapers, on
   browser-profiles' `d464078f2`), the owner idle for 765 s by `HIDIdleTime` read before the
   run: 11 pictures in 22 s, exit 0. The dither shows and fades down the chat in both
   appearances as on the agent desktop, off is the card alone, and Appearance › Wallpaper
   background lists its six surface rows. The page's switches and sliders are native views,
   which a window off screen doesn't capture, so the rows show without their controls.
10. The first look tried on screen. Done on 2026-11-16 (step 3's shots); the owner found it
    nothing like the reference.
11. The reference's look: Monodrive's dither and fade, drawn by the renderer's shader as the
    texture is sampled (section 4), the picture made without the dim, the blurred copy and
    dot tile gone, the settings mapped onto it (section 6). **Built** (`afab5daec`), tested
    headless through the real renderer (a mid-grey lights 8 cells of 16, density 1.5 lights
    12, the opacity and the fade as section 1 says), and shot in a test Koyo on the agent
    desktop on 2026-11-16, dark and light: the wallpaper in its own colours in 2 × 2 cells
    fixed to the screen, faint at the top, gone by three quarters down, the picture made in
    100 to 300 ms.
