//! ⌘⌥← and ⌘⌥→ show the space to the left and to the right (`docs/arc-sidebar.md` 7) from
//! wherever the keyboard is, the chat's composer and a terminal included, where ⌥← and ⌥→ stay
//! a word back and on. At the first or last space they show nothing new, and in the Orca
//! layout and on a settings page nothing at all.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::{
    AnyWindowHandle, AppContext as _, Entity, FocusHandle, Focusable as _, KeyContext, KeyUpEvent,
    Keystroke, Modifiers, PlatformInput, TestAppContext, VisualTestContext,
};

use super::{act, arc_shell};
use crate::arc::SPACES;
use crate::arc::actions::MoveProjectToNewSpace;
use crate::files::tests::{editing, listed, new_chat, press, typed, with_shell};
use crate::files::{At, Place};
use crate::right_panel::ShowFiles;
use crate::settings::{self, Layout};
use crate::shell::Shell;
use crate::test_support::{click_pane, draw, open_page, select, write};

/// The shell in the Arc layout over two projects, each in a space of its own, the first shown.
fn two_spaces(name: &str, cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Shell>, PathBuf) {
    let (window, shell, roots) = arc_shell(&[&format!("{name}-app"), &format!("{name}-site")], cx);
    act(window, MoveProjectToNewSpace(roots[1].clone()), cx);
    back_to_first(window, &shell, &roots[0], cx);
    (window, shell, roots[0].clone())
}

/// Shows the first space again, its project selected.
fn back_to_first(
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    app: &Path,
    cx: &mut TestAppContext,
) {
    shell.update(cx, |shell, cx| shell.show_space(0, cx));
    select(shell, app, cx);
    draw(window, cx);
}

fn space(cx: &TestAppContext) -> usize {
    cx.read(|cx| settings::get(cx).space)
}

fn focus(window: AnyWindowHandle, handle: &FocusHandle, cx: &mut TestAppContext) {
    (cx.update_window(window, |_, window, cx| window.focus(handle, cx))).unwrap();
    draw(window, cx);
}

/// Presses `key` in `window` and lets it up, as a person does: an arrow pressed again before
/// it came up is taken for macOS repeating it (`key_repeat`). Drawn first, so the keyboard is
/// where the last change left it. The action bound to it that ran, if one did, and the key
/// contexts it was pressed in.
fn pressed(
    window: AnyWindowHandle,
    key: &'static str,
    cx: &mut TestAppContext,
) -> (Option<&'static str>, Vec<KeyContext>) {
    let heard = Rc::new(RefCell::new(None));
    let hearing = heard.clone();
    let _observing = cx.update(|cx| {
        cx.observe_keystrokes(move |event, _, _| {
            let action = event.action.as_ref().map(|action| action.name());
            *hearing.borrow_mut() = Some((action, event.context_stack.clone()));
        })
    });
    press(window, key, cx);
    let keystroke = Keystroke::parse(key).unwrap();
    (cx.update_window(window, |_, window, cx| {
        window.dispatch_event(PlatformInput::KeyUp(KeyUpEvent { keystroke }), cx);
    }))
    .unwrap();
    draw(window, cx);
    let heard = heard.borrow_mut().take();
    // A view's own key handler that takes a key keeps it from the window's observers: no
    // binding ran, and the key went to what has the keyboard.
    heard.unwrap_or_else(|| {
        let stack = cx.update_window(window, |_, window, _| window.context_stack());
        (None, stack.unwrap())
    })
}

/// As [`pressed`], with the keyboard in `context`: the action that ran, if one did.
fn pressed_in(
    window: AnyWindowHandle,
    key: &'static str,
    context: &str,
    cx: &mut TestAppContext,
) -> Option<&'static str> {
    let (action, stack) = pressed(window, key, cx);
    let within = stack.iter().any(|at| at.contains(context));
    assert!(within, "{key} was pressed in {stack:?}, not {context}");
    action
}

