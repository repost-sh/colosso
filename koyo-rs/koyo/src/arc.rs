//! Arc layout: one project per space on the glass, its branch units with their tabs
//! listed beneath, and a project switcher at the bottom. Two fingers swipe between projects.

use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Axis, ClickEvent, Context, Div, FontWeight, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, ScrollDelta, ScrollEvent, StatefulInteractiveElement as _, Styled as _,
    TouchPhase, Window, div, px,
};

use crate::model::BranchUnit;
use crate::projects::{AddProject, NewWorktreeIn, project_menu, with_menu, worktree_menu};
use crate::shell::{
    Shell, action_button, add_project_row, ellipsized, icon, new_tab_button, no_matches,
    project_error, row, search_field, section_label, tab_close_button, tab_list, tab_row,
};
use crate::theme::{self, Palette};

/// Horizontal travel, in points, that switches the sidebar to the next project.
const SWIPE_THRESHOLD: f32 = 70.;

impl Shell {
    fn swipe(&mut self, event: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let ScrollDelta::Pixels(delta) = event.delta else {
            return;
        };
        let before = (self.swipe.unwrap_or(0.), self.workspace.active_project);
        let (dx, dy) = (delta.x.as_f32(), delta.y.as_f32());
        self.swipe = match (event.touch_phase, self.swipe) {
            (TouchPhase::Started, _) => Some(0.),
            // A gesture that starts vertical is a scroll, not a swipe.
            (TouchPhase::Moved, Some(0.)) if dy.abs() > dx.abs() => None,
            (TouchPhase::Moved, Some(travel)) => {
                Some((travel + dx).clamp(-theme::SIDEBAR_WIDTH, theme::SIDEBAR_WIDTH))
            }
            (TouchPhase::Ended, Some(travel)) => {
                if travel <= -SWIPE_THRESHOLD {
                    self.workspace.step_project(1);
                } else if travel >= SWIPE_THRESHOLD {
                    self.workspace.step_project(-1);
                }
                None
            }
            // Momentum events arrive after the fingers lift and carry no swipe.
            _ => None,
        };
        if (self.swipe.unwrap_or(0.), self.workspace.active_project) != before {
            cx.notify();
        }
    }
}

pub fn sidebar(shell: &Shell, cx: &mut Context<Shell>) -> impl IntoElement {
    let p = theme::palette(cx);
    let workspace = &shell.workspace;
    let project_index = workspace.active_project;

    let space = div()
        .flex_1()
        .min_h_0()
        .relative()
        .left(px(shell.swipe.unwrap_or(0.)))
        .flex()
        .flex_col()
        .gap_px()
        .map(|space| match workspace.project() {
            None => space.child(add_project_row("arc-first-project", p)),
            Some(project) => {
                let path = project.path.clone();
                let shown = project.shown_branches(&shell.search_query(cx));
                space
                    .child(with_menu(
                        div()
                            .h(px(28.))
                            .px_2()
                            .flex()
                            .items_center()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(project.name.clone()),
                        {
                            let path = path.clone();
                            move |_, _| project_menu(&path)
                        },
                    ))
                    .child(search_field(&shell.search, p).rounded(px(8.)).bg(p.hover))
                    .when_some(project.error.clone(), |space, error| {
                        space.child(project_error(error, p))
                    })
                    .when(shown.is_empty() && !project.branches.is_empty(), |space| {
                        space.child(no_matches(p))
                    })
                    .children(shown.into_iter().map(|branch_index| {
                        let branch = &project.branches[branch_index];
                        let active_tab =
                            (branch_index == project.active_branch).then_some(branch.active_tab);
                        branch_group(
                            shell,
                            project_index,
                            branch_index,
                            branch,
                            active_tab,
                            p,
                            cx,
                        )
                    }))
                    .child(
                        row("arc-new-worktree", false, p)
                            .mt_2()
                            .text_color(p.muted)
                            .on_click(move |_, window, cx| {
                                window.dispatch_action(Box::new(NewWorktreeIn(path.clone())), cx);
                            })
                            .child(icon(IconName::GitBranchPlus, 13.))
                            .child("New worktree"),
                    )
            }
        });

    div()
        .id("arc-sidebar")
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .on_scroll(cx.listener(Shell::swipe))
        .child(space)
        .child(switcher(shell, p, cx))
}

