//! Callbacks CEF waits on while the app answers, kept under a number the app
//! echoes back with the pane it was for.
use colosso_browser_ipc::PaneId;
use std::collections::BTreeMap;

pub(crate) struct Pending<T> {
    next: u64,
    waiting: BTreeMap<u64, (PaneId, T)>,
}

impl<T> Pending<T> {
    pub(crate) const fn new() -> Self {
        Self {
            next: 0,
            waiting: BTreeMap::new(),
        }
    }

    /// Keeps `value` for `pane`; returns its number.
    pub(crate) fn add(&mut self, pane: PaneId, value: T) -> u64 {
        self.next += 1;
        self.waiting.insert(self.next, (pane, value));
        self.next
    }

    /// Takes the value numbered `id`, if it was kept for `pane`.
    pub(crate) fn remove(&mut self, pane: PaneId, id: u64) -> Option<T> {
        match self.waiting.get(&id) {
            Some((p, _)) if *p == pane => self.waiting.remove(&id).map(|(_, value)| value),
            _ => None,
        }
    }

    /// Takes every value `which` picks, with its number.
    pub(crate) fn take(&mut self, which: impl Fn(PaneId, &T) -> bool) -> Vec<(u64, T)> {
        self.waiting
            .extract_if(.., |_, (pane, value)| which(*pane, value))
            .map(|(id, (_, value))| (id, value))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_come_back_only_for_their_pane() {
        let mut pending = Pending::new();
        let a = pending.add(1, "a");
        let b = pending.add(2, "b");
        pending.add(2, "c");
        assert_eq!(pending.remove(2, a), None);
        assert_eq!(pending.remove(1, a), Some("a"));
        assert_eq!(pending.remove(1, a), None);
        assert_eq!(
            pending.take(|pane, _| pane == 2),
            vec![(b, "b"), (b + 1, "c")]
        );
        assert!(pending.take(|_, _| true).is_empty());
    }
}
