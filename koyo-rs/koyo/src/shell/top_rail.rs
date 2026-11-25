//! The top rail (`docs/layout.md`, Tabs; `docs/sidebar.md` 1, 11): the pages that are no
//! checkout's, then the selected checkout's tabs: the pinned pages in a pill (`pinned.rs`), the
//! space's loose pages and the Settings tab as tabs (`unpinned.rs`) and their +, a hairline, and
//! the checkout's tabs, in the order the sidebar lists them from its head down. The selected
//! branch's Changes and Files buttons are at the top rail's end (`shell/rails.rs`). On a space
//! switch the rail's pieces that are the space's move with the sidebar's space (`docs/layout.md`):
//! the checkout's tabs, and the loose pages while each space has its own, each within its own
//! place on the rail, so none runs over what stays put (`sliding`). That's while **Tabs**
//! puts the tabs on the top rail; with them in the sidebar there's no rail, and the sidebar lists
//! them all (`sidebar/tab_rows.rs`).

use gpui_kit::component::Sizable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Axis, Context, Div, ElementId, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement as _, Stateful, Styled as _, div, px,
};

use crate::chat::Sign;
use crate::model::{Front, Tab};
use crate::pane::CLOSE;
use crate::shell::{
    Shell, new_page_button, new_tab_button, row_shape, selectable, tab_area, tab_close_button,
    tab_list, tab_row,
};
use crate::theme::{self, Palette};

mod pinned;
mod unpinned;

/// What the top rail shows after the window's buttons and before the selected checkout's tabs:
/// the pinned pages in their pill, or the zone that says where one goes; the space's loose pages,
/// or the spot that takes the first while a page's tab is in hand, and the Settings tab, while
/// it's open, as tabs; their +, always there, which opens the launcher for a loose page only
/// (`docs/layout.md`); and, while a checkout is selected, a hairline between them and its tabs.
/// The loose pages move with the sidebar's space on a switch while each space lists its own;
/// shared by every space, they stay put, as the pinned pages do. Right-clicked where nothing has
/// a menu of its own, they open the tab area's (`layout_menus::tab_area`).
pub fn pages(shell: &Shell, cx: &mut Context<Shell>) -> Div {
    let p = theme::palette(cx);
    let pinned = pinned::region(shell, shell.pinned_taking(), p, cx);
    let loose = unpinned::tabs(shell, p, cx).map(|tabs| {
        let tabs = shell.carry_loose(div().min_w_0().flex().items_center().child(tabs), cx);
        sliding(div().min_w_0(), tabs)
    });
    let settings = unpinned::settings_tab(shell, p, cx);
    let new_page = (new_page_button("rail-new-page", cx).small())
        .debug_selector(|| "rail-new-page".to_owned());
    let hairline = shell.workspace.branch().map(|_| {
        (div().w(px(1.)).h(px(18.)).mx_1().flex_shrink_0())
            .debug_selector(|| "rail-separator".to_owned())
            .bg(p.muted.opacity(0.35))
    });
    tab_area(div().min_w_0().h_full().flex().items_center().gap_1())
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
/// rail, and move with the sidebar's space on a switch, as the space's branch is theirs.
pub fn tabs(shell: &Shell, cx: &mut Context<Shell>) -> impl IntoElement + use<> {
    let p = theme::palette(cx);
    let workspace = &shell.workspace;
    let project_index = workspace.active_project().unwrap_or_default();
    let branch_index = workspace
        .project()
        .map_or(0, |project| project.active_branch);
    let branch = workspace.branch();
    let tabs = branch.map_or(&[][..], |branch| &branch.tabs);
    let shown = workspace.front().and_then(Front::checkout);
    let rows = (tabs.iter().enumerate())
        .map(|(tab_index, tab)| {
            let at = (project_index, branch_index, tab_index);
            let active = shown == Some(at);
            let row = Shell::tab_press(rail_tab(("rail-tab", tab_index), active, p), at, cx);
            #[cfg(feature = "visual-check")]
            let row = {
                use gpui_kit::base::ElementExt as _;
                row.on_prepaint(crate::visual::noting(format!("rail-tab-{tab_index}")))
            };
            let list = (Axis::Horizontal, &shell.tab_hand);
            let (sign, split_end) = signs(shell, tab, p, cx);
            tab_row(row, (at, tab), list, split_end, p, cx, |row, face, cx| {
                let close = ("rail-tab-close", tab_index);
                let close = tab_close_button(close, at, (face.dirty, CLOSE), p, cx);
                single_page(row, (face.icon(13.), face.label()), sign, close)
            })
        })
        .collect();
    let list = ((project_index, branch_index), Axis::Horizontal);
    let tabs = tab_area(shell.carry(div(), cx))
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
                new_tab_button("rail-new-tab", cx)
                    .small()
                    .debug_selector(|| "new-tab-button".to_owned())
                    .on_mouse_down(MouseButton::Left, select),
            )
        });
    sliding(div().min_w_0(), tabs)
}

