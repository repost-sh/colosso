//! A tab's panes: one, or two side by side or one over the other, each side a pane or a
//! split again. A tab dropped on a side of a pane splits it; a pane taken out leaves its
//! split to the other side.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use gpui_kit::{App, Entity, Image, SharedString};
use koyo_agent::protocol::ids::ThreadId;
use serde::{Deserialize, Serialize};

use super::PaneKind;
use crate::browser::BrowserView;
use crate::changes::ChangesTab;
use crate::files::{FileEditor, FilePane};

/// Ids for panes, never reused while the app runs.
static NEXT_PANE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
pub struct Pane {
    /// Which pane this is, wherever it moves.
    pub id: u64,
    pub kind: PaneKind,
    pub title: SharedString,
    /// A browser pane's page, or what it opens once its tab is shown (`view`, `set_view`).
    pub(super) page: Option<Page>,
    /// The file an editor pane shows, once the shell has opened one in it.
    pub file: Option<Entity<FilePane>>,
    /// The checkout's diffs, when it's an editor pane showing them rather than a file.
    pub changes: Option<Entity<ChangesTab>>,
    /// What an editor pane brought back with the window shows once its tab is (`unopened.rs`).
    /// Boxed, as few panes have one.
    pub unopened: Option<Box<super::Unopened>>,
    /// The workspace's turn when its tab was last shown, later turns higher: ⌃⇥ lists the
    /// latest first.
    pub shown_at: u64,
    /// When the person was last in it, later higher, and 0 until they have been
    /// (`Workspace::enter_pane`): of a tab's panes, the one they're in (`Tab::current`).
    pub entered_at: u64,
    /// A terminal pane's terminal, by the number `crate::terminals` knows it by.
    pub terminal: Option<u64>,
    /// The thread a chat pane shows: a new chat has none until its first send.
    pub chat: Option<ThreadId>,
}

impl Pane {
    /// An empty pane of `kind`, titled as a new tab of it.
    pub fn new(kind: PaneKind) -> Self {
        Pane {
            id: NEXT_PANE.fetch_add(1, Ordering::Relaxed),
            kind,
            title: kind.new_tab_title().into(),
            page: None,
            file: None,
            changes: None,
            unopened: None,
            shown_at: 0,
            entered_at: 0,
            terminal: None,
            chat: None,
        }
    }

    /// A browser pane showing `view`, titled as the view calls itself.
    pub fn browser(view: Entity<BrowserView>, cx: &App) -> Self {
        Pane {
            title: view.read(cx).title().into(),
            page: Some(Page::Made(view)),
            ..Pane::new(PaneKind::Browser)
        }
    }

    /// A browser pane with no view yet, titled `title`, that opens `unloaded`'s address once
    /// its tab is shown.
    pub fn unloaded(unloaded: Unloaded, title: SharedString) -> Self {
        Pane {
            title,
            page: Some(Page::Unloaded(Box::new(unloaded))),
            ..Pane::new(PaneKind::Browser)
        }
    }

    /// The page a browser pane shows, once it's made.
    pub fn view(&self) -> Option<&Entity<BrowserView>> {
        match &self.page {
            Some(Page::Made(view)) => Some(view),
            Some(Page::Unloaded(_)) | None => None,
        }
    }

    /// The pane shows `view` from now on, and no longer holds an address to open.
    pub fn set_view(&mut self, view: Entity<BrowserView>) {
        self.page = Some(Page::Made(view));
    }

    /// The address a browser pane with no view opens once its tab is shown.
    pub fn unloaded_address(&self) -> Option<&str> {
        match &self.page {
            Some(Page::Unloaded(unloaded)) => Some(&unloaded.address),
            Some(Page::Made(_)) | None => None,
        }
    }

    /// The address a browser pane shows, or opens once its tab is shown; `None` for another
    /// kind.
    pub fn address(&self, cx: &App) -> Option<String> {
        match &self.page {
            Some(Page::Made(view)) => Some(view.read(cx).address()),
            Some(Page::Unloaded(unloaded)) => Some(unloaded.address.clone()),
            None => None,
        }
    }

    /// Whether it's a browser pane showing another page's `DevTools`.
    pub fn is_inspector(&self, cx: &App) -> bool {
        self.view().is_some_and(|view| view.read(cx).is_inspector())
    }

    /// The site's icon a browser pane's tab shows: its page's, or the one kept for its address
    /// while it has no view.
    pub fn favicon(&self, cx: &App) -> Option<Arc<Image>> {
        match &self.page {
            Some(Page::Made(view)) => view.read(cx).favicon(),
            Some(Page::Unloaded(unloaded)) => unloaded.favicon.clone(),
            None => None,
        }
    }

    /// An editor pane showing `file`, titled with its name.
    pub fn file(file: Entity<FilePane>, cx: &App) -> Self {
        Pane {
            title: file.read(cx).name().into(),
            file: Some(file),
            ..Pane::new(PaneKind::Editor)
        }
    }

