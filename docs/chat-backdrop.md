# Koyo's chat backdrop: the wallpaper behind a chat, under a dot screen

Written 2026-11-16 on branch `main`, and built on `schulzfel/chat-backdrop` in the order of
section 10, which says what of it is built and checked so far. Drafted with Claude, from
research in the code and an interview with the owner. Keep it in step with
`koyo-rs/koyo/src/chat_backdrop/`. A chat's background shows the desktop
picture that lies behind the window, dimmed, under a fixed grid of dots, and blurred towards
the bottom, as in the owner's reference: a disk overview whose top half shows the picture
sharp under the dots and whose bottom half fades into a plain blur. The picture is drawn
once for each screen and only moved as the window moves, so a frame costs what drawing one
texture costs. What the owner decided is marked **[asked]**; what was proposed while
researching it, and is the owner's to change, **[proposed]**. Section 9 lists what can only be
settled once it's built.

## 1. What it shows

- **The screen's wallpaper, where it lies behind the window.** Each chat shows the part of
  the picture its pane covers on the screen, laid out as macOS lays it out (section 3.2). Not
  the windows behind Koyo's: those would need the screen captured live (section 8).
  **[asked]**
- **Dimmed towards the chat's own colour**, the card it sits on (`theme/card.rs`): dark in
  the dark appearance, pale in the light one, so the text over it stays readable either way.
  How far is the Dim setting. **[asked]**
- **A dot screen over it**, as in the reference: the picture itself stays smooth, and a fixed
  grid of dots lies over it. It's a screen, not a dither of the picture's colours. **[asked]**
  - The grid is staggered, each row's dots between the last row's, as in the reference.
    **[proposed]**
  - The dots are darker than the picture under them, in both appearances. **[proposed]**
  - The grid is fixed to the screen, not to the pane, so it holds still as the window moves
    and two chats side by side show one grid. **[proposed]**
- **Blurred towards the bottom.** A blurred copy of the dimmed picture fades in from where
  the blur starts, half way down the pane by default, to the bottom, where it's all there is.
  **[asked]**
- **The dots fade out as the blur fades in**, so the bottom is a plain blur, as in the
  reference. **[asked]**
- **The composer and the floating cards are see-through** over it: the prompt, the details
  card and the floating prompt take a translucent fill, so the blur shows behind them. Their
  text's contrast is checked at every Dim stop (section 9). **[asked]** Where macOS has
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

- **In chat tabs only**: a chat and a subagent's chat, in a pane of their own or one of a
  split. Terminals, files, pages and the rest keep the card. **[asked: chats; proposed: the
  rest]**
- The chat view has no background of its own (`chat/view.rs`, `impl Render for ChatView`): it
  lies on the pane's card, which rounds its corners and clips what it holds
  (`theme/card.rs`, `island`). The backdrop is the chat view's first child, absolute and
  filling it, so the card's corners clip it too. **[proposed]**
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
  colour), the screen's size and scale, the card's colour (so the appearance and the glass
  theme), Dim, and Quality. Off the main thread. **[proposed]**
- **Kept on the GPU as an ASTC texture, the only copy.** Uncompressed, the picture for a
  2560 × 1440 pt screen at 2× is 5120 × 2880 pixels, 59 MB, and GPUI keeps an image twice,
  once in the app's memory to upload it again and once in the GPU's atlas: 118 MB for one
  element. **The owner found that unacceptable for a UI element.** **[asked]**
  1. The wallpaper file is decoded by the system's decoders, as the viewer decodes a picture
     (`viewer/picture.rs`): HEIC, JPEG, PNG and the rest, at no more pixels than the layout
     needs, upright, in sRGB.
  2. It's drawn into a bitmap the screen's size, laid out as macOS lays it out (3.2), with
     Quartz (`viewer/bitmap.rs`, `Canvas`), and dimmed towards the card's colour.
  3. macOS's own encoder compresses it to ASTC: an `org.khronos.astc` image destination in
     ImageIO, its block size set by `kCGImagePropertyASTCBlockSize` (4 × 4 or 8 × 8, the only
     two ImageIO names). The result is a 16-byte header, then the blocks, 16 bytes each
     (`chat_backdrop/astc.rs`). The encoder writes the picture's bottom row of blocks first,
     as OpenGL counts rows, where the GPU reads the first row as the top one, so the picture
     is drawn into its bitmap upside down and reaches the GPU upright (`make.rs`,
     `upside_down`); a test reads the blocks back in the file's order and finds it upright,
     dimmed as asked.
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
  again when one does. The blur is 1.2 MB, as GPUI holds an image twice, the small copy it's
  blurred from, kept to blur again, 0.6 MB, and a dot tile 0.5 MB (section 4).
  **[asked: 8 × 8 at full resolution; proposed: letting go]**
