//! What the tests of the panes' kinds share: terminals the daemon lists, and a device to show.

use std::path::Path;

use gpui_kit::{Entity, TestAppContext};

use super::terminal_listed;
use crate::devices::Chosen;
use crate::shell::Shell;

/// The daemon lists the terminals `listed`, each by its handle and its name, at their prompts in
/// the checkout at `worktree`, which `shell` takes, each to a tab there.
pub fn terminals_listed(
    shell: &Entity<Shell>,
    worktree: &Path,
    listed: &[(&str, &str)],
    cx: &mut TestAppContext,
) {
    let listed = (listed.iter())
        .map(|(handle, name)| terminal_listed(handle, name, worktree))
        .collect();
    shell.update(cx, |shell, cx| shell.take_terminals(&[worktree.to_owned()], listed, cx));
}

/// An iPhone 17 Pro of Koyo's simulators, known by `udid`.
pub fn iphone(udid: &str) -> Chosen {
    Chosen {
        udid: udid.into(),
        name: "iPhone 17 Pro".into(),
        ipad: false,
        phone: false,
    }
}
