//! The pictures of the screens chats show the backdrop on: one a screen, made off the main
//! thread the first time a chat there asks for it, and again when what it's made from changes.
//! One is made at a time, the last one asked for next, and the last one made shows meanwhile.
//! How a picture looks is said to the renderer as it's drawn (`Texture::mark`), so changing the
//! look makes nothing again. A picture is kept while a chat on its screen shows it, and let go
//! a while after none does ([`KEPT`]).

use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use futures::channel::mpsc::UnboundedSender;
use gpui_kit::{App, Global, Task};
use objc2::MainThreadMarker;
use objc2_core_foundation::CFRetained;
use objc2_core_video::CVPixelBuffer;

use super::make::{self, Recipe};
use super::texture::Texture;
use super::wallpaper::{self, Wallpaper};
use crate::settings::{ChatBackdrop, Quality};

/// How long a screen's picture is kept once no chat shows it: a chat shown again within it,
/// after a terminal's tab say, shows its backdrop at once rather than the card while it's made.
const KEPT: Duration = Duration::from_secs(30);
/// How often the wallpapers are looked at while they're watched (`watch.rs`).
const EVERY: Duration = Duration::from_secs(1);

#[derive(Default)]
struct Screens {
    by_display: HashMap<u64, Screen>,
    /// What has the watch look at the screens again at once (`watch.rs`).
    tell: Option<Tell>,
}

type Tell = UnboundedSender<()>;

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
    look: Option<ChatBackdrop>,
}

/// What a chat holds while it shows a screen's backdrop. The first taken since none was, and
/// the last let go, tell the watch, which looks at a screen's wallpaper only while it's held.
pub struct Hold {
    pub display: u64,
    chats: Rc<()>,
    tell: Option<Tell>,
}

impl Drop for Hold {
    fn drop(&mut self) {
        if Rc::strong_count(&self.chats) == 2 {
            tell(self.tell.as_ref());
        }
    }
}

fn tell(watch: Option<&Tell>) {
    if let Some(watch) = watch {
        watch.unbounded_send(()).ok();
    }
}

/// Has `watch` told whenever the screens are to be looked at again at once, in place of the
/// watch told before, which ends: an app set up again in a test starts a second.
pub fn listen(watch: Tell, cx: &mut App) {
    cx.default_global::<Screens>().tell = Some(watch);
}

/// Has the watch look at the wallpapers again at once: a window was seen again, after a time
/// nobody could see one, when one may have changed unwatched.
pub fn look_again(cx: &App) {
    tell(cx.try_global::<Screens>().and_then(|screens| screens.tell.as_ref()));
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
        if screen.wallpaper.is_none() {
            log::warn!(
                "screen {display} has no wallpaper file AppKit names: its chat backdrop shows \
                 the card"
            );
        }
    }
    screen.wallpaper.clone()
}

/// The wallpaper `AppKit` names for screen `display`, from the main thread.
fn read(display: u64) -> Option<Wallpaper> {
    #[cfg(feature = "visual-check")]
    if let Some(stand_in) = super::visual::wallpaper() {
        return Some(stand_in);
    }
    #[cfg(debug_assertions)]
    if let Some(path) = std::env::var_os("KOYO_WALLPAPER") {
        return Some(stand_in(path.into()));
    }
    wallpaper::read(u32::try_from(display).ok()?, MainThreadMarker::new()?)
}

/// A debug build's wallpaper for every screen, from `KOYO_WALLPAPER`: a test Koyo on a desktop
/// with no wallpaper still has one to show, and no one's wallpaper is changed for it.
#[cfg(debug_assertions)]
fn stand_in(path: std::path::PathBuf) -> Wallpaper {
    let modified = std::fs::metadata(&path)
        .and_then(|file| file.modified())
        .ok();
    Wallpaper {
        image: super::dynamic::image(&path, modified),
        path,
        modified,
        scaling: super::layout::Scaling::Fill,
        fill: [0., 0., 0.],
    }
}

/// A hold on screen `display`'s picture, for a chat showing it. The first since none held it
/// reads the wallpaper again, which nothing watched meanwhile.
pub fn hold(display: u64, cx: &mut App) -> Hold {
    let watch = cx.default_global::<Screens>().tell.clone();
    let screen = screen(display, cx);
    if Rc::strong_count(&screen.chats) == 1 {
        (screen.read, screen.unheld_since) = (false, None);
        tell(watch.as_ref());
    }
    Hold {
        display,
        chats: screen.chats.clone(),
        tell: watch,
    }
}

/// What a chat paints to draw screen `display`'s picture as `recipe` says it, once it's made,
/// and the last one made meanwhile, looking as `look` says; and it's made. With no recipe, as
/// for a screen whose wallpaper can't be shown, nothing.
pub fn picture(
    display: u64,
    recipe: Option<Recipe>,
    look: ChatBackdrop,
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
        shown.texture.mark(&shown.placeholder, &look);
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
type Ready = Result<(Texture, [Duration; 3], Duration), String>;

/// Shows the picture made of `recipe` on screen `display`, or the card if it couldn't be
/// made, and makes the next one asked for meanwhile.
fn finished(display: u64, recipe: Recipe, made: Ready, cx: &mut App) {
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
    redraw(cx);
}

/// Draws every window again, so the chats in them ask for the pictures they then need.
fn redraw(cx: &mut App) {
    #[expect(clippy::disallowed_methods, reason = "the pictures are read untracked in paint")]
    cx.refresh_windows();
}

/// Looks again at the wallpaper of each screen a chat shows the backdrop on, as the watch does
/// (`watch.rs`), drawing the windows again when one changed; and lets the pictures no chat has
/// shown for [`KEPT`] go. How long until it's to run again unless the watch is told sooner: none
/// while nothing waits on it.
pub fn tick(cx: &mut App) -> Option<Duration> {
    let seen = any_window_seen(cx);
    let now = cx.background_executor().now();
    let mut changed = false;
    let screens = &mut cx.default_global::<Screens>().by_display;
    let next = (screens.iter_mut())
        .filter_map(|(&display, screen)| {
            if Rc::strong_count(&screen.chats) > 1 {
                if !seen {
                    return None;
                }
                if screen.read {
                    let wallpaper = read(display);
                    if wallpaper != screen.wallpaper {
                        log::info!("screen {display}'s wallpaper changed");
                        (screen.wallpaper, changed) = (wallpaper, true);
                    }
                }
                return Some(EVERY);
            }
            let since = *screen.unheld_since.get_or_insert(now);
            let left = KEPT.saturating_sub(now.saturating_duration_since(since));
            if left.is_zero() && screen.shown.is_some() {
                log::info!("let screen {display}'s chat backdrop go: no chat shows it");
                (screen.shown, screen.wanted, screen.making) = (None, None, None);
            }
            screen.shown.is_some().then_some(left)
        })
        .min();
    if changed {
        redraw(cx);
    }
    next
}

/// Whether a person can see any of Koyo's windows.
fn any_window_seen(cx: &mut App) -> bool {
    (cx.windows().into_iter()).any(|window| {
        (window.update(cx, |_, window, _| crate::sight::window_seen(window))).unwrap_or(false)
    })
}

/// What the pictures of the screens Koyo's windows are on take at `quality`, as the Quality
/// row says it, from `AppKit` as the row is drawn; nothing off the main thread.
pub fn memory(quality: Quality) -> String {
    let screens = MainThreadMarker::new().map(wallpaper::screens_of_windows);
    let screens = screens.unwrap_or_default();
    let bytes: usize = (screens.iter())
        .map(|&(width, height)| quality.bytes(width, height))
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
