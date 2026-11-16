//! The pictures of the screens chats show the backdrop on: one a screen, made off the main
//! thread the first time a chat there asks for it, and again when what it's made from changes.
//! One is made at a time, the last one asked for next, and the last one made shows meanwhile.
//! How a picture looks is said to the renderer as it's drawn (`texture::Look`), so changing the
//! look makes nothing again. A picture is kept while a chat on its screen shows it, and let go
//! a while after none does ([`KEPT`]).

use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::{App, Global, Pixels, Task};
use objc2::MainThreadMarker;
use objc2_core_foundation::CFRetained;
use objc2_core_video::CVPixelBuffer;

use super::make::{self, Recipe};
use super::texture::{Look, Texture};
use super::wallpaper::{self, Wallpaper};
use crate::settings::Quality;

/// How long a screen's picture is kept once no chat shows it: a chat shown again within it,
/// after a terminal's tab say, shows its backdrop at once rather than the card while it's made.
const KEPT: Duration = Duration::from_secs(30);

#[derive(Default)]
struct Screens {
    by_display: HashMap<u64, Screen>,
    /// The screens Koyo's windows are on, as their display IDs and sizes in pixels.
    windows: Vec<(u64, u32, u32)>,
}

impl Global for Screens {}

#[derive(Default)]
struct Screen {
    /// The wallpaper as last read, none when the screen has no file `AppKit` names; and
    /// whether it has been read.
    wallpaper: Option<Wallpaper>,
    read: bool,
    shown: Option<Shown>,
    /// The picture being made.
    making: Option<Task<()>>,
    /// The recipe the chats on the screen asked for last.
    wanted: Option<Recipe>,
    /// The last recipe that couldn't be made: it isn't tried again until something in it
    /// changes.
    failed: Option<Recipe>,
    /// Held by each chat showing the backdrop on the screen, and since when none has.
    chats: Rc<()>,
    unheld_since: Option<Instant>,
}

/// A screen's picture, on the GPU.
struct Shown {
    recipe: Recipe,
    /// Kept for as long as the picture shows; dropped, it lets the GPU's copy go.
    texture: Texture,
    /// What a chat paints to draw it, and the look it was last marked with.
    placeholder: CFRetained<CVPixelBuffer>,
    look: Option<Look>,
}

/// What a chat holds while it shows a screen's backdrop.
pub struct Hold {
    pub display: u64,
    _chats: Rc<()>,
}

fn screen(display: u64, cx: &mut App) -> &mut Screen {
    let screens = cx.default_global::<Screens>();
    screens.by_display.entry(display).or_default()
}

/// The wallpaper of screen `display`, read the first time it's asked for.
pub fn wallpaper(display: u64, cx: &mut App) -> Option<Wallpaper> {
    let screen = screen(display, cx);
    if !screen.read {
        (screen.wallpaper, screen.read) = (read(display), true);
    }
    screen.wallpaper.clone()
}

/// The wallpaper `AppKit` names for screen `display`, from the main thread.
fn read(display: u64) -> Option<Wallpaper> {
    #[cfg(feature = "visual-check")]
    if let Some(stand_in) = super::visual::wallpaper() {
        return Some(stand_in);
    }
    wallpaper::read(u32::try_from(display).ok()?, MainThreadMarker::new()?)
}

/// A hold on screen `display`'s picture, for a chat showing it. The first since none held it
/// reads the wallpaper again, which nothing watched meanwhile.
pub fn hold(display: u64, cx: &mut App) -> Hold {
    let screen = screen(display, cx);
    if Rc::strong_count(&screen.chats) == 1 {
        (screen.read, screen.unheld_since) = (false, None);
    }
    Hold {
        display,
        _chats: screen.chats.clone(),
    }
}

/// What a chat paints to draw screen `display`'s picture as `recipe` says it, once it's made,
/// and the last one made meanwhile, looking as `look` says; and it's made. With no recipe, as
/// for a screen whose wallpaper can't be shown, nothing.
pub fn picture(
    display: u64,
    recipe: Option<Recipe>,
    look: Look,
    cx: &mut App,
) -> Option<CFRetained<CVPixelBuffer>> {
    let screen = screen(display, cx);
    let Some(recipe) = recipe else {
        (screen.shown, screen.wanted) = (None, None);
        return None;
    };
    if screen.wanted.as_ref() != Some(&recipe) {
        screen.wanted = Some(recipe);
        start(display, cx);
    }
    let shown = self::screen(display, cx).shown.as_mut()?;
    if shown.look != Some(look) {
        shown.texture.mark(&shown.placeholder, look);
        shown.look = Some(look);
    }
    Some(shown.placeholder.clone())
}

/// Makes the picture screen `display`'s chats asked for last, unless it's the one shown, one is
/// being made, or it couldn't be made.
fn start(display: u64, cx: &mut App) {
    let screen = screen(display, cx);
    let Some(wanted) = screen.wanted.clone() else {
        return;
    };
    let shown = screen.shown.as_ref().map(|shown| &shown.recipe);
    if screen.making.is_some() || shown == Some(&wanted) || screen.failed.as_ref() == Some(&wanted)
    {
        return;
    }
    let making = cx.spawn(async move |cx| {
        let recipe = wanted.clone();
        let made = cx
            .background_executor()
            .spawn(async move {
                let started = Instant::now();
                let made = make::make(&recipe)?;
                let texture = Texture::new(&made.picture).ok_or("the GPU can't hold it")?;
                Ok::<_, String>((texture, made.took, started.elapsed()))
            })
            .await;
        cx.update(|cx| finished(display, wanted, made, cx));
    });
    self::screen(display, cx).making = Some(making);
}

