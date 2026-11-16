//! The pictures of the screens chats show the backdrop on: one a screen, made off the main
//! thread the first time a chat there asks for it, and again when what it's made from changes.
//! One is made at a time, the last one asked for next, and the last one made shows meanwhile,
//! so moving the Dim slider along shows each stop in turn rather than the card between them.
//! Each has its blur, made with it and again from its small copy when the blur's strength
//! changes. A picture is kept while a chat on its screen shows it, and let go a while after
//! none does ([`KEPT`]).

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::{App, Global, RenderImage, Task};
use objc2::MainThreadMarker;
use objc2_core_foundation::CFRetained;
use objc2_core_video::CVPixelBuffer;

use super::blur::Small;
use super::make::{self, Recipe};
use super::texture::Texture;
use super::wallpaper::{self, Wallpaper};
use crate::settings::Quality;
use crate::viewer::bitmap;

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
    /// The recipe the chats on the screen asked for last, and the blur: its sigma in the small
    /// copy's pixels, none for none.
    wanted: Option<Recipe>,
    wanted_blur: Option<f32>,
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
    _texture: Texture,
    /// What a chat paints to draw it.
    placeholder: CFRetained<CVPixelBuffer>,
    /// The small copy its blur is made from, the blur with its sigma, and the sigma of the
    /// blur being made.
    small: Arc<Small>,
    blur: Option<(f32, Arc<RenderImage>)>,
    blurring: Option<(f32, Task<()>)>,
}

