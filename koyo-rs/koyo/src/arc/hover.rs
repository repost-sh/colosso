//! What the pointer rests on in the Arc sidebar, and the card that says more of it
//! (`docs/arc-sidebar.md` 3, 7, 11): a project's after a moment on its tile, a pinned page's on
//! its own, a space's name after a shorter one on its dot, at once while another card's up, as
//! tooltips go from one to the next.
//! Tiles and dots note where they're drawn, which the card is placed by.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::{App, Bounds, Context, Pixels, Task, Window};

use super::card::{self, Card, PageCard, ProjectCard};
use super::{avatar, space_colour};
use crate::projects::display_path;
use crate::settings;
use crate::shell::Shell;

/// How long the pointer rests on a tile before its card shows, and on a space's dot before its
/// name does.
const TILE_WAIT: Duration = Duration::from_millis(400);
const SPACE_WAIT: Duration = Duration::from_millis(250);

/// What the card is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Hovered {
    /// The project at this place in the workspace, by its tile.
    Tile(usize),
    /// The space at this place in the list, by its dot.
    Space(usize),
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
        // Only a space's mark looks different pointed at; a tile lights by its own hover style.
        let redraw = matches!(what, Hovered::Space(_));
        if !hovered {
            if self.arc_hover.on == Some(what) {
                self.arc_hover.on = None;
                self.arc_hover.wait = None;
                card::hide(cx);
                if redraw {
                    cx.notify();
                }
            }
            return;
        }
        self.arc_hover.on = Some(what);
        if redraw {
            cx.notify();
        }
        if card::is_shown(cx) {
            self.show_card(what, window, cx);
            return;
        }
        let wait = match what {
            Hovered::Tile(_) | Hovered::Pin(_) => TILE_WAIT,
            Hovered::Space(_) => SPACE_WAIT,
        };
        self.arc_hover.wait = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(wait).await;
            this.update_in(cx, |this, window, cx| this.show_card(what, window, cx))
                .ok();
        }));
    }

    /// The pointer pressed: what it's on is picked, and its card goes.
    pub(super) fn press_hovered(&mut self, cx: &mut Context<Self>) {
        self.arc_hover.wait = None;
        card::hide(cx);
    }

    /// Shows the card of `what`, if the pointer's still on it and it was drawn.
    fn show_card(&mut self, what: Hovered, window: &mut Window, cx: &mut Context<Self>) {
        if self.arc_hover.on != Some(what) {
            return;
        }
        let Some(anchor) = self.arc_hover.drawn.borrow().get(&what).copied() else {
            return;
        };
        let card = match what {
            Hovered::Tile(index) => {
                let Some(card) = self.project_card(index, cx) else {
                    return;
                };
                Card::Project(card)
            }
            Hovered::Space(index) => {
                let Some(space) = settings::get(cx).spaces.get(index) else {
                    return;
                };
                Card::Space {
                    name: space.name.clone().into(),
                    colour: space_colour(index, cx),
                }
            }
            Hovered::Pin(key) => {
                let Some(card) = self.page_card(key, cx) else {
                    return;
                };
                Card::Page(card)
            }
        };
        card::show(card, anchor, window, cx);
    }

    /// What the card of the pinned page known by `key` says of it: its title and its host.
    fn page_card(&self, key: u64, cx: &App) -> Option<PageCard> {
        let page = self.workspace.pinned_of(key)?.tab.first();
        let address = page.address(cx).unwrap_or_default();
        Some(PageCard {
            title: page.title.clone(),
            host: super::top::host_of(&address).into(),
            icon: page.favicon(cx),
        })
    }

    /// What the card of the project at `index` says of it.
    fn project_card(&self, index: usize, cx: &mut Context<Self>) -> Option<ProjectCard> {
        let project = self.workspace.projects.get(index)?;
        let units = &project.branches;
        Some(ProjectCard {
            name: project.name.clone(),
            path: display_path(&project.path).into(),
            picture: avatar::picture(&project.path, cx),
            initials: project.initials(),
            worktrees: units.len(),
            tabs: units.iter().map(|unit| unit.tabs.len()).sum(),
            chats: (units.iter())
                .map(|unit| self.chats_working(&unit.worktree.path, cx))
                .sum(),
            terminals: (units.iter())
                .map(|unit| self.terminal_count(&unit.worktree.path))
                .sum(),
        })
    }
}
