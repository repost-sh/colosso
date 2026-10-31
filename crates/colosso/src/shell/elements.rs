//! The pieces the window and its sidebars are drawn from: buttons on the glass, rows, the
//! search field, and the menu of new tabs.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, App, Context, Div, ElementId, Entity, FocusHandle, Hsla, InteractiveElement as _,
    IntoElement, MouseButton, MouseDownEvent, ParentElement as _, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled, div, px,
};

use super::{NewBrowserTab, NewChat, NewEditorTab, NewTerminal, Shell};
use crate::menu::{self, MenuItem};
use crate::model::PaneKind;
use crate::pane::close_button;
use crate::projects::AddProject;
use crate::theme::{self, Palette};

fn new_tab_action(kind: PaneKind) -> Box<dyn Action> {
    match kind {
        PaneKind::Chat => Box::new(NewChat),
        PaneKind::Terminal => Box::new(NewTerminal),
        PaneKind::Browser => Box::new(NewBrowserTab),
        PaneKind::Editor => Box::new(NewEditorTab),
    }
}

/// Opens a menu under `trigger` offering a tab of each kind for the selected branch unit.
pub fn new_tab_menu(
    id: impl Into<ElementId>,
    trigger: Button,
    target: FocusHandle,
) -> impl IntoElement {
    menu::dropdown(id, trigger, target, || {
        PaneKind::ALL
            .into_iter()
            .map(|kind| MenuItem {
                icon: kind.icon(),
                label: kind.new_tab_label().into(),
                action: new_tab_action(kind),
            })
            .collect()
    })
}

/// Empty rail space that moves the window, since the app owns titlebar dragging.
pub(super) fn drag_area(id: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .h_full()
        .on_mouse_down(MouseButton::Left, |event: &MouseDownEvent, window, _| {
            if event.click_count == 2 {
                window.titlebar_double_click();
            } else {
                window.start_window_move();
            }
        })
}

pub fn icon_button(
    id: impl Into<ElementId>,
    icon: IconName,
    tooltip: impl Into<SharedString>,
    cx: &App,
) -> Button {
    glass_button(id, icon, cx).tooltip(tooltip)
}

/// An icon button that dispatches `action`; its tooltip shows the action's keybinding.
pub fn action_button(
    id: &'static str,
    icon: IconName,
    tooltip: impl Into<SharedString>,
    action: impl Action,
    cx: &App,
) -> Button {
    glass_button(id, icon, cx)
        .tooltip_with_action(tooltip, &action, None)
        .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
}

/// An icon on the glass, shaded from the palette rather than gpui-component's opaque theme, so
/// hovering, pressing and selecting it tint it the way they tint the sidebar's rows.
fn glass_button(id: impl Into<ElementId>, icon: IconName, cx: &App) -> Button {
    glass(id, theme::palette(cx).text, cx).icon(icon)
}

/// A button on the glass with whatever it's given to show, in `foreground`.
pub fn glass(id: impl Into<ElementId>, foreground: Hsla, cx: &App) -> Button {
    let p = theme::palette(cx);
    Button::new(id).custom(
        ButtonCustomVariant::new(cx)
            .foreground(foreground)
            .hover(p.hover)
            .active(p.selected),
    )
}

pub fn icon(name: IconName, size: f32) -> Icon {
    Icon::new(name).size(px(size))
}

/// A selectable row: highlighted when selected, tinted on hover otherwise.
pub fn row(id: impl Into<ElementId>, selected: bool, p: Palette) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(theme::ROW_HEIGHT))
        .flex_shrink_0()
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .rounded(px(6.))
        .text_sm()
        .cursor_pointer()
        .when(selected, |row| row.bg(p.selected))
        .when(!selected, |row| row.hover(|style| style.bg(p.hover)))
}

/// The × that closes the tab at `(project, branch, tab)` without selecting it.
pub fn tab_close_button(
    id: impl Into<ElementId>,
    (project, branch, tab): (usize, usize, usize),
    p: Palette,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    close_button(id, p).on_click(cx.listener(move |this, _, _, cx| {
        cx.stop_propagation();
        this.close_tab_at((project, branch, tab), cx);
    }))
}

/// The sidebar's search field, which narrows its worktrees as it's typed in.
pub fn search_field(search: &Entity<InputState>, p: Palette) -> Div {
    div()
        .h(px(theme::ROW_HEIGHT))
        .px_1()
        .flex()
        .items_center()
        .text_sm()
        .child(
            Input::new(search)
                .appearance(false)
                .small()
                .prefix(icon(IconName::Search, 14.).text_color(p.muted))
                .flex_1(),
        )
}

/// What a sidebar says where a search found no worktrees.
pub fn no_matches(p: Palette) -> Div {
    div()
        .h(px(theme::ROW_HEIGHT))
        .px_2()
        .flex()
        .items_center()
        .text_sm()
        .text_color(p.muted)
        .child("No worktrees match.")
}

pub fn section_label(p: Palette) -> Div {
    div()
        .h(px(24.))
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .text_xs()
        .text_color(p.muted)
}

/// The row that adds a first project, where a sidebar would list them.
pub fn add_project_row(id: &'static str, p: Palette) -> Stateful<Div> {
    row(id, false, p)
        .text_color(p.muted)
        .on_click(|_, window, cx| window.dispatch_action(Box::new(AddProject), cx))
        .child(icon(IconName::FolderPlus, 14.))
        .child("Add project")
}

/// Why a project's worktrees could not be read, under its name.
pub fn project_error(error: SharedString, p: Palette) -> Div {
    div()
        .px_2()
        .py_1()
        .text_xs()
        .text_color(p.danger)
        .child(error)
}

pub fn ellipsized(text: impl IntoElement) -> Div {
    div().min_w_0().truncate().child(text)
}
