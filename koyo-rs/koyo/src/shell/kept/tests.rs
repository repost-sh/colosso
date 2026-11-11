//! The window kept for the next one: the file, what it holds as tabs open, split, move and
//! close, what waits for a checkout not listed yet, and the browser tabs an earlier build kept.
//! `back.rs` has the window coming back.

use std::path::{Path, PathBuf};

use gpui_kit::{Entity, TestAppContext};

use super::{Kept, KeptCheckout, KeptPane, KeptProject, KeptTree, SAVE_DELAY, read_file};
use crate::model::{Edge, PaneKind, SplitAxis};
use crate::projects::read_worktrees;
use crate::shell::Shell;
use crate::test_support::{draw, open_page, repo, scratch, select, shell_with};

mod back;

fn page(url: &str, title: &str) -> KeptTree {
    KeptTree::Pane(KeptPane::Page {
        url: url.into(),
        title: title.into(),
    })
}

fn split(axis: SplitAxis, ratio: f32, first: KeptTree, second: KeptTree) -> KeptTree {
    KeptTree::Split {
        axis,
        ratio,
        first: Box::new(first),
        second: Box::new(second),
    }
}

fn checkout(project: &Path, checkout: &Path, shown: usize, tabs: &[KeptTree]) -> KeptCheckout {
    KeptCheckout {
        project: project.to_owned(),
        checkout: checkout.to_owned(),
        shown,
        tabs: tabs.to_vec(),
    }
}

/// The main checkout at `root`, selected in its project, with `tabs`, the one at `shown` shown.
fn main_with(root: &Path, shown: usize, tabs: &[KeptTree]) -> Kept {
    Kept {
        projects: vec![KeptProject {
            project: root.to_owned(),
            checkout: root.to_owned(),
        }],
        checkouts: vec![checkout(root, root, shown, tabs)],
    }
}

/// Tabs by their panes' titles, as [`tabs_of`] gives them.
fn titles(tabs: &[&[&str]]) -> Vec<Vec<String>> {
    (tabs.iter())
        .map(|tab| tab.iter().map(|title| (*title).to_owned()).collect())
        .collect()
}

/// A file for the window in a scratch folder of `name`'s, holding `kept` if there's any.
fn file(name: &str, kept: &Kept) -> PathBuf {
    let path = scratch(name).join("window.json");
    if *kept != Kept::default() {
        std::fs::write(&path, crate::kept_file::encode(kept).unwrap()).unwrap();
    }
    path
}

/// The shell keeps its window at `path`, read as at launch, and git lists `roots`' worktrees
/// again, as it does once the app has started.
fn launch(shell: &Entity<Shell>, path: &Path, roots: &[&Path], cx: &mut TestAppContext) {
    shell.update(cx, |shell, cx| {
        shell.keep_window_at(path);
        for root in roots {
            shell.show_worktrees(root, read_worktrees(root), cx);
        }
    });
    cx.run_until_parked();
}

/// What the file holds once the write coming is done.
fn written(path: &Path, cx: &mut TestAppContext) -> Kept {
    cx.run_until_parked();
    cx.executor().advance_clock(SAVE_DELAY);
    cx.run_until_parked();
    read_file(path)
}

/// Where the tab with pane `pane` is, as `(project, branch, tab)`.
fn at(shell: &Entity<Shell>, pane: u64, cx: &TestAppContext) -> (usize, usize, usize) {
    let found = cx.read(|cx| shell.read(cx).workspace.find_pane(|found| found.id == pane));
    found.expect("the pane is open").0
}

/// The tabs of the checkout at `root`, each by its panes' titles, and the place of the one
/// shown.
fn tabs_of(shell: &Entity<Shell>, root: &Path, cx: &TestAppContext) -> (Vec<Vec<String>>, usize) {
    cx.read(|cx| {
        let workspace = &shell.read(cx).workspace;
        let (project, branch) = workspace.unit_at(root).expect("the checkout is listed");
        let unit = &workspace.projects[project].branches[branch];
        let titles = |tab: &crate::model::Tab| {
            (tab.panes().into_iter())
                .map(|pane| pane.title.to_string())
                .collect()
        };
        (unit.tabs.iter().map(titles).collect(), unit.active_tab)
    })
}

