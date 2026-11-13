//! The loose pages kept with the window (`docs/arc-sidebar.md` 11, `docs/browser.md` 2): each
//! space's in the window's file, by the space's id, and back in a new window, the one each space
//! showed shown; a file from before them reads as it did.

use gpui_kit::{AppContext as _, TestAppContext};

use super::super::loose::{KeptLoose, KeptSpace};
use super::pinned::relaunched;
use super::{Kept, read_file, written};
use crate::browser::NO_PROJECT;
use crate::settings::{self, Layout};
use crate::test_support::{draw, open_page, repo, scratch, select, shell_with};

fn kept(url: &str, title: &str, project: &str) -> KeptLoose {
    KeptLoose {
        url: url.into(),
        title: title.into(),
        project: project.into(),
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
    let json: serde_json::Value = crate::kept_file::read(&path).unwrap();
    assert_eq!(json["spaces"][0]["space"], 2);
    assert_eq!(json["spaces"][0]["shown"], 1);
    assert_eq!(
        json["spaces"][0]["tabs"][0]["url"],
        "https://mail.example.com/"
    );
    assert_eq!(json["spaces"][0]["tabs"][1]["project"], "/src/app");

    // With none, the file is as it was before there were any; and such a file reads with none.
    let none = serde_json::to_value(Kept::default()).unwrap();
    assert!(none.get("spaces").is_none(), "{none}");
    std::fs::write(&path, r#"{"projects":[],"checkouts":[]}"#).unwrap();
    assert_eq!(read_file(&path), Kept::default());
    // A page's entry may leave out its title and its project.
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
    // The Arc layout, where the loose pages are, with a second space, which has no project.
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
    // In the first space, beside its project: two loose pages, the first of them shown.
    let (first, second) = cx
        .update_window(window, |_, window, cx| {
            shell.update(cx, |shell, cx| {
                let mut open = |url: &str| {
                    let opened = shell.open_page(Some(url.into()), (true, false), None, window, cx);
                    opened.expect("a loose page opens with no checkout to open it on")
                };
                let first = open("https://a.example/");
                let second = open("https://b.example/");
                shell.pick_loose(first, cx);
                (first, second)
            })
        })
        .unwrap();
    assert_ne!(first, second);
    // In the second, which shows no project, a page opened is a loose one.
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

    // The next launch, on the second space as the settings have it: its page, shown; and the
    // first space's when it's shown again, the one it showed shown.
    let back = relaunched(&window_file, cx);
    let loose = |cx: &TestAppContext| {
        cx.read(|cx| {
            let workspace = &back.read(cx).workspace;
            let pages = (workspace.loose().iter())
                .map(|page| page.tab.first().address(cx).unwrap_or_default());
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

    // Those of a space the settings no longer have are the space shown's, rather than lost.
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
