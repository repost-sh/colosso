//! The shell's side of the loose pages (`docs/arc-sidebar.md` 11), the pages open in a space
//! that are no project's: their list, down the Arc sidebar or along the Orca layout's top rail,
//! and a page's row in it; one opened where the person is in no worktree, shown as its row is
//! picked, moved among them, closed, taken from a checkout's tabs or to them, and to and from
//! the pinned pages. What a page's events do, a link it opens, its `DevTools` beside it, what
//! it tells the history, goes one way whichever kind of tab it's in (`browser_tabs.rs`).

use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Along as _, AnyElement, AppContext as _, Axis, Context, Div, InteractiveElement as _,
    MouseButton, Pixels, Point, Stateful, StatefulInteractiveElement as _, Styled as _, Window,
};

use super::Shell;
use super::closing::ToAsk;
use super::reorder::{Drop, List, Reorder, TabHand};
use super::splits::{Dragged, DraggedPane};
use super::tab_face::Face;
use crate::model::{ClosedFrom, ClosedTab, Loose, Tab};
use crate::settings::{self, LoosePages};
use crate::theme::Palette;

/// The space's loose pages, each a row `loose_row` made, in a list along `axis` with `gap`
/// between them that they're dragged along: let go at another place of it, one moves there.
pub fn loose_list(
    hand: &Rc<TabHand>,
    (axis, gap): (Axis, Pixels),
    rows: Vec<(u64, AnyElement)>,
    cx: &mut Context<Shell>,
) -> Reorder {
    let on_drop = cx.listener(|this, drop: &Drop, _, cx| {
        if let Drop::Move { from, to } = *drop {
            this.move_loose(from, to, cx);
        }
    });
    Reorder::pages(axis, rows, hand.clone(), gap, Rc::new(on_drop))
}

/// `row`, the loose page `page`'s, at `at` in its list along `axis`, with what `single` lays out
/// in it as its page's face has it: lit while it's in hand; picked, its page shows; closed by a
/// middle click; dragged, it's taken up by the list, and off it travels as a chip.
pub fn loose_row(
    row: Stateful<Div>,
    (at, page): (usize, &Loose),
    (axis, hand): (Axis, &Rc<TabHand>),
    p: Palette,
    cx: &mut Context<Shell>,
    single: impl FnOnce(Stateful<Div>, &Face) -> Stateful<Div>,
) -> Stateful<Div> {
    let key = page.key();
    let face = Face::of(page.tab.first(), cx);
    let lifted = hand.holding() == Some(key);
    let held = DraggedPane {
        what: Dragged::Loose,
        faces: vec![face.clone()],
        hand: Some(hand.clone()),
        grab: Point::default(),
    };
    let hand = hand.clone();
    let row = row
        .when(lifted, |row| row.bg(p.hover).shadow_sm())
        .on_click(cx.listener(move |this, _, _, cx| {
            this.show_tab_holding(key, cx);
        }))
        .on_mouse_down(
            MouseButton::Middle,
            cx.listener(move |this, _, window, cx| {
                this.close_apart(key, Shell::close_loose, window, cx);
            }),
        )
        .on_drag(held, move |held, grab, _, cx| {
            hand.take(List::Loose, key, at, grab.along(axis));
            let chip = held.chip(grab);
            cx.new(|_| chip)
        });
    single(row, &face)
}

/// `button`, the × of the loose page known by `key`: pressed, the page closes, and its row isn't
/// picked.
pub fn loose_close(button: Stateful<Div>, key: u64, cx: &mut Context<Shell>) -> Stateful<Div> {
    button
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _, window, cx| {
            cx.stop_propagation();
            this.close_apart(key, Shell::close_loose, window, cx);
        }))
}

impl Shell {
    /// Whether a browser tab opened now is a loose page: the person is in no worktree, as a
    /// pinned or a loose page is the tab shown, or no checkout is selected.
    pub(crate) fn opens_loose(&self) -> bool {
        self.workspace.apart_shown() || self.workspace.branch().is_none()
    }

    /// A loose page's row let go at another place of their list: it moves there.
    pub(crate) fn move_loose(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        if self.workspace.move_loose(from, to) {
            cx.notify();
        }
    }

    /// Closes the loose page known by `key`, as its ×, a middle click and ⌘W do: it's gone, its
    /// page and its `DevTools` with it, kept to open again where it was with ⌘⇧T, at the
    /// address it showed, as a worktree's tab closed is.
    pub(crate) fn close_loose(&mut self, key: u64, cx: &mut Context<Self>) {
        self.let_apart_devtools_go(key, cx);
        let Some(at) = self.workspace.loose_at(key) else {
            return;
        };
        let space = self.workspace.space_shown();
        if let Some(Loose { tab, project }) = self.workspace.close_loose(at) {
            let from = ClosedFrom::Loose { space, project };
            self.workspace.remember_closed(ClosedTab { from, at, tab }, cx);
        }
        cx.notify();
    }

