use super::stand_in::{document_of, made_by_a_page};
use super::{Restore, StandIns, Then, around, plan, shown};
use koyo_browser_ipc::{PageState, Scroll, Session, SessionEntry};

fn entries(urls: &[&str]) -> Vec<SessionEntry> {
    urls.iter()
        .map(|url| SessionEntry {
            url: (*url).into(),
            title: format!("Title of {url}"),
        })
        .collect()
}

fn urls(entries: &[SessionEntry]) -> Vec<&str> {
    entries.iter().map(|entry| entry.url.as_str()).collect()
}

/// How a page at an address is named here: a stand-in as `~` and the page it stands for.
fn said(url: &str, issued: &StandIns) -> String {
    issued.entry_of(url).map_or_else(|| url.to_owned(), |entry| format!("~{}", entry.url))
}

/// What the browser loads of a plan, in order, the engine finishing each load as asked: the
/// first page, then each the restore asks for as the one before ends; and how it ended.
fn loads(first: &str, restore: Option<Restore>, issued: &StandIns) -> (Vec<String>, Option<Then>) {
    let mut loads = vec![said(first, issued)];
    let Some(mut restore) = restore else {
        return (loads, None);
    };
    let mut at = first.to_owned();
    for _ in 0..50 {
        match restore.loaded(issued.has(&at)) {
            Then::Load(next) => {
                loads.push(said(&next, issued));
                at = next;
            }
            Then::Wait => {}
            done => return (loads, Some(done)),
        }
    }
    (loads, None)
}

const A: &str = "https://a.test/";
const B: &str = "https://b.test/";
const C: &str = "https://c.test/";
const D: &str = "https://d.test/";

#[test]
fn a_history_is_split_around_the_page_it_shows() {
    let (back, forward) = around(entries(&[A, B, C, D]), 2, &StandIns::default());
    assert_eq!((urls(&back), urls(&forward)), (vec![A, B], vec![D]));
    let (back, forward) = around(entries(&[A]), 0, &StandIns::default());
    assert!(back.is_empty() && forward.is_empty());
}

#[test]
fn a_history_keeps_web_pages_only_and_stand_ins_as_the_pages_they_stand_for() {
    let mut issued = StandIns::default();
    let stand_in = issued.issue(&entries(&[B])[0], 1, 0);
    let listed = vec![
        entries(&["file:///Users/x/a.html"])[0].clone(),
        SessionEntry {
            url: stand_in,
            title: String::new(),
        },
        entries(&["chrome://extensions/"])[0].clone(),
        entries(&[C])[0].clone(),
    ];
    let (back, forward) = around(listed, 3, &issued);
    assert_eq!(urls(&back), vec![B]);
    assert_eq!(back[0].title, format!("Title of {B}"));
    assert!(forward.is_empty());
}

#[test]
fn a_history_keeps_the_nearest_pages_each_way() {
    let many: Vec<String> = (0..50).map(|n| format!("https://p{n}.test/")).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    let (back, forward) = around(entries(&many), 25, &StandIns::default());
    assert_eq!((back.len(), forward.len()), (20, 20));
    assert_eq!(back.last().map(|e| e.url.as_str()), Some("https://p24.test/"));
    assert_eq!(forward[0].url, "https://p26.test/");
}

#[test]
fn a_page_with_nothing_to_bring_back_opens_as_it_did() {
    let mut issued = StandIns::default();
    let (first, restore) = plan(C, &Session::default(), &mut issued);
    assert_eq!((first.as_str(), restore), (C, None));
}

#[test]
fn a_page_brought_back_loads_a_stand_in_for_each_page_behind_it_then_itself() {
    let mut issued = StandIns::default();
    let session = Session {
        back: entries(&[A, B]),
        ..Session::default()
    };
    let (first, restore) = plan(C, &session, &mut issued);
    let (loaded, ended) = loads(&first, restore, &issued);
    assert_eq!(loaded, [format!("~{A}"), format!("~{B}"), C.to_owned()]);
    assert_eq!(ended, Some(Then::InPlace(None)), "in place, at its top");
}

