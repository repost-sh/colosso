//! Arc layout (`docs/arc-sidebar.md`): the sidebar on the glass as Arc's. Under the top rail's
//! browser controls, an address bar says what's shown; then the pages pinned for every space as
//! tiles, which stay as a space swipes away; then the space: its loose pages, the selected
//! project's name and each of its worktrees as a folder, its tabs under its header, and at its
//! foot the space's projects as tiles. The spaces line the sidebar's foot, and two fingers swipe
//! between them.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    App, Context, Div, Hsla, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _,
    Styled as _, Window, div, px,
};

use crate::shell::Shell;
use crate::theme::{self, Palette, gray};

mod actions;
pub(crate) mod avatar;
mod card;
mod clear;
mod delete_space;
mod foot;
mod header;
mod hover;
mod loose;
mod pinned;
mod rename;
mod swipe;
#[cfg(test)]
mod tests;
mod tile_grid;
mod tiles;
mod top;
#[cfg(feature = "visual-check")]
pub mod visual;
mod worktrees;

pub use actions::{NextSpace, PreviousSpace, SPACES, bind_keys, handle_actions};
pub use hover::Hover;
pub use pinned::DraggedPin;
pub use swipe::Swipe;
pub use tile_grid::TileHand;
pub use top::controls;

/// Between the sidebar's parts: the address bar, the pinned pages, the space, its projects' tiles,
/// the foot.
const GAP: f32 = 10.;

/// The shades the Arc sidebar lays on the glass, brighter than the rest of the window's, as
/// Arc's are: white over the colours in either appearance, the tab shown a pill of it.
#[derive(Clone, Copy)]
pub(crate) struct Look {
    pub p: Palette,
    /// The address bar.
    pub field: Hsla,
    pub tile: Hsla,
    pub tile_hover: Hsla,
    pub tile_selected: Hsla,
    /// The tab shown.
    pub pill: Hsla,
    /// A row under the pointer: a tab's, a worktree's header.
    pub row_hover: Hsla,
    /// The space's name, a worktree's, and its header's `…` and `+`.
    pub label: Hsla,
}

impl Look {
    pub fn of(cx: &App) -> Look {
        let p = theme::palette(cx);
        if cx.theme().mode.is_dark() {
            Look {
                p,
                // As Arc's: white laid thin over the colours, so the pill takes their hue and
                // stands out by lightness, with no shadow to muddy it.
                field: gray(1., 0.1),
                tile: gray(1., 0.1),
                tile_hover: gray(1., 0.15),
                tile_selected: gray(1., 0.24),
                pill: gray(1., 0.18),
                row_hover: gray(1., 0.08),
                label: gray(1., 0.55),
            }
        } else {
            Look {
                p,
                field: gray(1., 0.4),
                tile: gray(1., 0.35),
                tile_hover: gray(1., 0.5),
                tile_selected: gray(1., 0.8),
                pill: gray(1., 0.6),
                row_hover: gray(1., 0.3),
                label: gray(0., 0.5),
            }
        }
    }
}

/// The colour the space at `index` shows as, readable on the glass whatever its theme: the
/// hue of its theme's first colour at a lightness that stands out in the appearance on screen,
/// as a theme's colour can be near black in the dark; the text's colour for a theme with none,
/// or a grey one.
pub(crate) fn space_colour(index: usize, cx: &App) -> Hsla {
    let p = theme::palette(cx);
    let dark = cx.theme().mode.is_dark();
    match crate::settings::get(cx).space_colour(index, dark) {
        Some(colour) if colour.s >= 0.1 => Hsla {
            h: colour.h,
            s: colour.s.max(0.6),
            l: if dark { 0.7 } else { 0.45 },
            a: 1.,
        },
        _ => p.text,
    }
}

/// The space shown as a dot in its colour, by the project's name (`docs/arc-sidebar.md` 4).
pub(crate) fn space_dot(colour: Hsla) -> Div {
    crate::pane::dot(colour).size(px(9.))
}

pub fn sidebar(shell: &Shell, window: &mut Window, cx: &mut Context<Shell>) -> impl IntoElement {
    let look = Look::of(cx);
    let width = crate::settings::get(cx).sidebar_width();
    let projects = shell.space_projects(cx);
    // What's in hand that the pinned or the loose pages take, once it has left where it was
    // taken from: worked out once, for both.
    let (pins, loosens) = shell.taken_elsewhere(cx);
    // The space moves as two fingers or the spring have it (`swipe.rs`): its loose pages, its
    // project's name and worktrees, and its projects' tiles under them, fainter the further it's
    // from where it rests.
    let space = div()
        .flex_1()
        .min_h_0()
        .relative()
        .left(px(shell.swipe.offset))
        .opacity(shell.swipe.opacity(width))
        .flex()
        .flex_col()
        .gap(px(GAP))
        .child(worktrees::space(shell, loosens, look, window, cx))
        .children(tiles::grid(shell, &projects, look, cx));
    div()
        .id("arc-sidebar")
        .debug_selector(|| "arc-sidebar".to_owned())
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .gap(px(GAP))
        .on_scroll(cx.listener(Shell::swipe))
        .on_mouse_down(MouseButton::Right, cx.listener(Shell::show_space_menu))
        .child(top::address_bar(shell, look, window, cx))
        // Outside the space, so they stay where they are as it swipes away.
        .children(pinned::region(shell, pins, look, cx))
        .child(space)
        .child(foot::bar(shell, window, cx))
}
