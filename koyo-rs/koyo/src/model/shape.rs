//! The checkouts, and the tabs and panes, as numbers that change when one comes or goes, so what's
//! made for them knows when some went without gathering them all into sets after every change.
//! Nothing that changes them has to say so: they're changed in too many places for that.

use std::hash::{DefaultHasher, Hash as _, Hasher as _};

use super::Workspace;

impl Workspace {
    /// The checkouts there are, by their folders, as a number.
    pub fn checkouts_shape(&self) -> u64 {
        let mut shape = DefaultHasher::new();
        for unit in self.projects.iter().flat_map(|project| &project.branches) {
            unit.worktree.path.as_os_str().hash(&mut shape);
        }
        shape.finish()
    }

    /// The tabs and panes there are, closed tabs among them, with the terminals and chat threads
    /// of the panes, as a number.
    pub fn tabs_shape(&self) -> u64 {
        let mut shape = DefaultHasher::new();
        let open = self.everywhere().map(|visited| (false, visited.tab));
        let closed = self.closed.iter().map(|closed| (true, &closed.tab));
        for (is_closed, tab) in open.chain(closed) {
            is_closed.hash(&mut shape);
            tab.each_pane(&mut |pane| (pane.id, pane.terminal(), pane.chat()).hash(&mut shape));
        }
        shape.finish()
    }
}

#[cfg(test)]
mod tests;
