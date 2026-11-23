//! The window's Back and Forward, which walk the Settings tab's pages (a browser tab takes them
//! for its own history, as a browser does), the keys that open and close Settings, and its
//! pages. The shell answers them (`shell/settings_tab.rs`).

use gpui_kit::{App, KeyBinding, actions};

actions!(navigation, [GoBack, GoForward, OpenSettings, CloseSettings]);

/// A page of Settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SettingsPage {
    #[default]
    General,
    Chat,
    Accounts,
    Editor,
    Languages,
    Appearance,
    Browser,
    KeyboardShortcuts,
    McpServer,
    AgentDesktop,
    Devices,
    About,
}

/// Where the window's Back and Forward keys don't reach: a browser tab takes them for its own
/// history, as a browser does.
const OUTSIDE_BROWSER: &str = "!BrowserPage && !BrowserAddress && !BrowserFind";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-[", GoBack, Some(OUTSIDE_BROWSER)),
        KeyBinding::new("cmd-]", GoForward, Some(OUTSIDE_BROWSER)),
        KeyBinding::new("escape", CloseSettings, Some("Settings")),
    ]);
}

#[cfg(test)]
mod tests {
    use gpui_kit::{KeyContext, Keystroke};

    #[gpui_kit::test]
    fn a_browser_tab_takes_back_and_forward_for_its_own_history(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            crate::test_support::init(cx);
            let keymap = cx.key_bindings();
            let keymap = keymap.borrow();
            let first = |keys: &'static str, at: &[&str]| {
                let stack: Vec<KeyContext> =
                    at.iter().map(|at| KeyContext::parse(at).unwrap()).collect();
                let (bindings, _) =
                    keymap.bindings_for_input(&[Keystroke::parse(keys).unwrap()], &stack);
                bindings.first().map(|binding| binding.action().name())
            };
            for at in ["BrowserPage", "BrowserAddress", "BrowserFind"] {
                assert_eq!(
                    first("cmd-[", &["Workspace", at]),
                    Some("browser::Back"),
                    "{at}"
                );
                assert_eq!(
                    first("cmd-]", &["Workspace", at]),
                    Some("browser::Forward"),
                    "{at}"
                );
            }
            assert_eq!(first("cmd-[", &["Workspace"]), Some("navigation::GoBack"));
            assert_eq!(
                first("cmd-]", &["Workspace", "Editor"]),
                Some("navigation::GoForward")
            );
        });
    }
}
