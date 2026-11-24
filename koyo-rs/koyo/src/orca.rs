//! Orca layout: projects and their branch units in the sidebar inside the card, the wallpaper
//! behind them, and in the top rail the pages that are no checkout's, then the selected
//! checkout's tabs: the pinned pages in a pill (`pinned.rs`), the space's loose pages and the
//! Settings tab as tabs (`loose.rs`) and their +, a hairline, and the checkout's tabs, as the
//! Arc sidebar lists them from its head down (`docs/arc-sidebar.md` 11). A project's row folds
//! its branches away under it and brings them back, and a project or a branch dragged by its row
//! goes to another place among its own, as Arc's worktrees do (`shell/reorder.rs`), the order
//! kept for the next launch. The selected branch's Changes and Files buttons are at the top
//! rail's end (`shell/rails.rs`).

use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AppContext as _, Axis, ClickEvent, Context, Div, ElementId,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Stateful,
    StatefulInteractiveElement as _, Styled as _, div, px,
};

use crate::model::{BranchUnit, Front, Project};
use crate::pane::CLOSE;
use crate::projects::{AddProject, NewWorktreeIn, project_menu, with_menu, worktree_menu};
use crate::shell::{
    Drop, InHand, List, Reorder, Shell, action_button, add_project_row, ellipsized, icon,
    icon_button, key_of, new_page_button, new_tab_button, no_matches, project_error, row,
    row_shape, section_label, selectable, tab_close_button, tab_list, tab_row,
};
use crate::theme::{self, Palette};

mod loose;
mod pinned;

pub fn sidebar(shell: &Shell, cx: &mut Context<Shell>) -> impl IntoElement {
    let p = theme::palette(cx);
    let workspace = &shell.workspace;
    let query = shell.search_query();
    let shown: Vec<_> = workspace
        .projects
        .iter()
        .enumerate()
        .map(|(index, project)| (index, project, project.shown_branches(query)))
        .filter(|(_, _, branches)| query.trim().is_empty() || !branches.is_empty())
        .collect();
    div()
        .flex()
        .flex_col()
        .gap_px()
        .child(shell.search.clone())
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
        .child(project_list(shell, shown, p, cx))
}

/// The projects listed, each a row and its branches', dragged by its row to another place
/// among them: the order's saved, the Arc sidebar's too.
fn project_list(
    shell: &Shell,
    shown: Vec<(usize, &Project, Vec<usize>)>,
    p: Palette,
    cx: &mut Context<Shell>,
) -> Reorder {
    let order: Vec<usize> = shown.iter().map(|(index, ..)| *index).collect();
    let groups = (shown.into_iter().enumerate())
        .map(|(place, (project_index, project, branches))| {
            let key = key_of(&project.path);
            let at = (place, key, project_index);
            (key, project_group(shell, at, project, branches, p, cx).into_any_element())
        })
        .collect();
    let on_drop = cx.listener(move |this, drop: &Drop, _, cx| {
        if let Drop::Move { from, to } = *drop {
            this.move_project_among(&order, from, to, cx);
        }
    });
    let hand = shell.row_hand.clone();
    Reorder::rows(List::Projects, groups, hand, px(1.), Rc::new(on_drop))
}

/// A project's row, the `place`th listed and known by `key` as it moves, and, unless they're
/// folded away, its `branches`' rows, each dragged by its row to another place among them, the
/// order kept for the next launch. A search that finds a folded project's branches lists it by
/// its row.
fn project_group(
    shell: &Shell,
    (place, key, project_index): (usize, u64, usize),
    project: &Project,
    branches: Vec<usize>,
    p: Palette,
    cx: &mut Context<Shell>,
) -> Div {
    let folded = shell.is_folded(&project.path, cx);
    let branches = if folded { Vec::new() } else { branches };
    let workspace = &shell.workspace;
    let rows: Vec<(u64, AnyElement)> = (branches.iter().enumerate())
        .map(|(at, &branch_index)| {
            let branch = &project.branches[branch_index];
            let selected = Some(project_index) == workspace.active_project()
                && branch_index == project.active_branch;
            let key = key_of(&branch.worktree.path);
            let at = (project_index, branch_index, at, key);
            (key, branch_row(shell, at, branch, selected, p, cx).into_any_element())
        })
        .collect();
    let on_drop = cx.listener(move |this, drop: &Drop, _, cx| {
        let Drop::Move { from, to } = *drop else {
            return;
        };
        if let (Some(&from), Some(&to)) = (branches.get(from), branches.get(to)) {
            this.move_worktree(project_index, from, to, cx);
        }
    });
    let hand = shell.row_hand.clone();
    div()
        .flex()
        .flex_col()
        .gap_px()
        .child(project_row(shell, (place, key, project_index), project, p, cx))
        .when_some(project.error.clone(), |group, error| {
            group.child(project_error(error, p))
        })
        .when(!rows.is_empty(), |group| {
            let list = List::Worktrees(project_index);
            group.child(Reorder::rows(list, rows, hand, px(1.), Rc::new(on_drop)))
        })
}