#[test]
fn the_file_reads_back_what_was_written_and_a_bad_one_is_nothing() {
    let app = Path::new("/src/app");
    let feature = Path::new("/src/app-feature");
    let terminal = KeptTree::Pane(KeptPane::Terminal {
        handle: koyo_term_control::Handle("term_01".into()),
        name: "zsh".into(),
    });
    let chat = KeptTree::Pane(KeptPane::Chat {
        thread: koyo_agent::protocol::ids::ThreadId("0001".into()),
        title: "Plan".into(),
    });
    let kept = Kept {
        projects: vec![KeptProject {
            project: app.to_owned(),
            checkout: feature.to_owned(),
        }],
        checkouts: vec![
            checkout(
                app,
                app,
                1,
                &[
                    page("https://example.com/", "Example"),
                    split(SplitAxis::Row, 0.3, terminal, chat),
                ],
            ),
            checkout(
                app,
                feature,
                0,
                &[
                    KeptTree::Pane(KeptPane::NewChat),
                    KeptTree::Pane(KeptPane::File {
                        path: "/src/app-feature/README.md".into(),
                    }),
                    KeptTree::Pane(KeptPane::Changes),
                ],
            ),
        ],
    };
    let path = file("kept-window-file", &kept);
    assert_eq!(read_file(&path), kept);
    let json: serde_json::Value = crate::kept_file::read(&path).unwrap();
    assert_eq!(json["projects"][0]["checkout"], "/src/app-feature");
    let tabs = &json["checkouts"][0]["tabs"];
    assert_eq!(tabs[0]["kind"], "page");
    assert_eq!(tabs[0]["url"], "https://example.com/");
    assert_eq!(tabs[1]["axis"], "row");
    assert_eq!(tabs[1]["first"]["handle"], "term_01");
    assert_eq!(tabs[1]["second"]["thread"], "0001");
    assert_eq!(json["checkouts"][1]["tabs"][0]["kind"], "newChat");

    // A kind a later build keeps is read as one this build brings nothing back for.
    let later =
        r#"{"checkouts":[{"project":"/a","checkout":"/a","tabs":[{"kind":"canvas","board":7}]}]}"#;
    std::fs::write(&path, later).unwrap();
    let read = read_file(&path);
    assert_eq!(read.checkouts[0].tabs, [KeptTree::Pane(KeptPane::Unknown)]);
    assert_eq!(read.checkouts[0].shown, 0);

    std::fs::write(&path, "{ not json").unwrap();
    assert_eq!(read_file(&path), Kept::default());
    assert_eq!(
        read_file(&path.with_file_name("none.json")),
        Kept::default()
    );
}

#[gpui_kit::test]
fn the_file_follows_the_window_as_tabs_open_split_resize_and_close(cx: &mut TestAppContext) {
    let root = repo("kept-window-follow", &[("README.md", "hi\n")]);
    let path = file("kept-window-follow-file", &Kept::default());
    let (window, shell) = shell_with(&[&root], cx);
    launch(&shell, &path, &[&root], cx);
    select(&shell, &root, cx);

    let example = open_page(window, &shell, Some("https://example.com/"), cx);
    shell.update(cx, |shell, cx| shell.new_tab(PaneKind::Chat, cx));
    let docs = page("https://example.com/", "example.com");
    let new_chat = KeptTree::Pane(KeptPane::NewChat);
    assert_eq!(
        written(&path, cx),
        main_with(&root, 1, &[docs.clone(), new_chat.clone()]),
        "in order, a chat with nothing sent yet among them, the one shown noted"
    );

    // The chat goes right of the page: one tab, split.
    shell.update(cx, |shell, cx| {
        let unit = shell.workspace.branch_mut().unwrap();
        assert!(unit.drop_tab(1, example, Edge::Right));
        cx.notify();
    });
    let halves = split(SplitAxis::Row, 0.5, docs.clone(), new_chat.clone());
    assert_eq!(written(&path, cx), main_with(&root, 0, &[halves]));
    shell.update(cx, |shell, cx| {
        let unit = shell.workspace.branch_mut().unwrap();
        unit.tabs[0].split_at_mut(&[]).unwrap().ratio = 0.25;
        cx.notify();
    });
    let quarter = split(SplitAxis::Row, 0.25, docs, new_chat);
    let resized = main_with(&root, 0, std::slice::from_ref(&quarter));
    assert_eq!(written(&path, cx), resized);

    // A new tab's start page is a tab too, with no address.
    let start = open_page(window, &shell, None, cx);
    assert_eq!(
        written(&path, cx),
        main_with(&root, 1, &[quarter.clone(), page("", "New tab")])
    );
    let start_at = at(&shell, start, cx);
    shell.update(cx, |shell, cx| shell.close_tab_now(start_at, cx));
    assert_eq!(written(&path, cx), main_with(&root, 0, &[quarter]));

    // A pane closed leaves its split to the other side; the last tab closed, none is kept.
    shell.update(cx, |shell, cx| shell.close_pane_now(example, cx));
    let alone = main_with(&root, 0, &[KeptTree::Pane(KeptPane::NewChat)]);
    assert_eq!(written(&path, cx), alone);
    shell.update(cx, |shell, cx| shell.close_tab_now((0, 0, 0), cx));
    let none = written(&path, cx);
    assert!(none.checkouts.is_empty(), "closed, it's gone");
    assert_eq!(none.projects.len(), 1, "its project's checkout still noted");
    draw(window, cx);
}