#[gpui_kit::test]
fn the_keys_show_the_space_beside_and_stop_at_the_ends(cx: &mut TestAppContext) {
    let (window, shell, _) = two_spaces("space-keys-ends", cx);
    // With nothing focused, as when the window has just come forward.
    (cx.update_window(window, |_, window, cx| window.blur(cx))).unwrap();
    let [previous, next] = SPACES_MOVE;
    assert_eq!(
        (pressed(window, "cmd-alt-right", cx).0, space(cx)),
        (next, 1)
    );
    // The last space: nothing past it, and no wrapping round to the first.
    assert_eq!(
        (pressed(window, "cmd-alt-right", cx).0, space(cx)),
        (next, 1)
    );
    assert_eq!(
        (pressed(window, "cmd-alt-left", cx).0, space(cx)),
        (previous, 0)
    );
    assert_eq!(
        (pressed(window, "cmd-alt-left", cx).0, space(cx)),
        (previous, 0)
    );
    // From the shell's own focus, where the keyboard is with no tab's view in it.
    let root = cx.read(|cx| shell.read(cx).focus.clone());
    focus(window, &root, cx);
    assert_eq!(
        (pressed(window, "cmd-alt-right", cx).0, space(cx)),
        (next, 1)
    );
}

#[gpui_kit::test]
fn the_keys_show_the_space_beside_from_the_file_tree_and_a_viewer(cx: &mut TestAppContext) {
    let (window, shell, app) = two_spaces("space-keys-tree", cx);
    act(window, ShowFiles, cx);
    let tree = cx
        .read(|cx| shell.read(cx).right_panel.files().cloned())
        .expect("the Files panel");
    listed(&tree, cx);
    let tree = cx.read(|cx| tree.read(cx).focus_handle().clone());
    focus(window, &tree, cx);
    let action = pressed_in(window, "cmd-alt-right", "FilesTree", cx);
    assert_eq!((action, space(cx)), (SPACES_MOVE[1], 1));

    back_to_first(window, &shell, &app, cx);
    let picture = image::RgbaImage::from_pixel(8, 8, image::Rgba([128, 128, 128, 255]));
    picture.save(app.join("picture.png")).unwrap();
    with_shell(window, &shell, cx, |shell, window, cx| {
        shell.view_file(Place::file("picture.png"), window, cx);
    });
    let pane = cx.read(|cx| {
        let unit = shell.read(cx).workspace.branch().unwrap();
        unit.tabs[unit.active_tab].panes()[0].id
    });
    click_pane(window, pane, cx);
    let action = pressed_in(window, "cmd-alt-right", "Viewer", cx);
    assert_eq!((action, space(cx)), (SPACES_MOVE[1], 1));
}

/// In the editor and the composer ⌥← and ⌥→ jump a word, and ⌘⌥→ shows the next space.
#[gpui_kit::test]
fn the_keys_show_the_space_beside_from_text_where_option_arrows_jump_a_word(
    cx: &mut TestAppContext,
) {
    let (window, shell, app) = two_spaces("space-keys-text", cx);
    write(&app, "notes.txt", b"");
    let (_, editor, _) = editing(window, &shell, "notes.txt", At::Bytes(0..0), cx);
    typed(window, &editor, "hello world", cx);
    let handle = cx.read(|cx| editor.read(cx).focus_handle(cx));
    focus(window, &handle, cx);
    let cursor = |cx: &TestAppContext| cx.read(|cx| editor.read(cx).selected_range());
    assert_eq!(cursor(cx), 11..11);
    let action = pressed_in(window, "alt-left", "Input", cx);
    assert_eq!((action, cursor(cx), space(cx)), (WORDS[0], 6..6, 0));
    let action = pressed_in(window, "alt-right", "Input", cx);
    assert_eq!((action, cursor(cx), space(cx)), (WORDS[1], 11..11, 0));
    let action = pressed_in(window, "cmd-alt-right", "Input", cx);
    assert_eq!((action, space(cx)), (SPACES_MOVE[1], 1));

    // The chat's composer, where a new chat leaves the keyboard.
    back_to_first(window, &shell, &app, cx);
    new_chat(window, cx);
    draw(window, cx);
    cx.simulate_input(window, "hello world");
    let action = pressed_in(window, "alt-left", "ChatComposer", cx);
    assert_eq!((action, space(cx)), (WORDS[0], 0));
    let action = pressed_in(window, "alt-right", "ChatComposer", cx);
    assert_eq!((action, space(cx)), (WORDS[1], 0));
    let action = pressed_in(window, "cmd-alt-right", "ChatComposer", cx);
    assert_eq!((action, space(cx)), (SPACES_MOVE[1], 1));
}

