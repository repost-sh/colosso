//! The Files panel's state: the checkout's files as rows, opened folder by folder and kept
//! in step with the disk through the checkout's project; the row picked; and the files
//! viewed beside the tree, the one shown revealed in it.

use std::rc::Rc;
use std::sync::Arc;

use colosso_project::{Feed, Kind, Project, Reveal, Rows, Update};
use futures::StreamExt as _;
use gpui_kit::{
    App, Context, Entity, FocusHandle, ScrollStrategy, Subscription, Task,
    UniformListScrollHandle, Window,
};

use super::Place;
use super::tabs::{Hand, Viewed};
use super::render::ROW;
use crate::changes::Marks;
use crate::checkout::Checkout;
use crate::list::{page, stepped};
use crate::settings;
use crate::right_panel::split::{self, Split};

pub struct FilesPanel {
    pub(super) checkout: Entity<Checkout>,
    pub(super) rows: Rows,
    /// Whether the first scan is done, so no rows means an empty checkout.
    pub(super) scanned: bool,
    /// Why writes aren't followed, if they aren't.
    pub(super) unwatched: Option<String>,
    /// The marks drawn, so a read that changed none draws nothing again.
    marks: Arc<Marks>,
    /// The row picked, by its path, so it stays picked as rows come and go.
    pub(super) selected: Option<Arc<str>>,
    /// The file shown beside the tree, as the tree names it; `None` for one outside the
    /// checkout.
    pub(super) active: Option<Arc<str>>,
    /// A path to show once the listings of the folders above it come.
    revealing: Option<Arc<str>>,
    /// The files open beside the tree, in their tabs' order.
    pub(super) tabs: Vec<Viewed>,
    /// The tab shown.
    pub(super) shown: usize,
    /// The tab in hand, dragged along the strip.
    pub(super) hand: Rc<Hand>,
    /// The tree, and the file shown beside it.
    pub(super) split: Split,
    pub(super) focus: FocusHandle,
    pub(super) scroll: UniformListScrollHandle,
    _checkout: Subscription,
    _following: Option<Task<()>>,
}

impl FilesPanel {
    pub fn new(checkout: Entity<Checkout>, cx: &mut Context<Self>) -> Self {
        let _checkout = cx.observe(&checkout, |this, checkout, cx| {
            this.follow(cx);
            // Until the checkout's open, what it says is what's shown; after, only new marks
            // change how the rows look.
            let marks = checkout.read(cx).marks();
            if this._following.is_none() || !Arc::ptr_eq(marks, &this.marks) {
                this.marks = marks.clone();
                cx.notify();
            }
        });
        let marks = checkout.read(cx).marks().clone();
        let mut panel = FilesPanel {
            checkout,
            rows: Rows::default(),
            scanned: false,
            unwatched: None,
            marks,
            selected: None,
            active: None,
            revealing: None,
            tabs: Vec::new(),
            shown: 0,
            hand: Rc::default(),
            split: Split::new(&split::FILES),
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            _checkout,
            _following: None,
        };
        panel.follow(cx);
        panel
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// The checkout's project, once git's open on it.
    pub(super) fn project(&self, cx: &App) -> Option<Arc<Project>> {
        self.checkout.read(cx).project().cloned()
    }

    /// Starts taking the listings of the checkout's project, once it's open.
    fn follow(&mut self, cx: &mut Context<Self>) {
        if self._following.is_some() {
            return;
        }
        let Some(project) = self.project(cx) else {
            return;
        };
        let mut updates = project.subscribe(Feed::Everything);
        self._following = Some(cx.spawn(async move |this, cx| {
            while let Some(update) = updates.next().await {
                if this.update(cx, |this, cx| this.take(update, cx)).is_err() {
                    return;
                }
            }
        }));
    }

    /// Shows what an update brought: only the open folders it lists anew change, and only
    /// a change is drawn.
    pub(super) fn take(&mut self, update: Update, cx: &mut Context<Self>) {
        let Some(project) = self.project(cx) else {
            return;
        };
        let mut changed = if self.rows.is_empty() {
            self.rows.reset(&*project);
            !self.rows.is_empty()
        } else {
            self.rows.apply(&update.listings, &*project)
        };
        if update.scanned.is_some() && !self.scanned {
            self.scanned = true;
            changed = true;
        }
        if update.unwatched.is_some() && update.unwatched != self.unwatched {
            self.unwatched = update.unwatched;
            changed = true;
        }
        if let Some(path) = self.revealing.clone() {
            self.reveal(&path, cx);
        }
        if changed {
            cx.notify();
        }
    }

    /// Whether the panel has the card to itself.
    pub fn is_wide(&self) -> bool {
        self.split.is_wide()
    }

    /// Says whether the panel has the card to itself.
    pub fn set_wide(&mut self, wide: bool, cx: &mut Context<Self>) {
        if self.split.set_wide(wide) {
            cx.notify();
        }
    }

    /// Picks the file shown, as the tree names it, and reveals it: its folders opened and
    /// it scrolled to.
    pub(super) fn set_active(&mut self, active: Option<Arc<str>>, cx: &mut Context<Self>) {
        if self.active == active {
            return;
        }
        self.active.clone_from(&active);
        match active {
            // Picked already, as a file previewed from the tree is: it's where it was left.
            Some(path) if self.selected.as_ref() != Some(&path) => self.reveal(&path, cx),
            _ => self.revealing = None,
        }
        cx.notify();
    }

    /// Opens the folders above `path`, then picks it and scrolls to it; if a folder on the
    /// way isn't read yet, once it is; and not at all if it's not there.
    fn reveal(&mut self, path: &Arc<str>, cx: &mut Context<Self>) {
        let Some(project) = self.project(cx) else {
            self.revealing = Some(path.clone());
            return;
        };
        match self.rows.reveal(path, &*project) {
            Reveal::Shown(index) => {
                self.revealing = None;
                self.selected = Some(path.clone());
                self.scroll.scroll_to_item(index, ScrollStrategy::Center);
            }
            Reveal::Pending => self.revealing = Some(path.clone()),
            Reveal::Missing => self.revealing = None,
        }
        cx.notify();
    }

    /// The picked row's place, if it's shown.
    pub(super) fn cursor(&self) -> Option<usize> {
        (self.selected.as_deref()).and_then(|path| self.rows.position(path))
    }

    pub(super) fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.get(index) {
            self.selected = Some(row.path.clone());
            self.scroll.scroll_to_item(index, ScrollStrategy::Nearest);
            cx.notify();
        }
    }

