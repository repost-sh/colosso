//! What the pointer rests on in the sidebar, and the card that says more of it
//! (`docs/sidebar.md` 4, 11): a pinned page's after a moment on its tile, a space's name
//! after a shorter one on its dot, at once while another card's up, as tooltips go from one to
//! the next. Tiles and dots note where they're drawn, which the card is placed by.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::{App, Bounds, Context, Pixels, Task, Window};

use super::card::{self, Card, PageCard};
use crate::settings;
use crate::shell::Shell;

/// How long the pointer rests on a tile before its card shows, and on a space's dot before its
/// name does.
const TILE_WAIT: Duration = Duration::from_millis(400);
const SPACE_WAIT: Duration = Duration::from_millis(250);

/// What the card is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Hovered {
    /// The space known by this id, by its mark, whatever its place among them.
    Space(u64),
    /// The pinned page known by this, its page's pane, by its tile.
    Pin(u64),
}

/// What the pointer's on, where the tiles and dots were last drawn, and the wait for a card.
#[derive(Default)]
pub struct Hover {
    pub on: Option<Hovered>,
    drawn: Rc<RefCell<HashMap<Hovered, Bounds<Pixels>>>>,
    wait: Option<Task<()>>,
}

impl Hover {
    /// Where `what` was last drawn, for the visual check to press it there.
    #[cfg(feature = "visual-check")]
    pub fn drawn_at(&self, what: Hovered) -> Option<Bounds<Pixels>> {
        self.drawn.borrow().get(&what).copied()
    }

    /// Notes where `what` is drawn, given the bounds of what shows it (`ElementExt::on_prepaint`).
    pub fn noting(
        &self,
        what: Hovered,
    ) -> impl FnOnce(Bounds<Pixels>, &mut Window, &mut App) + use<> {
        let drawn = Rc::clone(&self.drawn);
        move |bounds, _, _| {
            drawn.borrow_mut().insert(what, bounds);
        }
    }
}

impl Shell {
    /// The pointer came onto `what`, or left it.
    pub(super) fn hover(
        &mut self,
        what: Hovered,
        hovered: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let redraw = matches!(what, Hovered::Space(_));
        if !hovered {
            if self.sidebar_hover.on == Some(what) {
                self.sidebar_hover.on = None;
                self.sidebar_hover.wait = None;
                card::hide(cx);
                if redraw {
                    cx.notify();
                }
            }
            return;
        }
        self.sidebar_hover.on = Some(what);
        if redraw {
            cx.notify();
        }
        if card::is_shown(cx) {
            self.show_card(what, window, cx);
            return;
        }
        let wait = match what {
            Hovered::Pin(_) => TILE_WAIT,
            Hovered::Space(_) => SPACE_WAIT,
        };
        self.sidebar_hover.wait = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(wait).await;
            this.update_in(cx, |this, window, cx| this.show_card(what, window, cx))
                .ok();
        }));
    }

    /// The pointer pressed: what it's on is picked, and its card goes.
    pub(super) fn press_hovered(&mut self, cx: &mut Context<Self>) {
        self.sidebar_hover.wait = None;
        card::hide(cx);
    }

    /// Shows the card of `what`, if the pointer's still on it and it was drawn.
    fn show_card(&mut self, what: Hovered, window: &mut Window, cx: &mut Context<Self>) {
        if self.sidebar_hover.on != Some(what) {
            return;
        }
        let Some(anchor) = self.sidebar_hover.drawn.borrow().get(&what).copied() else {
            return;
        };
        let card = match what {
            Hovered::Space(id) => {
                let settings = settings::get(cx);
                let Some(space) = settings.spaces.iter().find(|space| space.id == id) else {
                    return;
                };
                Card::Space(space.name.clone().into())
            }
            Hovered::Pin(key) => {
                let Some(card) = self.page_card(key) else {
                    return;
                };
                Card::Page(card)
            }
        };
        card::show(card, anchor, window, cx);
    }

    /// What the card of the pinned page known by `key` says of it: its title and its host.
    fn page_card(&self, key: u64) -> Option<PageCard> {
        let page = self.workspace.pinned_of(key)?.tab.first();
        let address = page.address().unwrap_or_default();
        Some(PageCard {
            title: page.title.clone(),
            host: super::address::host_of(address).into(),
            icon: page.favicon(),
        })
    }
}
