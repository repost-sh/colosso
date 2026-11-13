//! The pinned pages kept for the next launch (`docs/arc-sidebar.md` 11): the file reads back what
//! was written, a file as this build writes it still loads, one a later or an earlier build
//! wrote loses nothing, and they come back in a window whose own file can't be read.

use std::path::{Path, PathBuf};

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use serde_json::{Value, json};

use super::super::SAVE_DELAY;
use super::super::pinned::{FILE, KeptPinned, KeptPins, read_file};
use crate::settings::{self, Layout};
use crate::shell::Shell;
use crate::test_support::{draw, open_page, repo, scratch, select, shell_window, shell_with};

fn kept(url: &str, title: &str, pinned_url: &str, project: &str) -> KeptPinned {
    KeptPinned {
        url: url.into(),
        title: title.into(),
        pinned_url: pinned_url.into(),
        project: project.into(),
        more: serde_json::Map::new(),
    }
}

/// What `pins` reads back as, written and read again.
fn round_trip(pins: &KeptPins) -> KeptPins {
    KeptPins::read(&pins.encode().expect("it encodes")).expect("a file of pinned pages")
}

fn as_json(pins: &KeptPins) -> Value {
    serde_json::from_slice(&pins.encode().expect("it encodes")).expect("JSON")
}

#[test]
fn the_pinned_pages_file_reads_back_what_was_written() {
    let pins = KeptPins {
        tabs: vec![
            kept(
                "https://mail.example.com/inbox",
                "Inbox",
                "https://mail.example.com/",
                "/src/app",
            ),
            kept("", "New tab", "", "/src/site"),
        ],
        shown: Some(1),
        version: 1,
        ..KeptPins::default()
    };
    assert_eq!(round_trip(&pins), pins);
    let json = as_json(&pins);
    assert_eq!(json["version"], 1);
    assert_eq!(json["shown"], 1);
    assert_eq!(json["tabs"][0]["url"], "https://mail.example.com/inbox");
    assert_eq!(json["tabs"][0]["title"], "Inbox");
    assert_eq!(json["tabs"][0]["pinnedUrl"], "https://mail.example.com/");
    assert_eq!(json["tabs"][0]["project"], "/src/app");
    // None shown, and none pinned: a file all the same.
    let none = KeptPins::default();
    assert_eq!(as_json(&none), json!({ "version": 1, "tabs": [] }));
    assert_eq!(round_trip(&none).tabs, []);
}

/// The file exactly as this build writes it. A build after this one has to read it still: if
/// this test needs its JSON changed, files in people's data folders need the same care.
const TODAY: &str = r#"{"version":1,"shown":0,"tabs":[{"url":"https://mail.example.com/inbox","title":"Inbox (3)","pinnedUrl":"https://mail.example.com/","project":"/Users/me/src/app"},{"url":"http://localhost:3000/","title":"App","pinnedUrl":"http://localhost:3000/","project":"/Users/me/src/site"}]}"#;

#[test]
fn a_file_as_this_build_writes_it_still_loads() {
    let pins = KeptPins::read(TODAY.as_bytes()).expect("today's file");
    let mail = "https://mail.example.com/";
    let inbox = kept(
        "https://mail.example.com/inbox",
        "Inbox (3)",
        mail,
        "/Users/me/src/app",
    );
    let app = "http://localhost:3000/";
    let site = kept(app, "App", app, "/Users/me/src/site");
    assert_eq!(pins.tabs, [inbox, site]);
    assert_eq!((pins.shown, pins.version), (Some(0), 1));
    assert!(pins.unread.is_empty() && pins.more.is_empty());
    // And it's what this build writes for it.
    let written = String::from_utf8(pins.encode().expect("it encodes")).expect("text");
    assert_eq!(written, TODAY);
}