- **Quality is the person's to pick** (section 6): four stops, always compressed, never the
  uncompressed picture. **[asked]**

  | Stop | Blocks | Resolution | On that screen |
  |---|---|---|---|
  | Smallest | 8 × 8 | half | 0.9 MB |
  | | 4 × 4 | half | 3.7 MB |
  | **Default** | 8 × 8 | full | 3.7 MB |
  | Sharpest | 4 × 4 | full | 14.7 MB |

  The two middle stops cost the same. If they look the same once built, one goes and three
  stops stay (section 9).

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

### 3.3 Noticing a change

- macOS has no public notice that the wallpaper changed. Koyo asks `NSWorkspace` for the
  wallpaper of each screen a chat shows the backdrop on once a second, a few calls that cost
  next to nothing, and again at once when the Space changes
  (`NSWorkspaceActiveSpaceDidChangeNotification`): each Space can have a picture of its own.
  A change in what's asked (3.1) makes the picture again. **[proposed]** Built so
  (`chat_backdrop/watch.rs`, `screens.rs`, `tick`): a changed wallpaper draws the windows
  again, and each chat on that screen asks for the picture of the new one. A screen no chat
  had shown is read again when one does.
- The settings and the appearance are heard as the rest of the app hears them
  (`settings::observe`, `theme::follow_window_appearance`). Built: a change of the backdrop's
  settings draws the windows again; the appearance, a window moved to another screen and a
  screen's new size or scale draw them again already (GPUI's own refresh), and each chat
  then asks for the picture they call for.
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
  frame (`draw_page_surface` and `page_surface_fragment`, `vendor/README.md`). The fragment
  shader samples whatever texture it's given, compressed or not. What it lacks is a way to be
  given a texture Koyo made: it draws only pixel buffers, which can't hold a compressed format.
  So one more mark, beside a picture's, a capture's and a pane's (`Mark` in
  `metal_renderer.rs`): a placeholder BGRA pixel buffer marked as standing for a texture the
  app registered with the renderer, drawn with the page pipeline over the screen's rectangle,
  clipped to the pane. A `Koyo:` change, described in `vendor/README.md` as the others are.
  **[proposed]** Built as proposed: `register_texture` makes the texture of the blocks from
  any thread, on the system's GPU, as `ASTC_4x4_LDR` or `ASTC_8x8_LDR` (section 9), and gives
  it a number; `set_surface_texture` marks a pixel standing for it (`Mark::Texture`);
  `release_texture` lets it go. A test draws a picture so through the real renderer, headless,
  and finds it where it was laid out, its colours as they were, and nothing past the pane
  (`chat_backdrop/tests.rs`).
- **The blur** is a blurred copy of the dimmed picture, small (it's smooth, so about a tenth
  of the screen's size, about 1 MB), drawn as an ordinary GPUI image. GPUI can't fade an
  image with a gradient, but it applies an element's opacity to the images in it, so the blur
  is a stack of thin strips across the lower part of the pane, each a little more opaque than
  the one above it, from nothing where the blur starts to whole at the bottom. Since the
  copy is smooth, the strips' steps don't show. The surface path has no opacity, which is why
  the blur isn't drawn through it. **[proposed]** Built as proposed (`chat_backdrop/strips.rs`,
  `blur.rs`): the copy is a tenth of the screen's pixels a side (512 × 288 on the owner's
  screen, 0.6 MB, held twice as a GPUI image), made with the picture from its bitmap, and
  blurred by three box blurs one after another, which come within a tenth of a Gaussian's
  width, a sixth at the softest stop; Blur strength is that Gaussian's sigma, as CSS's
  `blur()` takes it. A change of strength blurs the copy again, which is kept, without making
  the picture again. The fade is 32 strips, each a thirty-second more opaque. Each strip draws
  the whole blur, as big as the screen, clipped to the strip, so no strip cuts the image and
  none of them moves it by a part of a pixel.
