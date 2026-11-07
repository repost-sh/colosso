//! The browser tabs kept for the next launch: the file, what it holds as tabs open, move and
//! close, the tabs it brings back and where, and their pages, made only once shown.

use std::path::{Path, PathBuf};

use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};

use super::{KeptCheckout, KeptTab, SAVE_DELAY, encode, read_file};
use crate::browser::BrowserView;
use crate::model::{BranchUnit, PaneKind};
use crate::projects::read_worktrees;
use crate::shell::Shell;
use crate::test_support::{draw, repo, scratch, select, shell_with};

fn tab(url: &str, title: &str) -> KeptTab {
    KeptTab {
        url: url.into(),
        title: title.into(),
    }
}

fn kept(project: &Path, checkout: &Path, tabs: &[KeptTab]) -> KeptCheckout {
    KeptCheckout {
        project: project.to_owned(),
        checkout: checkout.to_owned(),
        tabs: tabs.to_vec(),
    }
}

/// The main checkout at `root` with one tab, at `url` titled `title`.
fn one(root: &Path, url: &str, title: &str) -> Vec<KeptCheckout> {
    vec![kept(root, root, &[tab(url, title)])]
}

/// A file for the tabs in a scratch folder of `name`'s, holding `checkouts`.
fn file(name: &str, checkouts: &[KeptCheckout]) -> PathBuf {
    let path = scratch(name).join("browser-tabs.json");
    if !checkouts.is_empty() {
        std::fs::write(&path, encode(checkouts).unwrap()).unwrap();
    }
    path
}

/// The shell keeps its tabs at `path`, read as at launch, and git lists `roots`' worktrees
/// again, as it does once the app has started.
fn launch(shell: &Entity<Shell>, path: &Path, roots: &[&Path], cx: &mut TestAppContext) {
    shell.update(cx, |shell, cx| {
        shell.keep_browser_tabs_at(path);
        for root in roots {
            shell.show_worktrees(root, read_worktrees(root), cx);
        }
    });
    cx.run_until_parked();
}

/// What the file holds once the write coming is done.
fn written(path: &Path, cx: &mut TestAppContext) -> Vec<KeptCheckout> {
    cx.run_until_parked();
    cx.executor().advance_clock(SAVE_DELAY);
    cx.run_until_parked();
    read_file(path)
}

/// Each browser pane of the checkout at `root`, in order: its title, the address it shows or
/// opens once shown, and whether its page is made.
fn browsers(
    shell: &Entity<Shell>,
    root: &Path,
    cx: &TestAppContext,
) -> Vec<(String, String, bool)> {
    cx.read(|cx| {
        (unit(shell.read(cx), root).tabs.iter())
            .flat_map(|tab| tab.panes())
            .filter(|pane| pane.kind == PaneKind::Browser)
            .map(|pane| {
                let address = pane.address(cx).unwrap_or_default();
                (pane.title.to_string(), address, pane.view().is_some())
            })
            .collect()
    })
}

fn title(title: &str, url: &str, made: bool) -> (String, String, bool) {
    (title.into(), url.into(), made)
}

/// Opens a browser tab at `url` on the selected checkout, or the start page with none: its
/// pane.
fn open(
    shell: &Entity<Shell>,
    window: AnyWindowHandle,
    url: Option<&str>,
    cx: &mut TestAppContext,
) -> u64 {
    let url = url.map(str::to_owned);
    let opened = cx.update_window(window, |_, window, cx| {
        shell.update(cx, |shell, cx| {
            shell.open_browser_tab(url, false, window, cx)
        })
    });
    opened.unwrap().expect("a checkout is selected")
}

/// Where the tab with pane `pane` is, as `(project, branch, tab)`.
fn at(shell: &Entity<Shell>, pane: u64, cx: &TestAppContext) -> (usize, usize, usize) {
    let found = cx.read(|cx| shell.read(cx).workspace.find_pane(|found| found.id == pane));
    found.expect("the pane is open").0
}

/// The page pane `pane` shows, once made.
fn view_of(shell: &Entity<Shell>, pane: u64, cx: &TestAppContext) -> Option<Entity<BrowserView>> {
    let at = at(shell, pane, cx);
    cx.read(|cx| {
        let tab = shell.read(cx).workspace.tab(at)?;
        let pane = tab.panes().into_iter().find(|found| found.id == pane)?;
        pane.view().cloned()
    })
}

/// The checkout at `root`.
fn unit<'a>(shell: &'a Shell, root: &Path) -> &'a BranchUnit {
    (shell.workspace.projects.iter())
        .flat_map(|project| &project.branches)
        .find(|unit| unit.worktree.path == root)
        .expect("the checkout is listed")
}

/// The panes of the checkout at `root`'s tabs, each tab's first.
fn tab_panes(shell: &Entity<Shell>, root: &Path, cx: &TestAppContext) -> Vec<u64> {
    cx.read(|cx| {
        (unit(shell.read(cx), root).tabs.iter())
            .map(|tab| tab.first().id)
            .collect()
    })
}

