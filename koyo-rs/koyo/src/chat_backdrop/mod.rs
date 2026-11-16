//! The backdrop behind a chat (`docs/chat-backdrop.md`), and behind files, diffs, terminals, the
//! explorers and devices: the part of the screen's wallpaper the pane covers, dithered in its own
//! colours, faint, and fading out towards the bottom, as Monodrive's Dither effect looks
//! (github.com/donnes/monodrive, MIT).

mod astc;
mod dynamic;
mod element;
mod layout;
mod make;
mod screens;
#[cfg(test)]
mod tests;
mod texture;
#[cfg(feature = "visual-check")]
pub mod visual;
mod wallpaper;
mod watch;

use gpui_kit::{AnyElement, App, ElementId, Hsla, IntoElement as _, ParentElement, Styled};

pub use astc::Blocks;
pub use screens::memory;
pub use watch::init;

use crate::model::{Pane, PaneKind};
use crate::native::liquid;
use crate::settings::{self, Surface};
use crate::theme::Palette;

/// The backdrop behind `surface`, filling its parent, which must be `relative`; none while it's
/// turned off there, and the card shows. `key` tells it from the others drawn in the window.
pub fn backdrop(surface: Surface, key: u64, cx: &App) -> Option<AnyElement> {
    let settings = settings::get(cx).chat_backdrop;
    let id = ElementId::NamedInteger("chat-backdrop".into(), key);
    settings
        .shows(surface)
        .then(|| element::Picture { settings, id }.into_any_element())
}

/// The backdrop behind what `pane` shows; none behind a page, whose view covers it.
pub fn behind(pane: &Pane, cx: &App) -> Option<AnyElement> {
    let surface = match pane.kind {
        PaneKind::Browser => return None,
        PaneKind::Chat => Surface::Chat,
        PaneKind::Terminal => Surface::Terminal,
        PaneKind::Editor if pane.shows_changes() => Surface::Diff,
        PaneKind::Editor => Surface::Files,
        PaneKind::Desktop | PaneKind::Device => Surface::Devices,
    };
    backdrop(surface, pane.id, cx)
}

/// `surface`, floating over a chat as [`liquid::floating`] has it, but see-through over the
/// backdrop where there's no glass ([`fill`]).
pub fn floating<E: ParentElement + Styled>(
    surface: E,
    glass: Option<AnyElement>,
    p: Palette,
    cx: &App,
) -> E {
    liquid::floating_filled(surface, glass, fill(p, cx), p)
}

/// The fill of what floats over a chat where there's no glass: over the backdrop, the card's
/// own colour, as see-through as the card is, so the backdrop shows through as it does through
/// glass; solid otherwise (`Palette::floating`).
fn fill(p: Palette, cx: &App) -> Hsla {
    if settings::get(cx).chat_backdrop.shows(Surface::Chat) {
        p.card
    } else {
        p.floating()
    }
}
