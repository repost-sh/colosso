//! The projects of the space shown, as Arc's favourites (`docs/arc-sidebar.md` 3): a tile each,
//! its picture centred, sharing the sidebar's width a row at a time. The selected project's is
//! lit, and as ⌥← and ⌥→ step along the tiles the light passes to the next (`glide`). An indigo
//! dot in its corner says something in the project waits for the person, a spinner that
//! something works. Hovered, it names the project and sums up what's in it; right-clicked, it can
//! move to another space, be renamed, take another picture, or go; dragged, it moves among its
//! space's tiles (`tile_grid`).

use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

use gpui_kit::base::ElementExt as _;
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Axis, Context, Div, ElementId, FontWeight,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Render, Stateful,
    StatefulInteractiveElement as _, Styled as _, Window, div, img, px,
};

use super::Look;
use super::actions::{
    ChangeProjectIcon, MoveProject, MoveProjectToNewSpace, RenameProject, ResetProjectIcon,
};
use super::avatar;
use super::glide::{self, Lit};
use super::hover::Hovered;
use super::tile_grid::TileGrid;
use super::worktrees::TAB_ICON;
use crate::chat::{Activity, spinner};
use crate::model::Project;
use crate::pane::dot;
use crate::projects::{project_items, with_menu};
use crate::settings;
use crate::shell::Shell;
use crate::theme::Palette;

/// A tile's height, the room between tiles, and the picture in one, the size of a tab row's
/// icon: the pinned pages' tiles are the same (`pinned.rs`).
pub(super) const TILE: f32 = 44.;
pub(super) const GAP: f32 = 8.;
pub(super) const PICTURE: f32 = TAB_ICON;

/// How many tiles a row holds for `count` projects, or pinned pages: up to four, three for five
/// or six, so the rows come out even.
pub(super) fn per_row(count: usize) -> usize {
    match count {
        5 | 6 => 3,
        count => count.clamp(1, 4),
    }
}

/// The tiles of the space shown, for its `projects`, in a grid they're dragged about in; none
/// while it has no projects. The selected project's is lit: a project the space doesn't hold
/// isn't among them; at `now` the light may be passing from one tile to the next.
pub fn grid(
    shell: &Shell,
    projects: &[usize],
    look: Look,
    now: Instant,
    cx: &mut Context<Shell>,
) -> Option<TileGrid> {
    if projects.is_empty() {
        return None;
    }
    let selected = shell.workspace.active_project();
    let tiles = (projects.iter())
        .map(|&index| {
            let key = key_of(&shell.workspace.projects[index].path);
            let lit = (selected == Some(index), shell.swipe.glides.tiles.lit(key, now));
            let tile = tile(shell, (index, key), lit, look, cx);
            (key, tile.into_any_element())
        })
        .collect();
    let on_drop = cx.listener(|this, &(from, to): &(usize, usize), _, cx| {
        this.move_tile(from, to, cx);
    });
    let rows = (per_row(projects.len()), px(GAP));
    Some(TileGrid::new(
        "arc-tile-grid",
        tiles,
        rows,
        shell.tile_hand.clone(),
        Rc::new(on_drop),
    ))
}

/// What a project's tile, or a worktree's place in the sidebar, is known by as it moves,
/// whatever its place: its folder's.
pub(super) fn key_of(path: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

/// A tile or a worktree in hand. Nothing's drawn for it under the pointer: its grid or list
/// draws it there.
pub(super) struct DraggedTile;

impl Render for DraggedTile {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui_kit::Empty
    }
}

impl Shell {
    /// A tile of the space shown let go over another slot: its project moves there among the
    /// space's, in the sidebar's order, which is saved and which the Orca list shows too.
    fn move_tile(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        let tiles = self.space_projects(cx);
        if self.workspace.move_project_among(&tiles, from, to) {
            self.save_projects(cx);
            cx.notify();
        }
    }
}

/// What's in the project's corner: a dot while something waits for the person, a spinner
/// while something works.
fn corner(shell: &Shell, project: &Project, p: Palette, cx: &mut App) -> Option<Div> {
    match shell.project_activity(project, cx) {
        Activity::Quiet => None,
        Activity::Working => {
            let place = format!("tile-{}", project.path.display());
            Some(spinner(place, cx))
        }
        Activity::Attention => Some(dot(p.waiting)),
    }
}

/// The project's picture: the one picked or its git host's; its initials until there's one.
fn picture(project: &Project, p: Palette, cx: &mut App) -> AnyElement {
    match avatar::picture(&project.path, cx) {
        Some(picture) => img(picture)
            .size(px(PICTURE))
            .rounded(px(4.))
            .into_any_element(),
        None => div()
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(p.muted)
            .child(project.initials())
            .into_any_element(),
    }
}