/// A project's row: its folder, open while its branches show under it and closed while they're
/// folded away, its name, and under the pointer its New worktree button. Clicked, it folds them
/// away or brings them back, kept for the next launch as the Arc sidebar's project name keeps it.
/// Folded, it says at its end how the project's doing, as Arc's does: the spinner while a chat
/// in it works, the indigo dot once something in it waits for the person; the New worktree
/// button takes that place under the pointer. It's the `place`th listed, known by `key`.
fn project_row(
    shell: &Shell,
    (place, key, project_index): (usize, u64, usize),
    project: &Project,
    p: Palette,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let folded = shell.is_folded(&project.path, cx);
    let mark = folded.then(|| {
        let place = || format!("orca-project-{}", project.path.display());
        shell.project_activity(project, cx).sign(place, p, cx)
    });
    let mark = (mark.flatten()).map(|mark| {
        (div().absolute().inset_0().flex().items_center().justify_center())
            .group_hover("orca-project", gpui_kit::Styled::invisible)
            .child(mark.debug_selector(move || format!("orca-project-mark-{project_index}")))
    });
    let path = project.path.clone();
    let new_worktree = icon_button(
        ("orca-new-worktree", project_index),
        IconName::GitBranchPlus,
        "New worktree",
        cx,
    )
    .small()
    .debug_selector(move || format!("orca-new-worktree-{project_index}"))
    .on_click(move |_, window, cx| {
        cx.stop_propagation();
        window.dispatch_action(Box::new(NewWorktreeIn(path.clone())), cx);
    });
    let folder_icon = if folded {
        IconName::Folder
    } else {
        IconName::FolderOpen
    };
    let (toggled, menu) = (project.path.clone(), project.path.clone());
    let hand = shell.row_hand.clone();
    let row = row(("orca-project", project_index), false, p)
        .debug_selector(move || format!("orca-project-{project_index}"))
        .group("orca-project")
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_fold(&toggled, cx)))
        .on_drag(InHand, move |_, grab, _, cx| {
            hand.take(List::Projects, key, place, grab.y);
            cx.new(|_| InHand)
        })
        .child(icon(folder_icon, 14.).text_color(p.muted))
        .child(ellipsized(project.name.clone()).flex_1())
        .child(
            div()
                .relative()
                .child(
                    (div().opacity(0.))
                        .group_hover("orca-project", |style| style.opacity(1.))
                        .child(new_worktree),
                )
                .children(mark),
        );
    let row = with_menu(row, move |_, cx| project_menu(&menu, cx));
    #[cfg(feature = "visual-check")]
    let row = {
        use gpui_kit::base::ElementExt as _;
        row.on_prepaint(crate::visual::noting(format!("orca-project-{project_index}")))
    };
    row
}

/// A branch's row, the `place`th of its project's listed: clicked, it's selected; dragged, it
/// goes to another place among them.
fn branch_row(
    shell: &Shell,
    (project_index, branch_index, place, key): (usize, usize, usize, u64),
    branch: &BranchUnit,
    selected: bool,
    p: Palette,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let (path, is_main) = (branch.worktree.path.clone(), branch.is_main());
    let waiting = shell.terminals_needing_input(&path);
    let terminals = shell.has_terminals(&path);
    let mark = (shell.checkout_activity(&path, cx))
        .sign(|| format!("orca-branch-{}", path.display()), p, cx)
        .map(|mark| {
            mark.debug_selector(move || format!("orca-branch-mark-{project_index}-{branch_index}"))
        });
    let hand = shell.row_hand.clone();
    with_menu(
        row(
            ("orca-branch", project_index * 100 + branch_index),
            selected,
            p,
        ),
        move |_, _| worktree_menu(&path, is_main, terminals),
    )
    .debug_selector(move || format!("orca-branch-{project_index}-{branch_index}"))
    .pl_6()
    .when(!selected, |row| row.text_color(p.muted))
    .on_click(cx.listener(move |this, _, _, cx| {
        this.workspace.select_branch(project_index, branch_index);
        cx.notify();
    }))
    .on_drag(InHand, move |_, grab, _, cx| {
        hand.take(List::Worktrees(project_index), key, place, grab.y);
        cx.new(|_| InHand)
    })
    .child(icon(IconName::GitBranch, 13.))
    .child(ellipsized(branch.title(shell.own_prefix(project_index, cx))))
    .when(is_main, |row| {
        row.child(div().text_xs().text_color(p.muted).child("checkout"))
    })
    .when(waiting > 0, |row| {
        row.child(crate::terminals::waiting_badge(waiting, p))
    })
    .child(div().flex_1())
    .children(mark)
}

