//! The sidebar, on the window's glass (`docs/layout.md`): the projects of the space shown as rows,
//! each with its branches' rows under it, scrolling between the sidebar's top and its foot, which
//! stays: the Claude accounts' button, the spaces' marks and the + (`foot.rs`). A space with no
//! project shows the row that adds one. Two fingers across the sidebar swipe between the spaces,
//! the rows going with the space (`spaces::Swipe`). A project's row folds its branches away under
//! it and brings them back, and a project or a branch dragged by its row goes to another place
//! among its own (`shell/reorder.rs`), the order kept for the next launch. With **Tabs** in the
//! sidebar the tabs are here too, over and under the rows (`tab_rows.rs`), and the address bar
//! heads the sidebar while it's in it. The pinned pages' tiles, the address bar and the foot stay
//! as the rest scrolls, the unpinned pages' rows and the projects' together (`docs/layout.md`,
//! Sidebar). Right-clicked where nothing in it has a menu of its own, it opens the sidebar's
//! (`layout_menus::sidebar_menu`).

use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Axis, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div, px,
};

use super::rows::{at_end, branch_tabs};
use super::{Look, noted, shown_on_hover, tab_rows};
use crate::model::{BranchUnit, Front, Project};
use crate::pane::ROW_GLYPH;
use crate::projects::{NewWorktreeIn, project_menu, worktree_menu};
use crate::shell::{
    List, Reorder, Shell, add_project_row, ellipsized, icon, icon_button, indented, key_of,
    on_move, project_error, row_shape, sidebar_menu, with_menu,
};
use crate::{settings, theme};

/// How long a branch's row's buttons take to fade in or out as the pointer comes onto the row or
/// leaves, as the sidebar's other hovers do.
const HOVER_FADE: Duration = Duration::from_millis(150);

/// What every project's rows are drawn by, read once for them all as the sidebar is: its shades,
/// whether the tabs are in it, each branch's under the branch's row, and the checkout's tab in
/// front among them, if one is, the pill.
#[derive(Clone, Copy)]
struct Shared {
    look: Look,
    tabs: bool,
    shown: Option<(usize, usize, usize)>,
}

pub fn sidebar(shell: &Shell, window: &mut Window, cx: &mut Context<Shell>) -> impl IntoElement {
    let settings = settings::get(cx);
    let (tabs, address) = (settings.tabs_in_sidebar(), settings.address_in_sidebar());
    let front = shell.workspace.front();
    let look = Look::of(cx);
    let shared = Shared {
        look,
        tabs,
        shown: front.and_then(Front::checkout),
    };
    let projects = shell.space_projects(cx);
    let rows = if projects.is_empty() {
        add_project_row("sidebar-add-project", look.p).into_any_element()
    } else {
        project_list(shell, projects, shared, window, cx).into_any_element()
    };
    let rows = (shell.carry(div(), cx))
        .flex_shrink_0()
        .flex()
        .flex_col()
        .child(rows);
    let unpinned = tabs.then(|| tab_rows::unpinned(shell, front.and_then(Front::loose), look, cx));
    let space = div()
        .id("sidebar-space")
        .debug_selector(|| "sidebar-space".to_owned())
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .children(unpinned)
        .child(rows);
    let address = address.then(|| {
        let bar = div().flex_shrink_0().mb(px(tab_rows::GAP));
        bar.child(shell.sidebar_top.bar())
    });
    let area = div()
        .id("sidebar")
        .debug_selector(|| "sidebar".to_owned())
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .on_scroll(cx.listener(Shell::swipe));
    with_menu(area, |_, cx| sidebar_menu(cx))
        .children(address)
        .children(tabs.then(|| tab_rows::pinned(shell, look, cx)).flatten())
        .child(space)
        .child(super::foot::bar(shell, window, cx))
}