/// The row of spaces along the sidebar's foot, one per project, and the button that adds one.
fn switcher(shell: &Shell, p: Palette, cx: &mut Context<Shell>) -> Div {
    let workspace = &shell.workspace;
    let project_index = workspace.active_project;
    div()
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
                    row(("arc-space", index), active, p)
                        .size(px(26.))
                        .p_0()
                        .justify_center()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .when(!active, |space| space.text_color(p.muted))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.workspace.active_project = index;
                            cx.notify();
                        }))
                        .child(project.initials())
                }),
        )
        .child(
            action_button(
                "arc-new-space",
                IconName::Plus,
                "Add project",
                AddProject,
                cx,
            )
            .small(),
        )
}

/// A branch unit's header and its tabs; `active_tab` is set only on the selected branch.
fn branch_group(
    shell: &Shell,
    project_index: usize,
    branch_index: usize,
    branch: &BranchUnit,
    active_tab: Option<usize>,
    p: Palette,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let (path, is_main) = (branch.worktree.path.clone(), branch.is_main());
    let terminals = shell.has_terminals(&path);
    let selected = active_tab.is_some();
    let rows = (branch.tabs.iter().enumerate())
        .map(|(tab_index, tab)| {
            let active = active_tab == Some(tab_index);
            let at = (project_index, branch_index, tab_index);
            let row = row(("arc-tab", branch_index * 100 + tab_index), active, p)
                .group("arc-tab")
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
            let list = (Axis::Vertical, active, &shell.tab_hand);
            let (dot, split_dot) = shell.tab_dots(tab, p, cx);
            let row = row.children(split_dot);
            tab_row(row, (at, tab), list, p, cx, |row, face, cx| {
                let close = ("arc-tab-close", branch_index * 100 + tab_index);
                row.pr_1()
                    .child(div().text_color(p.muted).child(face.icon(13.)))
                    .children(dot)
                    .child(face.label().flex_1())
                    // Shown on the active tab, and on the others while hovered; a dot for
                    // unsaved edits always.
                    .child(tab_close_button(close, at, face.dirty, p, cx).when(
                        !active && !face.dirty,
                        |close| {
                            close
                                .opacity(0.)
                                .group_hover("arc-tab", |style| style.opacity(1.))
                        },
                    ))
            })
        })
        .collect();
    let list = ((project_index, branch_index), Axis::Vertical);
    div()
        .mt_2()
        .flex()
        .flex_col()
        .gap_px()
        .child(with_menu(
            section_label(p)
                .id(("arc-branch", branch_index))
                .pr_0()
                .rounded(px(6.))
                .cursor_pointer()
                .hover(|style| style.bg(p.hover))
                .when(selected, |header| header.text_color(p.text))
                // Selecting the branch unit points the changes panel at its checkout.
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.workspace.select_branch(project_index, branch_index);
                    cx.notify();
                }))
                .child(icon(IconName::GitBranch, 12.))
                .child(ellipsized(branch.name()))
                .when(is_main, |header| header.child("checkout"))
                .child(div().flex_1())
                .map(|header| {
                    let waiting = shell.terminals_needing_input(&branch.worktree.path);
                    header.when(waiting > 0, |header| {
                        header.child(crate::terminals::waiting_badge(waiting, p))
                    })
                })
                .children(shell.checkout_chat_mark(&branch.worktree.path, p, cx))
                .child(
                    new_tab_button(("arc-new-tab", branch_index), cx)
                        .xsmall()
                        // The launcher opens its tab on the selected branch unit, so select
                        // this one on the press that opens it.
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.workspace.select_branch(project_index, branch_index);
                                cx.notify();
                            }),
                        ),
                ),
            move |_, _| worktree_menu(&path, is_main, terminals),
        ))
        .child(tab_list(&shell.tab_hand, list, rows, px(1.), p, cx))
}
