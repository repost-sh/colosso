//! A tab's panes: one, or two side by side or one over the other, each side a pane or a
//! split again. A tab dropped on a side of a pane splits it; a pane taken out leaves its
//! split to the other side.

use std::sync::atomic::{AtomicU64, Ordering};

use gpui_kit::assets::IconName;
use gpui_kit::{App, Entity, SharedString};

use super::PaneKind;
use crate::browser::BrowserView;

/// Ids for panes, never reused while the app runs.
static NEXT_PANE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
pub struct Pane {
    /// Which pane this is, wherever it moves.
    pub id: u64,
    pub kind: PaneKind,
    pub title: SharedString,
    /// The page a browser pane shows, once the shell has made it.
    pub browser: Option<Entity<BrowserView>>,
}

impl Pane {
    /// An empty pane of `kind`, titled as a new tab of it.
    pub fn new(kind: PaneKind) -> Self {
        Pane {
            id: NEXT_PANE.fetch_add(1, Ordering::Relaxed),
            kind,
            title: kind.new_tab_title().into(),
            browser: None,
        }
    }

    /// A browser pane showing `view`, titled as the view calls itself.
    pub fn browser(view: Entity<BrowserView>, cx: &App) -> Self {
        Pane {
            title: view.read(cx).title().into(),
            browser: Some(view),
            ..Pane::new(PaneKind::Browser)
        }
    }
}

/// How a split lays its two sides out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

    pub fn has_pane(&self, id: u64) -> bool {
        self.panes().iter().any(|pane| pane.id == id)
    }

    pub fn title(&self) -> SharedString {
        match self {
            Tab::Single(pane) => pane.title.clone(),
            Tab::Split(split) => {
                format!("{} | {}", split.first.title(), split.second.title()).into()
            }
        }
    }

    pub fn icon(&self) -> IconName {
        match self {
            Tab::Single(pane) => pane.kind.icon(),
            Tab::Split(split) if split.axis == SplitAxis::Row => IconName::Columns2,
            Tab::Split(_) => IconName::Rows2,
        }
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
        assert_eq!(tab.icon(), gpui_kit::assets::IconName::Columns2);
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
