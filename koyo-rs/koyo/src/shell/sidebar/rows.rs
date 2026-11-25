//! A branch's tabs as rows under its row while the tabs are in the sidebar (`docs/layout.md`,
//! Sidebar), and the rows the unpinned pages and the pinned pages' tiles share with them: the one
//! shown a pill. A tab says at its end how it's doing: the spinner while a chat in it is at work,
//! an indigo dot once something in it waits for the person.

use std::path::Path;

use gpui_kit::base::motion::{Transition, transition};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Axis, Context, Div, ElementId, InteractiveElement as _, ParentElement as _,
    Stateful, Styled as _, Window, div, px,
};

use super::pinned::takes_pins;
use super::{Look, noted, shown_on_hover};
use crate::chat::Activity;
use crate::model::{BranchUnit, Tab};
use crate::pane::{ROW_GLYPH, row_sized};
use crate::settings;
use crate::shell::{
    MOTION, Shell, ellipsized, inset, row_shape, tab_area, tab_close_button, tab_list, tab_row,
    title_with_end,
};
use crate::theme::Palette;

/// A tab's row, and an unpinned page's.
const ROW: f32 = 36.;
/// Between a branch's tabs' rows, and between the unpinned pages'.
pub(super) const GAP: f32 = 2.;
/// The tabs' icons.
pub(super) const TAB_ICON: f32 = 16.;
/// How far in from a row's end what's there sits: a tab's ×.
pub(super) const ROW_END: f32 = 6.;

/// A row in the sidebar, as the Arc browser's are: a tab's, an unpinned page's. A row's shape
/// (`row_shape`), taller and rounder, its icon first, in a column `TAB_ICON` wide, its text
/// after.
pub(super) fn sidebar_row(id: impl Into<ElementId>) -> Stateful<Div> {
    row_shape(id)
        .h(px(ROW))
        .gap(px(10.))
        .rounded(px(super::ROUNDED))
        .cursor_pointer()
}

/// A tab's `icon`, `TAB_ICON` wide, at the head of its row, a little fainter than its title.
pub(super) fn tab_icon(icon: AnyElement, p: Palette) -> Div {
    div()
        .flex_shrink_0()
        .text_color(p.text.opacity(0.75))
        .child(icon)
}

