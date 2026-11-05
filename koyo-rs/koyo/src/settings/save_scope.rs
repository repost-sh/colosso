//! What ⌘S saves, and the menu that picks it.

use gpui_kit::Action;
use gpui_kit::component::native_menu::NativeMenu;
use serde::{Deserialize, Serialize};

use super::checked_menu;

/// What ⌘S saves: the file shown, as most editors do, or every file with unsaved edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SaveScope {
    #[default]
    Shown,
    All,
}

/// Sets what ⌘S saves, from the Editor settings' menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetSaveScope(pub SaveScope);

impl SaveScope {
    pub const ALL: [SaveScope; 2] = [SaveScope::Shown, SaveScope::All];

    pub fn label(self) -> &'static str {
        match self {
            SaveScope::Shown => "The file shown",
            SaveScope::All => "Every file with unsaved edits",
        }
    }

    /// A menu of what ⌘S can save, with `self` checked.
    pub fn menu(self) -> NativeMenu {
        checked_menu(SaveScope::ALL, self, SaveScope::label, |scope| Box::new(SetSaveScope(scope)))
    }
}
