//! Arc layout: one project per space on the glass, its branch units with their tabs
//! listed beneath, and a project switcher at the bottom. Two fingers swipe between projects.

use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Context, FocusHandle, FontWeight, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, ScrollDelta, ScrollEvent, StatefulInteractiveElement as _, Styled as _,
    TouchPhase, Window, div, px,
};

use crate::model::BranchUnit;
use crate::projects::{AddProject, NewWorktreeIn, project_menu, with_menu, worktree_menu};
use crate::shell::{
    Shell, action_button, add_project_row, ellipsized, icon, icon_button, new_tab_menu, no_matches,
    project_error, row, search_field, section_label, tab_close_button,
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
                            move || project_menu(&path)
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
                            &shell.focus,
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
                                window.dispatch_action(Box::new(NewWorktreeIn(path.clone())), cx)
                            })
                            .child(icon(IconName::GitBranchPlus, 13.))
                            .child("New worktree"),
                    )
            }
        });

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
        );

    div()
        .id("arc-sidebar")
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .on_scroll(cx.listener(Shell::swipe))
        .child(space)
        .child(switcher)
}

/// A branch unit's header and its tabs; `active_tab` is set only on the selected branch.
fn branch_group(
    focus: &FocusHandle,
    project_index: usize,
    branch_index: usize,
    branch: &BranchUnit,
    active_tab: Option<usize>,
    p: Palette,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let (path, is_main) = (branch.worktree.path.clone(), branch.is_main());
    div()
        .mt_2()
        .flex()
        .flex_col()
        .gap_px()
        .child(with_menu(
            section_label(p)
                .pr_0()
                .child(icon(IconName::GitBranch, 12.))
                .child(ellipsized(branch.name()))
                .when(is_main, |header| header.child("checkout"))
                .child(div().flex_1())
                .child(new_tab_menu(
                    ("arc-new-tab-menu", branch_index),
                    icon_button(("arc-new-tab", branch_index), IconName::Plus, "New tab", cx)
                        .xsmall()
                        // The menu opens its tab on the selected branch unit, so select
                        // this one on the press that opens the menu.
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.workspace.select_branch(project_index, branch_index);
                                cx.notify();
                            }),
                        ),
                    focus.clone(),
                )),
            move || worktree_menu(&path, is_main),
        ))
        .children(branch.tabs.iter().enumerate().map(|(tab_index, tab)| {
            let active = active_tab == Some(tab_index);
            row(("arc-tab", branch_index * 100 + tab_index), active, p)
                .group("arc-tab")
                .pr_1()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.workspace
                        .select_tab(project_index, branch_index, tab_index);
                    cx.notify();
                }))
                .child(icon(tab.icon(), 13.).text_color(p.muted))
                .child(ellipsized(tab.title()).flex_1())
                // Shown on the active tab, and on the others while hovered.
                .child(
                    tab_close_button(
                        ("arc-tab-close", branch_index * 100 + tab_index),
                        (project_index, branch_index, tab_index),
                        p,
                        cx,
                    )
                    .when(!active, |close| {
                        close
                            .opacity(0.)
                            .group_hover("arc-tab", |style| style.opacity(1.))
                    }),
                )
        }))
}
