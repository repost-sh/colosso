//! Orca layout: projects and their branch units in the sidebar inside the card, tabs along
//! the top.

use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Axis, ClickEvent, Context, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, div, px,
};

use crate::model::BranchUnit;
use crate::projects::{AddProject, NewWorktreeIn, project_menu, with_menu, worktree_menu};
use crate::shell::{
    Shell, action_button, add_project_row, ellipsized, icon, icon_button, new_tab_menu, no_matches,
    project_error, row, search_field, section_label, tab_close_button, tab_list, tab_row,
};
use crate::theme::{self, Palette};

pub fn sidebar(shell: &Shell, cx: &mut Context<Shell>) -> impl IntoElement {
    let p = theme::palette(cx);
    let workspace = &shell.workspace;
    let query = shell.search_query(cx);
    // Each project with the worktrees the search leaves it; all of them while it's empty.
    let shown: Vec<_> = workspace
        .projects
        .iter()
        .enumerate()
        .map(|(index, project)| (index, project, project.shown_branches(&query)))
        .filter(|(_, _, branches)| query.trim().is_empty() || !branches.is_empty())
        .collect();
    div()
        .flex()
        .flex_col()
        .gap_px()
        .child(search_field(&shell.search, p))
        .child(
            section_label(p)
                .mt_2()
                .pr_0()
                .child(div().flex_1().child("Projects"))
                .child(
                    action_button(
                        "orca-add-project",
                        IconName::Plus,
                        "Add project",
                        AddProject,
                        cx,
                    )
                    .xsmall(),
                ),
        )
        .when(workspace.projects.is_empty(), |sidebar| {
            sidebar.child(add_project_row("orca-first-project", p))
        })
        .when(
            shown.is_empty() && !workspace.projects.is_empty(),
            |sidebar| sidebar.child(no_matches(p)),
        )
        .children(shown.into_iter().map(|(project_index, project, branches)| {
            let path = project.path.clone();
            div()
                .flex()
                .flex_col()
                .gap_px()
                .child(with_menu(
                    div()
                        .group("orca-project")
                        .h(px(theme::ROW_HEIGHT))
                        .pl_2()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_sm()
                        .child(icon(IconName::Folder, 14.).text_color(p.muted))
                        .child(ellipsized(project.name.clone()).flex_1())
                        .child(
                            div()
                                .opacity(0.)
                                .group_hover("orca-project", |style| style.opacity(1.))
                                .child(
                                    icon_button(
                                        ("orca-new-worktree", project_index),
                                        IconName::GitBranchPlus,
                                        "New worktree",
                                        cx,
                                    )
                                    .small()
                                    .on_click(
                                        move |_, window, cx| {
                                            window.dispatch_action(
                                                Box::new(NewWorktreeIn(path.clone())),
                                                cx,
                                            );
                                        },
                                    ),
                                ),
                        ),
                    {
                        let path = project.path.clone();
                        move || project_menu(&path)
                    },
                ))
                .when_some(project.error.clone(), |group, error| {
                    group.child(project_error(error, p))
                })
                .children(branches.into_iter().map(|branch_index| {
                    let branch = &project.branches[branch_index];
                    let selected = project_index == workspace.active_project
                        && branch_index == project.active_branch;
                    branch_row(shell, project_index, branch_index, branch, selected, p, cx)
                }))
        }))
}

fn branch_row(
    shell: &Shell,
    project_index: usize,
    branch_index: usize,
    branch: &BranchUnit,
    selected: bool,
    p: Palette,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let (path, is_main) = (branch.worktree.path.clone(), branch.is_main());
    let waiting = shell.terminals_needing_input(&path);
    let terminals = shell.has_terminals(&path);
    let chat_mark = shell.checkout_chat_mark(&path, p, cx);
    with_menu(
        row(
            ("orca-branch", project_index * 100 + branch_index),
            selected,
            p,
        ),
        move || worktree_menu(&path, is_main, terminals),
    )
    .pl_6()
    .when(!selected, |row| row.text_color(p.muted))
    .on_click(cx.listener(move |this, _, _, cx| {
        this.workspace.select_branch(project_index, branch_index);
        cx.notify();
    }))
    .child(icon(IconName::GitBranch, 13.))
    .child(ellipsized(branch.name()))
    .when(is_main, |row| {
        row.child(div().text_xs().text_color(p.muted).child("checkout"))
    })
    .when(waiting > 0, |row| row.child(crate::terminals::waiting_badge(waiting, p)))
    .children(chat_mark)
}

pub fn tab_strip(shell: &Shell, cx: &mut Context<Shell>) -> impl IntoElement {
    let p = theme::palette(cx);
    let workspace = &shell.workspace;
    let project_index = workspace.active_project;
    let branch_index = workspace
        .project()
        .map_or(0, |project| project.active_branch);
    let branch = workspace.branch();
    let tabs = branch.map_or(&[][..], |branch| &branch.tabs);
    let active_tab = branch.map_or(0, |branch| branch.active_tab);
    let rows = (tabs.iter().enumerate())
        .map(|(tab_index, tab)| {
            let active = tab_index == active_tab;
            let at = (project_index, branch_index, tab_index);
            let row = row(("orca-tab", tab_index), active, p)
                .h(px(28.))
                // Narrowed to fit beside the + button, however many there are, down to their
                // icons, as a browser's are: each stays on screen to pick.
                .flex_shrink(1.)
                .min_w_0()
                .overflow_hidden()
                .when(!active, |tab| tab.text_color(p.muted))
                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                    this.workspace
                        .select_tab(project_index, branch_index, tab_index);
                    // A second click keeps a previewed file open.
                    if event.click_count() > 1 {
                        this.keep_tab(at, cx);
                    }
                    cx.notify();
                }))
                .on_mouse_down(
                    MouseButton::Middle,
                    cx.listener(move |this, _, window, cx| this.close_tab_at(at, window, cx)),
                );
            let list = (Axis::Horizontal, active, &shell.tab_hand);
            let (dot, split_dot) = shell.tab_dots(tab, p, cx);
            let row = row.children(split_dot);
            tab_row(row, (at, tab), list, p, cx, |row, face, cx| {
                let close = ("orca-tab-close", tab_index);
                row.max_w(px(220.))
                    .pl_2()
                    .pr_1()
                    .child(face.icon(13.))
                    .children(dot)
                    .child(face.label())
                    .child(tab_close_button(close, at, face.dirty, p, cx))
            })
        })
        .collect();
    let list = ((project_index, branch_index), Axis::Horizontal);
    div()
        .h(px(theme::TAB_STRIP_HEIGHT))
        .flex_shrink_0()
        .px_2()
        .flex()
        .items_center()
        .gap_1()
        .border_b_1()
        .border_color(p.hairline)
        .child(tab_list(&shell.tab_hand, list, rows, px(4.), p, cx))
        .when_some(branch, |strip, unit| {
            strip.child(new_tab_menu(
                "orca-new-tab-menu",
                icon_button("orca-new-tab", IconName::Plus, "New tab", cx)
                    .small()
                    .debug_selector(|| "new-tab-button".to_owned()),
                shell.focus.clone(),
                cx.weak_entity(),
                unit.worktree.path.clone(),
            ))
        })
}
