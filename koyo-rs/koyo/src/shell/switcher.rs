//! ⌃⇥ (`docs/tab-switcher.md`): the tabs looked at last, the latest first, wherever they are,
//! then the open ones not looked at yet; five at most, the one shown among them, as cards on a
//! panel of the app switcher's glass.
//!
//! The panel is a floating window that never has the keyboard: every key goes on being heard
//! where the person was, in the window behind, so none is missed while one window takes the
//! keys from another. ⌃⇥ opens it unseen, the tab before picked, and it shows once ⌃ has been
//! held a moment; each ⌃⇥ more moves the pick on and ⌃⇧⇥ back, round the ends, and shows it at
//! once. Letting go of ⌃ shows the tab picked, whether the panel shows yet or not: a ⌃⇥ tapped
//! goes to the tab before with no panel. A click on a card shows its tab, the pointer moved
//! over one picks it, and Escape, like a press in the window behind, leaves things as they
//! were.

use std::time::Duration;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyWindowHandle, App, Bounds, Context, Entity, IntoElement, ParentElement as _, Pixels, Render,
    Styled as _, Subscription, Task, Window, WindowHandle, div, px, size,
};

use super::Shell;
use crate::popup::{self, Kind, Opening};
use crate::settings::Layout;
use crate::theme;
use card::{CARD_GAP, CARD_HEIGHT, CARD_RADIUS, CARD_WIDTH, Card};

mod card;
mod show;
#[cfg(all(target_os = "macos", feature = "visual-check"))]
pub mod visual;

/// How many tabs it lists, the one shown among them, as Arc's does.
const MOST: usize = 5;
/// How long ⌃ is held after ⌃⇥ before the panel shows: let go sooner, the tab picked shows
/// without it, as with Arc's and macOS's.
const HOLD: Duration = Duration::from_millis(200);
/// The glass round the cards, and the panel's corners, concentric with theirs: at the panel's
/// full size, as the cards' sizes are.
const PADDING: f32 = 21.;
const RADIUS: f32 = CARD_RADIUS + PADDING;
/// The least room left between the panel and the sides of its screen.
const MARGIN: f32 = 24.;

/// ⌃⇥ (`by` 1) or ⌃⇧⇥ (`by` -1) in `window`: moves the pick `by` cards if the switcher is up,
/// and else opens it over `window` on the tabs to switch among, the pick `by` from the tab
/// shown. With one tab open there's nothing to switch to.
pub(super) fn switch(
    shell: &Shell,
    entity: Entity<Shell>,
    by: isize,
    window: &mut Window,
    cx: &mut App,
) {
    if let Some(panel) = panel(cx) {
        (panel.update(cx, |switcher, window, cx| switcher.step(by, window, cx))).ok();
        return;
    }
    let cards = cards(shell, cx);
    if cards.len() < 2 {
        return;
    }
    // The tab shown has its picture taken anew, by the frame this asks for, for its card.
    if let Some(shown) = shell.shown_tab_key() {
        super::pictures::retake(shown, cx);
        window.refresh();
    }
    let picked = stepped(0, by, cards.len());
    open(entity, cards, picked, window, cx);
}

/// ⌃ let go: the tab picked shows, and the switcher goes.
pub(super) fn released(cx: &mut App) {
    if let Some(panel) = panel(cx) {
        (panel.update(cx, |switcher, _, cx| switcher.pick(cx))).ok();
    }
}

/// The switcher's panel, if it's up, seen or not.
fn panel(cx: &App) -> Option<WindowHandle<Switcher>> {
    popup::window(Kind::Switcher, cx)?.downcast()
}

/// Where the panel for `count` cards goes, and at how much of its full size: across the middle
/// of the window at `behind`, shrunk if `screen`, what's free of the screen that window is on,
/// is too small for it, and moved to lie on that screen. A window that isn't on its screen at
/// all, as a check's off screen isn't, has the panel over it all the same.
fn placed(
    count: usize,
    behind: Bounds<Pixels>,
    screen: Option<Bounds<Pixels>>,
) -> (Bounds<Pixels>, f32) {
    let count = count as f32;
    let full = size(
        px(2. * PADDING + count * CARD_WIDTH + (count - 1.) * CARD_GAP),
        px(2. * PADDING + CARD_HEIGHT),
    );
    let room =
        (screen.filter(|screen| screen.intersects(&behind))).map(|screen| screen.inset(px(MARGIN)));
    let scale = room.map_or(1., |room| {
        (room.size.width / full.width)
            .min(room.size.height / full.height)
            .min(1.)
    });
    let mut bounds = Bounds::centered_at(behind.center(), full.map(|side| side * scale));
    if let Some(room) = room {
        let Bounds { origin, size } = &mut bounds;
        origin.x = origin.x.min(room.right() - size.width).max(room.left());
        origin.y = origin.y.min(room.bottom() - size.height).max(room.top());
    }
    (bounds, scale)
}

