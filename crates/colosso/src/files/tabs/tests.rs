use gpui_kit::{AnyWindowHandle, Entity, TestAppContext};

use super::super::tests::{checkout, open, selected};
use super::*;

/// The tabs open, by their paths in the checkout, a preview's ending in `*` and the one shown
/// in brackets.
fn tabs(panel: &Entity<FilesPanel>, cx: &TestAppContext) -> Vec<String> {
    cx.read(|cx| {
        let panel = panel.read(cx);
        (panel.tabs().iter().enumerate())
            .map(|(at, tab)| {
                let name = format!(
                    "{}{}",
                    tab.relative.as_deref().unwrap_or("?"),
                    if tab.pinned { "" } else { "*" }
                );
                if at == panel.shown { format!("[{name}]") } else { name }
            })
            .collect()
    })
}

fn with(
    window: AnyWindowHandle,
    panel: &Entity<FilesPanel>,
    cx: &mut TestAppContext,
    change: impl FnOnce(&mut FilesPanel, &mut Window, &mut Context<FilesPanel>),
) {
    (cx.update_window(window, |_, window, cx| {
        panel.update(cx, |panel, cx| change(panel, window, cx));
    }))
    .unwrap();
}

#[gpui_kit::test]
fn a_preview_gives_way_to_the_next_until_its_pinned(cx: &mut TestAppContext) {
    let root = checkout("files-tabs-preview");
    let (window, panel) = open(&root, cx);
    let preview = |path: &str, cx: &mut TestAppContext| {
        with(window, &panel, cx, |panel, window, cx| {
            panel.preview(Place::file(path), window, cx);
        });
        tabs(&panel, cx)
    };
    assert_eq!(preview("src/lib.rs", cx), ["[src/lib.rs*]"]);
    assert_eq!(preview("README.md", cx), ["[README.md*]"]);
    assert_eq!(selected(&panel, cx).as_deref(), Some("README.md"));
    // A file viewed for good opens beside the preview, which stays.
    with(window, &panel, cx, |panel, window, cx| {
        panel.view(Place::file("src/main.rs"), window, cx);
    });
    assert_eq!(tabs(&panel, cx), ["README.md*", "[src/main.rs]"]);
    // The next preview takes the preview's tab, where it is.
    assert_eq!(preview("new.txt", cx), ["[new.txt*]", "src/main.rs"]);
    // A file open already is shown in its tab.
    assert_eq!(preview("src/main.rs", cx), ["new.txt*", "[src/main.rs]"]);
    // Pinned, the preview stays when another file's previewed.
    with(window, &panel, cx, |panel, _, cx| panel.pin(0, cx));
    assert_eq!(
        preview("README.md", cx),
        ["new.txt", "src/main.rs", "[README.md*]"]
    );
    // Viewing the preview's file for good pins it.
    with(window, &panel, cx, |panel, window, cx| {
        panel.view(Place::file("README.md"), window, cx);
    });
    assert_eq!(tabs(&panel, cx), ["new.txt", "src/main.rs", "[README.md]"]);
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn closing_the_tab_shown_shows_the_next_or_else_the_one_before(cx: &mut TestAppContext) {
    let root = checkout("files-tabs-close");
    let (window, panel) = open(&root, cx);
    for path in ["README.md", "new.txt", "src/lib.rs"] {
        with(window, &panel, cx, |panel, window, cx| {
            panel.view(Place::file(path), window, cx);
        });
    }
    with(window, &panel, cx, |panel, _, cx| panel.show_tab(1, cx));
    with(window, &panel, cx, |panel, _, cx| panel.close(1, cx));
    assert_eq!(tabs(&panel, cx), ["README.md", "[src/lib.rs]"]);
    assert_eq!(selected(&panel, cx).as_deref(), Some("src/lib.rs"));
    with(window, &panel, cx, |panel, _, cx| panel.close(1, cx));
    assert_eq!(tabs(&panel, cx), ["[README.md]"]);
    with(window, &panel, cx, |panel, _, cx| panel.close(0, cx));
    assert!(tabs(&panel, cx).is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn the_arrow_keys_preview_each_file_they_land_on(cx: &mut TestAppContext) {
    let root = checkout("files-tabs-keys");
    let (window, panel) = open(&root, cx);
    cx.update_window(window, |_, window, cx| {
        let focus = panel.read(cx).focus_handle().clone();
        window.focus(&focus, cx);
        window.draw(cx).clear(cx);
    })
    .unwrap();
    let press = |keys: &str, cx: &mut TestAppContext| {
        cx.simulate_keystrokes(window, keys);
        cx.run_until_parked();
        tabs(&panel, cx)
    };
    // Rows: src/, target/, .gitignore, new.txt, README.md. A folder previews nothing.
    assert!(press("down", cx).is_empty());
    assert_eq!(press("end", cx), ["[README.md*]"]);
    assert_eq!(press("up", cx), ["[new.txt*]"]);
    assert_eq!(press("up", cx), ["[.gitignore*]"]);
    // Enter keeps it; the next file down is previewed beside it.
    assert_eq!(press("enter", cx), ["[.gitignore]"]);
    assert_eq!(press("down", cx), [".gitignore", "[new.txt*]"]);
    // Previews turned off, the keys only move the pick, and a click opens a tab for good.
    cx.update(|cx| settings::update(cx, |settings| settings.preview_files = false));
    assert_eq!(press("down", cx), [".gitignore", "[new.txt*]"]);
    assert_eq!(selected(&panel, cx).as_deref(), Some("README.md"));
    with(window, &panel, cx, |panel, window, cx| {
        panel.activate(4, false, window, cx);
    });
    assert_eq!(tabs(&panel, cx), [".gitignore", "new.txt*", "[README.md]"]);
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn a_tab_dragged_to_another_slot_leaves_the_one_shown_shown(cx: &mut TestAppContext) {
    let root = checkout("files-tabs-reorder");
    let (window, panel) = open(&root, cx);
    for path in ["README.md", "new.txt", "src/lib.rs"] {
        with(window, &panel, cx, |panel, window, cx| {
            panel.view(Place::file(path), window, cx);
        });
    }
    // The first tab dragged to the end: the one shown moves up a slot.
    with(window, &panel, cx, |panel, _, cx| panel.reorder(0, 2, cx));
    assert_eq!(tabs(&panel, cx), ["new.txt", "[src/lib.rs]", "README.md"]);
    // The one shown dragged to the front.
    with(window, &panel, cx, |panel, _, cx| panel.reorder(1, 0, cx));
    assert_eq!(tabs(&panel, cx), ["[src/lib.rs]", "new.txt", "README.md"]);
    // The last dragged before it: the one shown moves down a slot.
    with(window, &panel, cx, |panel, _, cx| panel.reorder(2, 0, cx));
    assert_eq!(tabs(&panel, cx), ["README.md", "[src/lib.rs]", "new.txt"]);
    assert_eq!(selected(&panel, cx).as_deref(), Some("src/lib.rs"));
    // Slots past the end, or the same slot, change nothing.
    with(window, &panel, cx, |panel, _, cx| panel.reorder(1, 3, cx));
    with(window, &panel, cx, |panel, _, cx| panel.reorder(1, 1, cx));
    assert_eq!(tabs(&panel, cx), ["README.md", "[src/lib.rs]", "new.txt"]);
    let _ = std::fs::remove_dir_all(&root);
}