    /// An editor pane showing the checkout's diffs, titled for them.
    pub fn changes(changes: Entity<ChangesTab>) -> Self {
        Pane {
            title: "Changes".into(),
            changes: Some(changes),
            ..Pane::new(PaneKind::Editor)
        }
    }
}

/// A browser pane's page: made, or what it opens once its tab is shown, never both.
#[derive(Clone, Debug)]
pub(super) enum Page {
    Made(Entity<BrowserView>),
    /// A tab brought back at launch, its page made when the tab is first shown
    /// (`docs/browser.md` 5). Boxed, as few panes have one.
    Unloaded(Box<Unloaded>),
}

/// What a browser pane with no view yet opens once its tab is shown.
#[derive(Clone, Debug)]
pub struct Unloaded {
    pub address: String,
    /// The icon kept for the address's site, shown on the tab meanwhile.
    pub favicon: Option<Arc<Image>>,
}

/// How a split lays its two sides out. The window's file keeps it by these names
/// (`shell/kept/shape.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SplitAxis {
    /// Side by side, the first on the left.
    Row,
    /// One over the other, the first on top.
    Column,
}

/// A side of a pane, where something dropped on it goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

impl Edge {
    pub const ALL: [Edge; 4] = [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom];

    fn axis(self) -> SplitAxis {
        match self {
            Edge::Left | Edge::Right => SplitAxis::Row,
            Edge::Top | Edge::Bottom => SplitAxis::Column,
        }
    }

    /// Whether what's dropped here comes first in the split.
    fn leads(self) -> bool {
        matches!(self, Edge::Left | Edge::Top)
    }
}

#[derive(Clone, Debug)]
pub struct Split {
    pub axis: SplitAxis,
    pub first: Tab,
    pub second: Tab,
    /// The first side's share of the room, between 0 and 1.
    pub ratio: f32,
}

/// The least share of a split's room either side keeps.
pub const SPLIT_MIN: f32 = 0.1;

#[derive(Clone, Debug)]
pub enum Tab {
    Single(Pane),
    Split(Box<Split>),
}

impl Tab {
    /// Two tabs side by side or one over the other, sharing the room evenly.
    pub fn split(axis: SplitAxis, first: Tab, second: Tab) -> Tab {
        Tab::Split(Box::new(Split {
            axis,
            first,
            second,
            ratio: 0.5,
        }))
    }

    /// Two tabs side by side, every column of either sharing the room evenly.
    pub fn beside(first: Tab, second: Tab) -> Tab {
        let (columns, last) = Tab::split(SplitAxis::Row, first, second).into_columns();
        // From the right: each column takes its share of the room it and those after it have.
        (columns.into_iter().rev().enumerate()).fold(last, |row, (after, column)| {
            Tab::Split(Box::new(Split {
                axis: SplitAxis::Row,
                first: column,
                second: row,
                ratio: 1. / (after + 2) as f32,
            }))
        })
    }

    /// The tab's columns but its last, left to right, and its last: a row split's sides,
    /// each a row split's again, and any other tab itself.
    fn into_columns(self) -> (Vec<Tab>, Tab) {
        match self {
            Tab::Split(split) if split.axis == SplitAxis::Row => {
                let Split { first, second, .. } = *split;
                let (mut columns, first) = first.into_columns();
                let (rest, last) = second.into_columns();
                columns.push(first);
                columns.extend(rest);
                (columns, last)
            }
            tab => (Vec::new(), tab),
        }
    }

    /// The first pane, left or top.
    pub fn first(&self) -> &Pane {
        match self {
            Tab::Single(pane) => pane,
            Tab::Split(split) => split.first.first(),
        }
    }

    /// The pane the person is in: the one they were in last, wherever the keyboard is now, and
    /// the first until they've been in any. The address bar, the page's keys and the keyboard
    /// coming back to the tab go by it.
    pub fn current(&self) -> &Pane {
        // The first of those entered as late: `max_by_key` alone would answer the last.
        let panes = self.panes().into_iter().rev();
        (panes.max_by_key(|pane| pane.entered_at)).unwrap_or_else(|| self.first())
    }

    /// The panes, left to right and top to bottom.
    pub fn panes(&self) -> Vec<&Pane> {
        match self {
            Tab::Single(pane) => vec![pane],
            Tab::Split(split) => {
                let mut panes = split.first.panes();
                panes.extend(split.second.panes());
                panes
            }
        }
    }

    pub fn panes_mut(&mut self) -> Vec<&mut Pane> {
        match self {
            Tab::Single(pane) => vec![pane],
            Tab::Split(split) => {
                let Split { first, second, .. } = &mut **split;
                let mut panes = first.panes_mut();
                panes.extend(second.panes_mut());
                panes
            }
        }
    }

    /// The latest turn any of its panes was shown at.
    pub fn shown_at(&self) -> u64 {
        (self.panes().iter())
            .map(|pane| pane.shown_at)
            .max()
            .unwrap_or(0)
    }

    /// The files its panes show, left to right and top to bottom.
    pub fn files(&self) -> Vec<&Entity<FilePane>> {
        self.panes()
            .into_iter()
            .filter_map(|pane| pane.file.as_ref())
            .collect()
    }

