//! ⌘T opens the launcher over the window; Tab goes through the kinds; Enter opens a tab of the
//! kind, empty or doing what's typed; and what the history holds is offered, filtered as it's
//! typed, and forgotten on ⇧⌫.

use std::path::Path;

use gpui_kit::{AnyWindowHandle, App, Entity, TestAppContext};

use super::here::{Here, OpenTab};
use super::rows::{self, Act, Query};
use super::{Kind, Launcher};
use crate::files::tests::{dialog, press, tabs};
use crate::history::{self, Place};
use crate::model::PaneKind;
use crate::new_worktree::NewWorktreeDialog;
use crate::shell::Shell;
use crate::test_support::{draw, repo, select, shell_with};

mod on_page;

/// A shell over a checkout of its own, selected.
fn shell(
    name: &str,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<Shell>, std::path::PathBuf) {
    let root = repo(name, &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    draw(window, cx);
    (window, shell, root)
}

/// Presses ⌘T in `window`: the launcher's window and the launcher.
fn launch(window: AnyWindowHandle, cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Launcher>) {
    launch_with(window, "cmd-t", cx)
}

/// Presses `keys` in `window`, which open the launcher: its window and the launcher.
fn launch_with(
    window: AnyWindowHandle,
    keys: &str,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<Launcher>) {
    press(window, keys, cx);
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
                if launcher.selected == Some(at) {
                    format!("[{title}]")
                } else {
                    title
                }
            })
            .collect()
    })
}

fn place(root: &Path) -> Place {
    Place {
        worktree: root.to_owned(),
        project: root.to_owned(),
    }
}

#[gpui_kit::test]
fn cmd_t_opens_the_launcher_and_tab_goes_through_its_kinds(cx: &mut TestAppContext) {
    let (window, _, _) = shell("launcher-kinds", cx);
    let (launcher_window, launcher) = launch(window, cx);
    assert_eq!(kind(&launcher, cx), Kind::Browser, "a browser tab first");
    press(launcher_window, "tab", cx);
    assert_eq!(kind(&launcher, cx), Kind::Chat);
    press(launcher_window, "tab", cx);
    assert_eq!(kind(&launcher, cx), Kind::Terminal);
    press(launcher_window, "tab", cx);
    assert_eq!(kind(&launcher, cx), Kind::Desktop);
    press(launcher_window, "tab", cx);
    assert_eq!(kind(&launcher, cx), Kind::Device);
    press(launcher_window, "tab", cx);
    assert_eq!(kind(&launcher, cx), Kind::Worktree);
    press(launcher_window, "tab", cx);
    assert_eq!(kind(&launcher, cx), Kind::Browser, "round again");
    press(launcher_window, "shift-tab", cx);
    assert_eq!(kind(&launcher, cx), Kind::Worktree, "and back");
    // ⌘T again closes it.
    press(launcher_window, "cmd-t", cx);
    cx.run_until_parked();
    assert!(dialog::<Launcher>(cx).is_none());
}

#[gpui_kit::test]
fn a_worktree_named_in_the_launcher_opens_the_new_worktree_dialog_with_that_name(
    cx: &mut TestAppContext,
) {
    let (window, _, _) = shell("launcher-worktree", cx);
    let (launcher_window, launcher) = launch(window, cx);
    press(launcher_window, "shift-tab", cx);
    assert_eq!(kind(&launcher, cx), Kind::Worktree);
    cx.simulate_input(launcher_window, "fix login");
    assert!(rows_of(&launcher, cx).is_empty(), "a name lists nothing");
    press(launcher_window, "enter", cx);
    cx.run_until_parked();
    assert!(dialog::<Launcher>(cx).is_none(), "the launcher closes");
    let (_, named) = dialog::<NewWorktreeDialog>(cx).expect("the New Worktree dialog");
    assert_eq!(cx.read(|cx| named.read(cx).typed_name(cx)), "fix login");
}

