//! The loose pages kept with the window (`docs/arc-sidebar.md` 11, `docs/browser.md` 2): each
//! space's in the window's file, by the space's id, and back in a new window, the one each space
//! showed shown; a file from before them reads as it did.

use gpui_kit::{AppContext as _, TestAppContext};

use super::super::loose::{KeptLoose, KeptSpace};
use super::pinned::relaunched;
use super::{Kept, read_file, written};
use crate::browser::NO_PROJECT;
use crate::settings::{self, Layout, LoosePages};
use crate::test_support::{draw, open_page, repo, scratch, select, shell_with};

fn kept(url: &str, title: &str, project: &str) -> KeptLoose {
    KeptLoose {
        url: url.into(),
        title: title.into(),
        project: project.into(),
        session: koyo_browser_ipc::Session::default(),
        shown_in: Vec::new(),
    }
}

#[test]
fn the_windows_file_keeps_each_spaces_loose_pages_and_one_from_before_them_still_reads() {
    let spaces = vec![KeptSpace {
        space: 2,
        shown: Some(1),
        tabs: vec![
            kept("https://mail.example.com/", "Inbox", NO_PROJECT),
            kept("http://localhost:3000/", "App", "/src/app"),
        ],
    }];
    let window = Kept {
        spaces,
        ..Kept::default()
    };
    let path = scratch("kept-loose-file").join("window.json");
    std::fs::write(&path, crate::kept_file::encode(&window).unwrap()).unwrap();
    assert_eq!(read_file(&path), window);
    let json: serde_json::Value =
        crate::blocking::in_test(|may| crate::kept_file::read(&path, may)).unwrap();
    assert_eq!(json["spaces"][0]["space"], 2);
    assert_eq!(json["spaces"][0]["shown"], 1);
    assert_eq!(
        json["spaces"][0]["tabs"][0]["url"],
        "https://mail.example.com/"
    );
    assert_eq!(json["spaces"][0]["tabs"][1]["project"], "/src/app");

    let none = serde_json::to_value(Kept::default()).unwrap();
    assert!(none.get("spaces").is_none(), "{none}");
    std::fs::write(&path, r#"{"projects":[],"checkouts":[]}"#).unwrap();
    assert_eq!(read_file(&path), Kept::default());
    std::fs::write(
        &path,
        r#"{"spaces":[{"space":1,"tabs":[{"url":"https://a.example/"}]}]}"#,
    )
    .unwrap();
    let read = read_file(&path);
    assert_eq!(read.spaces[0].tabs, [kept("https://a.example/", "", "")]);
    assert_eq!(read.spaces[0].shown, None);
}

#[gpui_kit::test]
fn loose_pages_come_back_in_their_spaces_with_the_one_each_showed(cx: &mut TestAppContext) {
    let root = repo("kept-loose-back", &[("README.md", "hi\n")]);
    let window_file = scratch("kept-loose-back-file").join("window.json");
    let (window, shell) = shell_with(&[&root], cx);
    let ids = cx.update(|cx| {
        settings::update(cx, |settings| {
            settings.layout = Layout::Arc;
            settings.add_space();
        });
        let spaces = &settings::get(cx).spaces;
        (spaces[0].id, spaces[1].id)
    });
    shell.update(cx, |shell, cx| {
        shell.keep_window_at(&window_file);
        shell.show_space(0, cx);
    });
    select(&shell, &root, cx);
    let (first, second) = cx
        .update_window(window, |_, window, cx| {
            shell.update(cx, |shell, cx| {
                let mut open = |url: &str| {
                    let opened = shell.open_page(Some(url.into()), (true, false), None, window, cx);
                    opened.expect("a loose page opens with no checkout to open it on")
                };
                let first = open("https://a.example/");
                let second = open("https://b.example/");
                shell.show_tab_holding(first, cx);
                (first, second)
            })
        })
        .unwrap();
    assert_ne!(first, second);
    shell.update(cx, |shell, cx| shell.show_space(1, cx));
    open_page(window, &shell, Some("https://c.example/"), cx);
    draw(window, cx);

    let file = written(&window_file, cx);
    let page = |url: &str, title: &str| kept(url, title, NO_PROJECT);
    let expected = vec![
        KeptSpace {
            space: ids.0,
            shown: Some(0),
            tabs: vec![
                page("https://a.example/", "a.example"),
                page("https://b.example/", "b.example"),
            ],
        },
        KeptSpace {
            space: ids.1,
            shown: Some(0),
            tabs: vec![page("https://c.example/", "c.example")],
        },
    ];
    assert_eq!(file.spaces, expected);

    let back = relaunched(&window_file, cx);
    let loose = |cx: &TestAppContext| {
        cx.read(|cx| {
            let workspace = &back.read(cx).workspace;
            let pages = (workspace.loose().into_iter())
                .map(|page| page.tab.first().address().unwrap_or_default().to_owned());
            (pages.collect::<Vec<_>>(), workspace.loose_shown())
        })
    };
    assert_eq!(loose(cx), (vec!["https://c.example/".to_owned()], Some(0)));
    back.update(cx, |shell, cx| shell.show_space(0, cx));
    let both = vec![
        "https://a.example/".to_owned(),
        "https://b.example/".to_owned(),
    ];
    assert_eq!(loose(cx), (both, Some(0)));
    let profile = cx.read(|cx| back.read(cx).workspace.loose()[0].project.clone());
    assert_eq!(profile, std::path::Path::new(NO_PROJECT));

    let mut orphaned = file;
    orphaned.spaces[1].space = 99;
    std::fs::write(&window_file, crate::kept_file::encode(&orphaned).unwrap()).unwrap();
    let again = relaunched(&window_file, cx);
    let count = cx.read(|cx| again.read(cx).workspace.loose().len());
    assert_eq!(
        count, 3,
        "the first space's two, and the one from the space that's gone"
    );
}

#[gpui_kit::test]
fn shared_loose_pages_come_back_shared_in_their_order_and_each_in_its_space_per_space(
    cx: &mut TestAppContext,
) {
    let root = repo("kept-loose-shared", &[("README.md", "hi\n")]);
    let window_file = scratch("kept-loose-shared-file").join("window.json");
    let (window, shell) = shell_with(&[&root], cx);
    let ids = cx.update(|cx| {
        settings::update(cx, |settings| {
            settings.layout = Layout::Arc;
            settings.loose_pages = LoosePages::Shared;
            settings.add_space();
        });
        let spaces = &settings::get(cx).spaces;
        (spaces[0].id, spaces[1].id)
    });
    shell.update(cx, |shell, cx| {
        shell.keep_window_at(&window_file);
        shell.show_space(0, cx);
    });
    select(&shell, &root, cx);
    let opened = |url: &'static str, cx: &mut TestAppContext| {
        (cx.update_window(window, |_, window, cx| {
            shell.update(cx, |shell, cx| {
                let opened = shell.open_page(Some(url.into()), (true, false), None, window, cx);
                opened.expect("a loose page opens with no checkout to open it on")
            })
        }))
        .unwrap()
    };
    let a = opened("https://a.example/", cx);
    opened("https://c.example/", cx);
    shell.update(cx, |shell, cx| {
        shell.show_space(1, cx);
        shell.show_tab_holding(a, cx);
    });
    opened("https://b.example/", cx);
    shell.update(cx, |shell, cx| shell.show_tab_holding(a, cx));
    draw(window, cx);

    let file = written(&window_file, cx);
    let filed: Vec<_> = (file.spaces.iter())
        .map(|kept| (kept.space, kept.shown, kept.tabs.len()))
        .collect();
    assert_eq!(
        filed,
        [(ids.0, None, 1), (ids.1, None, 1), (ids.0, Some(0), 1)],
        "each page under its own space, as a build before reads them"
    );
    assert_eq!(
        file.spaces[0].tabs[0].shown_in,
        [ids.1],
        "a, shown in the other space"
    );

    let addresses = |shell: &gpui_kit::Entity<crate::shell::Shell>, cx: &TestAppContext| {
        cx.read(|cx| {
            let workspace = &shell.read(cx).workspace;
            let pages = (workspace.loose().into_iter())
                .map(|page| page.tab.first().address().unwrap_or_default().to_owned());
            (pages.collect::<Vec<_>>(), workspace.loose_shown())
        })
    };
    let all = [
        "https://a.example/",
        "https://b.example/",
        "https://c.example/",
    ]
    .map(String::from);
    let back = relaunched(&window_file, cx);
    assert_eq!(addresses(&back, cx), (all.to_vec(), Some(0)), "a, shown");
    back.update(cx, |shell, cx| shell.show_space(0, cx));
    assert_eq!(
        addresses(&back, cx),
        (all.to_vec(), Some(0)),
        "a, still shown in the other space, as a pinned page would be"
    );

    for open in [&shell, &back] {
        open.update(cx, |shell, _| shell.workspace.select_project(None));
    }
    cx.update(|cx| settings::update(cx, |settings| settings.loose_pages = LoosePages::PerSpace));
    let apart = relaunched(&window_file, cx);
    apart.update(cx, |shell, cx| shell.show_space(1, cx));
    assert_eq!(
        addresses(&apart, cx),
        (vec!["https://b.example/".to_owned()], None),
        "it showed a, which is the other space's"
    );
    apart.update(cx, |shell, cx| shell.show_space(0, cx));
    let first = vec![
        "https://a.example/".to_owned(),
        "https://c.example/".to_owned(),
    ];
    assert_eq!(addresses(&apart, cx), (first, Some(1)));
}
