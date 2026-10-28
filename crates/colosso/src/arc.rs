//! Arc layout: one project per space on the glass, its branch units with their tabs
//! listed beneath, and a project switcher at the bottom. Two fingers swipe between projects.

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Context, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, px,
};

use crate::model::BranchKind;
use crate::shell::{Shell, ellipsized, icon, icon_button, row};
use crate::theme;

pub fn sidebar(shell: &Shell, cx: &mut Context<Shell>) -> impl IntoElement {
    let workspace = &shell.workspace;
    let project_index = workspace.active_project;
    let project = workspace.project();

    let space = div()
        .flex_1()
        .min_h_0()
        .relative()
        .left(px(shell.swipe.unwrap_or(0.)))
        .flex()
        .flex_col()
        .gap_px()
        .child(
            div()
                .h(px(28.))
                .px_2()
                .flex()
                .items_center()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .child(project.name.clone()),
        )
        .child(
            div()
                .h(px(theme::ROW_HEIGHT))
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded(px(8.))
                .bg(theme::hover())
                .text_sm()
                .text_color(theme::muted())
                .child(icon(IconName::Search, 14.))
                .child("Search"),
        )
        .children(
            project
                .branches
                .iter()
                .enumerate()
                .map(|(branch_index, branch)| {
                    div()
                        .mt_2()
                        .flex()
                        .flex_col()
                        .gap_px()
                        .child(
                            div()
                                .h(px(24.))
                                .px_2()
                                .flex()
                                .items_center()
                                .gap_2()
                                .text_xs()
                                .text_color(theme::muted())
                                .child(icon(IconName::GitBranch, 12.))
                                .child(ellipsized(branch.name.clone()))
                                .when(branch.kind == BranchKind::Checkout, |header| {
                                    header.child("checkout")
                                }),
                        )
                        .children(branch.tabs.iter().enumerate().map(|(tab_index, tab)| {
                            let selected = branch_index == project.active_branch
                                && tab_index == branch.active_tab;
                            row(("arc-tab", branch_index * 100 + tab_index), selected)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.workspace.select_tab(
                                        project_index,
                                        branch_index,
                                        tab_index,
                                    );
                                    cx.notify();
                                }))
                                .child(icon(tab.icon(), 13.).text_color(theme::muted()))
                                .child(ellipsized(tab.title()))
                        }))
                }),
        );

    let switcher = div()
        .h(px(32.))
        .flex_shrink_0()
        .px_1()
        .flex()
        .items_center()
        .gap_1()
        .children(
            workspace
                .projects
                .iter()
                .enumerate()
                .map(|(index, project)| {
                    let active = index == project_index;
                    div()
                        .id(("arc-space", index))
                        .size(px(26.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(6.))
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .cursor_pointer()
                        .when(active, |space| space.bg(theme::selected()))
                        .when(!active, |space| {
                            space
                                .text_color(theme::muted())
                                .hover(|style| style.bg(theme::hover()))
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.workspace.active_project = index;
                            cx.notify();
                        }))
                        .child(project.initials())
                }),
        )
        .child(icon_button("arc-new-space", IconName::Plus, "New project"));

    div()
        .id("arc-sidebar")
        .w(px(theme::SIDEBAR_WIDTH - 20.))
        .flex_shrink_0()
        .h_full()
        .pr(px(theme::GUTTER))
        .pb(px(theme::GUTTER))
        .flex()
        .flex_col()
        .overflow_hidden()
        .on_scroll(cx.listener(Shell::swipe))
        .child(space)
        .child(switcher)
}
