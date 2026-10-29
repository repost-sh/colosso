//! The Files panel's state: the checkout's files as rows, opened folder by folder and kept
//! in step with the disk through the checkout's project; the row picked; and the file the
//! active editor tab shows, which is revealed as it changes.

use std::sync::Arc;

use colosso_project::{Feed, Kind, Project, Reveal, Rows, Update};
use futures::StreamExt as _;
use gpui_kit::{
    App, Context, Entity, EventEmitter, FocusHandle, ScrollStrategy, Subscription, Task,
    UniformListScrollHandle,
};

use super::render::ROW;
use crate::changes::Marks;
use crate::checkout::Checkout;
use crate::right_panel::page;

/// Asks for the file at the path, relative to the checkout, to be opened in a tab.
pub struct OpenFile(pub Arc<str>);

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
    /// The file the active editor tab shows, relative to the checkout.
    pub(super) active: Option<Arc<str>>,
    /// A path to show once the listings of the folders above it come.
    revealing: Option<Arc<str>>,
    pub(super) focus: FocusHandle,
    pub(super) scroll: UniformListScrollHandle,
    _checkout: Subscription,
    _following: Option<Task<()>>,
}

impl EventEmitter<OpenFile> for FilesPanel {}

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

    /// Follows the file the active editor tab shows, relative to the checkout: it's
    /// revealed and picked when it changes.
    pub fn set_active(&mut self, active: Option<Arc<str>>, cx: &mut Context<Self>) {
        if self.active == active {
            return;
        }
        self.active.clone_from(&active);
        match active {
            Some(path) => self.reveal(&path, cx),
            None => self.revealing = None,
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
        let Some(last) = self.rows.len().checked_sub(1) else {
            return;
        };
        let index = match self.cursor() {
            Some(at) => at.saturating_add_signed(by).min(last),
            None if by > 0 => 0,
            None => last,
        };
        self.select(index, cx);
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

    /// Enter, or a click: opens a file in a tab, or opens or closes a folder.
    pub(super) fn activate(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(index) else {
            return;
        };
        match row.entry.kind {
            Kind::Dir => self.toggle(index, cx),
            Kind::File | Kind::Symlink { to_dir: false } => cx.emit(OpenFile(row.path.clone())),
            // Nothing is read through a link to a folder.
            Kind::Symlink { to_dir: true } => {}
        }
    }
}
