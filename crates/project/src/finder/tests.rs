use std::collections::BTreeMap;
use std::sync::Arc;

use super::*;
use crate::entry::natural;

/// `paths` by folder, each folder's files in their listing's order.
fn folders(paths: &[&str]) -> Vec<Folder> {
    let mut by_dir: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for path in paths {
        let dir = path.rsplit_once('/').map_or("", |(dir, _)| dir);
        by_dir.entry(dir).or_default().push(path);
    }
    (by_dir.into_iter())
        .map(|(dir, mut files)| {
            files.sort_by(|a, b| natural(a, b));
            Folder {
                path: dir.into(),
                files: files.into_iter().map(Arc::from).collect(),
            }
        })
        .collect()
}

/// `folders` of `paths`, each folder whose files are as they were in `before` the very
/// same, as the project hands them out.
fn refolder(before: &[Folder], paths: &[&str]) -> Vec<Folder> {
    let mut after = folders(paths);
    for folder in &mut after {
        let same = before
            .iter()
            .find(|was| was.path == folder.path && was.files == folder.files);
        if let Some(same) = same {
            folder.files = same.files.clone();
        }
    }
    after
}

fn finder(paths: &[&str]) -> Finder {
    let finder = Finder::new();
    finder.set_files(&folders(paths));
    finder
}

/// What a search for `typed` found, none asked for after it.
fn search(finder: &Finder, typed: &str, most: usize) -> Results {
    (finder.search(typed, most)).expect("no later search was asked for")
}

fn found(finder: &Finder, typed: &str) -> Vec<String> {
    let results = search(finder, typed, 50);
    (results.found.iter())
        .map(|found| found.path.to_string())
        .collect()
}

fn first(finder: &Finder, typed: &str) -> Option<String> {
    found(finder, typed).into_iter().next()
}

const REPO: &[&str] = &[
    "Cargo.toml",
    "README.md",
    "crates/colosso/src/main.rs",
    "crates/colosso/src/changes/view.rs",
    "crates/colosso/src/changes/mod.rs",
    "crates/colosso/src/checkout/mod.rs",
    "crates/colosso/src/cache/hover.rs",
    "crates/project/src/lib.rs",
    "crates/project/src/domain/rules.rs",
    "src/main.rs",
    "src/maintainers.rs",
];

#[test]
fn a_file_is_found_by_its_name_shortest_path_first() {
    let finder = finder(REPO);
    assert_eq!(first(&finder, "mainrs").as_deref(), Some("src/main.rs"));
    let found = found(&finder, "mainrs");
    assert_eq!(
        found.get(1).map(String::as_str),
        Some("crates/colosso/src/main.rs")
    );
}

#[test]
fn a_file_is_found_by_its_folders_initials() {
    let finder = finder(REPO);
    assert_eq!(
        first(&finder, "chv").as_deref(),
        Some("crates/colosso/src/changes/view.rs")
    );
}

#[test]
fn a_match_in_the_name_beats_one_in_the_folders() {
    let finder = finder(&["src/view/index.rs", "src/ui/preview.rs"]);
    assert_eq!(
        found(&finder, "view"),
        ["src/ui/preview.rs", "src/view/index.rs"]
    );
}

#[test]
fn case_matters_only_once_a_capital_is_typed() {
    let finder = finder(&["src/Main.java", "src/main.rs"]);
    assert_eq!(found(&finder, "main").len(), 2);
    assert_eq!(found(&finder, "Main"), ["src/Main.java"]);
}

#[test]
fn slashes_match_between_folders() {
    let finder = finder(REPO);
    assert_eq!(first(&finder, "src/main").as_deref(), Some("src/main.rs"));
    assert_eq!(
        found(&finder, "colosso/main"),
        ["crates/colosso/src/main.rs"]
    );
}

#[test]
fn recent_files_come_first_among_equal_matches() {
    let finder = finder(&["a/util.rs", "b/util.rs"]);
    assert_eq!(found(&finder, "util"), ["a/util.rs", "b/util.rs"]);
    finder.set_recent(&["b/util.rs".into()]);
    let results = search(&finder, "util", 50);
    let recent: Vec<(&str, bool)> = (results.found.iter())
        .map(|found| (&*found.path, found.recent))
        .collect();
    assert_eq!(recent, [("b/util.rs", true), ("a/util.rs", false)]);
}

#[test]
fn no_query_lists_recent_files_then_the_rest_in_the_trees_order() {
    let finder = finder(&[
        "README.md",
        "src/lib.rs",
        "src/main.rs",
        "src/a/x.rs",
        "b/y.rs",
    ]);
    finder.set_recent(&["src/main.rs".into(), "gone.rs".into()]);
    let results = search(&finder, "", 50);
    assert_eq!(results.matched, 5);
    let listed: Vec<(&str, bool)> = (results.found.iter())
        .map(|found| (&*found.path, found.recent))
        .collect();
    assert_eq!(
        listed,
        [
            ("src/main.rs", true),
            ("b/y.rs", false),
            ("src/a/x.rs", false),
            ("src/lib.rs", false),
            ("README.md", false),
        ]
    );
    assert_eq!(search(&finder, ":12", 2).found.len(), 2);
}