/// A tile's frame, a project's or a pinned page's alike (`pinned.rs`): a rounded square that
/// lights under the pointer, lit when it's the one `selected`, lifted off its grid while it's in
/// hand, and noting where it's drawn for the card of what's `hovered`, which a press puts away.
pub(super) fn tile_frame(
    id: impl Into<ElementId>,
    hovered: Hovered,
    (selected, lifted): (bool, bool),
    shell: &Shell,
    look: Look,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    div()
        .id(id)
        .min_w_0()
        .h(px(TILE))
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(super::ROUNDED))
        .cursor_pointer()
        .when(selected, |tile| tile.bg(look.tile_selected))
        .when(!selected, |tile| {
            tile.bg(look.tile).hover(|style| style.bg(look.tile_hover))
        })
        // In hand, it's lifted off the grid, over the tiles it passes.
        .when(lifted, |tile| tile.bg(look.tile_selected).shadow_md())
        // Where it's drawn, for its card; which comes a moment after the pointer rests on it.
        .on_prepaint(shell.arc_hover.noting(hovered))
        .on_hover(cx.listener(move |this, over: &bool, window, cx| {
            this.hover(hovered, *over, window, cx);
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| this.press_hovered(cx)),
        )
}

/// The tile of the project at `index` in the workspace, known in its space's grid by `key`: lit
/// when it's `selected`, or as far as the light passing through it has it.
fn tile(
    shell: &Shell,
    (index, key): (usize, u64),
    (selected, passing): (bool, Option<Lit>),
    look: Look,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let p = look.p;
    let project = &shell.workspace.projects[index];
    let path = project.path.clone();
    let corner = corner(shell, project, p, cx);
    let face = picture(project, p, cx);
    let hand = shell.tile_hand.clone();
    let lifted = hand.holding() == Some(key);
    let tile = tile_frame(
        ("arc-tile", index),
        Hovered::Tile(index),
        (selected && passing.is_none(), lifted),
        shell,
        look,
        cx,
    )
    .debug_selector(move || format!("arc-tile-{index}"))
    .on_drag(DraggedTile, move |_, grab, _, cx| {
        hand.take(key, grab);
        cx.new(|_| DraggedTile)
    })
    // Under the picture: its part of the light passing, laid over the unlit tile so it shows as
    // a lit one does.
    .children(passing.and_then(|lit| {
        let colour = glide::over(look.tile, look.tile_selected);
        let light = glide::light(lit, Axis::Horizontal, colour)?;
        Some(light.debug_selector(move || format!("arc-tile-light-{index}")))
    }))
    .child(face)
    .children(corner.map(|corner| div().absolute().top(px(6.)).right(px(6.)).child(corner)))
    .on_click(cx.listener(move |this, _, _, cx| this.select_project(index, cx)));
    with_menu(tile, move |_, cx| tile_menu(&path, cx))
}

/// The menu a tile opens (`docs/arc-sidebar.md` 3).
fn tile_menu(path: &Path, cx: &App) -> NativeMenu {
    let settings = settings::get(cx);
    let here = settings.space_of(path);
    let owned = || PathBuf::from(path);
    let spaces = (settings.spaces.iter().enumerate())
        .fold(NativeMenu::new(), |menu, (index, space)| {
            menu.menu_with_check(
                space.name.clone(),
                here == Some(index),
                Box::new(MoveProject(owned(), index)),
            )
        })
        .separator()
        .menu("New Space…", Box::new(MoveProjectToNewSpace(owned())));
    let mut menu = NativeMenu::new()
        .submenu("Move to", spaces)
        .separator()
        .menu("Rename…", Box::new(RenameProject(owned())))
        .menu("Change Icon…", Box::new(ChangeProjectIcon(owned())));
    if settings.project_icon(path).is_some() {
        menu = menu.menu("Reset Icon", Box::new(ResetProjectIcon(owned())));
    }
    project_items(menu.separator(), path)
}

#[cfg(test)]
mod tests {
    use super::per_row;

    #[test]
    fn rows_come_out_even() {
        assert_eq!(per_row(1), 1);
        assert_eq!(per_row(2), 2);
        assert_eq!(per_row(4), 4);
        assert_eq!(per_row(5), 3);
        assert_eq!(per_row(6), 3);
        assert_eq!(per_row(7), 4);
        assert_eq!(per_row(12), 4);
    }
}