#[test]
fn with_pages_ahead_it_ends_on_a_stand_in_that_goes_back_to_its_own() {
    let mut issued = StandIns::default();
    let session = Session {
        back: entries(&[A]),
        forward: entries(&[C, D]),
        ..Session::default()
    };
    let (first, mut restore) = plan(B, &session, &mut issued);
    let mut at = first.clone();
    let mut loaded = vec![said(&first, &issued)];
    while let Some(Then::Load(next)) = restore.as_mut().map(|r| r.loaded(issued.has(&at))) {
        loaded.push(said(&next, &issued));
        at = next;
    }
    assert_eq!(loaded, [A, B, C, D].map(|url| format!("~{url}")));
    assert!(document_of(&at).contains("\"back\":2"), "the last goes back two");
    let mut restore = restore.expect("waiting on the step back");
    assert_eq!(restore.loaded(true), Then::Wait, "the page's own stand-in loads");
    assert_eq!(restore.loaded(false), Then::InPlace(None), "and replaces itself");
}

#[test]
fn a_page_that_is_no_web_page_comes_back_without_its_history_but_scrolled() {
    let mut issued = StandIns::default();
    let session = Session {
        back: entries(&[A]),
        scroll: Scroll { x: 0, y: 640 },
        ..Session::default()
    };
    let (first, restore) = plan("file:///Users/x/notes.html", &session, &mut issued);
    assert_eq!(first, "file:///Users/x/notes.html");
    let (loaded, ended) = loads(&first, restore, &issued);
    assert_eq!(loaded, ["file:///Users/x/notes.html"]);
    assert!(
        matches!(ended, Some(Then::InPlace(Some(script))) if script.ends_with("(0, 640);")),
        "scrolled back once loaded"
    );
}

#[test]
fn another_page_loading_meanwhile_ends_what_is_brought_back() {
    let mut issued = StandIns::default();
    let session = Session {
        back: entries(&[A, B]),
        ..Session::default()
    };
    let (_, restore) = plan(C, &session, &mut issued);
    let mut restore = restore.expect("a history to bring back");
    assert_eq!(restore.loaded(false), Then::Abandoned, "an agent went elsewhere");
}

#[test]
fn a_page_that_never_replaces_its_stand_in_is_given_up_on() {
    let mut issued = StandIns::default();
    let session = Session {
        forward: entries(&[C]),
        ..Session::default()
    };
    let (first, mut restore) = plan(B, &session, &mut issued);
    let mut at = first;
    while let Some(Then::Load(next)) = restore.as_mut().map(|r| r.loaded(issued.has(&at))) {
        at = next;
    }
    let mut restore = restore.expect("waiting on the step back");
    assert_eq!(restore.loaded(true), Then::Wait, "the page's own stand-in loads");
    assert_eq!(
        restore.loaded(true),
        Then::InPlace(None),
        "its replacing ends with no page, a download say: as it is"
    );
}

#[test]
fn the_app_is_shown_the_page_brought_back_while_a_stand_in_shows_then_what_each_stands_for() {
    let mut issued = StandIns::default();
    let session = Session {
        back: entries(&[A]),
        ..Session::default()
    };
    let (first, restore) = plan(B, &session, &mut issued);
    let page = PageState {
        url: first,
        title: A.into(),
        loading: false,
        ..PageState::default()
    };
    let meanwhile = shown(&page, restore.as_ref(), &issued);
    let shown_as = (meanwhile.url.as_str(), meanwhile.title.as_str(), meanwhile.loading);
    assert_eq!(shown_as, (B, "", true));
    let later = shown(&page, None, &issued);
    assert_eq!((later.url.as_str(), later.loading), (A, false));
    assert_eq!(later.title, format!("Title of {A}"));
    let real = PageState {
        url: B.into(),
        loading: true,
        ..PageState::default()
    };
    assert_eq!(shown(&real, restore.as_ref(), &issued), real, "the page itself as it is");
}

#[test]
fn a_page_a_page_made_is_shown_at_its_own_address_whatever_it_holds() {
    let url = made_by_a_page(
        r#"{"url":"https://bank.test/","title":"Bank","x":"</script><h1>Sign in</h1><script>"}"#,
    );
    let page = PageState {
        url,
        title: "Bank".into(),
        ..PageState::default()
    };
    assert_eq!(shown(&page, None, &StandIns::default()), page, "never shown as https://bank.test/");
}

#[test]
fn a_stand_in_this_pane_was_not_given_is_shown_at_its_own_address() {
    let mut another_panes = StandIns::default();
    let url = another_panes.issue(&entries(&["https://bank.test/"])[0], 0, 0);
    let issued = StandIns::default();
    let page = PageState {
        url,
        title: "Bank".into(),
        ..PageState::default()
    };
    assert_eq!(
        shown(&page, None, &issued),
        page,
        "made as this host makes them, word for word, but for no history of this pane's"
    );
}