- **The dots** are a small tile, repeated across the pane as `backdrop::grain` repeats its
  grain, one tile pixel a screen pixel, its origin on the screen's so the grid holds still.
  Over the blur its strips fade out as the blur's fade in. **[proposed]** Built as proposed
  (`dots.rs`, `strips.rs`): the tile is about 256 pixels a side, whole dots across and whole
  pairs of rows down, black, each dot's coverage measured at 16 points a pixel. A row's dots
  are an even number of pixels apart, so the next row's sit half way between them on whole
  pixels, and the rows are a triangle's height apart, so each dot is as far from the six
  round it. A dot an even number of pixels wide sits on a pixel's corner, an odd one on its
  middle, so either fills whole pixels. Density takes the dots from five of their widths
  apart to two and a half, and their darkness from 18 % to 50 %; the stops are still to be
  tuned by eye (section 9). Tiles are made the first time a size, density and scale ask for
  one, and the last four are kept.
- **Order, bottom up:** the picture, the blur's strips, the dots' strips, then the chat.
- **While dragging, the backdrop may slip by a frame.** Koyo's window is dragged by macOS
  (`start_window_move`, which is `performWindowDragWithEvent`), which moves it before Koyo
  hears of it, so the picture is a frame behind until it catches up. The first version
  accepts that, and it's measured on the real window (section 9). If it shows, Koyo drags the
  window itself from its mouse events, and draws that frame in the same Core Animation
  transaction as the move (`presentsWithTransaction`, only while dragging, as it costs frame
  rate). **[asked: accept it at first; proposed: the fix]**

## 5. What a frame costs

Once the picture is made, a frame draws one texture clipped to the pane, the blur's strips
and the dots' tiles, all sampled by the GPU. Nothing is decoded, blurred or compressed while
the window moves, resizes or scrolls. The picture is made again only when what it's made from
changes (3.1), off the main thread. The card shows until a screen's first picture is ready;
after that the last one made shows until the next is, so moving the Dim slider along shows
each stop in turn rather than the card between them. One picture of a screen is made at a
time, and the last one asked for next (`chat_backdrop/screens.rs`). **[proposed]**

## 6. Settings

In Settings → Appearance, a section of its own, "Chat background", under "Theme". **[asked:
on by default, customizable, in Appearance]**

| Row | Control | Default | Shown |
|---|---|---|---|
| Chat background | switch | on | always |
| Dim | slider, Less to More | proposed 60% | when on |
| Dot size | slider, Fine to Coarse | proposed 1 pt | when on |
| Density | slider, Sparse to Dense: the grid's spacing and how dark its dots are | proposed as the reference | when on |
| Blur | switch | on | when on |
| Blur strength | slider, Soft to Strong | proposed 24 pt | when on and Blur is on |
| Blur starts | slider, High to Low | half way | when on and Blur is on |
| Quality | slider, Smallest to Sharpest, the memory the stop takes on the person's screens under it | 8 × 8, full | when on |

Built (`settings_view/chat_backdrop.rs`, `settings/chat_backdrop.rs`), the stops for now: Dim
30, 45, 60, 75 and 90 %; Dot size ½, 1, 1½ and 2 pt; Density 0, ¼, ½, ¾ and 1, from five dot
widths apart and 18 % dark to two and a half and 50 % (section 4); Blur strength 8, 16, 24, 36
and 48 pt; Blur starts a quarter, three eighths, half, five eighths and three quarters of the
way down; Quality its four stops (3.1). They're still to be tuned by eye (section 9).

- **Rows show only when they apply**, not greyed out: with the switch off, the section is the
  switch alone. **[asked]** The settings pages are built from the settings as they draw, so a
  row left out is simply not added; the MCP server page does the same with its port and
  clients (`settings_view/mcp_server.rs`). **[proposed: the blur's own rows following the
  same rule]**
- **Search still finds it.** Settings search looks at the rows shown, so with the switch off
  "dim" or "blur" would find nothing. The switch's description names what it reveals, for
  instance "Your wallpaper, dimmed under a dot screen behind chats. Turn it on to set the dim,
  the dot size and density, the blur, its strength and where it starts, and the quality."
  **[proposed]** Built so: off, the switch's description says that; on, its first sentence
  alone. The Blur switch does the same for its two rows while it's off ("Turn it on to set
  its strength and where it starts."). A test searches the section on, with the blur off,
  and off, for each row's word.
- **The Quality row's memory** is worked out from the sizes of the screens Koyo's windows are
  on, summed for more than one: "About 3.7 MB on this screen." **[asked: shown; proposed: how]**
  Built so, after "How sharp the wallpaper behind chats is kept, always compressed."; "on these
  2 screens" for two. The screens are noted once a second (`screens.rs`, `note_windows`), as a
  settings page is drawn from the settings alone, without its window.