/// The space's `projects`, by their place in the workspace, each a row and its branches', dragged
/// by its row to another place among them: the order's saved.
fn project_list(
    shell: &Shell,
    projects: Vec<usize>,
    shared: Shared,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> Reorder {
    let groups = (projects.iter().enumerate())
        .map(|(place, &project_index)| {
            let project = &shell.workspace.projects[project_index];
            let key = key_of(&project.path);
            let at = (place, key, project_index);
            (key, project_group(shell, at, project, shared, window, cx).into_any_element())
        })
        .collect();
    let on_drop = on_move(cx, move |this, from, to, cx| {
        this.move_project_among(&projects, from, to, cx);
    });
    let list = (List::Projects, Axis::Vertical, false);
    Reorder::rows(list, groups, shell.row_hand.clone(), px(1.), on_drop)
}

/// A project's row, the `place`th listed and known by `key` as it moves, and, unless they're
/// folded away, its branches' rows, each dragged by its row to another place among them, the
/// order kept for the next launch.
fn project_group(
    shell: &Shell,
    (place, key, project_index): (usize, u64, usize),
    project: &Project,
    shared: Shared,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> Div {
    let folded = settings::get(cx).is_folded(&project.path);
    let path = Rc::<Path>::from(project.path.as_path());
    let selected = (shell.workspace.active_project() == Some(project_index))
        .then_some(project.active_branch);
    let prefix = shell.own_prefix(project_index, cx).map(str::to_owned);
    let branches = if folded { &[][..] } else { &project.branches[..] };
    let rows: Vec<(u64, AnyElement)> = (branches.iter().enumerate())
        .map(|(branch_index, branch)| {
            let key = key_of(&branch.worktree.path);
            let at = (project_index, branch_index, key);
            let unit = (branch, selected == Some(branch_index));
            let owner = (&path, prefix.as_deref());
            (key, branch_rows(shell, at, unit, owner, shared, window, cx))
        })
        .collect();
    let on_drop = on_move(cx, move |this, from, to, cx| {
        this.move_worktree(project_index, from, to, cx);
    });
    let at = (place, key, project_index);
    div()
        .flex()
        .flex_col()
        .gap_px()
        .child(project_row(shell, at, (project, &path, folded), shared.look, cx))
        .when_some(project.error.clone(), |group, error| {
            group.child(project_error(error, shared.look.p))
        })
        .when(!rows.is_empty(), |group| {
            let list = (List::Worktrees(project_index), Axis::Vertical, false);
            group.child(Reorder::rows(list, rows, shell.row_hand.clone(), px(1.), on_drop))
        })
}

/// A project's row: its folder, open while its branches show under it and closed while they're
/// `folded` away, its name, and under the pointer its New worktree button. Clicked, it folds them
/// away or brings them back, kept for the next launch. Folded, it says at its end how the project's
/// doing, as a branch's row does: the spinner while a chat in it works, the indigo dot once
/// something in it waits for the person; the New worktree button takes that place under the
/// pointer. It's the `place`th listed, known by `key`, its folder `path`.
fn project_row(
    shell: &Shell,
    (place, key, project_index): (usize, u64, usize),
    (project, path, folded): (&Project, &Rc<Path>, bool),
    look: Look,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let p = look.p;
    let mark = folded.then(|| {
        let place = || format!("sidebar-project-{}", project.path.display());
        shell.project_activity(project, cx).sign(place, p, cx)
    });
    let mark = (mark.flatten()).map(|mark| {
        let name = move || format!("sidebar-project-mark-{project_index}");
        at_end(mark.debug_selector(name), "sidebar-project")
    });
    let new = Rc::clone(path);
    let new_worktree = icon_button(
        ("sidebar-new-worktree", project_index),
        IconName::GitBranchPlus,
        "New worktree",
        cx,
    )
    .small()
    .debug_selector(move || format!("sidebar-new-worktree-{project_index}"))
    .on_click(move |_, window, cx| {
        cx.stop_propagation();
        window.dispatch_action(Box::new(NewWorktreeIn(new.to_path_buf())), cx);
    });
    let folder_icon = if folded {
        IconName::Folder
    } else {
        IconName::FolderOpen
    };
    let (toggled, menu) = (Rc::clone(path), Rc::clone(path));
    let row = (look.lit(row_shape(("sidebar-project", project_index)), false))
        .debug_selector(move || format!("sidebar-project-{project_index}"))
        .group("sidebar-project")
        .on_click(move |_, _, cx| {
            settings::update(cx, |settings| settings.toggle_folded(&toggled));
        })
        .child(
            div()
                .debug_selector(move || format!("sidebar-project-icon-{project_index}"))
                .child(icon(folder_icon, 14.).text_color(p.muted)),
        )
        .child(ellipsized(project.name.clone()).flex_1())
        .child(
            div()
                .relative()
                .child(shown_on_hover(div(), "sidebar-project").child(new_worktree))
                .children(mark),
        );
    let row = shell.row_hand.lift(row, (List::Projects, key, place), Axis::Vertical);
    let row = with_menu(row, move |_, cx| project_menu(&menu, cx));
    noted(row, move || format!("sidebar-project-{project_index}"))
}

/// A branch's row and, while the tabs are in the sidebar, its tabs' rows under it
/// (`rows::branch_tabs`), which go with it as it's dragged.
fn branch_rows(
    shell: &Shell,
    at: (usize, usize, u64),
    branch: (&BranchUnit, bool),
    owner: (&Rc<Path>, Option<&str>),
    shared: Shared,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let row = branch_row(shell, at, branch, owner, shared, window, cx);
    let folder: &Path = owner.0;
    let tabs = (shared.tabs).then(|| {
        let drawn = (shared.shown, shared.look);
        branch_tabs(shell, (at, folder), branch.0, drawn, window, cx)
    });
    match tabs.flatten() {
        Some(tabs) => (div().flex().flex_col().child(row).child(tabs)).into_any_element(),
        None => row.into_any_element(),
    }
}

/// A branch's row, one step in from its project's, the `branch_index`th of its project's and known
/// by `key` as it moves, `selected` or not, its title leaving off the prefix the person's own
/// branches carry in its project, whose folder is `project`. Clicked with the tabs on the top rail
/// it's selected; with them in the sidebar its tabs collapse or come back, kept for the next
/// launch, and nothing's selected: a tab picked selects its branch (`docs/layout.md`, Sidebar).
/// Dragged, it goes to another place among them. Its Changes and Files buttons are at its end
/// while they're placed on the branch's row, under the pointer, fading in and out, and the one
/// whose panel is open always (`Shell::row_panel_buttons`).
fn branch_row(
    shell: &Shell,
    (project_index, branch_index, key): (usize, usize, u64),
    (branch, selected): (&BranchUnit, bool),
    (project, prefix): (&Rc<Path>, Option<&str>),
    shared: Shared,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let (look, tabs) = (shared.look, shared.tabs);
    let p = look.p;
    let (hovered, hover) =
        theme::hover::eased(("sidebar-branch-hover", key), HOVER_FADE, window, cx);
    let at = (project_index, branch_index);
    let root = branch.worktree.path.as_path();
    let panel = shell.row_panel_buttons((at, root), (ROW_GLYPH, look.pill), hovered, cx);
    let (activity, waiting) = shell.checkout_status(root, cx);
    let place = || format!("sidebar-branch-{}", root.display());
    let mark = (activity.sign(place, p, cx)).map(|mark| {
        let name = move || format!("sidebar-branch-mark-{project_index}-{branch_index}");
        mark.debug_selector(name)
    });
    let (path, is_main) = (Rc::<Path>::from(root), branch.is_main());
    let (menu, folders) = (Rc::clone(&path), (Rc::clone(project), path));
    let weak = cx.weak_entity();
    let id = ("sidebar-branch", project_index * 100 + branch_index);
    let row = with_menu(indented(look.lit(row_shape(id), selected)), move |_, cx| {
        let terminals = weak.read_with(cx, |shell, _| shell.has_terminals(&menu));
        worktree_menu(&menu, is_main, terminals.unwrap_or(false))
    })
    .debug_selector(move || format!("sidebar-branch-{project_index}-{branch_index}"))
    .when(!selected, |row| row.text_color(p.muted))
    .on_hover(hover)
    .on_click(cx.listener(move |this, _, _, cx| {
        if tabs {
            let (project, worktree) = &folders;
            settings::update(cx, |settings| settings.toggle_collapsed(project, worktree));
        } else {
            this.workspace.select_branch(project_index, branch_index);
            cx.notify();
        }
    }));
    let list = (List::Worktrees(project_index), key, branch_index);
    let row = shell.row_hand.lift(row, list, Axis::Vertical);
    row.child(
        div()
            .debug_selector(move || format!("sidebar-branch-icon-{project_index}-{branch_index}"))
            .child(icon(IconName::GitBranch, 13.)),
    )
    .child(ellipsized(branch.title(prefix)))
    .when(is_main, |row| {
        row.child(div().text_xs().text_color(p.muted).child("checkout"))
    })
    .when(waiting > 0, |row| {
        row.child(crate::terminals::waiting_badge(waiting, p))
    })
    .child(div().flex_1())
    .children(panel)
    .children(mark)
}