#[gpui_kit::test]
fn tabs_wait_for_their_checkout_and_go_with_it_or_its_project(cx: &mut TestAppContext) {
    let root = repo("kept-window-wait", &[("README.md", "hi\n")]);
    let later = repo("kept-window-wait-later", &[("README.md", "hi\n")]);
    let gone = root.with_file_name("kept-window-wait-gone");
    let example = page("https://example.com/", "Example");
    let waiting = checkout(&later, &later, 0, &[page("https://example.net/", "Later")]);
    let kept = Kept {
        projects: vec![KeptProject {
            project: later.clone(),
            checkout: later.clone(),
        }],
        checkouts: vec![
            checkout(&root, &root, 0, std::slice::from_ref(&example)),
            checkout(&root, &gone, 0, &[page("https://example.org/", "Gone")]),
            waiting.clone(),
        ],
    };
    let path = file("kept-window-wait-file", &kept);
    let (window, shell) = shell_with(&[&root], cx);
    // The second project is in the sidebar, its worktrees not listed yet.
    shell.update(cx, |shell, _| {
        shell.workspace.add_project(later.clone());
    });
    select(&shell, &root, cx);
    launch(&shell, &path, &[&root], cx);
    let now = written(&path, cx);
    assert_eq!(
        now.checkouts,
        [checkout(&root, &root, 0, &[example]), waiting.clone()],
        "a checkout its project lists no more goes; one not listed yet waits"
    );
    assert!(
        now.projects.contains(&kept.projects[0]),
        "and so does what its project had selected"
    );

    // A change elsewhere carries it along.
    open_page(window, &shell, Some("https://example.edu/"), cx);
    let carried = written(&path, cx);
    assert_eq!(carried.checkouts.last(), Some(&waiting));
    assert_eq!(carried.checkouts[0].tabs.len(), 2);

    // Listed, its tabs come back; its project removed, they go.
    shell.update(cx, |shell, cx| {
        shell.show_worktrees(&later, read_worktrees(&later), cx);
    });
    cx.run_until_parked();
    assert_eq!(tabs_of(&shell, &later, cx), (titles(&[&["Later"]]), 0));
    shell.update(cx, |shell, cx| shell.remove_project(&later, cx));
    let left = written(&path, cx);
    assert_eq!(left.checkouts.len(), 1);
    assert_eq!(left.checkouts[0].checkout, root);
    assert_eq!(left.projects.len(), 1);
}

#[gpui_kit::test]
fn a_change_still_settling_is_written_when_the_app_quits(cx: &mut TestAppContext) {
    let root = repo("kept-window-quit", &[("README.md", "hi\n")]);
    let path = file("kept-window-quit-file", &Kept::default());
    let (window, shell) = shell_with(&[&root], cx);
    launch(&shell, &path, &[&root], cx);
    select(&shell, &root, cx);
    open_page(window, &shell, Some("https://example.com/"), cx);
    cx.run_until_parked();
    assert!(!path.exists(), "written a moment later");

    cx.quit();

    assert_eq!(
        read_file(&path),
        main_with(&root, 0, &[page("https://example.com/", "example.com")])
    );
}