#[test]
fn a_file_another_build_wrote_loses_nothing_read_and_written_again() {
    // A later build's: fields this one doesn't know, in the file and in a page's entry, a kind
    // of entry it can't read as a page, and a later version.
    let later = json!({
        "version": 7,
        "shown": 2,
        "groups": [{ "name": "Work", "tabs": [0] }],
        "tabs": [
            { "url": "https://a.example/", "title": "A", "pinnedUrl": "https://a.example/home",
              "project": "/src/app", "muted": true, "zoom": 1.25 },
            { "kind": "folder", "name": "Later", "tabs": [] },
            { "url": "https://b.example/", "title": "B", "project": "/src/app" },
        ],
    });
    let pins = KeptPins::read(later.to_string().as_bytes()).expect("a later build's file");
    let urls: Vec<&str> = pins.tabs.iter().map(|tab| tab.url.as_str()).collect();
    assert_eq!(
        urls,
        ["https://a.example/", "https://b.example/"],
        "its pages, in order"
    );
    assert_eq!(
        pins.shown,
        Some(1),
        "the page shown, by its place among those read"
    );
    assert_eq!(pins.tabs[0].pinned_url, "https://a.example/home");
    assert_eq!(pins.tabs[0].more["muted"], true);
    assert_eq!(
        pins.unread,
        [json!({ "kind": "folder", "name": "Later", "tabs": [] })]
    );
    let again = as_json(&pins);
    assert_eq!(again["version"], 7, "not taken back to this build's");
    assert_eq!(again["groups"], later["groups"]);
    assert_eq!(again["tabs"][0], later["tabs"][0], "a page's entry, whole");
    assert_eq!(
        again["tabs"][2], later["tabs"][1],
        "what it couldn't read, after its pages"
    );
    assert_eq!(round_trip(&pins), pins);

    // An earlier build's, or one edited by hand: fields left out, or of another type, have
    // their defaults, and the page is pinned where it showed.
    let earlier = r#"{"tabs":[{"url":"https://a.example/"},{"url":"https://b.example/","title":7,"project":null},{"title":"no address"}],"shown":9}"#;
    let pins = KeptPins::read(earlier.as_bytes()).expect("an earlier build's file");
    let a = kept("https://a.example/", "", "https://a.example/", "");
    let b = kept("https://b.example/", "", "https://b.example/", "");
    assert_eq!(pins.tabs, [a, b]);
    assert_eq!(pins.shown, None, "no such page");
    assert_eq!(pins.unread, [json!({ "title": "no address" })]);
    assert_eq!(
        KeptPins::read(b"{}"),
        Some(KeptPins {
            version: 1,
            ..KeptPins::default()
        })
    );

    // What isn't a file of pinned pages at all is none, rather than no pages.
    for not in ["{ not json", "[]", "7", r#"{"tabs":{"a":1}}"#, ""] {
        assert_eq!(KeptPins::read(not.as_bytes()), None, "{not}");
    }
}

#[test]
fn a_file_that_cant_be_read_is_set_aside_rather_than_written_over() {
    let folder = scratch("kept-pinned-aside");
    let path = folder.join(FILE);
    assert_eq!(read_file(&path), KeptPins::default(), "no file yet");
    std::fs::write(&path, "{ half a fi").unwrap();
    assert_eq!(read_file(&path), KeptPins::default());
    assert!(!path.exists(), "out of the way of the next write");
    let aside = std::fs::read_to_string(folder.join("pinned-tabs.unreadable.json")).unwrap();
    assert_eq!(aside, "{ half a fi", "as it was");
    std::fs::write(&path, TODAY).unwrap();
    assert_eq!(read_file(&path).tabs.len(), 2);
}

/// What the pinned pages' file beside the window's at `window` holds once the write coming is
/// done.
fn written(window: &Path, cx: &mut TestAppContext) -> KeptPins {
    cx.run_until_parked();
    cx.executor().advance_clock(SAVE_DELAY);
    cx.run_until_parked();
    read_file(&window.with_file_name(FILE))
}

/// The pinned pages of `shell`: each page's address, its title, the address it's pinned at and
/// whether it's open; and the place of the one shown.
type Pins = (Vec<(String, String, String, bool)>, Option<usize>);

fn pins_of(shell: &Entity<Shell>, cx: &TestAppContext) -> Pins {
    cx.read(|cx| {
        let workspace = &shell.read(cx).workspace;
        let pins = (workspace.pinned().iter())
            .map(|pinned| {
                let page = pinned.tab.first();
                let address = page.address(cx).unwrap_or_default();
                let open = page.view().is_some();
                (address, page.title.to_string(), pinned.home.clone(), open)
            })
            .collect();
        (pins, workspace.pinned_shown())
    })
}

/// A window opened again on the file at `window`, as the next launch opens one: its shell.
pub(super) fn relaunched(window: &Path, cx: &mut TestAppContext) -> Entity<Shell> {
    let (_, shell) = shell_window(cx);
    shell.update(cx, |shell, cx| {
        shell.keep_window_at(window);
        shell.restore_pages(cx);
        // On the space shown, as a window opened is.
        shell.follow_space_pages(cx);
    });
    cx.run_until_parked();
    shell
}

