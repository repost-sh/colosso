//! The history ranks as Firefox's address bar does, and keeps what it's told.

use std::path::PathBuf;

use gpui_kit::TestAppContext;

use super::address::{display_url, host};
use super::{History, How, Kind, Place, Use};

const DAY: i64 = 24 * 60 * 60;
/// A fixed now, so ages are exact.
const NOW: i64 = 1_800_000_000;

fn place(worktree: &str, project: &str) -> Place {
    Place {
        worktree: PathBuf::from(worktree),
        project: PathBuf::from(project),
    }
}

fn here() -> Place {
    place("/work/app-feature", "/work/app")
}

/// Uses `text` of `kind` `days_ago`, as `how`, in `place`.
fn use_(history: &mut History, kind: Kind, text: &str, days_ago: i64, how: How, place: &Place) {
    history.used_at(
        kind,
        text,
        Use {
            at: NOW - days_ago * DAY,
            how,
        },
        place,
    );
}

fn page(history: &mut History, url: &str, title: &str, days_ago: i64, how: How) {
    use_(
        history,
        Kind::Page,
        url,
        days_ago,
        how,
        &place("/elsewhere", "/elsewhere"),
    );
    if let Some(entry) = history.find_mut(Kind::Page, url) {
        title.clone_into(&mut entry.title);
    }
}

fn texts(found: &[super::Found]) -> Vec<&str> {
    found.iter().map(|found| found.text.as_str()).collect()
}

#[test]
fn frecency_weighs_typing_over_following_and_halves_every_thirty_days() {
    let mut history = History::default();
    page(&mut history, "https://typed.dev/", "Typed", 0, How::Typed);
    page(
        &mut history,
        "https://followed.dev/",
        "Followed",
        0,
        How::Followed,
    );
    page(&mut history, "https://old.dev/", "Old", 30, How::Typed);
    let strength = |history: &History, url: &str| {
        let entry = history
            .of(Kind::Page)
            .find(|entry| entry.text == url)
            .unwrap();
        super::rank::strength(entry, NOW)
    };
    assert!((strength(&history, "https://typed.dev/") - 100.).abs() < 1e-6);
    assert!((strength(&history, "https://followed.dev/") - 50.).abs() < 1e-6);
    let old = strength(&history, "https://old.dev/");
    assert!((old - 50.).abs() < 1e-6, "half of typed, thirty days on");
    // Used three times, it counts three times as much.
    page(
        &mut history,
        "https://followed.dev/",
        "Followed",
        0,
        How::Followed,
    );
    page(
        &mut history,
        "https://followed.dev/",
        "Followed",
        0,
        How::Followed,
    );
    assert!((strength(&history, "https://followed.dev/") - 150.).abs() < 1e-6);
}

#[test]
#[expect(clippy::single_range_in_vec_init, reason = "one word's bytes matched")]
fn typed_words_find_pages_holding_every_one_best_first() {
    let mut history = History::default();
    page(
        &mut history,
        "https://github.com/zen-browser/desktop",
        "Zen Browser",
        1,
        How::Followed,
    );
    page(&mut history, "https://github.com/", "GitHub", 0, How::Typed);
    page(
        &mut history,
        "https://docs.rs/gpui",
        "gpui - Rust",
        0,
        How::Followed,
    );
    let found = history.find(Kind::Page, "git", &here(), 10, NOW);
    assert_eq!(
        texts(&found),
        [
            "https://github.com/",
            "https://github.com/zen-browser/desktop"
        ]
    );
    // Every word has to be there, in the title or the address.
    let found = history.find(Kind::Page, "zen desk", &here(), 10, NOW);
    assert_eq!(texts(&found), ["https://github.com/zen-browser/desktop"]);
    assert_eq!(found[0].title, "Zen Browser");
    assert_eq!(found[0].title_matched, [0..3]);
    assert_eq!(found[0].detail, "github.com/zen-browser/desktop");
    assert_eq!(found[0].detail_matched, [11..14, 23..27]);
    assert!(
        history
            .find(Kind::Page, "zen nothing", &here(), 10, NOW)
            .is_empty()
    );
}

