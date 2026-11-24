//! What the tests of the panes' kinds share: terminals the daemon lists, a device to show, and a
//! chat shown.

use std::path::Path;

use gpui_kit::{AnyWindowHandle, Entity, TestAppContext};

use super::{draw, shell_on, terminal_listed};
use crate::devices::Chosen;
use crate::model::PaneKind;
use crate::settings::Layout;
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

/// The shell in the Orca layout over a checkout of its own whose one tab, a chat, is shown,
/// motion reduced so what moves is where it goes at once: the window and the shell.
pub fn orca_on_chat(name: &str, cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Shell>) {
    let (window, shell, _) = shell_on(name, Layout::Orca, cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    shell.update(cx, |shell, cx| {
        shell.workspace.new_tab(PaneKind::Chat);
        cx.notify();
    });
    draw(window, cx);
    (window, shell)
}