/// An editor tab on the selected checkout, shown: something there before the tabs come back.
fn editor_tab(shell: &Entity<Shell>, cx: &mut TestAppContext) {
    shell.update(cx, |shell, cx| {
        shell.workspace.new_tab(PaneKind::Editor);
        cx.notify();
    });
    cx.run_until_parked();
}

fn shown(shell: &Entity<Shell>, cx: &TestAppContext) -> Option<(usize, usize, usize)> {
    cx.read(|cx| shell.read(cx).workspace.shown_tab())
}

/// A checkout of `name`'s with an editor tab shown, and a tab at example.com brought back after
/// it: the window, the shell, the checkout, the file and the brought-back tab's pane.
fn brought_back(
    name: &str,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<Shell>, PathBuf, PathBuf, u64) {
    let root = repo(name, &[("README.md", "hi\n")]);
    let path = file(
        &format!("{name}-file"),
        &one(&root, "https://example.com/", "Example"),
    );
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    editor_tab(&shell, cx);
    launch(&shell, &path, &[&root], cx);
    let restored = tab_panes(&shell, &root, cx)[1];
    (window, shell, root, path, restored)
}

#[test]
fn the_file_reads_back_what_was_written_and_a_bad_one_is_no_tabs() {
    let app = Path::new("/src/app");
    let checkouts = [
        kept(
            app,
            app,
            &[
                tab("https://example.com/", "Example"),
                tab("http://localhost:3000/", ""),
            ],
        ),
        kept(
            app,
            "/src/app-feature".as_ref(),
            &[tab("https://example.org/", "Org")],
        ),
    ];
    let path = file("kept-tabs-file", &checkouts);
    assert_eq!(read_file(&path), checkouts);
    let json: serde_json::Value = crate::kept_file::read(&path).unwrap();
    assert_eq!(json["checkouts"][1]["checkout"], "/src/app-feature");
    assert_eq!(
        json["checkouts"][0]["tabs"][0]["url"],
        "https://example.com/"
    );

    std::fs::write(&path, "{ not json").unwrap();
    assert!(read_file(&path).is_empty());
    assert!(read_file(&path.with_file_name("none.json")).is_empty());
}

#[gpui_kit::test]
fn the_file_follows_the_tabs_as_they_open_navigate_and_close(cx: &mut TestAppContext) {
    let root = repo("kept-tabs-follow", &[("README.md", "hi\n")]);
    let path = file("kept-tabs-follow-file", &[]);
    let (window, shell) = shell_with(&[&root], cx);
    launch(&shell, &path, &[&root], cx);
    select(&shell, &root, cx);

    let page = open(&shell, window, Some("https://example.com/"), cx);
    let opened = one(&root, "https://example.com/", "example.com");
    assert_eq!(written(&path, cx), opened);

    // A tab with no address yet leaves nothing behind.
    open(&shell, window, None, cx);
    assert_eq!(written(&path, cx), opened);

    let view = view_of(&shell, page, cx).expect("the page is made as it opens");
    view.update(cx, |view, cx| {
        view.navigate("https://example.org/".into(), cx);
    });
    assert_eq!(
        written(&path, cx),
        one(&root, "https://example.org/", "example.org")
    );

    let page_at = at(&shell, page, cx);
    shell.update(cx, |shell, cx| shell.close_tab_now(page_at, cx));
    assert_eq!(written(&path, cx), [], "closed, it's gone");
    draw(window, cx);
}

#[gpui_kit::test]
fn tabs_come_back_after_those_there_in_order_without_taking_the_focus(cx: &mut TestAppContext) {
    let first = repo("kept-tabs-order-a", &[("README.md", "hi\n")]);
    let second = repo("kept-tabs-order-b", &[("README.md", "hi\n")]);
    let path = file(
        "kept-tabs-order-file",
        &[
            kept(
                &first,
                &first,
                &[
                    tab("https://example.com/", "Example"),
                    tab("https://example.org/", ""),
                ],
            ),
            kept(&second, &second, &[tab("https://example.net/", "Net")]),
        ],
    );
    let (window, shell) = shell_with(&[&first, &second], cx);
    select(&shell, &first, cx);
    editor_tab(&shell, cx);
    let before = shown(&shell, cx);
    launch(&shell, &path, &[&first, &second], cx);
    draw(window, cx);

    assert_eq!(shown(&shell, cx), before, "the tab shown stays shown");
    assert_eq!(
        browsers(&shell, &first, cx),
        [
            title("Example", "https://example.com/", false),
            title("example.org", "https://example.org/", false),
        ],
        "after the editor tab, in order, titled as they were, no page made"
    );
    assert_eq!(tab_panes(&shell, &first, cx).len(), 3);
    assert_eq!(
        browsers(&shell, &second, cx),
        [title("Net", "https://example.net/", false)]
    );

    // Listed again, a checkout's tabs don't come back twice.
    shell.update(cx, |shell, cx| {
        shell.show_worktrees(&first, read_worktrees(&first), cx);
    });
    cx.run_until_parked();
    assert_eq!(browsers(&shell, &first, cx).len(), 2);
}

