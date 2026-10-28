//! Orca layout: projects and their branch units inside the card, tabs along the top.

use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, px,
};

use crate::model::{BranchKind, BranchUnit};
use crate::shell::{
    NewChat, Shell, action_button, ellipsized, icon, row, search_row, section_label,
};
use crate::theme::{self, Palette};

pub fn sidebar(shell: &Shell, cx: &mut Context<Shell>) -> impl IntoElement {
    let p = theme::palette(cx);
    let workspace = &shell.workspace;
    div()
        .w(px(theme::SIDEBAR_WIDTH))
        .flex_shrink_0()
        .h_full()
        .p_2()
        .flex()
        .flex_col()
        .gap_px()
        .border_r_1()
        .border_color(p.hairline)
        .child(search_row(p))
        .child(section_label(p).mt_2().child("Projects"))
        .children(
            workspace
                .projects
                .iter()
                .enumerate()
                .map(|(project_index, project)| {
                    div()
                        .flex()
                        .flex_col()
                        .gap_px()
                        .child(
                            div()
                                .h(px(theme::ROW_HEIGHT))
                                .px_2()
                                .flex()
                                .items_center()
                                .gap_2()
                                .text_sm()
                                .child(icon(IconName::Folder, 14.).text_color(p.muted))
                                .child(project.name.clone()),
                        )
                        .children(project.branches.iter().enumerate().map(
                            |(branch_index, branch)| {
                                let selected = project_index == workspace.active_project
                                    && branch_index == project.active_branch;
                                branch_row(project_index, branch_index, branch, selected, p, cx)
                            },
                        ))
                }),
        )
}

fn branch_row(
    project_index: usize,
    branch_index: usize,
    branch: &BranchUnit,
    selected: bool,
    p: Palette,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    row(
        ("orca-branch", project_index * 100 + branch_index),
        selected,
        p,
    )
    .pl_6()
    .when(!selected, |row| row.text_color(p.muted))
    .on_click(cx.listener(move |this, _, _, cx| {
        this.workspace.select_branch(project_index, branch_index);
        cx.notify();
    }))
    .child(icon(IconName::GitBranch, 13.))
    .child(ellipsized(branch.name.clone()))
    .when(branch.kind == BranchKind::Checkout, |row| {
        row.child(div().text_xs().text_color(p.muted).child("checkout"))
    })
}

pub fn tab_strip(shell: &Shell, cx: &mut Context<Shell>) -> impl IntoElement {
    let p = theme::palette(cx);
    let workspace = &shell.workspace;
    let project_index = workspace.active_project;
    let branch_index = workspace.project().active_branch;
    let branch = workspace.branch();
    div()
        .h(px(theme::TAB_STRIP_HEIGHT))
        .flex_shrink_0()
        .px_2()
        .flex()
        .items_center()
        .gap_1()
        .border_b_1()
        .border_color(p.hairline)
        .children(branch.tabs.iter().enumerate().map(|(tab_index, tab)| {
            let active = tab_index == branch.active_tab;
            row(("orca-tab", tab_index), active, p)
                .h(px(28.))
                .min_w_0()
                .max_w(px(220.))
                .pl_2()
                .pr_1()
                .when(!active, |tab| tab.text_color(p.muted))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.workspace
                        .select_tab(project_index, branch_index, tab_index);
                    cx.notify();
                }))
                .child(icon(tab.icon(), 13.))
                .child(ellipsized(tab.title()))
                .child(
                    div()
                        .id(("orca-tab-close", tab_index))
                        .size(px(18.))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(4.))
                        .hover(|style| style.bg(p.hover))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.workspace
                                .close_tab(project_index, branch_index, tab_index);
                            cx.notify();
                        }))
                        .child(icon(IconName::X, 12.)),
                )
        }))
        .child(action_button("orca-new-tab", IconName::Plus, "New chat", NewChat).small())
}
