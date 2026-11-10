//! The window coming back as the file has it: splits, order across kinds, the tab shown and
//! the checkout selected; nothing made until it's shown; and the panes whose terminal ended or
//! whose chat was deleted while the window was closed.

use std::path::Path;

use gpui_kit::{AppContext as _, TestAppContext};
use koyo_agent::protocol::ids::ThreadId;
use koyo_term_control::{Actor, ActorKind, Exit, Handle, TerminalRecord, TerminalSummary};

use super::{
    at, checkout, file, launch, main_with, page, read_file, split, tabs_of, titles, written,
};
use crate::model::{SplitAxis, Tab};
use crate::shell::kept::{Kept, KeptPane, KeptProject, KeptTree};
use crate::test_support::{draw, git, open_page, repo, select, shell_with, terminal_record};

#[gpui_kit::test]
fn the_window_comes_back_split_ordered_and_selected_as_it_was(cx: &mut TestAppContext) {
    let root = repo("kept-window-back", &[("README.md", "hi\n")]);
    let feature = root.with_file_name(format!("kept-window-back-feature-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&feature);
    let folder = feature.to_str().unwrap();
    git(&root, &["worktree", "add", "-q", "-b", "feature", folder]);
    let feature = std::fs::canonicalize(&feature).unwrap();
    let readme = feature.join("README.md");
    let kept = Kept {
        // The worktree, not the main checkout, was the one selected.
        projects: vec![KeptProject {
            project: root.clone(),
            checkout: feature.clone(),
        }],
        checkouts: vec![
            checkout(
                &root,
                &root,
                1,
                &[
                    page("https://example.com/", "Example"),
                    split(
                        SplitAxis::Column,
                        0.3,
                        KeptTree::Pane(KeptPane::NewChat),
                        page("http://localhost:3000/", "App"),
                    ),
                ],
            ),
            checkout(
                &root,
                &feature,
                1,
                &[
                    KeptTree::Pane(KeptPane::NewChat),
                    KeptTree::Pane(KeptPane::File { path: readme }),
                    KeptTree::Pane(KeptPane::Changes),
                    // A file gone since doesn't come back.
                    KeptTree::Pane(KeptPane::File {
                        path: feature.join("gone.rs"),
                    }),
                ],
            ),
        ],
    };
    let path = file("kept-window-back-file", &kept);
    let (window, shell) = shell_with(&[&root], cx);
    launch(&shell, &path, &[&root], cx);

    let selected = cx.read(|cx| {
        let unit = shell.read(cx).workspace.branch().expect("a checkout");
        unit.worktree.path.clone()
    });
    assert_eq!(selected, feature, "on the worktree it was on");
    assert_eq!(
        tabs_of(&shell, &root, cx),
        (titles(&[&["Example"], &["New chat", "App"]]), 1)
    );
    assert_eq!(
        tabs_of(&shell, &feature, cx),
        (titles(&[&["New chat"], &["README.md"], &["Changes"]]), 1)
    );
    cx.read(|cx| {
        let workspace = &shell.read(cx).workspace;
        let main = &workspace.projects[0].branches[workspace.unit_at(&root).unwrap().1];
        let Tab::Split(split) = &main.tabs[1] else {
            panic!("split as it was");
        };
        assert_eq!(split.axis, SplitAxis::Column);
        assert!((split.ratio - 0.3).abs() < f32::EPSILON);
        let made = (main.tabs.iter().flat_map(Tab::panes)).any(|pane| pane.view().is_some());
        assert!(!made, "no page is made until its tab is shown");
    });

    // The file shown opens as the window draws; the diffs once their tab is shown.
    draw(window, cx);
    let opened = |tab: usize, cx: &TestAppContext| {
        cx.read(|cx| {
            let unit = shell.read(cx).workspace.branch().expect("a checkout");
            let pane = unit.tabs[tab].first();
            (pane.file.is_some(), pane.changes.is_some(), pane.unopened.is_some())
        })
    };
    assert_eq!(opened(1, cx), (true, false, false), "the file it showed");
    assert_eq!(opened(2, cx), (false, false, true), "not shown, not opened");
    shell.update(cx, |shell, cx| {
        let (project, branch, _) = shell.workspace.shown_tab().unwrap();
        shell.workspace.select_tab(project, branch, 2);
        cx.notify();
    });
    draw(window, cx);
    assert_eq!(opened(2, cx), (false, true, false), "the checkout's diffs");

    // What's kept of it is what came back: all but the file that's gone, the diffs shown.
    let mut expected = kept;
    expected.checkouts[1].tabs.pop();
    expected.checkouts[1].shown = 2;
    assert_eq!(written(&path, cx), expected);
}

#[gpui_kit::test]
fn a_file_or_the_diffs_brought_back_is_found_rather_than_opened_again(cx: &mut TestAppContext) {
    let root = repo("kept-window-again", &[("README.md", "hi\n"), ("notes.md", "notes\n")]);
    let file = |name: &str| {
        KeptTree::Pane(KeptPane::File {
            path: root.join(name),
        })
    };
    let kept = main_with(
        &root,
        0,
        &[
            KeptTree::Pane(KeptPane::NewChat),
            file("README.md"),
            KeptTree::Pane(KeptPane::Changes),
        ],
    );
    let path = super::file("kept-window-again-file", &kept);
    let (window, shell) = shell_with(&[&root], cx);
    launch(&shell, &path, &[&root], cx);
    select(&shell, &root, cx);
    draw(window, cx);
    let before = titles(&[&["New chat"], &["README.md"], &["Changes"]]);
    assert_eq!(tabs_of(&shell, &root, cx), (before.clone(), 0));

    // The file opened again, as the tree or ⌘P opens it: its tab is shown, not another made.
    cx.update_window(window, |_, window, cx| {
        shell.update(cx, |shell, cx| {
            shell.view_file(crate::files::Place::file("README.md"), window, cx);
        });
    })
    .unwrap();
    draw(window, cx);
    assert_eq!(tabs_of(&shell, &root, cx), (before.clone(), 1));
    let opened = cx.read(|cx| {
        let pane = shell.read(cx).workspace.tab_shown().map(Tab::first);
        pane.is_some_and(|pane| pane.file.is_some() && pane.unopened.is_none())
    });
    assert!(opened, "the file it was brought back for");

    // And the diffs, as a file picked in the Changes panel shows them.
    cx.update_window(window, |_, window, cx| {
        shell.update(cx, |shell, cx| {
            let panel = shell.changes_panel(window, cx).expect("the changes panel");
            shell.show_changes(&panel, cx);
        });
    })
    .unwrap();
    draw(window, cx);
    assert_eq!(tabs_of(&shell, &root, cx), (before, 2));
    let diffs = cx.read(|cx| {
        let pane = shell.read(cx).workspace.tab_shown().map(Tab::first);
        pane.is_some_and(|pane| pane.changes.is_some())
    });
    assert!(diffs, "the checkout's diffs, in the tab brought back for them");
}

#[gpui_kit::test]
fn a_page_brought_back_opens_only_once_shown_and_reopens_at_its_address(cx: &mut TestAppContext) {
    let root = repo("kept-window-lazy", &[("README.md", "hi\n")]);
    let chat = KeptTree::Pane(KeptPane::NewChat);
    let kept = main_with(&root, 0, &[chat.clone(), page("https://example.com/", "Example")]);
    let path = file("kept-window-lazy-file", &kept);
    let (window, shell) = shell_with(&[&root], cx);
    launch(&shell, &path, &[&root], cx);
    select(&shell, &root, cx);
    draw(window, cx);
    let page_pane = cx.read(|cx| shell.read(cx).workspace.branch().unwrap().tabs[1].first().id);
    let view = |cx: &TestAppContext| {
        cx.read(|cx| {
            let unit = shell.read(cx).workspace.branch().unwrap();
            unit.pane(page_pane).and_then(|pane| pane.view().cloned())
        })
    };
    assert!(view(cx).is_none(), "not shown, no page");

    // Closed unshown, ⌘⇧T opens it again at its address.
    let page_at = at(&shell, page_pane, cx);
    shell.update(cx, |shell, cx| shell.close_tab_now(page_at, cx));
    assert_eq!(written(&path, cx), main_with(&root, 0, &[chat]));
    let reopened = cx.update_window(window, |_, window, cx| {
        shell.update(cx, |shell, cx| shell.reopen_closed_tab(window, cx))
    });
    assert_eq!(reopened.unwrap(), Some(page_at), "back where it was");
    draw(window, cx);
    let page = view(cx).expect("shown, its page is made");
    cx.read(|cx| {
        assert_eq!(page.read(cx).address(), "https://example.com/");
        assert!(!page.read(cx).is_start_page());
    });
    assert_eq!(read_file(&path).checkouts[0].tabs.len(), 1, "not written yet");
    let back = main_with(&root, 1, &kept.checkouts[0].tabs);
    assert_eq!(written(&path, cx), back, "kept again, and shown");
}

/// As the red button and the Dock do it: the window goes, its shell with it, and a new window
/// with a new shell opens while the app runs on (`main.rs`, `open_main_window`).
#[gpui_kit::test]
fn a_window_closed_and_opened_again_comes_back_as_it_was(cx: &mut TestAppContext) {
    let root = repo("kept-window-reopen", &[("README.md", "hi\n")]);
    let path = file("kept-window-reopen-file", &Kept::default());
    let (window, shell) = shell_with(&[&root], cx);
    launch(&shell, &path, &[&root], cx);
    select(&shell, &root, cx);
    let example = open_page(window, &shell, Some("https://example.com/"), cx);
    shell.update(cx, |shell, cx| {
        // An unsent chat right of the page, a quarter of the room the page's; then a tab
        // after them, and the split shown again.
        shell.new_tab(crate::model::PaneKind::Chat, cx);
        let unit = shell.workspace.branch_mut().unwrap();
        assert!(unit.drop_tab(1, example, crate::model::Edge::Right));
        unit.tabs[0].split_at_mut(&[]).unwrap().ratio = 0.25;
        cx.notify();
    });
    open_page(window, &shell, Some("https://example.org/"), cx);
    shell.update(cx, |shell, cx| {
        shell.workspace.select_tab(0, 0, 0);
        cx.notify();
    });
    cx.run_until_parked();
    let before = tabs_of(&shell, &root, cx);
    assert_eq!(
        before,
        (titles(&[&["example.com", "New chat"], &["example.org"]]), 0)
    );

    // Closed before the write a moment after the change: it's written as the shell goes.
    assert!(!path.exists());
    drop(shell);
    cx.update_window(window, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(read_file(&path).checkouts.len(), 1, "written on the way out");

    let (_, shell) = crate::test_support::in_window(
        gpui_kit::size(gpui_kit::px(1400.), gpui_kit::px(900.)),
        cx,
        |window, cx| cx.new(|cx| crate::shell::Shell::new(window, cx)),
    );
    shell.update(cx, |shell, _| {
        shell.workspace.add_project(root.clone());
    });
    launch(&shell, &path, &[&root], cx);
    assert_eq!(tabs_of(&shell, &root, cx), before);
    cx.read(|cx| {
        let unit = &shell.read(cx).workspace.projects[0].branches[0];
        let Tab::Split(split) = &unit.tabs[0] else {
            panic!("split as it was");
        };
        assert_eq!(split.axis, SplitAxis::Row);
        assert!((split.ratio - 0.25).abs() < f32::EPSILON);
        assert!(split.second.first().chat.is_none(), "the unsent chat, its tab kept");
    });
}

/// A terminal the daemon lists in the checkout at `worktree`, running.
fn terminal(handle: &str, name: &str, worktree: &Path) -> TerminalSummary {
    let at = worktree.to_str().expect("a path that's text");
    TerminalSummary {
        record: TerminalRecord {
            name: name.into(),
            ..terminal_record(handle, at, koyo_term_control::Status::Idle, "prompt")
        },
        preview: String::new(),
    }
}

fn kept_terminal(handle: &str, name: &str) -> KeptTree {
    KeptTree::Pane(KeptPane::Terminal {
        handle: Handle(handle.into()),
        name: name.into(),
    })
}

#[gpui_kit::test]
fn a_terminal_that_ended_leaves_its_pane_and_one_opened_meanwhile_gets_a_tab(
    cx: &mut TestAppContext,
) {
    let root = repo("kept-window-terminals", &[("README.md", "hi\n")]);
    let example = page("https://example.com/", "Example");
    let kept = main_with(
        &root,
        0,
        &[
            split(SplitAxis::Row, 0.5, kept_terminal("term_a", "zsh"), example.clone()),
            split(SplitAxis::Row, 0.5, example.clone(), kept_terminal("term_gone", "dev")),
            kept_terminal("term_gone_too", "tests"),
        ],
    );
    let path = file("kept-window-terminals-file", &kept);
    let (_, shell) = shell_with(&[&root], cx);
    launch(&shell, &path, &[&root], cx);
    assert_eq!(
        tabs_of(&shell, &root, cx).0,
        titles(&[&["zsh", "Example"], &["Example", "dev"], &["tests"]]),
        "where they were, until the daemon says"
    );

    // The daemon runs one of them still, and one the window didn't know of.
    shell.update(cx, |shell, cx| {
        let listed = vec![terminal("term_a", "zsh", &root), terminal("term_new", "build", &root)];
        shell.take_terminals(std::slice::from_ref(&root), listed, cx);
    });
    cx.run_until_parked();
    assert_eq!(
        tabs_of(&shell, &root, cx).0,
        titles(&[&["zsh", "Example"], &["Example"], &["build"]]),
        "a split keeps its other side, and what's new comes after"
    );
    let shows = cx.read(|cx| {
        let shell = shell.read(cx);
        let pane = shell.workspace.branch().unwrap().tabs[0].first();
        let kept = shell.terminals.kept(pane.terminal.unwrap());
        kept.map(|(handle, name)| (handle.0.clone(), name.to_owned()))
    });
    assert_eq!(shows, Some(("term_a".to_owned(), "zsh".to_owned())));
    let now = written(&path, cx);
    assert_eq!(
        now.checkouts[0].tabs,
        [
            split(SplitAxis::Row, 0.5, kept_terminal("term_a", "zsh"), example.clone()),
            example,
            kept_terminal("term_new", "build"),
        ]
    );
}

#[gpui_kit::test]
fn a_shell_a_daemon_restart_ended_runs_again_in_the_pane_it_had(cx: &mut TestAppContext) {
    let root = repo("kept-window-restart", &[("README.md", "hi\n")]);
    let example = page("https://example.com/", "Example");
    let beside = split(SplitAxis::Row, 0.5, kept_terminal("term_shell", "zsh"), example);
    let path = file("kept-window-restart-file", &main_with(&root, 0, &[beside]));
    let (_, shell) = shell_with(&[&root], cx);
    launch(&shell, &path, &[&root], cx);
    // The daemon came back after a restart: the person's shell is listed as ended by it.
    shell.update(cx, |shell, cx| {
        let mut ended = terminal("term_shell", "zsh", &root);
        ended.record.created_by = Actor {
            kind: ActorKind::Human,
            id: "app".into(),
            label: "you".into(),
        };
        ended.record.exit = Some(Exit {
            code: None,
            signal: None,
            daemon_restart: true,
        });
        shell.take_terminals(std::slice::from_ref(&root), vec![ended], cx);
    });
    cx.run_until_parked();
    assert_eq!(
        tabs_of(&shell, &root, cx).0,
        titles(&[&["zsh", "Example"]]),
        "in the split it was in"
    );
    let runs_again = cx.read(|cx| {
        let shell = shell.read(cx);
        let pane = shell.workspace.branch().unwrap().tabs[0].first();
        // A shell to open under its name has no handle until the daemon opens it.
        shell.terminals.kept(pane.terminal.unwrap()).is_none()
    });
    assert!(runs_again, "a shell run again, not the one that ended");
}

#[gpui_kit::test]
fn a_chat_deleted_leaves_its_pane_and_one_made_meanwhile_gets_a_tab(cx: &mut TestAppContext) {
    let root = repo("kept-window-chats", &[("README.md", "hi\n")]);
    let chat = |thread: &str, title: &str| {
        KeptTree::Pane(KeptPane::Chat {
            thread: ThreadId(thread.into()),
            title: title.into(),
        })
    };
    let new_chat = KeptTree::Pane(KeptPane::NewChat);
    let kept = main_with(
        &root,
        1,
        &[
            split(SplitAxis::Row, 0.5, chat("0002", "Deleted"), new_chat),
            chat("0001", "Plan"),
        ],
    );
    let path = file("kept-window-chats-file", &kept);
    let (_, shell) = shell_with(&[&root], cx);
    launch(&shell, &path, &[&root], cx);
    assert_eq!(
        tabs_of(&shell, &root, cx),
        (titles(&[&["Deleted", "New chat"], &["Plan"]]), 1),
        "titled as they were, until the host says"
    );

    let summary = |id: &str, title: &str| crate::chat::tests::summary(id, &root, title, true);
    // The host has the one still, renamed, and one made while the window was closed.
    let chats = shell.read_with(cx, |shell, _| shell.chats().clone());
    chats.update(cx, |chats, cx| {
        chats.hear_list(vec![summary("0001", "The plan"), summary("0003", "Made since")], cx);
    });
    cx.run_until_parked();
    assert_eq!(
        tabs_of(&shell, &root, cx),
        (titles(&[&["New chat"], &["The plan"], &["Made since"]]), 1),
        "the unsent chat keeps its tab; the tab shown stays shown"
    );
}