/// A web page keeps ⌥← and ⌥→, sent to it as keys for its own text fields; ⌘⌥→ shows the next
/// space.
#[gpui_kit::test]
fn the_keys_show_the_space_beside_from_a_web_page(cx: &mut TestAppContext) {
    let (window, shell, _) = two_spaces("space-keys-page", cx);
    let pane = open_page(window, &shell, Some("https://www.example.com/"), cx);
    click_pane(window, pane, cx);
    for key in ["alt-left", "alt-right"] {
        let action = pressed_in(window, key, "BrowserPage", cx);
        assert_eq!((action, space(cx)), (None, 0), "{key}");
    }
    let action = pressed_in(window, "cmd-alt-right", "BrowserPage", cx);
    assert_eq!((action, space(cx)), (SPACES_MOVE[1], 1));
}

#[gpui_kit::test]
fn the_keys_move_nothing_in_the_orca_layout_or_the_settings(cx: &mut TestAppContext) {
    let (window, _, _) = two_spaces("space-keys-orca", cx);
    cx.update(|cx| settings::update(cx, |settings| settings.layout = Layout::Orca));
    draw(window, cx);
    (cx.update_window(window, |_, window, cx| window.blur(cx))).unwrap();
    assert_eq!(
        (pressed(window, "cmd-alt-right", cx).0, space(cx)),
        (None, 0)
    );
    // Nor on a settings page in the Arc layout, where the sidebar lists the pages.
    cx.update(|cx| settings::update(cx, |settings| settings.layout = Layout::Arc));
    press(window, "cmd-,", cx);
    assert_eq!(
        (pressed(window, "cmd-alt-right", cx).0, space(cx)),
        (None, 0)
    );
}

/// Whether the shell's own focus has the keyboard after a few frames, where a pane that would
/// take it back has had its chance.
fn keyboard_at_window(
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    cx: &mut TestAppContext,
) -> bool {
    for _ in 0..3 {
        draw(window, cx);
    }
    let at = cx.update_window(window, |_, window, cx| {
        shell.read(cx).focus.is_focused(window)
    });
    at.unwrap()
}

