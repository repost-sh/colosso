//! The pinned pages as a test makes and reads them: pages opened on the selected checkout and
//! pinned, and the pinned pages by the addresses they show; and the loose pages by theirs.

use gpui_kit::{AnyWindowHandle, Entity, TestAppContext};

use super::{draw, open_page};
use crate::shell::Shell;

/// Opens a page at each of `addresses` on the selected checkout and pins them, in that order:
/// what their tiles and buttons are known by.
pub fn pin(
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    addresses: &[&str],
    cx: &mut TestAppContext,
) -> Vec<u64> {
    let keys: Vec<u64> = (addresses.iter())
        .map(|address| open_page(window, shell, Some(address), cx))
        .collect();
    shell.update(cx, |shell, cx| {
        for key in &keys {
            assert!(shell.pin_tab_of(*key, usize::MAX, cx), "a page's tab is pinned");
        }
    });
    draw(window, cx);
    keys
}

/// The pinned pages, by the addresses they show, in order.
pub fn pinned(shell: &Entity<Shell>, cx: &TestAppContext) -> Vec<String> {
    cx.read(|cx| {
        (shell.read(cx).workspace.pinned().iter())
            .map(|pinned| pinned.tab.first().address().unwrap_or_default().to_owned())
            .collect()
    })
}

/// The loose pages of the space shown, by the addresses they show, in order.
pub fn loose(shell: &Entity<Shell>, cx: &TestAppContext) -> Vec<String> {
    cx.read(|cx| {
        (shell.read(cx).workspace.loose().iter())
            .map(|page| page.tab.first().address().unwrap_or_default().to_owned())
            .collect()
    })
}
