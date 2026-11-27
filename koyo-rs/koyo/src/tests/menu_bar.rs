//! The menu bar adds no route for a key: an item shows only a key the window takes wherever the
//! item is enabled (`crate::bar`, `docs/native-layer.md` 7). Each item's kind is read from the
//! real keymap.

use gpui_kit::{
    Action, AnyWindowHandle, App, AppContext as _, KeyBinding, KeyContext, Keystroke, MenuItem,
    OperatingSystem, SharedString, TestAppContext,
};

use crate::go_to_file::GoToFile;
use crate::test_support::{
    click_pane, in_front, keyboard_in, open_page, repo, select, shell_window, shell_with, shown,
};

/// The exceptions `crate::bar` gives its reason for: commands of the window bound in some
/// contexts only. One no longer bound so comes off.
fn excepted() -> [Box<dyn Action>; 1] {
    [Box::new(GoToFile)]
}

/// Each action item of the bar by its name, but the system's six, which `edit_menu`'s tests
/// hold to a stricter rule: no key at all.
fn commands() -> Vec<(SharedString, Box<dyn Action>)> {
    let items = crate::bar().into_iter().flat_map(|menu| menu.items);
    items
        .filter_map(|item| match item {
            MenuItem::Action {
                name,
                action,
                os_action: None,
                ..
            } => Some((name, action)),
            _ => None,
        })
        .collect()
}

/// Whether `action` is bound, and only in some contexts.
fn in_contexts_only(action: &dyn Action, cx: &App) -> bool {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let mut bindings = keymap.bindings_for_action(action).peekable();
    bindings.peek().is_some() && bindings.all(|binding| binding.predicate().is_some())
}

/// The commands bound in some contexts only and not excepted, which are a page's.
fn page_commands(cx: &App) -> Vec<(SharedString, Box<dyn Action>)> {
    let excepted = excepted();
    let commands = commands().into_iter();
    commands
        .filter(|(_, action)| in_contexts_only(action.as_ref(), cx))
        .filter(|(_, action)| !excepted.iter().any(|taken| taken.partial_eq(action.as_ref())))
        .collect()
}

/// The stand-in context stack `create_menu_item` (`vendor/gpui-fast-macos/src/platform.rs`)
/// reads a binding's context in: a Zed workspace's editor, as it builds it.
fn stand_in() -> Vec<KeyContext> {
    let context = |name| {
        let mut context = KeyContext::new_with_defaults(OperatingSystem::Mac);
        context.add(name);
        context
    };
    let (mut workspace, mut pane) = (context("Workspace"), context("Pane"));
    pane.extend(&context("Editor"));
    workspace.extend(&pane);
    vec![workspace]
}

/// The binding whose key the menu shows beside an item, as `create_menu_item` picks it: the first
/// with no context or a context that holds in [`stand_in`], else the first. `!BrowserPage` holds
/// there, so Go to File… shows ⌘P; `BrowserPage` doesn't.
fn shown_binding<'a>(bindings: &[&'a KeyBinding]) -> Option<&'a KeyBinding> {
    let stand_in = stand_in();
    let holds = bindings.iter().find(|binding| {
        binding.predicate().is_none_or(|predicate| predicate.eval(&stand_in))
    });
    holds.or(bindings.first()).copied()
}

/// The key the menu shows beside `action`'s item, if any.
fn shown_key(action: &dyn Action, cx: &App) -> Option<Keystroke> {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let bindings: Vec<&KeyBinding> = keymap.bindings_for_action(action).collect();
    let binding = shown_binding(&bindings)?;
    let [key] = binding.keystrokes() else {
        panic!("{} is bound to one key", action.name());
    };
    Some(key.inner().clone())
}

