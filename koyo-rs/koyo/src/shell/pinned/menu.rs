//! A pinned page's menu (`docs/arc-sidebar.md` 11), in either layout, as a right-click on its
//! tile in the Arc sidebar or its button in the Orca layout's top rail opens it: what it offers,
//! which of it can be picked, and what each pick does, which the window's root hears.

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{Action, App, Context, Div, InteractiveElement, Window};

use crate::arc::rename;
use crate::browser::{Address, read_address};
use crate::projects::with_menu;
use crate::shell::Shell;

/// Closes the pinned page known by this, its page's pane: it stays pinned.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = koyo, no_json)]
pub struct ClosePinned(pub u64);

/// Unpins the pinned page known by this onto the selected worktree's tabs.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = koyo, no_json)]
pub struct Unpin(pub u64);

/// Removes the pinned page known by this from the pinned, its page with it.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = koyo, no_json)]
pub struct RemovePinned(pub u64);

/// Opens a copy of the pinned or the loose page known by this, its page's pane, in a tab of the
/// selected worktree.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = koyo, no_json)]
pub struct CopyToWorktree(pub u64);

/// Pins the pinned page known by this at the address it shows.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = koyo, no_json)]
pub struct RepinAtCurrent(pub u64);

/// Asks for the address the pinned page known by this is pinned at.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = koyo, no_json)]
pub struct EditPinned(pub u64);

impl Shell {
    /// Has the window's root handle what a pinned page's menu picks, and a loose page's.
    pub(in crate::shell) fn handle_pinned(root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(
            cx.listener(|this, CopyToWorktree(key): &CopyToWorktree, window, cx| {
                this.copy_to_worktree(*key, window, cx);
            }),
        )
        .on_action(
            cx.listener(|this, ClosePinned(key): &ClosePinned, window, cx| {
                this.close_apart(*key, Shell::close_pinned, window, cx);
            }),
        )
        .on_action(cx.listener(|this, Unpin(key): &Unpin, _, cx| this.unpin(*key, cx)))
        .on_action(
            cx.listener(|this, RemovePinned(key): &RemovePinned, window, cx| {
                this.close_apart(*key, Shell::remove_pinned, window, cx);
            }),
        )
        .on_action(
            cx.listener(|this, RepinAtCurrent(key): &RepinAtCurrent, _, cx| {
                this.repin_at_current(*key, cx);
            }),
        )
        .on_action(
            cx.listener(|this, EditPinned(key): &EditPinned, window, cx| {
                this.ask_pinned_address(*key, window, cx);
            }),
        )
    }

    /// `element`, the tile or the button of the pinned page known by `key`, opens its menu as
    /// it's right-clicked.
    pub(crate) fn with_pinned_menu<E: InteractiveElement>(
        element: E,
        key: u64,
        cx: &Context<Self>,
    ) -> E {
        let shell = cx.weak_entity();
        with_menu(element, move |_, cx| {
            let menu = shell.read_with(cx, |shell, _| PinnedMenu::of(shell, key).build());
            menu.unwrap_or_else(|_| NativeMenu::new())
        })
    }

    /// Asks for the address the pinned page known by `key` is pinned at, the one it has in the
    /// field: an address typed as in the address bar, which it's then pinned at. Left empty, or
    /// words that are no address, it stays pinned where it was.
    fn ask_pinned_address(&mut self, key: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(at) = self.workspace.pinned_at(key) else {
            return;
        };
        let home = self.workspace.pinned()[at].home.clone();
        let shell = cx.weak_entity();
        let done = move |typed: String, cx: &mut App| {
            let Some(Address::Page(address)) = read_address(&typed) else {
                return;
            };
            (shell.update(cx, |shell, cx| shell.repin_at(key, address, cx))).ok();
        };
        let asks = ("Edit Pinned Page", "Save");
        rename::ask(asks, home, "Address", window, cx, done);
    }
}

/// What a pinned page's menu offers (`docs/arc-sidebar.md` 11), and which of it can be picked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PinnedMenu {
    key: u64,
    /// Whether its page shows another address than the one it's pinned at, to pin it at.
    pub strayed: bool,
    /// Whether it has a page to close.
    pub open: bool,
    /// Whether a worktree is selected, to copy it into.
    pub worktree: bool,
}

impl PinnedMenu {
    /// The menu of the pinned page known by `key`, as `shell` has it now.
    pub fn of(shell: &Shell, key: u64) -> PinnedMenu {
        PinnedMenu {
            key,
            strayed: shell.pinned_strayed(key).is_some(),
            open: shell.pinned_is_open(key),
            worktree: shell.checkout_shown().is_some(),
        }
    }

    /// The menu, as Arc's for a pinned tab: the address it's pinned at, to change; then closing
    /// it, which leaves it pinned, unpinning it, and a copy of it in the selected worktree; and
    /// removing it, apart.
    fn build(self) -> NativeMenu {
        let key = self.key;
        let edit = NativeMenu::new()
            .menu_with_disabled(
                "Replace Pinned URL with Current",
                !self.strayed,
                Box::new(RepinAtCurrent(key)),
            )
            .menu("Edit…", Box::new(EditPinned(key)));
        NativeMenu::new()
            .submenu("Edit Pinned Page", edit)
            .separator()
            .menu_with_disabled("Close", !self.open, Box::new(ClosePinned(key)))
            .menu("Unpin", Box::new(Unpin(key)))
            .menu_with_disabled(
                "Copy to Worktree",
                !self.worktree,
                Box::new(CopyToWorktree(key)),
            )
            .separator()
            .menu("Remove Pinned Tab", Box::new(RemovePinned(key)))
    }
}
