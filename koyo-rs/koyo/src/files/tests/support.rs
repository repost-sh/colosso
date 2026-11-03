//! What the files' tests start from and look at: a checkout to list, the tree alone in a
//! window or in a shell, files opened in tabs and typed into, the tabs as they're shown,
//! keys pressed and the question about unsaved edits answered.

use std::path::Path;
use std::time::{Duration, Instant};

use koyo_project::Feed;
use gpui_kit::component::input::EditorState;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Context, Entity, TestAppContext, Window, px, size,
};

use super::super::closing::SaveDialog;
use super::super::{At, FileEditor, FilePane, FilesPanel, Place};
use crate::checkout::{Checkout, Document};
use crate::modal;
use crate::right_panel;
use crate::settings;
use crate::shell::Shell;
use crate::test_support::{git, in_window, scratch, select, shell_with, write};

/// A checkout with a change and a file git doesn't know, and an ignored build folder.
pub(crate) fn checkout(name: &str) -> std::path::PathBuf {
    let root = scratch(name);
    git(&root, &["init", "-q", "-b", "main"]);
    write(&root, ".gitignore", b"target/\n");
    write(&root, "README.md", b"# Test\n");
    write(&root, "src/lib.rs", b"pub fn a() {}\n");
    write(&root, "src/main.rs", b"fn main() {}\n");
    write(&root, "src/nested/deep.rs", b"// deep\n");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "first"]);
    write(&root, "src/lib.rs", b"pub fn a() {}\npub fn b() {}\n");
    write(&root, "new.txt", b"new\n");
    write(&root, "target/debug/app", b"\0binary");
    root
}

/// The panel for the checkout at `root`, in a window of its own.
pub(crate) fn open(root: &Path, cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<FilesPanel>) {
    // The project lists and watches the checkout on a thread of its own, which wakes the
    // panel's task from outside the test's scheduler.
    cx.executor().allow_parking();
    let root = root.to_owned();
    cx.update(gpui_kit::init);
    cx.update(settings::init);
    cx.update(right_panel::init);
    let (window, panel) = in_window(size(px(320.), px(600.)), cx, |window, cx| {
        let checkout = cx.new(|cx| Checkout::new(root, window, cx));
        cx.new(|cx| FilesPanel::new(checkout, cx))
    });
    settle(&panel, cx, |panel| panel.scanned);
    (window, panel)
}

/// Lets the panel take what the checkout's project publishes until it's `done`, waiting
/// for the project between looks.
pub(crate) fn settle(
    panel: &Entity<FilesPanel>,
    cx: &mut TestAppContext,
    done: impl Fn(&FilesPanel) -> bool,
) {
    // The checkout opens git, and with it the project the panel lists.
    cx.run_until_parked();
    let project = cx.read(|cx| panel.read(cx).checkout.read(cx).project().cloned());
    // Subscribed before looking, so whatever's published after the look wakes the wait.
    let updates = project
        .expect("the checkout opened")
        .subscribe(Feed::Everything);
    // The checkouts of the tests running at once each restart the process's one file
    // watcher, which takes the platform a while each time.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| done(panel.read(cx))) {
            return;
        }
        let left = deadline.checked_duration_since(Instant::now());
        updates.next_within(left.expect("the panel never got there"));
    }
}

/// The rows shown, indented by depth, a folder's name ending in `/`.
pub(crate) fn shown(panel: &Entity<FilesPanel>, cx: &TestAppContext) -> Vec<String> {
    cx.read(|cx| {
        (panel.read(cx).rows.rows().iter())
            .map(|row| {
                let slash = if row.entry.is_folder() { "/" } else { "" };
                format!(
                    "{}{}{slash}",
                    "  ".repeat(row.depth as usize),
                    row.entry.name
                )
            })
            .collect()
    })
}

pub(crate) fn selected(panel: &Entity<FilesPanel>, cx: &TestAppContext) -> Option<String> {
    cx.read(|cx| panel.read(cx).selected.as_ref().map(ToString::to_string))
}