#[test]
fn a_word_starts_where_firefox_says_and_anywhere_counts_when_few_do() {
    // Whether the words hold the token, and whether it starts a word in them.
    let holds = |words: &str, token: &str| {
        let mut history = History::default();
        use_(&mut history, Kind::Search, words, 0, How::Typed, &here());
        let entry = history.of(Kind::Search).next().unwrap();
        super::rank::holds(entry, &[token.to_owned()])
    };
    assert_eq!(holds("GitHub", "hub"), Some(true), "a hump starts a word");
    assert_eq!(holds("www.google.com", "goo"), Some(true));
    assert_eq!(holds("google", "oogle"), Some(false));
    assert_eq!(
        holds("Grüße aus Köln", "köln"),
        Some(true),
        "and so does text that isn't ASCII"
    );
    assert_eq!(holds("Grüße aus Köln", "öln"), Some(false));
    assert_eq!(holds("google", "bing"), None);
    // With few found at words' starts, those found anywhere are found too.
    let mut history = History::default();
    page(&mut history, "https://google.com/", "Google", 0, How::Typed);
    assert_eq!(
        texts(&history.find(Kind::Page, "oogle", &here(), 10, NOW)),
        ["https://google.com/"]
    );
}

#[test]
fn what_was_picked_after_typing_comes_first_when_typed_again() {
    let mut history = History::default();
    page(&mut history, "https://github.com/", "GitHub", 0, How::Typed);
    page(
        &mut history,
        "https://gitlab.com/",
        "GitLab",
        5,
        How::Followed,
    );
    assert_eq!(
        history.find(Kind::Page, "git", &here(), 10, NOW)[0].text,
        "https://github.com/"
    );
    history.learn(Kind::Page, "gitl".into(), "https://gitlab.com/", NOW);
    let found = history.find(Kind::Page, "git", &here(), 10, NOW);
    assert_eq!(
        texts(&found),
        ["https://gitlab.com/", "https://github.com/"]
    );
    assert!(found[0].learned);
    // Picked again it counts more, a tenth of it fading each pick, and it fades by the day.
    history.learn(Kind::Page, "gitl".into(), "https://gitlab.com/", NOW);
    let learned = &history.learned[0];
    assert!((learned.uses - 1.9).abs() < 1e-6);
    let later = super::rank::faded(learned, NOW + 90 * DAY);
    assert!(later < 1.9 * 0.11, "{later}");
}

#[test]
fn with_nothing_typed_the_top_sites_show_one_page_a_host() {
    let mut history = History::default();
    page(&mut history, "https://github.com/", "GitHub", 0, How::Typed);
    page(
        &mut history,
        "https://github.com/koyo",
        "Koyo",
        0,
        How::Typed,
    );
    page(
        &mut history,
        "https://www.github.com/x",
        "X",
        0,
        How::Followed,
    );
    // A link followed once doesn't make a top site.
    page(&mut history, "https://once.dev/", "Once", 0, How::Followed);
    let found = history.find(Kind::Page, "", &here(), 8, NOW);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].text.starts_with("https://github.com/"));
}

#[test]
fn words_with_nothing_typed_are_the_latest_those_used_here_first() {
    let mut history = History::default();
    let elsewhere = place("/other", "/other");
    use_(
        &mut history,
        Kind::Command,
        "cargo test",
        2,
        How::Typed,
        &here(),
    );
    use_(&mut history, Kind::Command, "ls", 0, How::Typed, &elsewhere);
    use_(
        &mut history,
        Kind::Command,
        "npm run dev",
        1,
        How::Typed,
        &place("/work/app-main", "/work/app"),
    );
    let found = history.find(Kind::Command, "", &here(), 10, NOW);
    assert_eq!(texts(&found), ["npm run dev", "cargo test", "ls"]);
    // Typed, words that start with it rank above words that hold it.
    use_(
        &mut history,
        Kind::Command,
        "git status",
        0,
        How::Typed,
        &elsewhere,
    );
    use_(
        &mut history,
        Kind::Command,
        "echo git",
        0,
        How::Typed,
        &elsewhere,
    );
    use_(
        &mut history,
        Kind::Command,
        "echo git",
        0,
        How::Typed,
        &elsewhere,
    );
    let found = history.find(Kind::Command, "git", &here(), 10, NOW);
    assert_eq!(texts(&found), ["git status", "echo git"]);
}

#[test]
fn a_host_typed_before_completes_what_is_typed() {
    let mut history = History::default();
    page(
        &mut history,
        "https://www.google.com/",
        "Google",
        0,
        How::Typed,
    );
    page(&mut history, "https://gogs.io/", "Gogs", 0, How::Followed);
    assert_eq!(history.autofill("goo", NOW).as_deref(), Some("google.com"));
    assert_eq!(history.autofill("GOO", NOW).as_deref(), Some("google.com"));
    // Only hosts typed in lately complete, and nothing with a space or a path.
    assert_eq!(history.autofill("gog", NOW), None);
    assert_eq!(history.autofill("goo gle", NOW), None);
    assert_eq!(history.autofill("google.com/", NOW), None);
    assert_eq!(history.autofill("", NOW), None);
    let mut old = History::default();
    page(&mut old, "https://google.com/", "Google", 100, How::Typed);
    assert_eq!(old.autofill("goo", NOW), None, "typed too long ago");
}

