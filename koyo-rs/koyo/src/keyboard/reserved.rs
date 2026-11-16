//! The keys Koyo keeps for itself while a page has the keyboard: the tab and window keys, which
//! never reach the page, as in Chrome. Every other key Koyo binds goes to the page first, and
//! to Koyo only when the page leaves it (`docs/keyboard.md`).

use gpui_kit::Keystroke;

/// Koyo's own bindings of the tab and window keys: ⌘T a new tab, ⌘W the tab closed, ⌘N a new
/// worktree, ⌘L the address, ⌘⇧T a closed tab back, ⌘Q quitting, ⌃⇥ and ⌃⇧⇥ the tab
/// switcher. Koyo binds no ⌘1 to ⌘9.
pub const RESERVED: [&str; 8] = [
    "cmd-t",
    "cmd-w",
    "cmd-n",
    "cmd-l",
    "cmd-shift-t",
    "cmd-q",
    "ctrl-tab",
    "ctrl-shift-tab",
];

/// Whether `keystroke` is one Koyo keeps for itself.
pub fn reserved(keystroke: &Keystroke) -> bool {
    RESERVED.iter().any(|key| {
        Keystroke::parse(key)
            .is_ok_and(|key| key.modifiers == keystroke.modifiers && key.key == keystroke.key)
    })
}