    /// Shows the space known by `space`, unless it's the one shown or is gone.
    pub(super) fn show_space_known_by(&mut self, space: u64, cx: &mut Context<Self>) {
        let settings = settings::get(cx);
        let index = settings.space_known_by(space);
        if let Some(index) = index.filter(|index| *index != settings.space) {
            self.show_space(index, cx);
        }
    }

    /// Closes the pinned or the loose page known by `key` with `close`, as the person asks by
    /// its ×, its menu or ⌘W, once the person may leave it
    /// (`leaving.rs`): closed with the keyboard in it,
    /// what shows in its place has the keyboard, as a closed tab's does (`close_tab_at`).
    pub(crate) fn close_apart(
        &mut self,
        key: u64,
        close: fn(&mut Self, u64, &mut Context<Self>),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panes = self.workspace.tab_holding(key).map(Tab::panes).unwrap_or_default();
        let ask = ToAsk::of(&panes, self.pane_with_keyboard(window, cx), cx);
        let close = move |shell: &mut Self, cx: &mut Context<Self>| close(shell, key, cx);
        self.ask_then(ask, close, window, cx);
    }

    /// The `DevTools` of the pinned or the loose page known by `key` close, their pane with
    /// them if they were detached beside it.
    pub(super) fn let_apart_devtools_go(&mut self, key: u64, cx: &mut Context<Self>) {
        let page = self.workspace.pane(key);
        if let Some(view) = page.and_then(|page| page.view().cloned()) {
            self.let_devtools_go(view, cx);
        }
    }

    /// Closes the pinned or the loose page shown, as ⌘W does: a pinned one stays pinned, a loose
    /// one is gone. Whether one was shown.
    pub(crate) fn close_apart_shown(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let shown = self.workspace.apart_shown_key();
        if let Some(key) = shown {
            self.close_apart(key, Self::close_pane_now, window, cx);
        }
        shown.is_some()
    }

    /// Moves the checkout's tab holding pane `pane` among the space's loose pages, at `place`,
    /// as it's let go on their list, and shows it there. Only a tab whose single pane is a page
    /// goes.
    pub(crate) fn loosen_tab_of(&mut self, pane: u64, place: usize, cx: &mut Context<Self>) {
        let tab = self.page_tab_of(pane);
        if tab.is_some_and(|at| self.workspace.loosen_tab(at, place)) {
            cx.notify();
        }
    }

    /// Moves the loose page known by `key` to the checkout `to`, as `(project, branch)`, at
    /// `place` among its tabs, as its row's let go there: a tab of that checkout, shown.
    pub(crate) fn settle_loose(
        &mut self,
        key: u64,
        to: (usize, usize),
        place: usize,
        cx: &mut Context<Self>,
    ) {
        let at = self.workspace.loose_at(key);
        if at.is_some_and(|at| self.workspace.settle_loose(at, to, place)) {
            cx.notify();
        }
    }

    /// Unpins the page known by `key` among the space's loose pages, at `place`, as its tile's
    /// let go on their list, and shows it there.
    pub(crate) fn loosen_pinned(&mut self, key: u64, place: usize, cx: &mut Context<Self>) {
        let at = self.workspace.pinned_at(key);
        if at.is_some_and(|at| self.workspace.loosen_pinned(at, place)) {
            cx.notify();
        }
    }

    /// The loose pages follow the space the settings show, however it came to be shown, and
    /// whether the settings share them among the spaces.
    pub(crate) fn follow_space_pages(&mut self, cx: &mut Context<Self>) {
        let settings = settings::get(cx);
        let shared = settings.loose_pages == LoosePages::Shared;
        let space = (self.workspace).show_pages_of(settings.shown_space_id());
        if self.workspace.share_loose(shared) || space {
            cx.notify();
        }
    }

    /// The space known by `gone` was deleted: its loose pages are the space known by `to`'s,
    /// where its projects went, or close with it.
    pub(crate) fn hand_on_loose(&mut self, gone: u64, to: Option<u64>, cx: &mut Context<Self>) {
        drop(self.workspace.hand_on_loose(gone, to));
        cx.notify();
    }

    /// The settings changed: every terminal's program follows the app's theme, its tab shown or
    /// not; each project goes by the name the person gave it in Koyo, or its folder's; and the
    /// loose pages are those of the space shown. A pinned or a loose page shown stays shown
    /// whichever layout lists it.
    pub(super) fn settings_changed(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.terminals.tell_theme(cx);
        let settings = settings::get(cx);
        self.workspace.name_projects(&settings.project_looks);
        self.follow_space_pages(cx);
    }
}