/// The tabs of the branch `unit` at `at`, as `(project, branch)` and known by `key` as it moves,
/// of the project at `project`, as rows under its row while the tabs are in the sidebar
/// (`docs/layout.md`), what's in each a step further in than in the branch's (`inset`), the tab
/// `shown` a pill: sliding closed as they're collapsed and open as they're brought back. None
/// while they're collapsed or it has no tab. Right-clicked where nothing has a menu of its own,
/// they open the tab area's.
pub(super) fn branch_tabs(
    shell: &Shell,
    ((project_index, branch_index, key), project): ((usize, usize, u64), &Path),
    unit: &BranchUnit,
    (shown, look): (Option<(usize, usize, usize)>, Look),
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> Option<Div> {
    let target = if settings::get(cx).is_collapsed(project, &unit.worktree.path) {
        0.
    } else {
        1.
    };
    let id = ElementId::from(("sidebar-branch-open", key));
    let open = transition(id, target, Transition::new(MOTION), window, cx);
    if open == 0. || unit.tabs.is_empty() {
        return None;
    }
    let rows: Vec<_> = (unit.tabs.iter().enumerate())
        .map(|(tab_index, tab)| {
            let at = (project_index, branch_index, tab_index);
            tab_row_of(shell, at, tab, shown == Some(at), look, cx)
        })
        .collect();
    let list = ((project_index, branch_index), Axis::Vertical);
    let tabs = tab_list(&shell.tab_hand, list, rows, px(GAP), look.p, cx);
    let height = unit.tabs.len() as f32 * (ROW + GAP);
    let sliding = div()
        .flex_shrink_0()
        .overflow_hidden()
        .when(open < 1., |sliding| sliding.h(px(height * open)))
        .child(div().pt(px(GAP)).child(tabs));
    Some(tab_area(sliding))
}

/// The tab `tab` at `at` as its row in the sidebar, what's in it a step further in than in its
/// branch's: the one shown (`active`) a pill, the others tinted while hovered, and at its end how
/// it's doing (`sign`), or its × while it's hovered, or shown with nothing to say. A pinned
/// page's tile let go on it is unpinned there. A chat's, but for the tab shown's, says under its
/// title what was said last in it.
fn tab_row_of(
    shell: &Shell,
    at: (usize, usize, usize),
    tab: &Tab,
    active: bool,
    look: Look,
    cx: &mut Context<Shell>,
) -> (u64, AnyElement) {
    let p = look.p;
    let (project_index, branch_index, tab_index) = at;
    let first = tab.first().id;
    let (activity, said) = shell.tab_status(tab, !active, cx);
    let row = sidebar_row(("sidebar-tab", branch_index * 100 + tab_index))
        .group("sidebar-tab")
        .pl(inset(2));
    let row = Shell::tab_press(look.lit(row, active).pr(px(ROW_END)), at, cx)
        .when(matches!(tab, Tab::Split(_)), |row| {
            row.children(sign(activity, ("sidebar-tab", first), look, cx))
        });
    let row = takes_pins(row, look, cx, move |this, key, cx| {
        this.unpin_onto(key, (project_index, branch_index), tab_index, cx);
    });
    let row = noted(row, move || format!("sidebar-tab-{branch_index}-{tab_index}"));
    let list = (Axis::Vertical, &shell.tab_hand);
    tab_row(row, (at, tab), list, p, cx, |row, face, cx| {
        let close = ("sidebar-tab-close", branch_index * 100 + tab_index);
        let mark = sign(activity, ("sidebar-tab", first), look, cx);
        let mark = mark.map(|sign| at_end(sign, "sidebar-tab"));
        let hidden = !face.dirty && (!active || mark.is_some());
        let shown = !hidden || mark.is_some();
        let close = tab_close_button(close, at, (face.dirty, ROW_GLYPH), p, cx);
        let close = row_sized(close).when(hidden, |close| shown_on_hover(close, "sidebar-tab"));
        let icon = tab_icon(face.icon(TAB_ICON), p).debug_selector(move || {
            format!("tab-icon-{project_index}-{branch_index}-{tab_index}")
        });
        let end = div().relative().children(mark).child(close);
        let said = said.map(|said| {
            ellipsized(said)
                .debug_selector(move || format!("sidebar-tab-said-{first}"))
                .text_xs()
                .line_height(px(15.))
                .text_color(look.label.opacity(0.75))
        });
        let title = div()
            .id(("sidebar-tab-title", first))
            .debug_selector(move || format!("sidebar-tab-title-{first}"))
            .flex()
            .flex_col()
            .child(
                face.label()
                    .when(said.is_some(), |label| label.line_height(px(18.))),
            )
            .children(said);
        let title = title_with_end(title, end, shown, "sidebar-tab");
        row.child(icon).child(title)
    })
}

/// What a tab's row says of how it's doing: the spinner while a chat in it is at work, an indigo
/// dot once something in it waits for the person. It's known as `of` and by `key`, its first
/// pane's.
fn sign(
    activity: Activity,
    (of, key): (&'static str, u64),
    look: Look,
    cx: &mut Context<Shell>,
) -> Option<Div> {
    let name = match activity {
        Activity::Quiet => return None,
        Activity::Working => "working",
        Activity::Attention => "waiting",
    };
    let sign = activity.sign(|| format!("{of}-{key}"), look.p, cx)?;
    Some(sign.debug_selector(move || format!("{of}-{name}-{key}")))
}

/// `sign` at the end of a row of `group`, in the place of what shows there under the pointer, a
/// tab's × or a project's New worktree button, which it leaves to that while the pointer's on
/// the row. A file's dot for unsaved edits never meets it there: only a chat or a terminal has
/// something to say.
pub(super) fn at_end(sign: Div, group: &'static str) -> Div {
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .group_hover(group, gpui_kit::Styled::invisible)
        .child(sign)
}
