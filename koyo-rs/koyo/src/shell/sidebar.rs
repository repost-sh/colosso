//! The sidebar on the glass (`docs/layout.md`, Sidebar; `docs/sidebar.md`): its frame
//! (`frame.rs`); the projects' rows and their branches' rows (`projects.rs`); the tabs, while
//! **Tabs** puts them here (`tab_rows.rs`): the pinned pages' tiles (`pinned.rs`), the unpinned
//! pages' rows (`unpinned.rs`) and each branch's tabs' rows (`rows.rs`); the address bar with
//! back, forward and reload (`address.rs`); and the foot with the spaces' marks (`foot.rs`). The
//! top rail is made of some of the same pieces (`shell/top_rail.rs`), and what the spaces do is
//! `shell/spaces.rs`.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{App, Div, Hsla, InteractiveElement, Stateful, Styled};

use crate::shell::selectable;
use crate::theme::{self, Palette, gray};

mod address;
mod card;
pub(crate) mod foot;
mod frame;
mod hover;
mod mark_drag;
pub(crate) mod pinned;
mod projects;
pub(crate) mod rows;
mod tab_rows;
#[cfg(test)]
mod tests;
pub(crate) mod unpinned;
#[cfg(feature = "visual-check")]
pub mod visual;

pub use address::Top;
pub use frame::{SidebarFrame, SidebarResize, slide_out};
pub use hover::Hover;
pub use projects::sidebar;

/// The corners of a tile and a row alike.
const ROUNDED: f32 = 10.;

/// The shades the sidebar lays on the glass, brighter than the rest of the window's, as the Arc
/// browser's are: white over the colours in either appearance, the tab shown a pill of it.
#[derive(Clone, Copy)]
pub(crate) struct Look {
    pub p: Palette,
    /// The address bar.
    pub field: Hsla,
    pub tile: Hsla,
    pub tile_hover: Hsla,
    pub tile_selected: Hsla,
    /// The tab shown, and the branch selected.
    pub pill: Hsla,
    /// A row under the pointer, or under what it takes as that's in hand: a tab's, an unpinned
    /// page's, a project's, a branch's.
    pub row_hover: Hsla,
    /// What's said in the sidebar beside the rows rather than as one: what a chat's tab says
    /// under its title, the pinned pages' zone and the unpinned pages' strip, and their dashes.
    pub label: Hsla,
}

impl Look {
    pub fn of(cx: &App) -> Look {
        let p = theme::palette(cx);
        if cx.theme().mode.is_dark() {
            Look {
                p,
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

    /// `row` on the glass, as the Arc browser's rows are: the pill while it's `selected`, the
    /// tab shown or the branch selected, and lit under the pointer otherwise.
    pub fn lit(self, row: Stateful<Div>, selected: bool) -> Stateful<Div> {
        selectable(row, selected, (self.pill, self.row_hover))
    }
}

/// `element` unseen until the pointer's on its `group`, a row's: a row's × and buttons.
fn shown_on_hover<E: InteractiveElement + Styled>(element: E, group: &'static str) -> E {
    element.opacity(0.).group_hover(group, |style| style.opacity(1.))
}

/// `element`, noting where it's drawn as `name` says, for the visual check to find it
/// (`crate::visual::noting`).
#[cfg(feature = "visual-check")]
fn noted<E: gpui_kit::base::ElementExt>(element: E, name: impl FnOnce() -> String) -> E {
    element.on_prepaint(crate::visual::noting(name()))
}

/// `element` as it is: only the visual check notes where things are drawn.
#[cfg(not(feature = "visual-check"))]
fn noted<E>(element: E, _: impl FnOnce() -> String) -> E {
    element
}