/// What making a screen's picture gave: the texture, how long decoding, laying out and
/// compressing took, and all of it.
type Made = Result<(Texture, [Duration; 3], Duration), String>;

/// Shows the picture made of `recipe` on screen `display`, or the card if it couldn't be
/// made, and makes the next one asked for meanwhile.
fn finished(display: u64, recipe: Recipe, made: Made, cx: &mut App) {
    let shown = made.and_then(|(texture, took, all)| {
        let placeholder = Texture::placeholder().ok_or("no pixel stands for it")?;
        let [decoded, laid_out, compressed] = took.map(|took| took.as_millis());
        log::info!(
            "made screen {display}'s chat backdrop in {} ms (decoded in {decoded} ms, laid out in \
             {laid_out} ms, compressed in {compressed} ms): {:.1} MB",
            all.as_millis(),
            texture.bytes as f64 / 1e6,
        );
        Ok(Shown {
            recipe: recipe.clone(),
            texture,
            placeholder,
            look: None,
        })
    });
    let screen = screen(display, cx);
    screen.making = None;
    match shown {
        Ok(shown) => (screen.shown, screen.failed) = (Some(shown), None),
        Err(why) => {
            log::warn!(
                "screen {display}'s chat backdrop couldn't be made from {}: {why}",
                recipe.wallpaper.path.display()
            );
            (screen.shown, screen.failed) = (None, Some(recipe));
        }
    }
    start(display, cx);
    cx.refresh_windows();
}

/// Looks again at what the screens' pictures are made from: the screens Koyo's windows are on,
/// and the wallpaper of each screen a chat shows the backdrop on, drawing the windows again when
/// either changed, so they ask for the pictures they then need; and lets the pictures no chat
/// has shown for [`KEPT`] go.
pub fn tick(cx: &mut App) {
    note_windows(cx);
    let now = Instant::now();
    let mut changed = false;
    for (&display, screen) in &mut cx.default_global::<Screens>().by_display {
        if Rc::strong_count(&screen.chats) > 1 {
            let wallpaper = if screen.read { read(display) } else { continue };
            if wallpaper != screen.wallpaper {
                log::info!("screen {display}'s wallpaper changed");
                (screen.wallpaper, changed) = (wallpaper, true);
            }
            continue;
        }
        let since = *screen.unheld_since.get_or_insert(now);
        if screen.shown.is_some() && now.saturating_duration_since(since) >= KEPT {
            log::info!("let screen {display}'s chat backdrop go: no chat shows it");
            (screen.shown, screen.wanted, screen.making) = (None, None, None);
        }
    }
    if changed {
        cx.refresh_windows();
    }
}

/// Notes the screens Koyo's windows are on, for the memory the Quality row names.
fn note_windows(cx: &mut App) {
    let mut screens: Vec<(u64, u32, u32)> = Vec::new();
    for handle in cx.windows() {
        let screen = handle.update(cx, |_, window, cx| {
            let display = window.display(cx)?;
            let size = display.bounds().size;
            let pixels = |points: Pixels| (points.as_f32() * window.scale_factor()).round() as u32;
            Some((
                u64::from(display.id()),
                pixels(size.width),
                pixels(size.height),
            ))
        });
        if let Ok(Some(screen)) = screen
            && !screens.iter().any(|(display, ..)| *display == screen.0)
        {
            screens.push(screen);
        }
    }
    screens.sort_unstable();
    let noted = &mut cx.default_global::<Screens>().windows;
    if *noted != screens {
        *noted = screens;
        cx.refresh_windows();
    }
}

/// What the pictures of the screens Koyo's windows are on take at `quality`, as the Quality
/// row says it; nothing before they're noted.
pub fn memory(quality: Quality, cx: &App) -> String {
    let screens = cx
        .try_global::<Screens>()
        .map_or(&[][..], |screens| &screens.windows);
    let bytes: usize = (screens.iter())
        .map(|&(_, width, height)| quality.bytes(width, height))
        .sum();
    let megabytes = bytes as f64 / 1e6;
    match screens.len() {
        0 => String::new(),
        1 => format!("About {megabytes:.1} MB on this screen."),
        count => format!("About {megabytes:.1} MB on these {count} screens."),
    }
}

/// Whether screen `display`'s picture is the one its chats asked for last, and nothing is
/// being made: for the visual check, which saves a picture of the window then.
#[cfg(feature = "visual-check")]
pub fn settled(display: u64, cx: &App) -> bool {
    let screen = cx
        .try_global::<Screens>()
        .and_then(|screens| screens.by_display.get(&display));
    screen.is_some_and(|screen| {
        let shown = screen.shown.as_ref().map(|shown| &shown.recipe);
        screen.making.is_none() && shown.is_some() && shown == screen.wanted.as_ref()
    })
}