#[test]
fn highlights_are_the_bytes_that_matched() {
    let finder = finder(&["src/main.rs", "docs/naïve.md"]);
    let results = search(&finder, "mainrs:42", 50);
    assert_eq!(results.query.line, Some(42));
    assert_eq!(results.found[0].name, 4);
    assert_eq!(results.highlights(0), [4..8, 9..11]);
    let naive = search(&finder, "nav", 50);
    assert_eq!(&*naive.found[0].path, "docs/naïve.md");
    assert_eq!(naive.highlights(0), [5..7, 9..10]);
    assert!(naive.highlights(1).is_empty());
}

#[test]
fn files_added_and_removed_are_found_and_no_longer_found() {
    let mut paths = REPO.to_vec();
    let mut listed = folders(&paths);
    let finder = Finder::new();
    assert!(finder.set_files(&listed));
    assert!(!finder.set_files(&refolder(&listed, &paths)));
    assert_eq!(
        found(&finder, "hovr"),
        ["crates/colosso/src/cache/hover.rs"]
    );
    paths.push("crates/colosso/src/hover/mod.rs");
    paths.push("new/hover_new.rs");
    listed = refolder(&listed, &paths);
    assert!(finder.set_files(&listed));
    assert_eq!(
        found(&finder, "hovr"),
        [
            "crates/colosso/src/cache/hover.rs",
            "new/hover_new.rs",
            "crates/colosso/src/hover/mod.rs"
        ]
    );
    assert_eq!(found(&finder, "hovrne"), ["new/hover_new.rs"]);
    paths.retain(|path| !path.starts_with("crates/colosso/src/cache"));
    assert!(finder.set_files(&refolder(&listed, &paths)));
    assert_eq!(found(&finder, "hovr").len(), 2);
    assert_eq!(finder.len(), paths.len());
}

/// Each prefix typed on and erased again, against a fresh finder searching it at once.
fn typed_as_searched_afresh(finder: &Finder, paths: &[&str], typed: &str) {
    let prefixes = (1..=typed.len()).chain((0..typed.len()).rev());
    for end in prefixes.filter(|end| typed.is_char_boundary(*end)) {
        let fresh = search(&finder_of(paths), &typed[..end], 1000);
        let kept = search(finder, &typed[..end], 1000);
        assert_eq!(kept.found, fresh.found, "{:?}", &typed[..end]);
        assert_eq!(kept.matched, fresh.matched);
    }
}

fn finder_of(paths: &[&str]) -> Finder {
    finder(paths)
}

#[test]
fn typing_on_and_erasing_find_what_a_fresh_search_finds() {
    let finder = finder(REPO);
    for typed in ["crmain", "src !ma", "^ma", "rs$", "co mod"] {
        typed_as_searched_afresh(&finder, REPO, typed);
    }
}

#[test]
fn many_threads_find_what_one_finds() {
    let paths: Vec<String> = (0..3)
        .flat_map(|dir| (0..7000).map(move |file| format!("dir-{dir}/sub/file-{file}.rs")))
        .collect();
    let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
    let many = finder(&paths);
    let one = Finder::with_threads(1);
    one.set_files(&folders(&paths));
    for typed in ["f", "fi", "file-12", "d2 99", ""] {
        let (many, one) = (search(&many, typed, 200), search(&one, typed, 200));
        assert_eq!(many.found, one.found, "{typed:?}");
        assert_eq!(many.matched, one.matched);
    }
    assert_eq!(search(&many, "file-6999", 5).matched, 3);
}

#[test]
fn searches_kept_take_up_files_changed_meanwhile() {
    let mut paths = REPO.to_vec();
    let mut listed = folders(&paths);
    let finder = Finder::new();
    finder.set_files(&listed);
    for end in 1..=4 {
        search(&finder, &"crmo"[..end], 50);
    }
    paths.push("crates/colosso/src/more/mod.rs");
    paths.push("crates/colosso/src/changes/remote.rs");
    paths.retain(|path| *path != "crates/colosso/src/checkout/mod.rs");
    listed = refolder(&listed, &paths);
    assert!(finder.set_files(&listed));
    typed_as_searched_afresh(&finder, &paths, "crmo");
}

#[test]
fn a_search_gives_way_to_one_asked_for_after_it() {
    let finder = finder(REPO);
    let (earlier, later) = (finder.ask(), finder.ask());
    assert!(finder.search_for(earlier, "crmo", 50).is_none());
    let latest = finder.search_for(later, "crm", 50);
    let found = |results: Results| results.found.into_iter().map(|found| found.path);
    let fresh = search(&finder_of(REPO), "crm", 50);
    assert!(latest.map(found).into_iter().flatten().eq(found(fresh)));
    // What gave way left nothing half matched for the next keystroke to narrow.
    typed_as_searched_afresh(&finder, REPO, "crmo");
}