#[gpui_kit::test]
fn pinned_pages_come_back_in_order_where_they_were_with_the_one_shown(cx: &mut TestAppContext) {
    let root = repo("kept-pinned-back", &[("README.md", "hi\n")]);
    let window_file: PathBuf = scratch("kept-pinned-back-file").join("window.json");
    let (window, shell) = shell_with(&[&root], cx);
    // The Arc layout, where the pinned pages are.
    cx.update(|cx| settings::update(cx, |settings| settings.layout = Layout::Arc));
    shell.update(cx, |shell, _| shell.keep_window_at(&window_file));
    select(&shell, &root, cx);
    let pages = [
        "https://a.example/",
        "https://b.example/",
        "https://c.example/",
    ];
    let keys: Vec<u64> = (pages.iter())
        .map(|page| open_page(window, &shell, Some(page), cx))
        .collect();
    shell.update(cx, |shell, cx| {
        for key in &keys {
            assert!(shell.pin_tab_of(*key, cx));
        }
        // The last moves first, and the second goes on from where it's pinned.
        shell.move_pinned(2, 0, cx);
    });
    let view = cx.read(|cx| shell.read(cx).workspace.pane(keys[1])?.view().cloned());
    cx.update_window(window, |_, _, cx| {
        (view.expect("the second page, open")).update(cx, |view, cx| {
            view.navigate("https://b.example/later".into(), cx);
        });
    })
    .unwrap();
    draw(window, cx);

    let file = written(&window_file, cx);
    let urls: Vec<&str> = file.tabs.iter().map(|tab| tab.url.as_str()).collect();
    assert_eq!(urls, [pages[2], pages[0], "https://b.example/later"]);
    assert_eq!(
        file.tabs[2].pinned_url, pages[1],
        "where it's pinned, not where it went"
    );
    assert_eq!(file.tabs[0].project, root);
    assert_eq!(
        file.shown,
        Some(0),
        "the last opened was the tab shown, and went on showing"
    );

    // The next launch: in the order of their tiles, each at the address it showed, titled as
    // it was, pinned where it was, the one shown shown, and no page made but the one shown's.
    let (before, _) = pins_of(&shell, cx);
    let back = relaunched(&window_file, cx);
    let (pins, shown) = pins_of(&back, cx);
    assert_eq!(shown, Some(0));
    let back_as = |(place, page): (usize, &(String, String, String, bool))| {
        let (address, title, home, _) = page.clone();
        (address, title, home, place == 0)
    };
    let expected: Vec<_> = before.iter().enumerate().map(back_as).collect();
    assert_eq!(pins, expected, "only the one shown has its page made");
    let profile = cx.read(|cx| back.read(cx).workspace.pinned()[1].project.clone());
    assert_eq!(
        profile, root,
        "of the project it was pinned from, for its sign-ins"
    );
    // Nothing changed since: the file isn't written again, and reads as it did.
    assert_eq!(written(&window_file, cx), file);

    // Closed, the third is back where it's pinned, which the file keeps too; removed, the first
    // is gone from it.
    let third = cx.read(|cx| back.read(cx).workspace.pinned()[2].key());
    let first = cx.read(|cx| back.read(cx).workspace.pinned()[0].key());
    back.update(cx, |shell, cx| {
        shell.close_pinned(third, cx);
        shell.remove_pinned(first, cx);
    });
    let file = written(&window_file, cx);
    let urls: Vec<&str> = file.tabs.iter().map(|tab| tab.url.as_str()).collect();
    assert_eq!(urls, [pages[0], pages[1]]);
    assert_eq!(file.shown, None);

    // The Orca layout shows none of them, switched to or launched in: they stay pinned.
    back.update(cx, |shell, cx| {
        let first = shell.workspace.pinned()[0].key();
        shell.pick_pinned(first, cx);
    });
    assert_eq!(pins_of(&back, cx).1, Some(0));
    cx.update(|cx| settings::update(cx, |settings| settings.layout = Layout::Orca));
    cx.run_until_parked();
    assert_eq!(pins_of(&back, cx).1, None);
    assert_eq!(pins_of(&back, cx).0.len(), 2);
}

#[gpui_kit::test]
fn a_window_file_that_cant_be_read_doesnt_take_the_pinned_pages_with_it(cx: &mut TestAppContext) {
    let root = repo("kept-pinned-alone", &[("README.md", "hi\n")]);
    let folder = scratch("kept-pinned-alone-file");
    let window_file = folder.join("window.json");
    std::fs::write(&window_file, "{ not json at all").unwrap();
    // A later build's file, with what this one doesn't read.
    let later = json!({
        "version": 3,
        "theme": "glass",
        "tabs": [
            { "url": "https://a.example/", "title": "A", "project": root, "muted": true },
            { "kind": "folder", "name": "Later" },
        ],
    });
    std::fs::write(folder.join(FILE), later.to_string()).unwrap();
    // The app as it is once a window has been open, its project in the sidebar.
    let (_, _before) = shell_with(&[&root], cx);
    let shell = relaunched(&window_file, cx);
    let (pins, shown) = pins_of(&shell, cx);
    let home = "https://a.example/".to_owned();
    let a = (home.clone(), "A".to_owned(), home, false);
    assert_eq!((pins, shown), (vec![a], None));

    // Pinned at another address, the file is written with what it held that this build doesn't
    // read still in it.
    let pinned = cx.read(|cx| shell.read(cx).workspace.pinned()[0].key());
    shell.update(cx, |shell, cx| {
        shell.repin_at(pinned, "https://a.example/home".into(), cx);
    });
    let file: Value = {
        cx.run_until_parked();
        cx.executor().advance_clock(SAVE_DELAY);
        cx.run_until_parked();
        crate::kept_file::read(&folder.join(FILE)).expect("the file, written")
    };
    assert_eq!(file["version"], 3);
    assert_eq!(file["theme"], "glass");
    assert_eq!(file["tabs"][0]["muted"], true);
    assert_eq!(file["tabs"][0]["pinnedUrl"], "https://a.example/home");
    assert_eq!(
        file["tabs"][0]["url"], "https://a.example/home",
        "closed, it's where it's pinned"
    );
    assert_eq!(
        file["tabs"][1],
        json!({ "kind": "folder", "name": "Later" })
    );
}
