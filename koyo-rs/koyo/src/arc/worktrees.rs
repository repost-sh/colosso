//! The space under its tiles (`docs/arc-sidebar.md` 4, 5): the selected project's name, then each
//! of its worktrees as Arc's folder, under a header (`header.rs`) that collapses it, opens a tab
//! in it and drags it to another place among the project's; open, its tabs as Arc's rows, the one
//! shown a pill. A tab says at its end how it's doing, as its project's tile does: the spinner
//! while a chat in it is at work, an indigo dot once something in it waits for the person.

use std::rc::Rc;

use gpui_kit::base::motion::{Transition, transition};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Axis, ClickEvent, Context, Div, ElementId, FontWeight,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Stateful,
    StatefulInteractiveElement as _, Styled as _, Window, div, px,
};

use super::DraggedPin;
use super::glide;
use super::header::header;
use super::tiles::key_of;
use super::{Look, space_colour, space_dot};
use crate::chat::{Activity, spinner};
use crate::model::{BranchUnit, Tab};
use crate::pane::{ROW_GLYPH, dot, row_sized};
use crate::settings;
use crate::shell::{
    Dragged, DraggedPane, Drop, Face, MOTION, Reorder, Shell, add_project_row, ellipsized,
    project_error, tab_close_button, tab_list, tab_row, title_with_end,
};
use crate::theme::Palette;

/// A tab's row, and a worktree's header.
const ROW: f32 = 36.;
/// Between a worktree's rows.
const GAP: f32 = 2.;
/// Between the worktrees: more than between a worktree's rows, so each header sits on its own
/// tabs and the worktrees stand apart.
const WORKTREE_GAP: f32 = 12.;
/// The tabs' icons.
pub(super) const TAB_ICON: f32 = 16.;
/// How far in from a row's end what's there sits: a header's `…` and `+`, a tab's ×.
pub(super) const ROW_END: f32 = 6.;

/// The list the worktrees of the project at `project` are dragged along.
pub(super) fn list_of(project: usize) -> u64 {
    project as u64
}

/// A row of a worktree's, as Arc's are: its header, a tab. Its icon goes first, in a column
/// `TAB_ICON` wide, its text after.
pub(super) fn arc_row(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(ROW))
        .flex_shrink_0()
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(10.))
        .rounded(px(super::ROUNDED))
        .cursor_pointer()
        .text_sm()
}

/// A row that shows a tab, a worktree's or a loose page, known by `key`, its first pane: the
/// pill while it's the tab shown, or as far as the pill passing through it has it (`glide`), and
/// lit under the pointer otherwise. Before anything else goes in it, as the pill goes under.
pub(super) fn pill_row(
    row: Stateful<Div>,
    (key, active): (u64, bool),
    shell: &Shell,
    look: Look,
    cx: &App,
) -> Stateful<Div> {
    let passing = shell.swipe.glides.rows.lit(key, cx.background_executor().now());
    let light = passing.and_then(|lit| glide::light(lit, Axis::Vertical, look.pill));
    row.relative()
        .when(active && passing.is_none(), |row| row.bg(look.pill))
        .when(!active, |row| row.hover(|style| style.bg(look.row_hover)))
        .children(light.map(|light| light.debug_selector(move || format!("arc-tab-light-{key}"))))
}

/// A tab's icon at the head of its row, a little fainter than its title.
pub(super) fn tab_icon(face: &Face, p: Palette) -> Div {
    div()
        .flex_shrink_0()
        .text_color(p.text.opacity(0.75))
        .child(face.icon(TAB_ICON))
}