- Sliders are `NativeSlider` over stops, switches `NativeSwitch` (`settings_view/`), each
  value kept in `settings.json` as the stop's value, as the other sliders keep theirs. The
  stops' values are tuned by eye against the reference once built. **[proposed]**
- In `settings.json` **[proposed]**:

  ```json
  "chat_backdrop": {
    "on": true,
    "dim": 0.6,
    "dot_size": 1.0,
    "density": 0.5,
    "blur": true,
    "blur_strength": 24.0,
    "blur_start": 0.5,
    "quality": "astc8_full"
  }
  ```

## 7. What it builds on

| Need | Already there |
|---|---|
| Rendering an image once on the CPU and drawing it as a GPUI image | `backdrop.rs` (the glass theme's gradient and grain) |
| Decoding any wallpaper format at a set size | `viewer/picture.rs` (ImageIO) |
| Drawing it laid out into a bitmap | `viewer/bitmap.rs` (`Canvas`, Quartz) |
| A tile repeated one pixel to a screen pixel | `backdrop::grain` |
| Drawing a texture over one rectangle, clipped to another, rounded | vendored `metal_renderer.rs`, `draw_page_surface` |
| Hearing the window move and resize | `cx.observe_window_bounds`, used by `shell.rs` and `popup.rs` |
| Rounded clipping of the chat | `theme/card.rs`, `island` |
| A settings row and its controls | `settings_view/components.rs`, `native_slider.rs`, `native_switch.rs` |

Where it would go **[proposed]**: a module of its own, `koyo-rs/koyo/src/chat_backdrop/`, a
file for each job (reading the wallpaper, laying it out, making the picture, the dots, the
blur, the element), each under the 500 lines `script/hygiene` holds them to; the setting in
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
- **A true ordered dither** (Bayer) of the picture's colours, and **halftone dots** whose
  size follows its brightness. The owner picked the dot screen, as in the reference.
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
  (`make.rs`, `making_a_screens_picture_takes`, run with `--ignored`.)
- Whether 8 × 8 blocks show under the dim and the dots, against 4 × 4 at half resolution,
  which costs the same, by screenshots side by side. If they look alike, one of the two
  middle stops goes. Measured so far, against the picture uncompressed and dimmed 60 %, on
  the owner's wallpaper: 8 × 8 full 40.9 dB, 4 × 4 half 39.7 dB, 4 × 4 full 47.0 dB, 8 × 8
  half 36.7 dB. The screenshots are still to be taken.
- Whether the ASTC texture is sampled as sRGB or not: GPUI's atlas holds plain BGRA and
  blends its colours as they are, so the texture's format should match, or the picture comes
  out darker. **Settled**: not as sRGB. Drawn through the renderer headless, a picture
  dimmed to 128 comes out 128 as `ASTC_8x8_LDR`, and 54 as `ASTC_8x8_sRGB`, which takes the
  colours out of sRGB as it samples them (`the_gpu_draws_a_screens_picture_as_it_was_laid_out_and_only_where_its_pane_is`;
  the sRGB format tried once and put back).
- How far the backdrop slips while the window is dragged, recorded on the real window.
- The text's contrast over the see-through composer and cards, at each Dim stop, in both
  appearances.
- What macOS names for a moving (aerial) wallpaper: a still the backdrop can use, or nothing,
  which shows the card.
- That a downloaded wallpaper's file, under `com.apple.mobileAssetDesktop`, can be read by
  Koyo as it runs.
- The stops' values for Dim, Dot size, Density and the blur, by eye against the reference.

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
   The screenshots wait for the agent desktop.
4. The blur's strips and the dots' tiles. **Built**: `blur.rs`, `dots.rs`, `strips.rs`;
   tested for the blur's width, the dots' places in their tile and the strips meeting from
   where the blur starts to the bottom. How they look waits for the agent desktop.
5. The see-through composer and cards. **Built** (section 1); how they read waits for the
   agent desktop.
6. The settings, their rows and their search wording. **Built** (section 6); the stops'
   values wait to be tuned by eye.
7. Noticing a change of wallpaper, Space, screen and settings. **Built** (3.3); how it does
   on screen waits for the agent desktop.
8. The drag measured; Koyo's own drag only if the slip shows.
9. A visual check, as the tab switcher has (`docs/tab-switcher.md` 7): the shell in a real
   window off screen, over a wallpaper kept in the checkout rather than the person's, saving
   screenshots with the backdrop on and off, in both appearances, and at each Quality stop.
