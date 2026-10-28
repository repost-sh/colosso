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

use crate::model::{BranchKind, BranchUnit};
use crate::shell::{
    Shell, ellipsized, icon, icon_button, new_tab_menu, row, search_row, section_label,
    sidebar_resize_handle,
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
        .child(search_row(p).rounded(px(8.)).bg(p.hover))
        .children(
            project
                .branches
                .iter()
                .enumerate()
                .map(|(branch_index, branch)| {
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
                }),
        )
        .child(
            row("arc-new-worktree", false, p)
                .mt_2()
                .text_color(p.muted)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.workspace.new_worktree(project_index);
                    cx.notify();
                }))
                .child(icon(IconName::GitBranchPlus, 13.))
                .child("New worktree"),
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
        .child(icon_button("arc-new-space", IconName::Plus, "New project").small());

    div()
        .id("arc-sidebar")
        .relative()
        .w(px(shell.sidebar_width))
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
        .child(sidebar_resize_handle())
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
    div()
        .mt_2()
        .flex()
        .flex_col()
        .gap_px()
        .child(
            section_label(p)
                .pr_0()
                .child(icon(IconName::GitBranch, 12.))
                .child(ellipsized(branch.name.clone()))
                .when(branch.kind == BranchKind::Checkout, |header| {
                    header.child("checkout")
                })
                .child(div().flex_1())
                .child(new_tab_menu(
                    ("arc-new-tab-menu", branch_index),
                    icon_button(("arc-new-tab", branch_index), IconName::Plus, "New tab")
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
        )
        .children(branch.tabs.iter().enumerate().map(|(tab_index, tab)| {
            row(
                ("arc-tab", branch_index * 100 + tab_index),
                active_tab == Some(tab_index),
                p,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.workspace
                    .select_tab(project_index, branch_index, tab_index);
                cx.notify();
            }))
            .child(icon(tab.icon(), 13.).text_color(p.muted))
            .child(ellipsized(tab.title()))
        }))
}