/// The space's loose pages, then the project shown's name and its worktrees, scrolling between
/// the pinned pages and the space's tiles. `loosens` says something the loose pages take is in
/// hand.
pub fn space(
    shell: &Shell,
    loosens: bool,
    look: Look,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let p = look.p;
    // The project's name, after its space's dot; neither while no project is shown, as a space
    // with no project has none to name.
    let project =
        (shell.workspace.active_project()).map(|index| (index, &shell.workspace.projects[index]));
    let colour = space_colour(settings::get(cx).space, cx);
    let heading = project.map(|(_, project)| {
        div()
            .debug_selector(|| "arc-project-name".to_owned())
            .h(px(28.))
            .flex_shrink_0()
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(8.))
            .text_sm()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(look.label)
            .child(space_dot(colour))
            .child(ellipsized(project.name.clone()))
    });
    let mut body: Vec<AnyElement> = Vec::new();
    match project {
        None => body.push(
            add_project_row("arc-add-project", p)
                .px(px(10.))
                .into_any_element(),
        ),
        Some((project_index, project)) => {
            if let Some(error) = project.error.clone() {
                body.push(project_error(error, p).into_any_element());
            }
            // A pinned or a loose page shown, no tab of a worktree's is: none is the pill.
            let shown = shell.workspace.shown_checkout_tab();
            let worktrees = (project.branches.iter().enumerate())
                .map(|(branch_index, unit)| {
                    let at = (project_index, branch_index);
                    let active_tab = shown.filter(|shown| (shown.0, shown.1) == at);
                    let active_tab = active_tab.map(|shown| shown.2);
                    worktree(shell, at, unit, active_tab, look, window, cx)
                })
                .collect();
            // A worktree let go among them goes there, in both layouts' lists.
            let on_drop = cx.listener(move |this, drop: &Drop, _, cx| {
                if let Drop::Move { from, to } = *drop {
                    this.move_worktree(project_index, from, to, cx);
                }
            });
            let (list, hand) = (list_of(project_index), shell.worktree_hand.clone());
            let worktrees =
                Reorder::worktrees(list, worktrees, hand, px(WORKTREE_GAP), Rc::new(on_drop));
            body.push(worktrees.into_any_element());
        }
    }
    div()
        .id("arc-space")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .children(super::loose::list(shell, loosens, look, cx))
        .children(heading)
        .children(body)
}

