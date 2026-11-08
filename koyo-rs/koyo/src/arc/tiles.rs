//! The projects of the space shown, as Arc's favourites (`docs/arc-sidebar.md` 3): a tile each,
//! its picture centred, sharing the sidebar's width a row at a time. The selected project's is
//! lit. A red dot in its corner says something in the project waits for the person, a spinner
//! that something works. Hovered, it names the project and sums up what's in it; right-clicked,
//! it can move to another space, be renamed, take another picture, or go.

use std::path::{Path, PathBuf};

use gpui_kit::base::ElementExt as _;
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, Div, FontWeight, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, div, img, px,
};

use super::Look;
use super::actions::{
    ChangeProjectIcon, MoveProject, MoveProjectToNewSpace, RenameProject, ResetProjectIcon,
};
use super::avatar;
use super::hover::Hovered;
use crate::chat::{Activity, spinner};
use crate::model::Project;
use crate::pane::dot;
use crate::projects::{project_items, with_menu};
use crate::settings;
use crate::shell::Shell;
use crate::theme::Palette;

const TILE: f32 = 44.;
const GAP: f32 = 8.;
const PICTURE: f32 = 20.;

/// How many tiles a row holds for `count` projects: up to four, three for five or six, so the
/// rows come out even.
fn per_row(count: usize) -> usize {
    match count {
        5 | 6 => 3,
        count => count.clamp(1, 4),
    }
}

/// The tiles of the space shown, for its `projects`; none while it has no projects. The
/// selected project's is lit: a project the space doesn't hold isn't among them.
pub fn grid(shell: &Shell, projects: &[usize], look: Look, cx: &mut Context<Shell>) -> Option<Div> {
    if projects.is_empty() {
        return None;
    }
    let per_row = per_row(projects.len());
    let selected = shell.workspace.active_project();
    let mut rows = Vec::new();
    for row in projects.chunks(per_row) {
        let mut tiles: Vec<AnyElement> = Vec::new();
        for &index in row {
            tiles.push(tile(shell, index, selected == Some(index), look, cx).into_any_element());
        }
        // A short last row keeps the tiles as wide as the others'.
        tiles.extend((row.len()..per_row).map(|_| div().flex_1().into_any_element()));
        rows.push(div().flex().gap(px(GAP)).children(tiles));
    }
    Some(
        div()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .children(rows),
    )
}

/// What's in the project's corner: a dot while something waits for the person, a spinner
/// while something works.
fn corner(shell: &Shell, project: &Project, p: Palette, cx: &mut App) -> Option<Div> {
    let activity = (project.branches.iter())
        .map(|unit| shell.checkout_activity(&unit.worktree.path, cx))
        .max()?;
    match activity {
        Activity::Quiet => None,
        Activity::Working => {
            let place = format!("tile-{}", project.path.display());
            Some(spinner(place, cx))
        }
        Activity::Attention => Some(dot(p.danger)),
    }
}

/// The project's picture: the one picked or its git host's; its initials until there's one.
fn picture(project: &Project, p: Palette, cx: &mut App) -> AnyElement {
    match avatar::picture(&project.path, cx) {
        Some(picture) => img(picture)
            .size(px(PICTURE))
            .rounded(px(5.))
            .into_any_element(),
        None => div()
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(p.muted)
            .child(project.initials())
            .into_any_element(),
    }
}

fn tile(
    shell: &Shell,
    index: usize,
    selected: bool,
    look: Look,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let p = look.p;
    let project = &shell.workspace.projects[index];
    let path = project.path.clone();
    let corner = corner(shell, project, p, cx);
    let face = picture(project, p, cx);
    let tile = div()
        .id(("arc-tile", index))
        .debug_selector(move || format!("arc-tile-{index}"))
        .flex_1()
        .min_w_0()
        .h(px(TILE))
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(10.))
        .cursor_pointer()
        .when(selected, |tile| tile.bg(look.tile_selected))
        .when(!selected, |tile| {
            tile.bg(look.tile).hover(|style| style.bg(look.tile_hover))
        })
        .child(face)
        .children(corner.map(|corner| div().absolute().top(px(6.)).right(px(6.)).child(corner)))
        // Where it's drawn, for its card; which comes a moment after the pointer rests on it.
        .on_prepaint(shell.arc_hover.noting(Hovered::Tile(index)))
        .on_hover(cx.listener(move |this, hovered: &bool, window, cx| {
            this.hover(Hovered::Tile(index), *hovered, window, cx);
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| this.press_hovered(cx)),
        )
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
