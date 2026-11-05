//! ⌘T opens the launcher over the window; Tab goes through the kinds; Enter opens a tab of the
//! kind, empty or doing what's typed; and what the history holds is offered, filtered as it's
//! typed, and forgotten on ⇧⌫.

use std::path::Path;

use gpui_kit::{AnyWindowHandle, App, Entity, TestAppContext};

use super::rows::{self, Act, Query};
use super::here::{Here, OpenTab};
use super::{Kind, Launcher};
use crate::files::tests::{dialog, press, tabs};
use crate::history::{self, Place};
use crate::model::PaneKind;
use crate::shell::Shell;
use crate::test_support::{draw, repo, select, shell_with};

/// A shell over a checkout of its own, selected.
fn shell(name: &str, cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Shell>, std::path::PathBuf) {
    let root = repo(name, &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    draw(window, cx);
    (window, shell, root)
}

/// Presses ⌘T in `window`: the launcher's window and the launcher.
fn launch(window: AnyWindowHandle, cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Launcher>) {
    press(window, "cmd-t", cx);
    cx.run_until_parked();
    dialog::<Launcher>(cx).expect("the launcher")
}

fn kind(launcher: &Entity<Launcher>, cx: &TestAppContext) -> Kind {
    cx.read(|cx| launcher.read(cx).kind)
}

/// The launcher's rows, by title, the one Enter picks in brackets.
fn rows_of(launcher: &Entity<Launcher>, cx: &TestAppContext) -> Vec<String> {
    cx.read(|cx| {
        let launcher = launcher.read(cx);
        (launcher.rows.iter().enumerate())
            .map(|(at, row)| {
                let title = row.title.to_string();
                if launcher.selected == Some(at) { format!("[{title}]") } else { title }
            })
            .collect()
    })
}

fn place(root: &Path) -> Place {
    Place { worktree: root.to_owned(), project: root.to_owned() }
}

#[gpui_kit::test]
fn cmd_t_opens_the_launcher_and_tab_goes_browser_chat_terminal(cx: &mut TestAppContext) {
    let (window, _, _) = shell("launcher-kinds", cx);
    let (launcher_window, launcher) = launch(window, cx);
    assert_eq!(kind(&launcher, cx), Kind::Browser, "a browser tab first");
    press(launcher_window, "tab", cx);
    assert_eq!(kind(&launcher, cx), Kind::Chat);
    press(launcher_window, "tab", cx);
    assert_eq!(kind(&launcher, cx), Kind::Terminal);
    press(launcher_window, "tab", cx);
    assert_eq!(kind(&launcher, cx), Kind::Browser, "round again");
    press(launcher_window, "shift-tab", cx);
    assert_eq!(kind(&launcher, cx), Kind::Terminal, "and back");
    // ⌘T again closes it.
    press(launcher_window, "cmd-t", cx);
    cx.run_until_parked();
    assert!(dialog::<Launcher>(cx).is_none());
}

#[gpui_kit::test]
fn enter_with_nothing_typed_opens_an_empty_tab_of_the_kind(cx: &mut TestAppContext) {
    let (window, shell, _) = shell("launcher-empty", cx);
    let (launcher_window, _) = launch(window, cx);
    press(launcher_window, "enter", cx);
    cx.run_until_parked();
    assert!(dialog::<Launcher>(cx).is_none(), "it closes once it's done");
    assert_eq!(tabs(&shell, cx), ["[New tab]"], "a browser tab on the start page");

    let (launcher_window, _) = launch(window, cx);
    press(launcher_window, "tab enter", cx);
    cx.run_until_parked();
    assert_eq!(tabs(&shell, cx), ["New tab", "[New chat]"]);
}

#[gpui_kit::test]
fn commands_run_before_are_offered_latest_first_and_filtered_as_typed(cx: &mut TestAppContext) {
    let (window, _, root) = shell("launcher-commands", cx);
    cx.update(|cx| {
        history::used(history::Kind::Command, "cargo test", &place(&root), cx);
        history::used(history::Kind::Command, "npm run dev", &place(&root), cx);
    });
    let (launcher_window, launcher) = launch(window, cx);
    press(launcher_window, "shift-tab", cx);
    assert_eq!(kind(&launcher, cx), Kind::Terminal);
    assert_eq!(rows_of(&launcher, cx), ["npm run dev", "cargo test"], "nothing picked with nothing typed");

    cx.simulate_input(launcher_window, "car");
    cx.run_until_parked();
    assert_eq!(rows_of(&launcher, cx), ["[car]", "cargo test"], "running what's typed comes first");
    // ⇧⌫ forgets the row picked, when it's the history's.
    press(launcher_window, "down shift-backspace", cx);
    assert_eq!(rows_of(&launcher, cx), ["[car]"]);
    assert_eq!(cx.read(|cx| history::get(cx).of(history::Kind::Command).count()), 1);
    // → takes the row's text into the field.
    cx.update(|cx| history::used(history::Kind::Command, "cargo build", &place(&root), cx));
    cx.simulate_input(launcher_window, "g");
    press(launcher_window, "down right", cx);
    let typed = cx.read(|cx| launcher.read(cx).input.read(cx).value().to_string());
    assert_eq!(typed, "cargo build");
}

#[gpui_kit::test]
fn a_message_typed_for_a_chat_opens_one_that_says_it(cx: &mut TestAppContext) {
    let (window, shell, _) = shell("launcher-chat", cx);
    let (launcher_window, _) = launch(window, cx);
    press(launcher_window, "tab", cx);
    cx.simulate_input(launcher_window, "why is the build slow");
    press(launcher_window, "enter", cx);
    draw(window, cx);
    draw(window, cx);
    assert_eq!(tabs(&shell, cx), ["[New chat]"]);
    let said: Vec<String> = cx.read(|cx| {
        (history::get(cx).of(history::Kind::Prompt)).map(|entry| entry.text.clone()).collect()
    });
    assert_eq!(said, ["why is the build slow"], "the new chat's composer sent it");
}

#[gpui_kit::test]
fn an_address_typed_opens_its_page_and_words_are_searched(cx: &mut TestAppContext) {
    let (window, shell, _) = shell("launcher-browse", cx);
    let (launcher_window, launcher) = launch(window, cx);
    cx.simulate_input(launcher_window, "example.com/docs");
    cx.run_until_parked();
    let first = cx.read(|cx| launcher.read(cx).rows[0].act.clone());
    assert_eq!(first, Act::Visit("https://example.com/docs".into()));
    press(launcher_window, "enter", cx);
    cx.run_until_parked();
    let address = cx.read(|cx| {
        let unit = shell.read(cx).workspace.branch().expect("a checkout");
        let pane = unit.tabs[0].first();
        pane.browser.as_ref().map(|view| view.read(cx).address())
    });
    assert_eq!(address.as_deref(), Some("https://example.com/docs"));

    let (launcher_window, _) = launch(window, cx);
    cx.simulate_input(launcher_window, "rust gpui");
    press(launcher_window, "enter", cx);
    cx.run_until_parked();
    let searched: Vec<String> = cx.read(|cx| {
        (history::get(cx).of(history::Kind::Search)).map(|entry| entry.text.clone()).collect()
    });
    assert_eq!(searched, ["rust gpui"]);
    assert_eq!(tabs(&shell, cx).len(), 2);
}

#[gpui_kit::test]
fn text_dismissed_is_offered_again_when_it_opens_next(cx: &mut TestAppContext) {
    let (window, _, _) = shell("launcher-dismissed", cx);
    let (launcher_window, _) = launch(window, cx);
    press(launcher_window, "tab", cx);
    cx.simulate_input(launcher_window, "half a thought");
    press(launcher_window, "escape", cx);
    cx.run_until_parked();
    assert!(dialog::<Launcher>(cx).is_none());
    let (_, launcher) = launch(window, cx);
    let (typed, kind) = cx.read(|cx| {
        let launcher = launcher.read(cx);
        (launcher.input.read(cx).value().to_string(), launcher.kind)
    });
    assert_eq!((typed.as_str(), kind), ("half a thought", Kind::Chat));
}

/// A browser tab open on `url`, titled `title`.
fn tab(pane: u64, title: &str, url: &str) -> OpenTab {
    OpenTab { pane, kind: PaneKind::Browser, title: title.to_owned().into(), url: url.to_owned(), favicon: None }
}

fn browser_rows(here: &Here, typed: &str, suggested: &[String], cx: &mut App) -> Vec<rows::Row> {
    let query = Query { kind: Kind::Browser, typed, autofilled: None, here, suggested };
    rows::rows(&query, cx)
}

#[gpui_kit::test]
#[expect(clippy::single_range_in_vec_init, reason = "one word's bytes matched")]
fn a_page_a_tab_shows_is_switched_to_and_searches_share_the_rows(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let here = Here {
        place: place(Path::new("/work/app")),
        tabs: vec![tab(7, "Zen Browser", "https://github.com/zen-browser/desktop")],
        chats: Vec::new(),
    };
    cx.update(|cx| {
        let elsewhere = place(Path::new("/elsewhere"));
        history::visited("https://github.com/zen-browser/desktop", "Zen", history::How::Typed, &elsewhere, cx);
        for n in 0..12 {
            let url = format!("https://zen.example/{n}");
            history::visited(&url, &format!("zen {n}"), history::How::Followed, &elsewhere, cx);
        }
        let suggested: Vec<String> = (0..12).map(|n| format!("zen suggestion {n}")).collect();
        let rows = browser_rows(&here, "zen", &suggested, cx);
        assert_eq!(rows.len(), rows::MOST);
        assert_eq!(rows[0].act, Act::Search("zen".into()), "searching what's typed first");
        let searches = rows.iter().filter(|row| matches!(row.act, Act::Search(_))).count();
        assert_eq!(searches, 1 + 6, "two rows in three of those left are searches");
        let switch = rows.iter().find(|row| row.act == Act::Switch(7)).expect("the tab is switched to");
        assert_eq!(switch.title, "Zen Browser");
        assert_eq!(switch.title_matched, [0..3], "what matched in the tab's own title");
        assert!(!rows.iter().any(|row| row.act == Act::Visit("https://github.com/zen-browser/desktop".into())));
    });
}

#[test]
fn suggestions_answer_the_words_asked_about_only() {
    let body = br#"["rust gp",["rust gpui","rust gpu programming"],[],{}]"#;
    assert_eq!(
        super::suggest::parse("Rust gp", body),
        Some(vec!["rust gpui".to_owned(), "rust gpu programming".to_owned()])
    );
    assert_eq!(super::suggest::parse("other", body), None, "a late answer to other words");
    assert_eq!(super::suggest::parse("x", b"not json"), None);
    assert!(!super::suggest::is_address("rust gpui"));
    assert!(super::suggest::is_address("example.com"));
    assert!(super::suggest::is_address("localhost:3000"));
}

#[gpui_kit::test]
fn the_page_the_first_row_visits_is_not_listed_again(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let here = Here { place: place(Path::new("/work/app")), tabs: Vec::new(), chats: Vec::new() };
    cx.update(|cx| {
        history::visited("https://example.com/", "Example Domain", history::How::Typed, &here.place, cx);
        let query = Query { kind: Kind::Browser, typed: "exa", autofilled: Some("example.com"), here: &here, suggested: &[] };
        let rows = rows::rows(&query, cx);
        let visits: Vec<&Act> = rows.iter().map(|row| &row.act).filter(|act| matches!(act, Act::Visit(_))).collect();
        assert_eq!(visits, [&Act::Visit("https://example.com/".into())], "the autofilled page, once");
    });
}
