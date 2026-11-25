//! Whether the loose pages, the unpinned ones, are each space's own or shared by every space
//! (`docs/layout.md`, `docs/sidebar.md` 11.3), and the menu that picks it. Shared unless the
//! person picks Per space.

use gpui_kit::Action;
use gpui_kit::component::native_menu::NativeMenu;
use serde::{Deserialize, Serialize};

use super::checked_menu;

/// Which loose pages the top rail and the sidebar list, the pages after the pinned ones that are no
/// worktree's. Either way each is filed under the space it was opened in or brought to, so going
/// back to [`LoosePages::PerSpace`] puts each back in its space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LoosePages {
    /// Each space lists its own.
    PerSpace,
    /// Every space lists the same ones: all of them.
    #[default]
    Shared,
}

/// Sets which loose pages the sidebar lists, from the General settings' menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetLoosePages(pub LoosePages);

impl LoosePages {
    pub const ALL: [LoosePages; 2] = [LoosePages::PerSpace, LoosePages::Shared];

    pub fn label(self) -> &'static str {
        match self {
            LoosePages::PerSpace => "Per space",
            LoosePages::Shared => "Shared",
        }
    }

    /// A menu of the choices, with `self` checked.
    pub fn menu(self) -> NativeMenu {
        checked_menu(LoosePages::ALL, self, LoosePages::label, |pages| {
            Box::new(SetLoosePages(pages))
        })
    }
}