/// A worktree's header and, unless it's collapsed, its tabs, sliding open or closed as it's
/// opened or collapsed, by the key it keeps as it moves among the project's;
/// `active_tab` is set only on the selected worktree.
fn worktree(
    shell: &Shell,
    (project_index, branch_index): (usize, usize),
    unit: &BranchUnit,
    active_tab: Option<usize>,
    look: Look,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> (u64, AnyElement) {
    let p = look.p;
    let project = &shell.workspace.projects[project_index].path;
    let path = &unit.worktree.path;
    let key = key_of(path);
    // As far open as it's come, as a tab slides into place.
    let target = if settings::get(cx).is_collapsed(project, path) {
        0.
    } else {
        1.
    };
    let id = ElementId::from(("arc-open", key));
    let open = transition(id, target, Transition::new(MOTION), window, cx);
    let at = (project_index, branch_index, key);
    let header = header(shell, at, unit, look, window, cx);
    // Collapsed and still, or with no tabs, it has no rows: none is drawn, or picked.
    if open == 0. || unit.tabs.is_empty() {
        return (key, header.into_any_element());
    }
    let rows: Vec<_> = (unit.tabs.iter().enumerate())
        .map(|(tab_index, tab)| {
            let at = (project_index, branch_index, tab_index);
            tab_row_of(shell, at, tab, active_tab == Some(tab_index), look, cx)
        })
        .collect();
    let list = ((project_index, branch_index), Axis::Vertical);
    let tabs = tab_list(&shell.tab_hand, list, rows, px(GAP), p, cx);
    // As they slide, they're clipped to the share of their height `open` has them at; still,
    // they take their own. Each row is `ROW` high, a gap before it, so their height is known
    // before they're drawn.
    let height = unit.tabs.len() as f32 * (ROW + GAP);
    let sliding = div()
        .flex_shrink_0()
        .overflow_hidden()
        .when(open < 1., |sliding| sliding.h(px(height * open)))
        .child(div().pt(px(GAP)).child(tabs));
    let worktree = div().flex().flex_col().child(header).child(sliding);
    (key, worktree.into_any_element())
}

/// Has `row` take a tab dragged from another checkout's list, which then goes first among the
/// tabs of the checkout `to`, as `(project, branch)`; lit while one's over it. And a pinned
/// page's tile, unpinned there.
pub(super) fn takes_tabs(
    row: Stateful<Div>,
    to: (usize, usize),
    look: Look,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let row = row
        .drag_over::<DraggedPane>(move |style, held, _, _| match held.what {
            Dragged::Tab(project, branch, _) if (project, branch) != to => style.bg(look.row_hover),
            Dragged::Loose => style.bg(look.row_hover),
            Dragged::Tab(..) | Dragged::Pane(_) => style,
        })
        .on_drop(cx.listener(move |this, held: &DraggedPane, _, cx| {
            this.drop_tab_on_checkout(held, to, 0, cx);
        }));
    takes_pins(row, to, 0, look, cx)
}

/// Has `row` take a pinned page's tile let go on it (`docs/arc-sidebar.md` 11): unpinned, it's a
/// tab of the checkout `to`, as `(project, branch)`, at `place` among its tabs, and shows there;
/// lit while one's over it.
fn takes_pins(
    row: Stateful<Div>,
    to: (usize, usize),
    place: usize,
    look: Look,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    row.drag_over::<DraggedPin>(move |style, _, _, _| style.bg(look.row_hover))
        .on_drop(cx.listener(move |this, pin: &DraggedPin, _, cx| {
            this.unpin_onto(pin.key, to, place, cx);
        }))
}

/// The tab `tab` at `at` as Arc's row: the one shown a pill, the others tinted while hovered,
/// and at its end how it's doing (`sign`), or its × while it's hovered, or shown with nothing to
/// say. A chat's, but for the tab shown's, says under its title what was said last in it.
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
    let (activity, first) = (shell.tab_activity(tab, cx), tab.first().id);
    // The tab shown shows its chat beside the sidebar, so what was said is said there.
    let said = shell.tab_said(tab, cx).filter(|_| !active);
    let row = arc_row(("arc-tab", branch_index * 100 + tab_index)).group("arc-tab");
    let row = pill_row(row, (first, active), shell, look, cx)
        .pr(px(ROW_END))
        .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
            this.pick_tab(project_index, branch_index, tab_index, cx);
            // A second click keeps a previewed file open.
            if event.click_count() > 1 {
                this.keep_tab(at, cx);
            }
        }))
        .on_mouse_down(
            MouseButton::Middle,
            cx.listener(move |this, _, window, cx| this.close_tab_at(at, window, cx)),
        )
        // A split's row is its panes, each with a × of its own, and no end to say it at: what
        // the tab says goes before them.
        .when(matches!(tab, Tab::Split(_)), |row| {
            row.children(sign(activity, first, look, cx))
        });
    // A pinned page's tile let go on it is unpinned where it is.
    let row = takes_pins(row, (project_index, branch_index), tab_index, look, cx);
    // The visual check takes a tab up where its row is drawn.
    #[cfg(feature = "visual-check")]
    let row = {
        use gpui_kit::base::ElementExt as _;
        let name = format!("arc-tab-{branch_index}-{tab_index}");
        row.on_prepaint(crate::visual::noting(name))
    };
    let list = (Axis::Vertical, &shell.tab_hand);
    tab_row(row, (at, tab), list, p, cx, |row, face, cx| {
        let close = ("arc-tab-close", branch_index * 100 + tab_index);
        let mark = sign(activity, first, look, cx).map(at_end);
        // Shown on the tab shown, and on the others while hovered; a dot for unsaved edits
        // always. What the row says of its tab has the ×'s place until the pointer's on it.
        let hidden = !face.dirty && (!active || mark.is_some());
        // Something's at its end, or only once the pointer's on the row.
        let shown = !hidden || mark.is_some();
        let close = tab_close_button(close, at, (face.dirty, ROW_GLYPH), p, cx);
        let close = row_sized(close).when(hidden, |close| {
            close
                .opacity(0.)
                .group_hover("arc-tab", |style| style.opacity(1.))
        });
        let icon = tab_icon(face, p);
        let end = div().relative().children(mark).child(close);
        // With what was said, two lines in a row's height, as Arc's are; it's fainter than the
        // labels, saying less than the title it's under.
        let said = said.map(|said| {
            ellipsized(said)
                .debug_selector(move || format!("arc-tab-said-{first}"))
                .text_xs()
                .line_height(px(15.))
                .text_color(look.label.opacity(0.75))
        });
        let title = div()
            .id(("arc-tab-title", first))
            .debug_selector(move || format!("arc-tab-title-{first}"))
            .flex()
            .flex_col()
            .child(
                face.label()
                    .when(said.is_some(), |label| label.line_height(px(18.))),
            )
            .children(said);
        let title = title_with_end(title, end, shown, "arc-tab");
        row.child(icon).child(title)
    })
}

/// What a tab's row says of how the tab's doing (`docs/arc-sidebar.md` 5), as its project's
/// tile does in its corner: the spinner while a chat in it is at work, an indigo dot once something
/// in it waits for the person. The tab is known by its first pane, `pane`.
fn sign(activity: Activity, pane: u64, look: Look, cx: &mut Context<Shell>) -> Option<Div> {
    let (sign, name) = match activity {
        Activity::Quiet => return None,
        Activity::Working => (spinner(format!("arc-tab-{pane}"), cx), "working"),
        Activity::Attention => (dot(look.p.waiting), "waiting"),
    };
    Some(sign.debug_selector(move || format!("arc-tab-{name}-{pane}")))
}

/// `sign` at the end of a single pane's row: in the ×'s place, which it leaves to the × while
/// the pointer's on the row. A file's dot for unsaved edits never meets it there: only a chat
/// or a terminal has something to say.
fn at_end(sign: Div) -> Div {
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        // Not drawn at all while the row's hovered, rather than faded: the spinner is a view,
        // which a fade round it wouldn't reach.
        .group_hover("arc-tab", gpui_kit::Styled::invisible)
        .child(sign)
}