    /// Moves the pick `by` rows, stopping at either end; from nothing, to the first or the
    /// last row.
    pub(super) fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        if let Some(index) = stepped(self.cursor(), by, self.rows.len()) {
            self.select(index, cx);
        }
    }

    /// Moves the pick a page of rows down, or up.
    pub(super) fn page(&mut self, direction: isize, cx: &mut Context<Self>) {
        self.step(direction * page(&self.scroll, ROW), cx);
    }

    /// Moves the pick to the next file down, or up, passing folders over.
    pub(super) fn step_file(&mut self, by: isize, cx: &mut Context<Self>) {
        let len = self.rows.len() as isize;
        let mut index = self
            .cursor()
            .map_or(if by > 0 { -1 } else { len }, |at| at as isize);
        loop {
            index += by;
            let Some(row) = usize::try_from(index).ok().and_then(|at| self.rows.get(at)) else {
                return;
            };
            if !row.entry.is_folder() {
                return self.select(index as usize, cx);
            }
        }
    }

    /// Left: closes the picked folder, or picks the folder the picked row is in.
    pub(super) fn close_or_parent(&mut self, cx: &mut Context<Self>) {
        let Some(at) = self.cursor() else {
            return;
        };
        let row = &self.rows.rows()[at];
        if row.expanded {
            let path = row.path.clone();
            self.rows.collapse(&path);
            cx.notify();
        } else if let Some(parent) = self.rows.parent(at) {
            self.select(parent, cx);
        }
    }

    /// Right: opens the picked folder, or picks the first row in it if it's open.
    pub(super) fn open_or_child(&mut self, cx: &mut Context<Self>) {
        let Some(at) = self.cursor() else {
            return;
        };
        let row = &self.rows.rows()[at];
        if row.entry.kind != Kind::Dir {
            return;
        }
        if row.expanded {
            let depth = row.depth;
            if self.rows.get(at + 1).is_some_and(|next| next.depth > depth) {
                self.select(at + 1, cx);
            }
        } else {
            self.toggle(at, cx);
        }
    }

    /// Opens or closes the folder at `index`.
    pub(super) fn toggle(&mut self, index: usize, cx: &mut Context<Self>) {
        let (Some(row), Some(project)) = (self.rows.get(index), self.project(cx)) else {
            return;
        };
        let path = row.path.clone();
        if row.expanded {
            self.rows.collapse(&path);
        } else {
            self.rows.expand(&path, &*project);
        }
        cx.notify();
    }

    /// Enter, or a click: shows a file beside the tree, in a tab of its own if `pinned` and
    /// else as a preview, or opens or closes a folder.
    pub(super) fn activate(
        &mut self,
        index: usize,
        pinned: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self.rows.get(index) else {
            return;
        };
        match row.entry.kind {
            Kind::Dir => self.toggle(index, cx),
            Kind::File | Kind::Symlink { to_dir: false } => {
                let place = Place::file(&*row.path);
                if pinned {
                    self.view(place, window, cx);
                } else {
                    self.preview(place, window, cx);
                }
            }
            // Nothing is read through a link to a folder.
            Kind::Symlink { to_dir: true } => {}
        }
    }

    /// Previews the picked file, as the arrow keys do once they've moved the pick; a folder
    /// stays as it is.
    pub(super) fn preview_picked(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = (self.cursor()).and_then(|at| self.rows.get(at));
        if let Some(row) = picked.filter(|row| !row.entry.is_folder())
            && settings::get(cx).preview_files
        {
            let place = Place::file(&*row.path);
            self.preview(place, window, cx);
        }
    }
}