/// A key bound with no context has ⌘ or ⌃, which GPUI hears first wherever the keyboard is, a
/// page's included (`koyo_seam::keys::route`); and each exception is still bound in some
/// contexts only.
#[gpui_kit::test]
fn a_key_bound_everywhere_has_cmd_or_ctrl_and_each_exception_is_needed(cx: &mut TestAppContext) {
    cx.update(crate::test_support::init);
    cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        for (name, action) in commands() {
            let everywhere = keymap.bindings_for_action(action.as_ref());
            let everywhere = everywhere.filter(|binding| binding.predicate().is_none());
            for key in everywhere.flat_map(KeyBinding::keystrokes) {
                let modifiers = key.modifiers();
                assert!(modifiers.platform || modifiers.control, "{name}'s key has ⌘ or ⌃");
            }
        }
        let commands = commands();
        for taken in excepted() {
            let needed = (commands.iter()).any(|(_, action)| {
                taken.partial_eq(action.as_ref()) && in_contexts_only(action.as_ref(), cx)
            });
            assert!(needed, "{} is in the bar and bound in some contexts only", taken.name());
        }
    });
}

/// Wherever the keyboard is, an item the menu would enable shows only a key the window takes
/// there itself, so the menu never gets it.
fn each_enabled_item_shows_a_key_the_window_takes(
    window: AnyWindowHandle,
    cx: &mut TestAppContext,
) {
    in_front(window, true, cx);
    for (name, action) in commands() {
        let Some(key) = cx.update(|cx| shown_key(action.as_ref(), cx)) else {
            continue;
        };
        if !cx.update(|cx| cx.is_action_available(action.as_ref())) {
            continue;
        }
        assert!(
            key.modifiers.platform || key.modifiers.control,
            "{name}'s key has ⌘ or ⌃"
        );
        let bound = cx.update_window(window, |_, window, cx| {
            let keymap = cx.key_bindings();
            let keymap = keymap.borrow();
            let (bindings, _) =
                keymap.bindings_for_input(std::slice::from_ref(&key), &window.context_stack());
            bindings
                .iter()
                .map(|binding| binding.action().boxed_clone())
                .collect::<Vec<_>>()
        });
        let taken = (bound.unwrap().iter())
            .any(|bound| cx.update(|cx| cx.is_action_available(bound.as_ref())));
        assert!(
            taken,
            "{name}: the window takes {key:?} itself where the keyboard is"
        );
    }
}

/// Whether each of a page's commands is enabled, as the menu reads it.
fn page_commands_enabled(
    window: AnyWindowHandle,
    cx: &mut TestAppContext,
) -> Vec<(SharedString, bool)> {
    in_front(window, true, cx);
    let pages = cx.update(|cx| page_commands(cx));
    pages
        .into_iter()
        .map(|(name, action)| (name, cx.update(|cx| cx.is_action_available(action.as_ref()))))
        .collect()
}

/// A command bound in some contexts only and not excepted is a page's: enabled with the keyboard
/// in a page, and disabled anywhere else, where its key isn't the page's. In either, an enabled
/// item's key is the window's.
#[gpui_kit::test]
fn a_page_command_is_enabled_only_in_a_page_and_no_item_takes_a_key(cx: &mut TestAppContext) {
    let root = repo("menu-bar-page", &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    assert!(!keyboard_in(window, "BrowserPage", cx));
    let outside = page_commands_enabled(window, cx);
    assert!(!outside.is_empty(), "the bar has a page's commands");
    for (name, enabled) in outside {
        assert!(!enabled, "{name} is disabled outside a page");
    }
    each_enabled_item_shows_a_key_the_window_takes(window, cx);

    let pane = open_page(window, &shell, Some("https://example.com/"), cx);
    click_pane(window, pane, cx);
    assert!(
        keyboard_in(window, "BrowserPage", cx),
        "the keyboard is in the page"
    );
    for (name, enabled) in page_commands_enabled(window, cx) {
        assert!(enabled, "{name} is enabled in a page");
    }
    each_enabled_item_shows_a_key_the_window_takes(window, cx);
}

/// Koyo Help shows the README as a page in Koyo.
#[gpui_kit::test]
fn koyo_help_shows_the_readme_as_a_page(cx: &mut TestAppContext) {
    cx.update(crate::test_support::init);
    let (window, shell) = shell_window(cx);
    let help = commands()
        .into_iter()
        .find(|(name, _)| name.as_ref() == "Koyo Help");
    let (_, action) = help.expect("a Koyo Help item");
    (cx.update_window(window, |_, window, cx| window.dispatch_action(action, cx))).unwrap();
    cx.run_until_parked();
    let readme = "https://github.com/repost-sh/koyo#readme";
    assert_eq!(shown(&shell, cx).0.as_deref(), Some(readme));
}
