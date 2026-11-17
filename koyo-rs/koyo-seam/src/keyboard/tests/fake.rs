//! `AppKit` as the keyboard sees it, kept in memory: each window's first responder, the window
//! each page's views are in, whether text is being composed, and what was asked of it.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::keyboard::hooks;
use crate::keyboard::reconcile::{AppKit, Responder};
use crate::keyboard::state::{PaneId, WindowKey};

#[derive(Default)]
pub struct Fake {
    first: RefCell<HashMap<WindowKey, Responder>>,
    pages: RefCell<HashMap<PaneId, WindowKey>>,
    pub marked: Cell<bool>,
    /// Every first responder asked for, by window.
    pub asked: RefCell<Vec<(WindowKey, Responder)>>,
    /// Every window made key.
    pub keyed: RefCell<Vec<WindowKey>>,
    /// Whether a change is told to the keyboard, as the window tells it of every change.
    pub tells: Cell<bool>,
    /// Whether the windows aren't key.
    pub not_key: Cell<bool>,
}

impl Fake {
    /// The page in `pane` has its views in `window`, or in none.
    pub fn page_in(&self, pane: PaneId, window: Option<WindowKey>) {
        let mut pages = self.pages.borrow_mut();
        match window {
            Some(window) => pages.insert(pane, window),
            None => pages.remove(&pane),
        };
    }

    /// `window`'s first responder is `to`, as a view or `AppKit` made it, telling the keyboard
    /// as the window does.
    pub fn moved(&self, window: WindowKey, to: Responder) {
        let before = self.first.borrow_mut().insert(window, to).unwrap_or(Responder::Gpui);
        if self.tells.get() && before != to {
            hooks::heard(window, before, to);
        }
    }
}

impl AppKit for Fake {
    fn first_responder(&self, window: WindowKey) -> Responder {
        self.first.borrow().get(&window).copied().unwrap_or(Responder::Gpui)
    }

    fn make_first_responder(&self, window: WindowKey, to: Responder) -> bool {
        self.asked.borrow_mut().push((window, to));
        let made = match to {
            Responder::Page(pane) => self.pages.borrow().get(&pane) == Some(&window),
            Responder::Other => false,
            Responder::Gpui | Responder::Window => true,
        };
        if made {
            self.moved(window, to);
        }
        made
    }

    fn page_window(&self, pane: PaneId) -> Option<WindowKey> {
        self.pages.borrow().get(&pane).copied()
    }

    fn has_marked_text(&self, _: WindowKey) -> bool {
        self.marked.get()
    }

    fn make_key(&self, window: WindowKey) {
        self.keyed.borrow_mut().push(window);
    }

    fn is_key(&self, _: WindowKey) -> bool {
        !self.not_key.get()
    }
}

/// What waits for the main thread's next turn.
type Tasks = RefCell<Vec<Box<dyn FnOnce()>>>;

/// Tasks for the main thread's next turn, run when a test says so.
#[derive(Clone, Default)]
pub struct Turns(Rc<Tasks>);

impl Turns {
    /// Runs what waited for the next turn; how many there were.
    pub fn next(&self) -> usize {
        let tasks = std::mem::take(&mut *self.0.borrow_mut());
        let count = tasks.len();
        for task in tasks {
            task();
        }
        count
    }
}

/// A fake `AppKit` the keyboard moves first responders in, as the app's real one, from now on
/// in this test's thread, and the turns it comes back on.
pub fn installed() -> (Rc<Fake>, Turns) {
    let fake = Rc::new(Fake::default());
    fake.tells.set(true);
    let turns = Turns::default();
    let queue = turns.clone();
    hooks::install(fake.clone(), Box::new(move |_, task| queue.0.borrow_mut().push(task)));
    hooks::click_by_the_press(false);
    (fake, turns)
}