/// What a chat paints its screen's backdrop with.
pub struct Showing {
    pub placeholder: CFRetained<CVPixelBuffer>,
    pub blur: Option<Arc<RenderImage>>,
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

/// What a chat paints to draw screen `display`'s picture, as `recipe` says it once it's made
/// and the last one made meanwhile, with its blur `blur` wide, a sigma in its small copy's
/// pixels; and they're made. With no recipe, as for a screen whose wallpaper can't be shown,
/// nothing.
pub fn picture(
    display: u64,
    recipe: Option<Recipe>,
    blur: Option<f32>,
    cx: &mut App,
) -> Option<Showing> {
    let screen = screen(display, cx);
    screen.wanted_blur = blur;
    let Some(recipe) = recipe else {
        let shown = screen.shown.take();
        screen.wanted = None;
        let_go(shown, cx);
        return None;
    };
    if screen.wanted.as_ref() != Some(&recipe) {
        screen.wanted = Some(recipe);
        start(display, cx);
    }
    blur_as_wanted(display, cx);
    let shown = self::screen(display, cx).shown.as_ref()?;
    Some(Showing {
        placeholder: shown.placeholder.clone(),
        blur: shown.blur.as_ref().map(|(_, image)| image.clone()),
    })
}

/// Lets `shown` go: its texture, and its blur out of every window's atlas.
fn let_go(shown: Option<Shown>, cx: &mut App) {
    let blur = shown.and_then(|shown| shown.blur).map(|(_, image)| image);
    bitmap::release(blur.into_iter().collect(), cx);
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
    let sigma = screen.wanted_blur;
    let making = cx.spawn(async move |cx| {
        let recipe = wanted.clone();
        let made = cx
            .background_executor()
            .spawn(async move {
                let started = Instant::now();
                let made = make::make(&recipe)?;
                let texture = Texture::new(&made.picture).ok_or("the GPU can't hold it")?;
                let blur =
                    sigma.and_then(|sigma| Some((sigma, made.small.blurred(sigma).image()?)));
                let took = (made.took, started.elapsed());
                Ok::<_, String>(Made {
                    texture,
                    small: Arc::new(made.small),
                    blur,
                    took,
                })
            })
            .await;
        cx.update(|cx| finished(display, wanted, made, cx));
    });
    self::screen(display, cx).making = Some(making);
}

/// What making a screen's picture gave.
struct Made {
    texture: Texture,
    small: Arc<Small>,
    blur: Option<(f32, Arc<RenderImage>)>,
    /// How long decoding, laying out and compressing took, and all of it.
    took: ([Duration; 3], Duration),
}

/// Shows the picture made of `recipe` on screen `display`, or the card if it couldn't be
/// made, and makes the next one asked for meanwhile.
fn finished(display: u64, recipe: Recipe, made: Result<Made, String>, cx: &mut App) {
    let shown = made.and_then(|made| {
        let placeholder = made.texture.placeholder().ok_or("no pixel stands for it")?;
        let ([decoded, laid_out, compressed], all) = made.took;
        log::info!(
            "made screen {display}'s chat backdrop in {} ms (decoded in {} ms, laid out in {} ms, \
             compressed in {} ms): {:.1} MB",
            all.as_millis(),
            decoded.as_millis(),
            laid_out.as_millis(),
            compressed.as_millis(),
            made.texture.bytes as f64 / 1e6,
        );
        Ok(Shown {
            recipe: recipe.clone(),
            _texture: made.texture,
            placeholder,
            small: made.small,
            blur: made.blur,
            blurring: None,
        })
    });
    let screen = screen(display, cx);
    screen.making = None;
    let old = match shown {
        Ok(shown) => {
            screen.failed = None;
            screen.shown.replace(shown)
        }
        Err(why) => {
            log::warn!(
                "screen {display}'s chat backdrop couldn't be made from {}: {why}",
                recipe.wallpaper.path.display()
            );
            screen.failed = Some(recipe);
            screen.shown.take()
        }
    };
    let_go(old, cx);
    start(display, cx);
    blur_as_wanted(display, cx);
    cx.refresh_windows();
}

/// Blurs screen `display`'s picture as its chats asked for last, unless it's blurred so or
/// being blurred; with no blur asked for, lets its blur go.
fn blur_as_wanted(display: u64, cx: &mut App) {
    let screen = screen(display, cx);
    let wanted = screen.wanted_blur;
    let Some(shown) = screen.shown.as_mut() else {
        return;
    };
    let Some(sigma) = wanted else {
        let old = shown.blur.take().map(|(_, image)| image);
        shown.blurring = None;
        bitmap::release(old.into_iter().collect(), cx);
        return;
    };
    let blurred = shown.blur.as_ref().map(|(sigma, _)| *sigma);
    if blurred == Some(sigma) || shown.blurring.is_some() {
        return;
    }
    let small = shown.small.clone();
    let task = cx.spawn(async move |cx| {
        let from = small.clone();
        let image = cx
            .background_executor()
            .spawn(async move { from.blurred(sigma).image() })
            .await;
        cx.update(|cx| blurred_as(display, &small, sigma, image, cx));
    });
    if let Some(shown) = self::screen(display, cx).shown.as_mut() {
        shown.blurring = Some((sigma, task));
    }
}

/// Shows `image`, the blur `sigma` wide of the picture whose small copy is `small`, on screen
/// `display`, if that picture still shows; and blurs it again if a blur of another width has
/// been asked for meanwhile.
fn blurred_as(
    display: u64,
    small: &Arc<Small>,
    sigma: f32,
    image: Option<Arc<RenderImage>>,
    cx: &mut App,
) {
    let screen = screen(display, cx);
    let shown = (screen.shown.as_mut()).filter(|shown| Arc::ptr_eq(&shown.small, small));
    let old = match (shown, image) {
        (Some(shown), Some(image)) => {
            shown.blurring = None;
            shown.blur.replace((sigma, image)).map(|(_, old)| old)
        }
        (Some(shown), None) => {
            shown.blurring = None;
            None
        }
        (None, image) => image,
    };
    bitmap::release(old.into_iter().collect(), cx);
    blur_as_wanted(display, cx);
    cx.refresh_windows();
}

/// Looks again at what the screens' pictures are made from: the screens Koyo's windows are on,
/// and the wallpaper of each screen a chat shows the backdrop on, drawing the windows again when
/// either changed, so they ask for the pictures they then need; and lets the pictures no chat
/// has shown for [`KEPT`] go.
pub fn tick(cx: &mut App) {
    note_windows(cx);
    let now = Instant::now();
    let (mut changed, mut gone) = (false, Vec::new());
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
            gone.push(screen.shown.take());
            (screen.wanted, screen.making) = (None, None);
        }
    }
    for shown in gone {
        let_go(shown, cx);
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
            let pixels = |points: gpui_kit::Pixels| (points.as_f32() * window.scale_factor()).round() as u32;
            Some((u64::from(display.id()), pixels(size.width), pixels(size.height)))
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
    let screens = cx.try_global::<Screens>().map_or(&[][..], |screens| &screens.windows);
    let bytes: usize = (screens.iter()).map(|&(_, width, height)| quality.bytes(width, height)).sum();
    let megabytes = bytes as f64 / 1e6;
    match screens.len() {
        0 => String::new(),
        1 => format!("About {megabytes:.1} MB on this screen."),
        count => format!("About {megabytes:.1} MB on these {count} screens."),
    }
}

/// Whether screen `display`'s picture and its blur are the ones its chats asked for last, and
/// nothing is being made: for the visual check, which saves a picture of the window then.
#[cfg(feature = "visual-check")]
pub fn settled(display: u64, cx: &App) -> bool {
    let Some(screen) = cx.try_global::<Screens>().and_then(|screens| screens.by_display.get(&display))
    else {
        return false;
    };
    let Some(shown) = &screen.shown else {
        return false;
    };
    let blur = shown.blur.as_ref().map(|(sigma, _)| *sigma);
    screen.making.is_none()
        && shown.blurring.is_none()
        && screen.wanted.as_ref() == Some(&shown.recipe)
        && blur == screen.wanted_blur
}
