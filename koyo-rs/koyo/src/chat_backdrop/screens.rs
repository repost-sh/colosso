//! The pictures of the screens chats show the backdrop on: one a screen, made off the main
//! thread the first time a chat there asks for it, and again when what it's made from changes.
//! One is made at a time, the last one asked for next, and the last one made shows meanwhile,
//! so moving the Dim slider along shows each stop in turn rather than the card between them.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use gpui_kit::{App, Global, Task};
use objc2::MainThreadMarker;
use objc2_core_foundation::CFRetained;
use objc2_core_video::CVPixelBuffer;

use super::make::{self, Recipe};
use super::texture::Texture;
use super::wallpaper::{self, Wallpaper};

#[derive(Default)]
struct Screens {
    by_display: HashMap<u64, Screen>,
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
    /// Held by each chat showing the backdrop on the screen.
    chats: Rc<()>,
}

/// A screen's picture, on the GPU.
struct Shown {
    recipe: Recipe,
    /// Kept for as long as the picture shows; dropped, it lets the GPU's copy go.
    _texture: Texture,
    /// What a chat paints to draw it.
    placeholder: CFRetained<CVPixelBuffer>,
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
    wallpaper::read(u32::try_from(display).ok()?, MainThreadMarker::new()?)
}

/// A hold on screen `display`'s picture, for a chat showing it.
pub fn hold(display: u64, cx: &mut App) -> Hold {
    Hold {
        display,
        _chats: screen(display, cx).chats.clone(),
    }
}

/// What a chat paints to draw screen `display`'s picture, as `recipe` says it once it's made
/// and the last one made meanwhile; and it's made. With no recipe, as for a screen whose
/// wallpaper can't be shown, nothing.
pub fn picture(display: u64, recipe: Option<Recipe>, cx: &mut App) -> Option<CFRetained<CVPixelBuffer>> {
    let screen = screen(display, cx);
    let Some(recipe) = recipe else {
        (screen.shown, screen.wanted) = (None, None);
        return None;
    };
    if screen.wanted.as_ref() != Some(&recipe) {
        screen.wanted = Some(recipe);
        start(display, cx);
    }
    let screen = self::screen(display, cx);
    screen.shown.as_ref().map(|shown| shown.placeholder.clone())
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

type Made = Result<(Texture, [std::time::Duration; 3], std::time::Duration), String>;

/// Shows the picture made of `recipe` on screen `display`, or the card if it couldn't be
/// made, and makes the next one asked for meanwhile.
fn finished(display: u64, recipe: Recipe, made: Made, cx: &mut App) {
    let screen = screen(display, cx);
    screen.making = None;
    let placeholder = made.and_then(|(texture, took, all)| {
        let placeholder = texture.placeholder().ok_or("no pixel stands for it")?;
        let [decoded, laid_out, compressed] = took.map(|took| took.as_millis());
        log::info!(
            "made screen {display}'s chat backdrop in {} ms (decoded in {decoded}, laid out in \
             {laid_out}, compressed in {compressed}): {:.1} MB",
            all.as_millis(),
            texture.bytes as f64 / 1e6,
        );
        Ok((texture, placeholder))
    });
    match placeholder {
        Ok((texture, placeholder)) => {
            screen.shown = Some(Shown {
                recipe,
                _texture: texture,
                placeholder,
            });
            screen.failed = None;
        }
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
