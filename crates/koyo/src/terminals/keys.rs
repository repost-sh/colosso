//! Which keys a terminal's program gets while its terminal has focus, and which the app takes.
//! A binding of the app's fires before a view hears the key, so one that matches in a
//! terminal takes the key from the program: these keep the app's to the keys it means to take.

use gpui_kit::{AsKeystroke, KeyContext, Keystroke, OperatingSystem, TestAppContext};

/// The keys without Command the app takes from a terminal: tab switching, as browsers and
/// Ghostty switch tabs. Any other is the program's.
const THE_APPS: [&str; 2] = ["ctrl-tab", "ctrl-shift-tab"];

#[gpui_kit::test]
fn a_terminal_gets_its_keys_before_the_app(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::init(cx);
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let stack: Vec<KeyContext> = ["Root", koyo_term::CONTEXT]
            .iter()
            .map(|at| KeyContext::parse(at).unwrap())
            .collect();
        let fires = |keys: &[Keystroke]| {
            let (bindings, _) = keymap.bindings_for_input(keys, &stack);
            bindings.first().map(|binding| binding.action().name())
        };
        // The window's Tab is unbound here, so the view's key handler encodes it for the program.
        for keys in ["tab", "shift-tab"] {
            let action = fires(&[Keystroke::parse(keys).unwrap()]);
            assert_eq!(action, None, "{keys} isn't left to the terminal's program");
        }
        for (keys, _) in &koyo_term::MAC_EDITING {
            let action = fires(&[Keystroke::parse(keys).unwrap()]);
            assert_eq!(action, Some("terminal::SendText"), "{keys}");
        }
        for binding in keymap.bindings() {
            let keys: Vec<Keystroke> = (binding.keystrokes().iter())
                .map(|keystroke| keystroke.as_keystroke().clone())
                .collect();
            let Some(action) = fires(&keys) else {
                continue;
            };
            let named: Vec<String> = (keys.iter())
                .map(|keystroke| keystroke.unparse(OperatingSystem::Mac))
                .collect();
            let named = named.join(" ");
            let commanded = keys.iter().all(|keystroke| keystroke.modifiers.platform);
            assert!(
                action.starts_with("terminal") || commanded || THE_APPS.contains(&named.as_str()),
                "{named} goes to {action} in a terminal, not to its program: scope the binding \
                 away from the terminal, or say in THE_APPS that the app takes it",
            );
        }
    });
}