#[gpui_kit::test]
fn enter_with_nothing_typed_opens_an_empty_tab_of_the_kind(cx: &mut TestAppContext) {
    let (window, shell, _) = shell("launcher-empty", cx);
    let (launcher_window, _) = launch(window, cx);
    press(launcher_window, "enter", cx);
    cx.run_until_parked();
    assert!(dialog::<Launcher>(cx).is_none(), "it closes once it's done");
    assert_eq!(
        tabs(&shell, cx),
        ["[New tab]"],
        "a browser tab on the start page"
    );

    let (launcher_window, _) = launch(window, cx);
    press(launcher_window, "tab enter", cx);
    cx.run_until_parked();
    assert_eq!(tabs(&shell, cx), ["New tab", "[New chat]"]);
}

#[gpui_kit::test]
fn the_desktop_kind_opens_the_agent_desktops_one_tab(cx: &mut TestAppContext) {
    let (window, shell, _) = shell("launcher-desktop", cx);
    let (launcher_window, launcher) = launch(window, cx);
    press(launcher_window, "tab tab tab", cx);
    assert_eq!(kind(&launcher, cx), Kind::Desktop);
    press(launcher_window, "enter", cx);
    cx.run_until_parked();
    assert_eq!(tabs(&shell, cx), ["[Agent desktop]"]);

    // There is one agent desktop: asked for again, its tab is shown, not a second made.
    let (launcher_window, _) = launch(window, cx);
    press(launcher_window, "enter", cx);
    cx.run_until_parked();
    assert_eq!(tabs(&shell, cx), ["Agent desktop", "[New tab]"]);
    let (launcher_window, _) = launch(window, cx);
    press(launcher_window, "tab tab tab enter", cx);
    cx.run_until_parked();
    assert_eq!(tabs(&shell, cx), ["[Agent desktop]", "New tab"]);
}

#[gpui_kit::test]
fn the_device_kind_opens_a_device_tab_on_its_picker(cx: &mut TestAppContext) {
    let (window, shell, _) = shell("launcher-device", cx);
    let (launcher_window, launcher) = launch(window, cx);
    press(launcher_window, "tab tab tab tab", cx);
    assert_eq!(kind(&launcher, cx), Kind::Device);
    press(launcher_window, "enter", cx);
    cx.run_until_parked();
    assert_eq!(tabs(&shell, cx), ["[Devices]"]);
}

fn simulator(udid: &str, name: &str, ipad: bool) -> crate::devices::Simulator {
    crate::devices::Simulator {
        udid: udid.into(),
        name: name.into(),
        system: "iOS 27.2".into(),
        booted: false,
        booted_before: true,
        ipad,
    }
}

/// The Device kind lists Koyo's simulators, then the devices plugged in, those named as typed,
/// and picking one opens its tab; a device a tab shows is a switch to it.
#[gpui_kit::test]
fn the_device_kind_lists_the_simulators_and_opens_the_one_picked(cx: &mut TestAppContext) {
    let (window, shell, _) = shell("launcher-device-rows", cx);
    cx.update(|cx| {
        let listed = Ok(vec![simulator("U-1", "iPhone 17 Pro", false), simulator("U-2", "iPad Pro 13-inch (M5)", true)]);
        let plugged = vec![crate::devices::connected::Connected {
            udid: "REAL".into(),
            name: "Sam's iPad".into(),
            ipad: true,
            system: "iPadOS 27.0.1".into(),
            missing: None,
        }];
        crate::devices::devices(cx).update(cx, |devices, cx| {
            devices.listed(listed, None, cx);
            devices.connected_afresh(Ok(plugged), std::time::Instant::now(), cx);
        });
    });
    let (launcher_window, launcher) = launch(window, cx);
    press(launcher_window, "tab tab tab tab", cx);
    assert_eq!(rows_of(&launcher, cx), ["iPhone 17 Pro", "iPad Pro 13-inch (M5)", "Sam's iPad"]);
    cx.simulate_input(launcher_window, "ipad");
    cx.run_until_parked();
    assert_eq!(rows_of(&launcher, cx), ["[iPad Pro 13-inch (M5)]", "Sam's iPad"]);
    press(launcher_window, "enter", cx);
    cx.run_until_parked();
    assert_eq!(tabs(&shell, cx), ["[iPad Pro 13-inch (M5)]"]);

    shell.update(cx, |shell, cx| shell.device_pane(None, true, cx));
    cx.run_until_parked();
    assert_eq!(tabs(&shell, cx), ["iPad Pro 13-inch (M5)", "[Devices]"]);
    let (launcher_window, launcher) = launch(window, cx);
    press(launcher_window, "tab tab tab tab", cx);
    assert_eq!(rows_of(&launcher, cx), ["iPad Pro 13-inch (M5)", "iPhone 17 Pro", "Sam's iPad"], "the open tab first");
    let first = cx.read(|cx| launcher.read(cx).rows[0].act.clone());
    assert!(matches!(first, Act::Switch(_)), "{first:?}");
}