/// What the top rail shows after the window's buttons and before the selected checkout's tabs:
/// the pinned pages in their pill, or the zone that says where one goes; the space's loose pages,
/// or the spot that takes the first while a page's tab is in hand, and the Settings tab, while
/// it's open, as tabs; their +, always there, which opens the launcher for a loose page only
/// (`docs/layout.md`); and, while a checkout is selected, a hairline between them and its tabs.
pub fn pages(shell: &Shell, cx: &mut Context<Shell>) -> Div {
    let p = theme::palette(cx);
    let pinned = pinned::region(shell, shell.pinned_taking(), p, cx);
    let loose = loose::tabs(shell, p, cx);
    let settings = loose::settings_tab(shell, p, cx);
    let new_page = (new_page_button("orca-new-page", cx).small())
        .debug_selector(|| "orca-new-page".to_owned());
    let hairline = shell.workspace.branch().map(|_| {
        (div().w(px(1.)).h(px(18.)).mx_1().flex_shrink_0())
            .debug_selector(|| "orca-rail-separator".to_owned())
            .bg(p.muted.opacity(0.35))
    });
    (div().min_w_0().h_full().flex().items_center().gap_1())
        .children(pinned)
        .children(loose)
        .children(settings)
        .child(new_page)
        .children(hairline)
}

/// The selected checkout's tabs, in the top rail after the pages that are no checkout's, and the
/// new-tab button after them: each picked, kept, closed and dragged along the list as the person
/// likes, lit while it's the tab shown, rather than a pinned or a loose page. The button opens a
/// tab of the checkout, whichever page shows. They narrow to fit beside what's after them in the
/// rail.
pub fn tabs(shell: &Shell, cx: &mut Context<Shell>) -> impl IntoElement + use<> {
    let p = theme::palette(cx);
    let workspace = &shell.workspace;
    let project_index = workspace.active_project().unwrap_or_default();
    let branch_index = workspace
        .project()
        .map_or(0, |project| project.active_branch);
    let branch = workspace.branch();
    let tabs = branch.map_or(&[][..], |branch| &branch.tabs);
    let shown = match workspace.front() {
        Some(Front::Checkout(at)) => Some(at),
        _ => None,
    };
    let rows = (tabs.iter().enumerate())
        .map(|(tab_index, tab)| {
            let at = (project_index, branch_index, tab_index);
            let active = shown == Some(at);
            let row = rail_tab(("orca-tab", tab_index), active, p)
                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                    this.pick_tab(project_index, branch_index, tab_index, cx);
                    if event.click_count() > 1 {
                        this.keep_tab(at, cx);
                    }
                }))
                .on_mouse_down(
                    MouseButton::Middle,
                    cx.listener(move |this, _, window, cx| this.close_tab_at(at, window, cx)),
                );
            #[cfg(feature = "visual-check")]
            let row = {
                use gpui_kit::base::ElementExt as _;
                row.on_prepaint(crate::visual::noting(format!("orca-tab-{tab_index}")))
            };
            let list = (Axis::Horizontal, &shell.tab_hand);
            let (dot, split_dot) = shell.tab_dots(tab, p, cx);
            let row = row.children(split_dot);
            tab_row(row, (at, tab), list, p, cx, |row, face, cx| {
                let close = ("orca-tab-close", tab_index);
                let close = tab_close_button(close, at, (face.dirty, CLOSE), p, cx);
                single_page(row, (face.icon(13.), face.label()), dot, close)
            })
        })
        .collect();
    let list = ((project_index, branch_index), Axis::Horizontal);
    div()
        .min_w_0()
        .h_full()
        .flex()
        .items_center()
        .gap_1()
        .child(tab_list(&shell.tab_hand, list, rows, px(4.), p, cx))
        .when(branch.is_some(), |tabs| {
            let select = cx.listener(move |this, _, _, cx| {
                this.workspace.select_branch(project_index, branch_index);
                cx.notify();
            });
            tabs.child(
                new_tab_button("orca-new-tab", cx)
                    .small()
                    .debug_selector(|| "new-tab-button".to_owned())
                    .on_mouse_down(MouseButton::Left, select),
            )
        })
}

/// A tab in the top rail, a checkout's or a loose page's: a row 28 high that narrows to fit
/// beside the others, its text muted unless it's `active`.
fn rail_tab(id: impl Into<ElementId>, active: bool, p: Palette) -> Stateful<Div> {
    selectable(rail_shape(id), active, p).when(!active, |tab| tab.text_color(p.muted))
}

/// A tab's shape in the top rail, without what the pointer does to it: a row 28 high that
/// narrows to fit beside the others.
fn rail_shape(id: impl Into<ElementId>) -> Stateful<Div> {
    row_shape(id).h(px(28.)).flex_shrink(1.).min_w_0().overflow_hidden()
}

/// What a rail tab of a single page or of Settings shows, at most 220 wide: its `icon`, `dot`
/// when there's one, its `title` and `close`.
fn single_page(
    tab: Stateful<Div>,
    (icon, title): (AnyElement, Div),
    dot: Option<Div>,
    close: Stateful<Div>,
) -> Stateful<Div> {
    tab.max_w(px(220.))
        .pl_2()
        .pr_1()
        .child(icon)
        .children(dot)
        .child(title)
        .child(close)
}

#[cfg(test)]
mod tests;