#[gpui_kit::test]
fn a_tab_brought_back_opens_its_page_only_once_shown(cx: &mut TestAppContext) {
    let (window, shell, root, _, restored) = brought_back("kept-tabs-lazy", cx);
    draw(window, cx);
    assert!(
        view_of(&shell, restored, cx).is_none(),
        "not shown, no page"
    );

    let restored_at = at(&shell, restored, cx);
    shell.update(cx, |shell, cx| {
        let (project, branch, tab) = restored_at;
        shell.workspace.select_tab(project, branch, tab);
        cx.notify();
    });
    draw(window, cx);
    let view = view_of(&shell, restored, cx).expect("shown, its page is made");
    cx.read(|cx| {
        assert_eq!(view.read(cx).address(), "https://example.com/");
        assert!(!view.read(cx).is_start_page());
    });
    assert_eq!(
        browsers(&shell, &root, cx),
        [title("Example", "https://example.com/", true)],
        "titled as it was until the page says otherwise"
    );
    let unloaded = cx.read(|cx| {
        let tab = shell.read(cx).workspace.tab(restored_at);
        tab.map(|tab| tab.first().unloaded_address().is_some())
    });
    assert_eq!(unloaded, Some(false));
}

#[gpui_kit::test]
fn a_tab_brought_back_and_closed_unshown_reopens_at_its_address(cx: &mut TestAppContext) {
    let (window, shell, root, path, restored) = brought_back("kept-tabs-reopen", cx);
    let restored_at = at(&shell, restored, cx);
    shell.update(cx, |shell, cx| shell.close_tab_now(restored_at, cx));
    assert_eq!(written(&path, cx), [], "closed, it's gone from the file");

    let reopened = cx.update_window(window, |_, window, cx| {
        shell.update(cx, |shell, cx| shell.reopen_closed_tab(window, cx))
    });
    assert_eq!(reopened.unwrap(), Some(restored_at), "back where it was");
    draw(window, cx);
    let view = view_of(&shell, restored, cx).expect("reopened, it has its page");
    cx.read(|cx| assert_eq!(view.read(cx).address(), "https://example.com/"));
    assert_eq!(
        written(&path, cx),
        one(&root, "https://example.com/", "Example")
    );
}

#[gpui_kit::test]
fn tabs_wait_for_their_checkout_and_go_with_it_or_its_project(cx: &mut TestAppContext) {
    let root = repo("kept-tabs-wait", &[("README.md", "hi\n")]);
    let later = repo("kept-tabs-wait-later", &[("README.md", "hi\n")]);
    let gone = root.with_file_name("kept-tabs-wait-gone");
    let path = file(
        "kept-tabs-wait-file",
        &[
            kept(&root, &root, &[tab("https://example.com/", "Example")]),
            kept(&root, &gone, &[tab("https://example.org/", "Gone")]),
            kept(&later, &later, &[tab("https://example.net/", "Later")]),
        ],
    );
    let (window, shell) = shell_with(&[&root], cx);
    // The second project is in the sidebar, its worktrees not listed yet.
    shell.update(cx, |shell, _| {
        shell.workspace.add_project(later.clone());
    });
    select(&shell, &root, cx);
    launch(&shell, &path, &[&root], cx);
    let waiting = kept(&later, &later, &[tab("https://example.net/", "Later")]);
    assert_eq!(
        written(&path, cx),
        [
            kept(&root, &root, &[tab("https://example.com/", "Example")]),
            waiting.clone(),
        ],
        "a checkout its project lists no more goes; one not listed yet waits"
    );

    // A change elsewhere carries it along.
    open(&shell, window, Some("https://example.edu/"), cx);
    let carried = written(&path, cx);
    assert_eq!(carried.last(), Some(&waiting));
    assert_eq!(carried[0].tabs.len(), 2);

    // Listed, its tabs come back; its project removed, they go.
    shell.update(cx, |shell, cx| {
        shell.show_worktrees(&later, read_worktrees(&later), cx);
    });
    cx.run_until_parked();
    assert_eq!(
        browsers(&shell, &later, cx),
        [title("Later", "https://example.net/", false)]
    );
    shell.update(cx, |shell, cx| shell.remove_project(&later, cx));
    let left = written(&path, cx);
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].checkout, root);
}

#[gpui_kit::test]
fn a_change_still_settling_is_written_when_the_app_quits(cx: &mut TestAppContext) {
    let root = repo("kept-tabs-quit", &[("README.md", "hi\n")]);
    let path = file("kept-tabs-quit-file", &[]);
    let (window, shell) = shell_with(&[&root], cx);
    launch(&shell, &path, &[&root], cx);
    select(&shell, &root, cx);
    open(&shell, window, Some("https://example.com/"), cx);
    cx.run_until_parked();
    assert!(!path.exists(), "written a moment later");

    cx.quit();

    assert_eq!(
        read_file(&path),
        one(&root, "https://example.com/", "example.com")
    );
}