/// An agent's `desktop_open` brings the tab out of sight: after the tab that shows, the
/// person kept where they are; on a checkout with no tab there's nothing to keep them on.
#[gpui_kit::test]
fn the_desktops_tab_opened_out_of_sight_keeps_the_tab_that_shows(cx: &mut TestAppContext) {
    let (window, shell, _) = shell("launcher-desktop-unseen", cx);
    let open = |cx: &mut TestAppContext| {
        shell.update(cx, |shell, cx| shell.desktop_pane(false, cx));
        cx.run_until_parked();
    };
    open(cx);
    assert_eq!(tabs(&shell, cx), ["[Agent desktop]"], "the only tab shows");

    shell.update(cx, |shell, cx| {
        let pane = shell.desktop_pane(false, cx).expect("the desktop's pane");
        shell.close_pane_now(pane, cx);
    });
    let (launcher_window, _) = launch(window, cx);
    press(launcher_window, "enter", cx);
    cx.run_until_parked();
    assert_eq!(tabs(&shell, cx), ["[New tab]"]);
    open(cx);
    assert_eq!(tabs(&shell, cx), ["[New tab]", "Agent desktop"]);
}

#[gpui_kit::test]
fn commands_run_before_are_offered_latest_first_and_filtered_as_typed(cx: &mut TestAppContext) {
    let (window, _, root) = shell("launcher-commands", cx);
    cx.update(|cx| {
        history::used(history::Kind::Command, "cargo test", &place(&root), cx);
        history::used(history::Kind::Command, "npm run dev", &place(&root), cx);
    });
    let (launcher_window, launcher) = launch(window, cx);
    press(launcher_window, "tab tab", cx);
    assert_eq!(kind(&launcher, cx), Kind::Terminal);
    assert_eq!(
        rows_of(&launcher, cx),
        ["npm run dev", "cargo test"],
        "nothing picked with nothing typed"
    );

    cx.simulate_input(launcher_window, "car");
    cx.run_until_parked();
    assert_eq!(
        rows_of(&launcher, cx),
        ["[car]", "cargo test"],
        "running what's typed comes first"
    );
    // ⇧⌫ forgets the row picked, when it's the history's.
    press(launcher_window, "down shift-backspace", cx);
    assert_eq!(rows_of(&launcher, cx), ["[car]"]);
    assert_eq!(
        cx.read(|cx| history::get(cx).of(history::Kind::Command).count()),
        1
    );
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
        (history::get(cx).of(history::Kind::Prompt))
            .map(|entry| entry.text.clone())
            .collect()
    });
    assert_eq!(
        said,
        ["why is the build slow"],
        "the new chat's composer sent it"
    );
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
        pane.view().map(|view| view.read(cx).address())
    });
    assert_eq!(address.as_deref(), Some("https://example.com/docs"));

    let (launcher_window, _) = launch(window, cx);
    cx.simulate_input(launcher_window, "rust gpui");
    press(launcher_window, "enter", cx);
    cx.run_until_parked();
    let searched: Vec<String> = cx.read(|cx| {
        (history::get(cx).of(history::Kind::Search))
            .map(|entry| entry.text.clone())
            .collect()
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
    OpenTab {
        pane,
        kind: PaneKind::Browser,
        title: title.to_owned().into(),
        url: url.to_owned(),
        device: None,
        favicon: None,
    }
}

fn browser_rows(here: &Here, typed: &str, suggested: &[String], cx: &mut App) -> Vec<rows::Row> {
    let query = Query {
        kind: Kind::Browser,
        typed,
        autofilled: None,
        here,
        suggested,
    };
    rows::rows(&query, cx)
}

#[gpui_kit::test]
#[expect(clippy::single_range_in_vec_init, reason = "one word's bytes matched")]
fn a_page_a_tab_shows_is_switched_to_and_searches_share_the_rows(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let here = Here {
        place: place(Path::new("/work/app")),
        tabs: vec![tab(
            7,
            "Zen Browser",
            "https://github.com/zen-browser/desktop",
        )],
        chats: Vec::new(),
    };
    cx.update(|cx| {
        let elsewhere = place(Path::new("/elsewhere"));
        history::visited(
            "https://github.com/zen-browser/desktop",
            "Zen",
            history::How::Typed,
            &elsewhere,
            cx,
        );
        for n in 0..12 {
            let url = format!("https://zen.example/{n}");
            history::visited(
                &url,
                &format!("zen {n}"),
                history::How::Followed,
                &elsewhere,
                cx,
            );
        }
        let suggested: Vec<String> = (0..12).map(|n| format!("zen suggestion {n}")).collect();
        let rows = browser_rows(&here, "zen", &suggested, cx);
        assert_eq!(rows.len(), rows::MOST);
        assert_eq!(
            rows[0].act,
            Act::Search("zen".into()),
            "searching what's typed first"
        );
        let searches = rows
            .iter()
            .filter(|row| matches!(row.act, Act::Search(_)))
            .count();
        assert_eq!(
            searches,
            1 + 6,
            "two rows in three of those left are searches"
        );
        let switch = rows
            .iter()
            .find(|row| row.act == Act::Switch(7))
            .expect("the tab is switched to");
        assert_eq!(switch.title, "Zen Browser");
        assert_eq!(
            switch.title_matched,
            [0..3],
            "what matched in the tab's own title"
        );
        assert!(
            !rows
                .iter()
                .any(|row| row.act == Act::Visit("https://github.com/zen-browser/desktop".into()))
        );
    });
}

#[test]
fn suggestions_answer_the_words_asked_about_only() {
    let body = br#"["rust gp",["rust gpui","rust gpu programming"],[],{}]"#;
    assert_eq!(
        super::suggest::parse("Rust gp", body),
        Some(vec![
            "rust gpui".to_owned(),
            "rust gpu programming".to_owned()
        ])
    );
    assert_eq!(
        super::suggest::parse("other", body),
        None,
        "a late answer to other words"
    );
    assert_eq!(super::suggest::parse("x", b"not json"), None);
    assert!(!super::suggest::is_address("rust gpui"));
    assert!(super::suggest::is_address("example.com"));
    assert!(super::suggest::is_address("localhost:3000"));
}

#[gpui_kit::test]
fn the_page_the_first_row_visits_is_not_listed_again(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let here = Here {
        place: place(Path::new("/work/app")),
        tabs: Vec::new(),
        chats: Vec::new(),
    };
    cx.update(|cx| {
        history::visited(
            "https://example.com/",
            "Example Domain",
            history::How::Typed,
            &here.place,
            cx,
        );
        let query = Query {
            kind: Kind::Browser,
            typed: "exa",
            autofilled: Some("example.com"),
            here: &here,
            suggested: &[],
        };
        let rows = rows::rows(&query, cx);
        let visits: Vec<&Act> = rows
            .iter()
            .map(|row| &row.act)
            .filter(|act| matches!(act, Act::Visit(_)))
            .collect();
        assert_eq!(
            visits,
            [&Act::Visit("https://example.com/".into())],
            "the autofilled page, once"
        );
    });
}
