//! ⌃⇥ lists the open files, the latest shown first, moves the pick with each press, and
//! shows the file picked when ⌃ is let go, in either window, or Enter is pressed.

use gpui_kit::{AnyWindowHandle, Modifiers, TestAppContext, VisualTestContext};

use super::Switcher;
use crate::files::At;
use crate::files::tests::{checkout, dialog, editing, press, shell_showing};

#[gpui_kit::test]
fn ctrl_tab_lists_the_files_latest_first_and_letting_go_shows_the_one_picked(cx: &mut TestAppContext) {
    let root = checkout("files-switcher");
    let (window, _, files) = shell_showing(&root, cx);
    for path in ["src/lib.rs", "src/main.rs"] {
        editing(window, &files, path, At::Bytes(0..0), cx);
    }
    // The switcher open, if it is: its window, what it lists and the pick.
    let open = |cx: &mut TestAppContext| {
        let (window, switcher) = dialog::<Switcher>(cx)?;
        cx.read(|cx| {
            let switcher = switcher.read(cx);
            let names: Vec<String> = (switcher.rows.iter()).map(|row| row.name.to_string()).collect();
            Some((window, names, switcher.selected))
        })
    };
    let picked = |cx: &mut TestAppContext| open(cx).map(|open| open.2);
    let shown = |cx: &TestAppContext| {
        cx.read(|cx| (files.read(cx).viewing()).and_then(|viewed| viewed.relative.clone()))
    };
    let let_go = |window: AnyWindowHandle, cx: &mut TestAppContext| {
        VisualTestContext::from_window(window, cx).simulate_modifiers_change(Modifiers::none());
    };

    // Shown in turn: README, lib, main. ⌃⇥ lists them the latest first, the one before picked.
    press(window, "ctrl-tab", cx);
    let (switcher, names, selected) = open(cx).expect("the switcher");
    assert_eq!((names, selected), (vec!["main.rs".into(), "lib.rs".into(), "README.md".into()], 1));
    // Each ⌃⇥ moves the pick on, round the end, and ⌃⇧⇥ back.
    press(switcher, "ctrl-tab", cx);
    assert_eq!(picked(cx), Some(2));
    press(switcher, "ctrl-tab", cx);
    assert_eq!(picked(cx), Some(0));
    press(switcher, "ctrl-shift-tab", cx);
    assert_eq!(picked(cx), Some(2));
    // ⌃ let go in the switcher's window: the file picked is shown and the switcher's gone.
    let_go(switcher, cx);
    assert!(open(cx).is_none(), "closed once picked");
    assert_eq!(shown(cx).as_deref(), Some("README.md"));
    // Opened again, the file just left is the one before. A quick tap lets ⌃ go before the
    // switcher's window has the keyboard: heard in the window behind, it picks the same.
    press(window, "ctrl-tab", cx);
    let (_, names, selected) = open(cx).expect("the switcher again");
    assert_eq!((names, selected), (vec!["README.md".into(), "main.rs".into(), "lib.rs".into()], 1));
    let_go(window, cx);
    assert!(open(cx).is_none());
    assert_eq!(shown(cx).as_deref(), Some("src/main.rs"));
    // Enter picks too.
    press(window, "ctrl-tab", cx);
    let (switcher, ..) = open(cx).expect("the switcher a third time");
    press(switcher, "enter", cx);
    assert!(open(cx).is_none());
    assert_eq!(shown(cx).as_deref(), Some("README.md"));
    let _ = std::fs::remove_dir_all(&root);
}
