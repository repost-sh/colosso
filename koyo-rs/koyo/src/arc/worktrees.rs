//! The space under its tiles (`docs/arc-sidebar.md` 4, 5): the selected project's name, then each
//! of its worktrees as Arc's divider, the worktree's name on the left and, while the pointer's on
//! it, Clear or Delete on the right; a New Tab row, and its tabs as Arc's rows, the one shown a
//! pill. A tab says at its end how it's doing, as its project's tile does: the spinner while a
//! chat in it is at work, an indigo dot once something in it waits for the person.

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, AnyElement, Axis, ClickEvent, Context, Div, FontWeight, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, Stateful, StatefulInteractiveElement as _,
    Styled as _, Window, div, px, relative,
};

use super::actions::ClearWorktree;
use super::{Look, space_colour, space_dot};
use crate::chat::{Activity, spinner};
use crate::launcher::NewTab;
use crate::model::{BranchUnit, Tab};
use crate::pane::dot;
use crate::projects::{DeleteWorktree, with_menu, worktree_menu};
use crate::settings;
use crate::shell::{
    Dragged, DraggedPane, Shell, add_project_row, ellipsized, icon, project_error,
    tab_close_button, tab_list, tab_row,
};
use crate::theme;

/// A tab's row, and New Tab's.
const ROW: f32 = 36.;
/// How long a divider's Clear or Delete takes to fade in or out as the pointer comes onto it or
/// leaves.
const OFFER_FADE: Duration = Duration::from_millis(150);
/// The tabs' icons.
const TAB_ICON: f32 = 16.;

/// The project shown's name and its worktrees, scrolling under the tiles.
pub fn space(
    shell: &Shell,
    look: Look,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let p = look.p;
    // The project's name, in its space's dot; none while no project is shown, as an empty space
    // shows none.
    let project =
        (shell.workspace.active_project()).map(|index| (index, &shell.workspace.projects[index]));
    let name = project.map(|(_, project)| project.name.clone());
    let colour = space_colour(settings::get(cx).space, cx);
    let header = div()
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
        .children(name.map(ellipsized));
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
            for (branch_index, unit) in project.branches.iter().enumerate() {
                let active_tab = (branch_index == project.active_branch).then_some(unit.active_tab);
                let at = (project_index, branch_index);
                let worktree = worktree(shell, at, unit, active_tab, look, window, cx);
                body.push(worktree.into_any_element());
            }
        }
    }
    div()
        .id("arc-space")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .child(header)
        .children(body)
}

/// A worktree's divider, its New Tab row and its tabs; `active_tab` is set only on the
/// selected worktree.
fn worktree(
    shell: &Shell,
    (project_index, branch_index): (usize, usize),
    unit: &BranchUnit,
    active_tab: Option<usize>,
    look: Look,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let p = look.p;
    let selected = active_tab.is_some();
    let divider = divider(
        shell,
        (project_index, branch_index),
        unit,
        selected,
        look,
        window,
        cx,
    );
    let new_tab = div()
        .id(("arc-new-tab", branch_index))
        .debug_selector(move || format!("arc-new-tab-{branch_index}"))
        .h(px(ROW))
        .flex_shrink_0()
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(10.))
        .rounded(px(10.))
        .cursor_pointer()
        .text_sm()
        .text_color(look.label)
        .hover(|style| style.bg(look.row_hover))
        .child(icon(IconName::Plus, TAB_ICON))
        .child("New Tab")
        // The launcher opens its tab on the selected worktree, so this one is selected on the
        // press that opens it.
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, _, cx| {
                this.workspace.select_branch(project_index, branch_index);
                cx.notify();
            }),
        )
        .on_click(|_, window, cx| window.dispatch_action(Box::new(NewTab), cx));
    let new_tab = takes_tabs(new_tab, (project_index, branch_index), look, cx);
    let rows = (unit.tabs.iter().enumerate())
        .map(|(tab_index, tab)| {
            let at = (project_index, branch_index, tab_index);
            tab_row_of(shell, at, tab, active_tab == Some(tab_index), look, cx)
        })
        .collect();
    let list = ((project_index, branch_index), Axis::Vertical);
    div()
        .flex()
        .flex_col()
        .gap_px()
        .child(divider)
        .child(new_tab)
        .child(tab_list(&shell.tab_hand, list, rows, px(2.), p, cx))
}

