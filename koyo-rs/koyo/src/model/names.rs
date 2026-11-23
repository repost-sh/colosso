//! What the sidebars call a worktree: its branch, or the commit it's detached at; and that in
//! words, as both layouts' rows show it.

use gpui_kit::SharedString;
use koyo_git::Head;

use super::{BranchUnit, short};

impl BranchUnit {
    /// What the sidebar calls it: its branch, or the commit it is detached at.
    pub fn name(&self) -> SharedString {
        match &self.worktree.head {
            Head::Branch(branch) => branch.clone().into(),
            Head::Detached(commit) => format!("detached at {}", short(commit)).into(),
        }
    }

    /// Its name in words, as both layouts' sidebars show it: its branch less `prefix`, the one
    /// the person's own branches carry, its dashes and underscores spaces and its first letter a
    /// capital, so `schulzfel/some-nasty-name` is "Some nasty name". A detached checkout keeps
    /// its name.
    pub fn title(&self, prefix: Option<&str>) -> SharedString {
        let Head::Branch(branch) = &self.worktree.head else {
            return self.name();
        };
        let own = prefix.and_then(|prefix| branch.strip_prefix(prefix)?.strip_prefix('/'));
        let mut words = own.unwrap_or(branch).replace(['-', '_'], " ");
        if let Some(first) = words.get_mut(..1) {
            first.make_ascii_uppercase();
        }
        words.into()
    }
}
