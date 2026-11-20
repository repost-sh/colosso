//! Each kind of pane kept as what it shows, and brought back showing it: a page, an agent's
//! page as an agent's with the history and scroll it had, a terminal by its handle, a chat by its thread and one with nothing
//! sent, a file, the diffs and a device; never the agent desktop or a page's `DevTools`. A file
//! and the diffs not opened yet are kept as what they will open.

use gpui_kit::TestAppContext;
use koyo_agent::protocol::ids::ThreadId;
use koyo_term_control::Handle;

use super::{file, launch, main_with, page, written};
use crate::browser::Dock;
use crate::model::{Pane, PaneKind};
use crate::shell::kept::{KeptPane, KeptTree};
use crate::test_support::{draw, repo, select, shell_with, terminals_listed};

#[gpui_kit::test]
fn each_kind_of_pane_is_kept_as_what_it_shows_and_comes_back_so(cx: &mut TestAppContext) {
    let root = repo("kept-window-kinds", &[("README.md", "hi\n")]);
    let tabs = [
        page("https://example.com/", "Example"),
        KeptTree::Pane(KeptPane::Page {
            url: "https://agent.example/".into(),
            title: "Agent".into(),
            by_agent: true,
            session: koyo_browser_ipc::Session {
                back: vec![koyo_browser_ipc::SessionEntry {
                    url: "https://agent.example/start".into(),
                    title: "Start".into(),
                }],
                forward: Vec::new(),
                scroll: koyo_browser_ipc::Scroll { x: 0, y: 300 },
            },
        }),
        KeptTree::Pane(KeptPane::Terminal {
            handle: Handle("term_kinds".into()),
            name: "zsh".into(),
        }),
        KeptTree::Pane(KeptPane::Chat {
            thread: ThreadId("0001".into()),
            title: "Plan".into(),
        }),
        KeptTree::Pane(KeptPane::NewChat),
        KeptTree::Pane(KeptPane::File {
            path: root.join("README.md"),
        }),
        KeptTree::Pane(KeptPane::Changes),
        KeptTree::Pane(KeptPane::Device {
            udid: "UDID-KINDS".into(),
            name: "iPhone 17 Pro".into(),
            ipad: false,
            phone: false,
        }),
    ];
    let kept = main_with(&root, 0, &tabs);
    let path = file("kept-window-kinds-file", &kept);
    let (window, shell) = shell_with(&[&root], cx);
    launch(&shell, &path, &[&root], cx);
    select(&shell, &root, cx);
    terminals_listed(&shell, &root, &[("term_kinds", "zsh")], cx);
    for at in (0..tabs.len()).rev() {
        shell.update(cx, |shell, cx| {
            let (project, branch, _) = shell.workspace.shown_tab().expect("a tab shown");
            shell.workspace.select_tab(project, branch, at);
            cx.notify();
        });
        draw(window, cx);
    }
    let example = cx.read(|cx| {
        let unit = shell.read(cx).workspace.branch().expect("the checkout");
        unit.tabs[0].first().view().cloned()
    });
    let example = example.expect("shown, its page is made");
    example.update(cx, |page, cx| page.inspect(None, cx));
    draw(window, cx);
    example.update(cx, |page, cx| page.dock_devtools(Dock::Detached, cx));
    shell.update(cx, |shell, cx| {
        shell.desktop_pane(false, cx);
    });
    draw(window, cx);
    let shown = cx.read(|cx| {
        let tab = shell.read(cx).workspace.tab_shown().expect("the first tab");
        (tab.panes().iter()).filter(|pane| pane.is_inspector()).count()
    });
    assert_eq!(shown, 1, "the page's DevTools beside it");
    assert_eq!(
        written(&path, cx),
        main_with(&root, 0, &tabs),
        "as they were brought back, and no more"
    );
}

/// A file and the diffs brought back with the window, their tabs not shown since, are kept
/// again as they were brought back when the window is kept for another change.
#[gpui_kit::test]
fn a_file_and_the_diffs_not_shown_since_they_came_back_are_kept_as_they_were(
    cx: &mut TestAppContext,
) {
    let root = repo("kept-window-waiting", &[("README.md", "hi\n")]);
    let file_tab = KeptTree::Pane(KeptPane::File {
        path: root.join("README.md"),
    });
    let tabs = [
        KeptTree::Pane(KeptPane::NewChat),
        file_tab,
        KeptTree::Pane(KeptPane::Changes),
    ];
    let path = file("kept-window-waiting-file", &main_with(&root, 0, &tabs));
    let (window, shell) = shell_with(&[&root], cx);
    launch(&shell, &path, &[&root], cx);
    select(&shell, &root, cx);
    draw(window, cx);
    shell.update(cx, |shell, cx| shell.new_tab(PaneKind::Chat, cx));
    draw(window, cx);
    let waiting = cx.read(|cx| {
        let unit = shell.read(cx).workspace.branch().expect("the checkout");
        let opened = |pane: &Pane| pane.file().is_some() || pane.changes().is_some();
        !(unit.tabs[1..3].iter()).any(|tab| opened(tab.first()))
    });
    assert!(waiting, "neither is opened while its tab isn't shown");
    let mut again = tabs.to_vec();
    again.push(KeptTree::Pane(KeptPane::NewChat));
    assert_eq!(written(&path, cx), main_with(&root, 3, &again));
}