/// A tab's sign where its row shows it: after a single pane's icon (the first), or one for
/// all of a split's panes, after them (the second). Its spinner says its chats' work (`docs/activity.md`). Its dot says
/// what's most urgent: a terminal that waits for the person, then what a chat has for them, then
/// whatever a terminal's doing.
fn signs(shell: &Shell, tab: &Tab, p: Palette, cx: &mut App) -> (Option<Div>, Option<Div>) {
    let chats = shell.tab_chats_activity(tab, cx);
    let chat = chats.attention.map(|attention| attention.colour(p));
    let dot = match shell.tab_terminal_colour(tab, p) {
        Some((colour, true)) => Some(colour),
        Some((colour, false)) => chat.or(Some(colour)),
        None => chat,
    };
    let name = || format!("tab-{}", tab.first().id);
    let sign = Sign::new(name, chats.work, dot, cx).map(Sign::in_line);
    match tab {
        Tab::Single(_) => (sign, None),
        Tab::Split(_) => (None, sign),
    }
}

/// `piece`, the space's own on the top rail, carried with the sidebar's space on a switch
/// (`Shell::carry`) inside `place`, its own place on the rail, as tall as the rail: the place
/// stays put and clips it, so sliding out and in it never runs over what stays beside it, the
/// pinned pages, the pages' + and the window's buttons (`docs/layout.md`, Motion).
pub(super) fn sliding(place: Div, piece: impl IntoElement) -> Div {
    place.h_full().flex().items_center().overflow_hidden().child(piece)
}

/// A tab in the top rail, a checkout's or a loose page's: a row 28 high that narrows to fit
/// beside the others, its text muted unless it's `active`.
fn rail_tab(id: impl Into<ElementId>, active: bool, p: Palette) -> Stateful<Div> {
    selectable(rail_shape(id), active, (p.selected, p.hover))
        .when(!active, |tab| tab.text_color(p.muted))
}

/// A tab's shape in the top rail, without what the pointer does to it: a row 28 high that
/// narrows to fit beside the others.
fn rail_shape(id: impl Into<ElementId>) -> Stateful<Div> {
    row_shape(id).h(px(28.)).flex_shrink(1.).min_w_0().overflow_hidden()
}

/// What a rail tab of a single page or of Settings shows, at most 220 wide: its `icon`, `sign`
/// when there's one, its `title` and `close`.
fn single_page(
    tab: Stateful<Div>,
    (icon, title): (AnyElement, Div),
    sign: Option<Div>,
    close: Stateful<Div>,
) -> Stateful<Div> {
    tab.max_w(px(220.))
        .pl_2()
        .pr_1()
        .child(icon)
        .children(sign)
        .child(title)
        .child(close)
}

#[cfg(test)]
mod tests;