#[test]
fn addresses_show_as_firefox_shows_them() {
    assert_eq!(display_url("https://www.google.com/"), "google.com");
    assert_eq!(
        display_url("https://github.com/zen-browser/"),
        "github.com/zen-browser/"
    );
    assert_eq!(
        display_url("http://localhost:3000/"),
        "http://localhost:3000"
    );
    assert_eq!(
        display_url("http://www.example.com/a"),
        "http://www.example.com/a"
    );
    assert_eq!(
        host("https://www.GitHub.com/x?y").as_deref(),
        Some("github.com")
    );
    assert_eq!(
        host("http://localhost:3000/").as_deref(),
        Some("localhost:3000")
    );
    assert_eq!(host("about:blank"), None);
}

#[test]
fn entries_remember_where_they_were_used_and_what_is_kept_is_bounded() {
    let mut history = History::default();
    for n in 0..6 {
        let worktree = format!("/work/{n}");
        use_(
            &mut history,
            Kind::Prompt,
            "hello",
            0,
            How::Typed,
            &place(&worktree, "/work"),
        );
    }
    let entry = history.of(Kind::Prompt).next().unwrap();
    assert_eq!(entry.count, 6);
    assert_eq!(entry.worktrees.len(), 4, "the latest four");
    assert_eq!(entry.worktrees[0], PathBuf::from("/work/5"));
    assert_eq!(entry.projects, [PathBuf::from("/work")]);
    for _ in 0..12 {
        use_(&mut history, Kind::Prompt, "hello", 0, How::Typed, &here());
    }
    assert_eq!(
        history.of(Kind::Prompt).next().unwrap().uses.len(),
        super::SAMPLED
    );
}

#[gpui_kit::test]
fn a_visit_counts_and_a_page_retitled_or_the_engines_own_does_not(cx: &mut TestAppContext) {
    cx.update(|cx| {
        super::init(cx);
        super::visited("https://example.com/", "", How::Typed, &here(), cx);
        super::retitled("https://example.com/", "Example", cx);
        super::visited("https://example.com/", "", How::Followed, &here(), cx);
        super::retitled("https://elsewhere.example/", "Not kept", cx);
        super::visited("about:blank", "", How::Followed, &here(), cx);
        let history = super::get(cx);
        let pages: Vec<_> = history.of(Kind::Page).collect();
        assert_eq!(pages.len(), 1);
        assert_eq!((pages[0].count, pages[0].title.as_str()), (2, "Example"));
        assert_eq!(pages[0].uses[0].how, How::Typed);
        super::used(Kind::Search, "  rust gpui ", &here(), cx);
        super::picked(Kind::Search, "rust", "rust gpui", cx);
        super::remove(Kind::Page, "https://example.com/", cx);
        let history = super::get(cx);
        assert_eq!(history.of(Kind::Page).count(), 0);
        assert_eq!(history.of(Kind::Search).next().unwrap().text, "rust gpui");
        super::clear_browsing(cx);
        assert_eq!(super::get(cx).of(Kind::Search).count(), 0);
    });
}

#[gpui_kit::test]
fn changes_are_written_a_moment_after_the_first_however_steady_they_come(cx: &mut TestAppContext) {
    let file = crate::test_support::scratch("history-saves").join(super::FILE);
    cx.update(|cx| {
        super::init(cx);
        cx.global_mut::<History>().file = crate::kept_file::KeptFile::new(Some(file.clone()));
    });
    let said = |text: &str, cx: &mut TestAppContext| {
        cx.update(|cx| super::used(Kind::Prompt, text, &here(), cx));
    };
    let written = || std::fs::read_to_string(&file).unwrap_or_default();
    said("one", cx);
    cx.executor().advance_clock(super::SAVE_DELAY / 2);
    said("two", cx);
    cx.run_until_parked();
    assert_eq!(written(), "", "not yet");
    cx.executor().advance_clock(super::SAVE_DELAY / 2);
    cx.run_until_parked();
    assert!(
        written().contains("\"one\"") && written().contains("\"two\""),
        "both, a moment after the first"
    );
    said("three", cx);
    cx.executor().advance_clock(super::SAVE_DELAY);
    cx.run_until_parked();
    assert!(
        written().contains("\"three\""),
        "and the next change, a moment after it"
    );
}