/// Opens the switcher's panel, unseen, over `window`, with `cards` and the one at `picked`
/// picked.
fn open(shell: Entity<Shell>, cards: Vec<Card>, picked: usize, window: &mut Window, cx: &mut App) {
    let screen = window.display(cx).map(|display| display.visible_bounds());
    let (bounds, scale) = placed(cards.len(), window.bounds(), screen);
    let opening = Opening {
        kind: Kind::Switcher,
        bounds,
        kept: false,
        source: None,
    };
    let behind = window.window_handle();
    popup::open(opening, window, cx, |window, cx: &mut Context<Switcher>| {
        let reveal = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(HOLD).await;
            this.update_in(cx, Switcher::show).ok();
        });
        // Heard before the pane the person is in hears it, and kept from it.
        let escape = cx.intercept_keystrokes(|event, _, cx| {
            if event.keystroke.key == "escape" {
                popup::close(Kind::Switcher, cx);
                cx.stop_propagation();
            }
        });
        #[cfg(target_os = "macos")]
        let glass = crate::native::window_glass::fill(window, RADIUS * scale, cx);
        #[cfg(not(target_os = "macos"))]
        let glass = false;
        Switcher {
            shell,
            behind,
            cards,
            picked,
            scale,
            shown: false,
            glass,
            _reveal: reveal,
            _escape: escape,
        }
    });
}

/// The tabs to switch among as cards (`Workspace::latest`): those looked at, the latest first,
/// then the open ones not looked at yet; pinned and loose pages only in the Arc layout, which
/// lists them.
fn cards(shell: &Shell, cx: &mut App) -> Vec<Card> {
    let apart = Shell::layout(cx) == Layout::Arc;
    (shell.workspace.latest(MOST, apart).iter())
        .filter_map(|visited| Card::of(visited, &shell.workspace, cx))
        .collect()
}

/// Where a step of `by` from `at` lands among `len`, round the ends.
fn stepped(at: usize, by: isize, len: usize) -> usize {
    (at as isize + by).rem_euclid(len.max(1) as isize) as usize
}

/// The switcher while ⌃ holds it up: what its panel's window shows.
pub(super) struct Switcher {
    shell: Entity<Shell>,
    /// The window it floats over, where the keys are heard and the tab picked is shown.
    behind: AnyWindowHandle,
    cards: Vec<Card>,
    picked: usize,
    /// How much of its full size it's drawn at, to fit its screen.
    scale: f32,
    /// Whether its window shows yet.
    shown: bool,
    /// Whether its window is glass; one that isn't is tinted, as the app's other floating
    /// windows are.
    glass: bool,
    /// The wait for ⌃ to have been held, which shows it.
    _reveal: Task<()>,
    /// Escape, which closes it.
    _escape: Subscription,
}

impl Switcher {
    /// Moves the pick `by` cards, round the ends, and shows the panel: past the tab before, the
    /// person has to see where the pick is.
    fn step(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        self.select(stepped(self.picked, by, self.cards.len()), cx);
        self.show(window, cx);
    }

    fn select(&mut self, at: usize, cx: &mut Context<Self>) {
        if at != self.picked {
            self.picked = at;
            cx.notify();
        }
    }

    /// Shows the panel's `window`, if it doesn't show yet.
    fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.shown {
            self.shown = true;
            popup::show(window, cx);
        }
    }

    /// Closes the switcher and shows the tab picked in the window behind, once that window is
    /// free: ⌃ let go is heard in it, and a window can't be reached while it hears an event.
    fn pick(&self, cx: &mut App) {
        popup::close(Kind::Switcher, cx);
        let Some(key) = self.cards.get(self.picked).map(Card::key) else {
            return;
        };
        let (shell, behind) = (self.shell.clone(), self.behind);
        cx.defer(move |cx| {
            (behind.update(cx, |_, window, cx| {
                shell.update(cx, |shell, cx| shell.show_visited(key, window, cx));
            }))
            .ok();
        });
    }
}

impl Render for Switcher {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let cards: Vec<_> = (self.cards.iter().enumerate())
            .map(|(index, card)| card.element(index, index == self.picked, self.scale, p, cx))
            .collect();
        div()
            .size_full()
            .flex()
            .items_center()
            .p(px(PADDING * self.scale))
            .gap(px(CARD_GAP * self.scale))
            .text_color(p.text)
            .when(!self.glass, |panel| panel.bg(p.menu))
            .children(cards)
    }
}

/// The switcher, if it's up, as a test or the check reads it: its cards' titles, the pick, and
/// whether its panel shows.
#[cfg(any(test, feature = "visual-check"))]
fn state(cx: &App) -> Option<(Vec<String>, usize, bool)> {
    let switcher = popup::root::<Switcher>(Kind::Switcher, cx)?;
    let titles = (switcher.cards.iter())
        .map(|card| card.title.to_string())
        .collect();
    Some((titles, switcher.picked, switcher.shown))
}

#[cfg(test)]
mod tests;