    /// The editors of its files with unsaved edits.
    pub fn unsaved(&self, cx: &App) -> Vec<Entity<FileEditor>> {
        (self.files().into_iter())
            .filter_map(|file| file.read(cx).unsaved().cloned())
            .collect()
    }

    pub fn has_pane(&self, id: u64) -> bool {
        self.panes().iter().any(|pane| pane.id == id)
    }

    /// Puts `incoming` at `edge` of the pane `target`, the two sharing its room. False, and
    /// nothing changes, when the tab has no such pane.
    pub fn place(&mut self, target: u64, edge: Edge, incoming: Tab) -> bool {
        match self {
            Tab::Single(pane) if pane.id == target => {
                let here = Tab::Single(pane.clone());
                let (first, second) = if edge.leads() {
                    (incoming, here)
                } else {
                    (here, incoming)
                };
                *self = Tab::split(edge.axis(), first, second);
                true
            }
            Tab::Single(_) => false,
            Tab::Split(split) => {
                if split.first.has_pane(target) {
                    split.first.place(target, edge, incoming)
                } else {
                    split.second.place(target, edge, incoming)
                }
            }
        }
    }

    /// Takes the pane `id` out, its split's other side taking the split's room. `None`
    /// when the tab has no such pane, or it's the tab's only one.
    pub fn take(&mut self, id: u64) -> Option<Pane> {
        let Tab::Split(split) = self else {
            return None;
        };
        let (taken, other) = match (&split.first, &split.second) {
            (Tab::Single(pane), other) if pane.id == id => (pane.clone(), other.clone()),
            (other, Tab::Single(pane)) if pane.id == id => (pane.clone(), other.clone()),
            _ => {
                return split.first.take(id).or_else(|| split.second.take(id));
            }
        };
        *self = other;
        Some(taken)
    }

    /// The split at `path` from the top, each step into the second side when true.
    pub fn split_at_mut(&mut self, path: &[bool]) -> Option<&mut Split> {
        let Tab::Split(split) = self else {
            return None;
        };
        match path.split_first() {
            None => Some(split),
            Some((&second, rest)) if second => split.second.split_at_mut(rest),
            Some((_, rest)) => split.first.split_at_mut(rest),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Edge, Pane, SplitAxis, Tab};
    use crate::model::PaneKind;

    fn single() -> (Tab, u64) {
        let pane = Pane::new(PaneKind::Chat);
        let id = pane.id;
        (Tab::Single(pane), id)
    }

    fn ids(tab: &Tab) -> Vec<u64> {
        tab.panes().iter().map(|pane| pane.id).collect()
    }

    #[test]
    fn a_tab_dropped_on_a_side_of_a_pane_takes_that_side() {
        let (mut tab, a) = single();
        let (left, b) = single();
        assert!(tab.place(a, Edge::Left, left));
        assert_eq!(ids(&tab), [b, a]);
        let Tab::Split(split) = &tab else {
            panic!("a split");
        };
        assert_eq!(split.axis, SplitAxis::Row);
        let (below, c) = single();
        assert!(tab.place(a, Edge::Bottom, below));
        assert_eq!(ids(&tab), [b, a, c]);
        assert!(
            tab.split_at_mut(&[true])
                .is_some_and(|split| split.axis == SplitAxis::Column)
        );
    }

    #[test]
    fn a_row_of_tabs_shares_the_room_evenly_taking_in_rows_column_by_column() {
        let (mut pair, a) = single();
        let (right, b) = single();
        pair.place(a, Edge::Right, right);
        let (mut stack, c) = single();
        let (below, d) = single();
        stack.place(c, Edge::Bottom, below);
        let row = Tab::beside(pair, stack);
        assert_eq!(ids(&row), [a, b, c, d]);
        let Tab::Split(split) = &row else {
            panic!("a split");
        };
        // The row gives up its two columns; the column split is one.
        assert!((split.ratio - 1. / 3.).abs() < f32::EPSILON);
        let Tab::Split(rest) = &split.second else {
            panic!("a split");
        };
        assert!((rest.ratio - 0.5).abs() < f32::EPSILON);
        assert_eq!(rest.axis, SplitAxis::Row);
        let whole = matches!(&rest.second, Tab::Split(split) if split.axis == SplitAxis::Column);
        assert!(whole, "the column split stays whole");
    }

    #[test]
    fn a_pane_taken_out_leaves_its_room_to_the_other_side() {
        let (mut tab, a) = single();
        let (right, b) = single();
        let (below, c) = single();
        tab.place(a, Edge::Right, right);
        tab.place(b, Edge::Bottom, below);
        assert_eq!(tab.take(b).map(|pane| pane.id), Some(b));
        assert_eq!(ids(&tab), [a, c]);
        assert_eq!(tab.take(a).map(|pane| pane.id), Some(a));
        assert!(matches!(&tab, Tab::Single(pane) if pane.id == c));
        assert!(tab.take(c).is_none(), "a tab keeps its last pane");
        let (other, d) = single();
        assert!(!tab.place(d + 100, Edge::Top, other), "no such pane");
    }
}