/// A chat's prompt has the keyboard only when the person puts it there: a press in the sidebar
/// takes it out, and another space shown leaves it at the window, its chat's prompt or not.
/// Picking the chat's tab puts it back.
#[gpui_kit::test]
fn a_chats_prompt_has_the_keyboard_only_when_the_person_puts_it_there(cx: &mut TestAppContext) {
    let (window, shell, app) = two_spaces("space-keys-prompt", cx);
    // A chat in each space, the first's prompt with the keyboard.
    shell.update(cx, |shell, cx| shell.show_space(1, cx));
    new_chat(window, cx);
    back_to_first(window, &shell, &app, cx);
    new_chat(window, cx);
    assert!(
        !keyboard_at_window(window, &shell, cx),
        "a new chat's prompt has it"
    );

    // A press in the sidebar, where nothing takes the keyboard: out of the prompt, for good.
    let mut vcx = VisualTestContext::from_window(window, cx);
    let at = vcx
        .debug_bounds("arc-project-name")
        .expect("the project's name")
        .center();
    vcx.simulate_click(at, Modifiers::default());
    assert!(
        keyboard_at_window(window, &shell, cx),
        "pressed outside the prompt"
    );

    // The other space, its chat shown: the keyboard stays at the window, and back again too.
    assert_eq!(
        (pressed(window, "cmd-alt-right", cx).0, space(cx)),
        (SPACES_MOVE[1], 1)
    );
    assert!(
        keyboard_at_window(window, &shell, cx),
        "the next space's chat"
    );
    assert_eq!(
        (pressed(window, "cmd-alt-left", cx).0, space(cx)),
        (SPACES_MOVE[0], 0)
    );
    assert!(
        keyboard_at_window(window, &shell, cx),
        "back in the first space"
    );

    // Its tab picked in the sidebar, the chat's prompt takes the keyboard.
    shell.update(cx, |shell, cx| shell.pick_tab(0, 0, 0, cx));
    assert!(!keyboard_at_window(window, &shell, cx), "its tab picked");
}

/// What ⌘⌥← and ⌘⌥→ run where they move between spaces, and ⌥← and ⌥→ in a text field.
const SPACES_MOVE: [Option<&str>; 2] = [Some("arc::PreviousSpace"), Some("arc::NextSpace")];
const WORDS: [Option<&str>; 2] = [
    Some("input::MoveToPreviousWord"),
    Some("input::MoveToNextWord"),
];

/// The keymap's side of it, for a terminal, the agent desktop and a device too, which a test
/// can't run: in each key context stack the window can have, what ⌘⌥← and ⌘⌥→, and ⌥← and ⌥→,
/// run, whatever order they were bound in.
#[gpui_kit::test]
fn the_keymap_has_the_spaces_on_command_option_arrows_everywhere(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::init(cx);
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let fires = |stack: &[&str], key: &'static str| -> Option<&'static str> {
            let stack: Vec<_> = (stack.iter())
                .map(|context| KeyContext::parse(context).unwrap())
                .collect();
            let (bindings, _) =
                keymap.bindings_for_input(&[Keystroke::parse(key).unwrap()], &stack);
            bindings.first().map(|binding| binding.action().name())
        };
        let terminal = [Some("terminal::SendText"); 2];
        // Where spaces show, ⌘⌥← and ⌘⌥→ move them; ⌥← and ⌥→ are as each place has them.
        let places: &[(&[&str], [Option<&str>; 2])] = &[
            (&["Root", SPACES], [None; 2]),
            (&["Root", SPACES, "FilesTree"], [None; 2]),
            (&["Root", SPACES, "Viewer"], [None; 2]),
            (&["Root", SPACES, koyo_term::CONTEXT], terminal),
            (&["Root", SPACES, "ChatComposer", "Input"], WORDS),
            (&["Root", SPACES, "BrowserAddress", "Input"], WORDS),
            (&["Root", SPACES, "BrowserPage"], [None; 2]),
            (&["Root", SPACES, "AgentDesktop"], [None; 2]),
            (&["Root", SPACES, "Device"], [None; 2]),
        ];
        for (stack, option) in places {
            let command = [fires(stack, "cmd-alt-left"), fires(stack, "cmd-alt-right")];
            assert_eq!(command, SPACES_MOVE, "⌘⌥ in {stack:?}");
            let plain = [fires(stack, "alt-left"), fires(stack, "alt-right")];
            assert_eq!(plain, *option, "⌥ in {stack:?}");
        }
        // The Orca layout, and a settings page: no spaces shown, so the keys are unbound.
        for stack in [&["Root"][..], &["Root", "Settings"]] {
            let command = [fires(stack, "cmd-alt-left"), fires(stack, "cmd-alt-right")];
            assert_eq!(command, [None; 2], "⌘⌥ in {stack:?}");
        }
    });
}