/// What a worktree's divider offers at its end while it's hovered (`docs/arc-sidebar.md` 5).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Offer {
    /// It has tabs: closing them all, once the person says so.
    Clear,
    /// It has none, and it's a linked worktree: deleting it, through the Delete Worktree dialog.
    Delete,
}

impl Offer {
    /// What a worktree with `tabs` tabs offers; a main checkout with none, which git can't delete,
    /// offers nothing.
    fn of(tabs: usize, is_main: bool) -> Option<Offer> {
        match (tabs, is_main) {
            (0, true) => None,
            (0, false) => Some(Offer::Delete),
            _ => Some(Offer::Clear),
        }
    }

    fn word(self) -> &'static str {
        match self {
            Offer::Clear => "Clear",
            Offer::Delete => "Delete",
        }
    }

    /// The id of its word on a divider, which the branch's index follows.
    fn id(self) -> &'static str {
        match self {
            Offer::Clear => "arc-clear",
            Offer::Delete => "arc-delete",
        }
    }

    /// The room the line leaves the word: the word and the gap before it.
    fn room(self) -> f32 {
        match self {
            Offer::Clear => 38.,
            Offer::Delete => 46.,
        }
    }

    fn action(self, worktree: PathBuf) -> Box<dyn Action> {
        match self {
            Offer::Clear => Box::new(ClearWorktree(worktree)),
            Offer::Delete => Box::new(DeleteWorktree(worktree)),
        }
    }
}

/// A worktree's divider: its name, which selects it, the line, and at its end what the worktree
/// offers, Clear or Delete, shown only while the divider's hovered, fading in and out as Arc's
/// does; right-clicked, the worktree's menu.
fn divider(
    shell: &Shell,
    (project_index, branch_index): (usize, usize),
    unit: &BranchUnit,
    selected: bool,
    look: Look,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let p = look.p;
    let (path, is_main) = (unit.worktree.path.clone(), unit.is_main());
    let terminals = shell.has_terminals(&path);
    // What it offers shows only while the divider's hovered, fading in and out as Arc's does,
    // the line making room for it as it comes and running to the end once it's gone.
    let offer = Offer::of(unit.tabs.len(), is_main);
    let (shown, hover) = theme::hover::eased(
        gpui_kit::ElementId::Name(format!("arc-offer-{}", path.display()).into()),
        OFFER_FADE,
        window,
        cx,
    );
    let divider = div()
        .id(("arc-divider", branch_index))
        .debug_selector(move || format!("arc-divider-{branch_index}"))
        .h(px(26.))
        .mt(px(6.))
        .px(px(10.))
        .relative()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(8.))
        .rounded(px(6.))
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .on_hover(hover)
        .child(
            div()
                .id(("arc-worktree", branch_index))
                .min_w_0()
                .max_w(relative(0.7))
                .truncate()
                .cursor_pointer()
                .text_color(if selected {
                    p.text.opacity(0.8)
                } else {
                    look.label
                })
                .hover(|style| style.text_color(p.text))
                // Selecting the worktree points the changes panel at its checkout.
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.workspace.select_branch(project_index, branch_index);
                    cx.notify();
                }))
                .child(unit.name()),
        )
        .child(
            div()
                .flex_1()
                .h(px(1.))
                .mr(px(offer.map_or(0., Offer::room) * shown))
                .bg(look.line),
        )
        .when_some(offer, |divider, offer| {
            let (word, worktree) = (offer.word(), path.clone());
            divider.child(
                div()
                    .id((offer.id(), branch_index))
                    .debug_selector(move || format!("{}-{branch_index}", offer.id()))
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right(px(10.))
                    .flex()
                    .items_center()
                    .text_color(p.danger)
                    .opacity(0.85 * shown)
                    .when(shown > 0., |clear| {
                        clear.cursor_pointer().hover(|style| style.opacity(1.))
                    })
                    .child(word)
                    .on_click(move |_, window, cx| {
                        window.dispatch_action(offer.action(worktree.clone()), cx);
                    }),
            )
        });
    let divider = takes_tabs(divider, (project_index, branch_index), look, cx);
    let menu_path = path;
    with_menu(divider, move |_, _| {
        worktree_menu(&menu_path, is_main, terminals)
    })
}