/// Changes `panel` in its `window`.
pub(crate) fn with(
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

/// Changes `shell` in its `window`.
pub(crate) fn with_shell(
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    cx: &mut TestAppContext,
    change: impl FnOnce(&mut Shell, &mut Window, &mut Context<Shell>),
) {
    (cx.update_window(window, |_, window, cx| {
        shell.update(cx, |shell, cx| change(shell, window, cx));
    }))
    .unwrap();
}

/// Shows the file at `path` in a tab with the cursor `at` a place, and what's behind the
/// tab: its view, the editor in it, and its document.
pub(crate) fn editing(
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    path: &str,
    at: At,
    cx: &mut TestAppContext,
) -> (Entity<FileEditor>, Entity<EditorState>, Entity<Document>) {
    with_shell(window, shell, cx, |shell, window, cx| {
        let place = Place {
            path: path.into(),
            at: Some(at),
        };
        shell.view_file(place, window, cx);
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let file = shell.read(cx).viewing().unwrap();
        let file = file.read(cx);
        let editor = file.editor.read(cx).editor().unwrap().clone();
        let document = file.editor.read(cx).document().expect("its document");
        (file.editor.clone(), editor, document)
    })
}

/// The tabs open on the selected checkout, each by its panes' titles joined with `|`, a
/// preview's ending in `*`, a file with unsaved edits marked `+`, and the one shown in
/// brackets.
pub(crate) fn tabs(shell: &Entity<Shell>, cx: &TestAppContext) -> Vec<String> {
    cx.read(|cx| {
        let shell = shell.read(cx);
        let Some(unit) = shell.workspace.branch() else {
            return Vec::new();
        };
        (unit.tabs.iter().enumerate())
            .map(|(at, tab)| {
                let panes: Vec<String> = (tab.panes().into_iter())
                    .map(|pane| {
                        let file = pane.file.as_ref().map(|file| file.read(cx));
                        let preview = if file.is_some_and(FilePane::is_preview) { "*" } else { "" };
                        let dirty = if file.is_some_and(FilePane::is_dirty) { "+" } else { "" };
                        format!("{}{preview}{dirty}", pane.title)
                    })
                    .collect();
                let name = panes.join("|");
                if at == unit.active_tab { format!("[{name}]") } else { name }
            })
            .collect()
    })
}

/// Types `text` into `editor` at its cursor, and lets the typing be heard.
pub(crate) fn typed(
    window: AnyWindowHandle,
    editor: &Entity<EditorState>,
    text: &str,
    cx: &mut TestAppContext,
) {
    (cx.update_window(window, |_, window, cx| {
        editor.update(cx, |editor, cx| editor.insert(text.to_owned(), window, cx));
    }))
    .unwrap();
    cx.run_until_parked();
}

/// A shell over the checkout at `root`, its Files panel open and listed and README.md
/// shown in a tab: the window, the shell and the panel.
pub(crate) fn shell_showing(
    root: &Path,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<Shell>, Entity<FilesPanel>) {
    let (window, shell, files) = shell_with_tree(root, cx);
    with_shell(window, &shell, cx, |shell, window, cx| {
        shell.view_file(Place::file("README.md"), window, cx);
    });
    cx.run_until_parked();
    (window, shell, files)
}

/// A shell over the checkout at `root`, its Files panel open and listed, no file open: the
/// window, the shell and the panel.
pub(crate) fn shell_with_tree(
    root: &Path,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<Shell>, Entity<FilesPanel>) {
    let (window, shell) = shell_with(&[root], cx);
    select(&shell, root, cx);
    cx.dispatch_action(window, right_panel::ShowFiles);
    cx.run_until_parked();
    let files = cx.read(|cx| shell.read(cx).right_panel.files().cloned()).expect("the Files panel");
    settle(&files, cx, |files| files.scanned);
    (window, shell, files)
}

/// Presses `keys` in `window`, drawn first so the keyboard is where the last change left it.
pub(crate) fn press(window: AnyWindowHandle, keys: &str, cx: &mut TestAppContext) {
    (cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))).unwrap();
    cx.simulate_keystrokes(window, keys);
}

/// The dialog on screen, if one is and it shows a `T`: its window and the `T`.
pub(crate) fn dialog<T: 'static>(cx: &mut TestAppContext) -> Option<(AnyWindowHandle, Entity<T>)> {
    cx.read(|cx| Some((modal::window(cx)?, modal::current::<T>(cx)?)))
}

/// Answers the question asked with `act`, keeping no hold on the dialog, which goes with its
/// window.
pub(in crate::files) fn answer(
    cx: &mut TestAppContext,
    act: fn(&mut SaveDialog, &mut Window, &mut Context<SaveDialog>),
) {
    let (dialog_window, dialog) = dialog::<SaveDialog>(cx).expect("asked");
    (cx.update_window(dialog_window, |_, window, cx| {
        dialog.update(cx, |dialog, cx| act(dialog, window, cx));
    }))
    .unwrap();
    cx.run_until_parked();
}