#[gpui_kit::test]
fn the_browser_tabs_an_earlier_build_kept_come_back_the_first_time(cx: &mut TestAppContext) {
    let root = repo("kept-window-before", &[("README.md", "hi\n")]);
    let path = scratch("kept-window-before-file").join("window.json");
    let before = path.with_file_name("browser-tabs.json");
    let tabs = serde_json::json!({ "checkouts": [{
        "project": root, "checkout": root,
        "tabs": [
            { "url": "https://example.com/", "title": "Example" },
            { "url": "http://localhost:3000/" },
        ],
    }] });
    std::fs::write(&before, tabs.to_string()).unwrap();
    let (window, shell) = shell_with(&[&root], cx);
    launch(&shell, &path, &[&root], cx);
    assert_eq!(
        tabs_of(&shell, &root, cx),
        (titles(&[&["Example"], &["localhost:3000"]]), 0),
        "each a tab of its own, as they came back then"
    );
    // From then on the window's own file has them, and the old one is left as it was.
    select(&shell, &root, cx);
    open_page(window, &shell, Some("https://example.org/"), cx);
    let now = written(&path, cx);
    assert_eq!(now.checkouts[0].tabs.len(), 3);
    assert!(before.exists());
    shell.update(cx, |shell, cx| {
        shell.keep_window_at(&path);
        shell.show_worktrees(&root, read_worktrees(&root), cx);
    });
    assert_eq!(
        read_file(&path),
        now,
        "read from its own file, not the old one"
    );
}

#[gpui_kit::test]
fn a_device_s_tab_is_kept_by_its_device_and_the_picker_s_isnt(cx: &mut TestAppContext) {
    let root = repo("kept-window-device", &[("README.md", "hi\n")]);
    let path = file("kept-window-device-file", &Kept::default());
    let (window, shell) = shell_with(&[&root], cx);
    launch(&shell, &path, &[&root], cx);
    select(&shell, &root, cx);
    let chosen = crate::devices::Chosen {
        udid: "UDID-K".into(),
        name: "iPhone 17 Pro".into(),
        ipad: false,
        phone: false,
    };
    shell.update(cx, |shell, cx| {
        shell.device_pane(Some(chosen), true, cx);
        shell.device_pane(None, true, cx);
    });
    let device = KeptTree::Pane(KeptPane::Device {
        udid: "UDID-K".into(),
        name: "iPhone 17 Pro".into(),
        ipad: false,
        phone: false,
    });
    assert_eq!(written(&path, cx), main_with(&root, 0, &[device]));
    draw(window, cx);
}

/// QU7: a device's tab brought back starts nothing, a simulator being gigabytes; its Start
/// starts it (here, with no host under test, the start is tried and fails).
#[gpui_kit::test]
fn a_device_s_tab_brought_back_starts_nothing_until_asked(cx: &mut TestAppContext) {
    let root = repo("kept-window-device-back", &[("README.md", "hi\n")]);
    let device = KeptTree::Pane(KeptPane::Device {
        udid: "UDID-BACK".into(),
        name: "iPhone 17 Pro".into(),
        ipad: false,
        phone: false,
    });
    let path = file("kept-window-device-back-file", &main_with(&root, 0, &[device]));
    let (window, shell) = shell_with(&[&root], cx);
    launch(&shell, &path, &[&root], cx);
    select(&shell, &root, cx);
    assert_eq!(tabs_of(&shell, &root, cx), (titles(&[&["iPhone 17 Pro"]]), 0));
    let view = cx.read(|cx| shell.read(cx).shown_device_view(cx)).expect("the device's tab");
    let link = cx.read(|cx| view.read(cx).link().cloned()).expect("its link");
    assert_eq!(cx.read(|cx| link.read(cx).state.clone()), crate::devices::State::Off);
    // The tab's Start, as the person presses it: nothing else runs, so it starts at once.
    (gpui_kit::AppContext::update_window(cx, window, |_, window, cx| view.update(cx, |view, cx| view.start_asking(window, cx)))).expect("the window");
    let state = cx.read(|cx| link.read(cx).state.clone());
    assert!(matches!(state, crate::devices::State::Failed(_)), "{state:?}");
}