/// Has `row` take a tab dragged from another checkout's list, which then goes first among the
/// tabs of the checkout `to`, as `(project, branch)`; lit while one's over it.
fn takes_tabs(
    row: Stateful<Div>,
    to: (usize, usize),
    look: Look,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    row.drag_over::<DraggedPane>(move |style, held, _, _| match held.what {
        Dragged::Tab(project, branch, _) if (project, branch) != to => style.bg(look.row_hover),
        _ => style,
    })
    .on_drop(cx.listener(move |this, held: &DraggedPane, _, cx| {
        this.drop_tab_on_checkout(held, to, 0, cx);
    }))
}

/// The tab `tab` at `at` as Arc's row: the one shown a pill, the others tinted while hovered,
/// and at its end how it's doing (`sign`), or its × while it's hovered, or shown with nothing to
/// say.
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
    let row = div()
        .id(("arc-tab", branch_index * 100 + tab_index))
        .h(px(ROW))
        .flex_shrink_0()
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(10.))
        .rounded(px(10.))
        .text_sm()
        .cursor_pointer()
        .group("arc-tab")
        .when(active, |row| row.bg(look.pill))
        .when(!active, |row| row.hover(|style| style.bg(look.row_hover)))
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
        )
        // A split's row is its panes, each with a × of its own, and no end to say it at: what
        // the tab says goes before them.
        .when(matches!(tab, Tab::Split(_)), |row| {
            row.children(sign(activity, first, look, cx))
        });
    let list = (Axis::Vertical, &shell.tab_hand);
    tab_row(row, (at, tab), list, p, cx, |row, face, cx| {
        let close = ("arc-tab-close", branch_index * 100 + tab_index);
        let mark = sign(activity, first, look, cx).map(at_end);
        // Shown on the tab shown, and on the others while hovered; a dot for unsaved edits
        // always. What the row says of its tab has the ×'s place until the pointer's on it.
        let hidden = !face.dirty && (!active || mark.is_some());
        let close = tab_close_button(close, at, face.dirty, p, cx).when(hidden, |close| {
            close
                .opacity(0.)
                .group_hover("arc-tab", |style| style.opacity(1.))
        });
        let icon = div()
            .flex_shrink_0()
            .text_color(p.text.opacity(0.75))
            .child(face.icon(TAB_ICON));
        let end = div().relative().flex_shrink_0().children(mark).child(close);
        row.pr(px(6.))
            .child(icon)
            .child(face.label().flex_1())
            .child(end)
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

#[cfg(test)]
mod tests {
    use super::Offer;

    #[test]
    fn a_worktree_with_tabs_offers_clear_and_an_empty_one_delete() {
        assert!(matches!(Offer::of(3, false), Some(Offer::Clear)));
        assert!(matches!(Offer::of(1, true), Some(Offer::Clear)));
        assert!(matches!(Offer::of(0, false), Some(Offer::Delete)));
        // Git can't delete a main checkout, and with no tabs there's nothing to clear.
        assert!(Offer::of(0, true).is_none());
    }
}
