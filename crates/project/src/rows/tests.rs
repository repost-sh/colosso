use std::cell::RefCell;

use super::*;
use crate::entry::order;

/// Listings kept in memory, and the folders asked to be read.
#[derive(Default)]
struct Fake {
    dirs: HashMap<Arc<str>, Arc<[Entry]>>,
    loads: RefCell<Vec<String>>,
}

impl Fake {
    /// Lists `names` in `dir`: a name ending in `/` is a folder, one starting with `!` is
    /// ignored, and one ending in `@` is a link.
    fn put(&mut self, dir: &str, names: &[&str]) -> Arc<[Entry]> {
        let mut entries: Vec<Entry> = names.iter().map(|name| entry(name)).collect();
        entries.sort_by(order);
        let entries: Arc<[Entry]> = entries.into();
        self.dirs.insert(dir.into(), entries.clone());
        entries
    }

    fn loads(&self) -> Vec<String> {
        self.loads.borrow_mut().drain(..).collect()
    }
}

impl Listings for Fake {
    fn listing(&self, dir: &str) -> Option<Arc<[Entry]>> {
        self.dirs.get(dir).cloned()
    }

    fn load(&self, dir: &str) {
        self.loads.borrow_mut().push(dir.to_owned());
    }
}

fn entry(name: &str) -> Entry {
    let ignored = name.starts_with('!');
    let name = name.trim_start_matches('!');
    let kind = if name.ends_with('/') {
        Kind::Dir
    } else if name.ends_with('@') {
        Kind::Symlink { to_dir: true }
    } else {
        Kind::File
    };
    Entry {
        name: name.trim_end_matches(['/', '@']).into(),
        kind,
        ignored,
    }
}

/// An update listing `dir` anew as `listing`.
fn update(dir: &str, listing: Arc<[Entry]>) -> HashMap<Arc<str>, Option<Arc<[Entry]>>> {
    HashMap::from([(Arc::from(dir), Some(listing))])
}

/// The rows as the tree draws them: indented by depth, `/` after a folder, `-` after an
/// open one, `!` before an ignored one.
fn shown(rows: &Rows) -> Vec<String> {
    (rows.rows().iter())
        .map(|row| {
            format!(
                "{}{}{}{}{}",
                "  ".repeat(row.depth as usize),
                if row.entry.ignored { "!" } else { "" },
                row.entry.name,
                if row.entry.is_folder() { "/" } else { "" },
                if row.expanded { "-" } else { "" },
            )
        })
        .collect()
}

/// A checkout with sources, a nested folder, and an ignored build folder not read yet.
fn checkout() -> Fake {
    let mut fake = Fake::default();
    fake.put(
        "",
        &["file10", "src/", "file2", "!target/", "A.md", "docs/"],
    );
    fake.put("src", &["main.rs", "lib.rs", "bin/", "util/"]);
    fake.put("src/bin", &["tool.rs", "deep/"]);
    fake.put("src/bin/deep", &["er/"]);
    fake.put("src/bin/deep/er", &["most.rs"]);
    fake.put("src/util", &["mod.rs"]);
    fake.put("docs", &["guide.md"]);
    fake
}

fn rows_of(fake: &Fake) -> Rows {
    let mut rows = Rows::default();
    rows.reset(fake);
    rows
}

#[test]
fn the_top_is_shown_folders_first_in_natural_order() {
    let rows = rows_of(&checkout());
    assert_eq!(
        shown(&rows),
        ["docs/", "src/", "!target/", "A.md", "file2", "file10"]
    );
}

#[test]
fn a_folder_closed_and_opened_again_shows_the_folders_open_inside_it() {
    let fake = checkout();
    let mut rows = rows_of(&fake);
    assert!(rows.expand("src", &fake));
    assert!(rows.expand("src/bin", &fake));
    assert!(!rows.expand("src/bin", &fake), "it's open already");
    assert_eq!(
        shown(&rows)[1..9],
        [
            "src/-",
            "  bin/-",
            "    deep/",
            "    tool.rs",
            "  util/",
            "  lib.rs",
            "  main.rs",
            "!target/"
        ]
    );
    assert!(rows.collapse("src"));
    assert!(!rows.collapse("src"));
    assert_eq!(shown(&rows)[1..3], ["src/", "!target/"]);
    assert!(rows.expanded.contains("src/bin") && !rows.expanded.contains("src"));
    rows.expand("src", &fake);
    assert_eq!(shown(&rows)[2..5], ["  bin/-", "    deep/", "    tool.rs"]);
    // A file and a folder not shown can't be opened.
    assert!(!rows.expand("A.md", &fake));
    assert!(!rows.expand("src/bin/deep/er", &fake));
}

#[test]
fn every_row_is_found_by_its_path_and_its_folder_by_its_row() {
    let mut fake = checkout();
    fake.put("", &["a/", "a.rs", "A", "b/", "x2", "x10"]);
    fake.put("a", &["b/", "a"]);
    fake.put("a/b", &["c"]);
    fake.put("b", &["a"]);
    let mut rows = rows_of(&fake);
    for dir in ["a", "a/b", "b"] {
        rows.expand(dir, &fake);
    }
    assert_eq!(rows.len(), 10);
    for (index, row) in rows.rows().iter().enumerate() {
        assert_eq!(rows.position(&row.path), Some(index), "{}", row.path);
        let above = row.path.rsplit_once('/').map(|(dir, _)| dir);
        let parent = rows.parent(index).map(|at| &*rows.rows()[at].path);
        assert_eq!(parent, above, "{}", row.path);
    }
    for missing in ["", "c", "a/c", "a/b/c/d", "b/a/x", "x3"] {
        assert_eq!(rows.position(missing), None, "{missing}");
    }
}

#[test]
fn a_new_listing_changes_only_the_rows_of_its_folder() {
    let mut fake = checkout();
    let mut rows = rows_of(&fake);
    rows.expand("src", &fake);
    rows.expand("src/bin", &fake);
    let before = rows.rows().to_vec();
    let listing = fake.put("src", &["main.rs", "new.rs", "bin/", "util/"]);
    let changed = update("src", listing);
    assert!(rows.apply(&changed, &fake));
    assert_eq!(
        shown(&rows)[1..9],
        [
            "src/-",
            "  bin/-",
            "    deep/",
            "    tool.rs",
            "  util/",
            "  main.rs",
            "  new.rs",
            "!target/"
        ]
    );
    // The folder open inside kept its rows.
    assert!(Arc::ptr_eq(&rows.rows()[2].path, &before[2].path));
    // The same listing again, and one for a folder that's closed, change nothing.
    assert!(!rows.apply(&changed, &fake));
    let closed = fake.put("src/util", &["mod.rs", "more.rs"]);
    assert!(!rows.apply(&update("src/util", closed), &fake));
    // A folder gone takes its rows with it, and a new one at the top is put in its place.
    fake.dirs.remove("src/bin");
    let top = fake.put("", &["file10", "src/", "file2", "!target/", "A.md", "zz/"]);
    let changed = HashMap::from([(Arc::from("src/bin"), None), (Arc::from(""), Some(top))]);
    assert!(rows.apply(&changed, &fake));
    assert_eq!(
        shown(&rows),
        [
            "src/-",
            "  bin/-",
            "  util/",
            "  main.rs",
            "  new.rs",
            "!target/",
            "zz/",
            "A.md",
            "file2",
            "file10"
        ]
    );
}

#[test]
fn revealing_a_path_opens_the_folders_above_it_at_once() {
    let fake = checkout();
    let mut rows = rows_of(&fake);
    let at = rows.reveal("src/bin/deep/er/most.rs", &fake);
    assert_eq!(at, Reveal::Shown(5));
    assert_eq!(rows.rows()[5].depth, 4);
    for dir in ["src", "src/bin", "src/bin/deep", "src/bin/deep/er"] {
        assert!(rows.rows()[rows.position(dir).unwrap()].expanded, "{dir}");
    }
    // Revealing what's shown changes nothing.
    let before = rows.rows().to_vec();
    assert_eq!(
        rows.reveal("src/lib.rs", &fake),
        Reveal::Shown(rows.position("src/lib.rs").unwrap())
    );
    assert_eq!(rows.rows(), before);
    assert!(fake.loads().is_empty());
}

#[test]
fn a_path_not_there_is_missing_rather_than_waited_for() {
    let fake = checkout();
    let mut rows = rows_of(&fake);
    assert_eq!(rows.reveal("src/gone.rs", &fake), Reveal::Missing);
    assert_eq!(rows.reveal("gone/deeper/file.rs", &fake), Reveal::Missing);
    assert_eq!(
        rows.reveal("src/bin/deep/er/gone.rs", &fake),
        Reveal::Missing
    );
    // The folders that are there are open all the same.
    assert!(rows.rows()[rows.position("src/bin/deep/er").unwrap()].expanded);
}

#[test]
fn an_ignored_folder_is_asked_for_and_shown_once_read() {
    let mut fake = checkout();
    let mut rows = rows_of(&fake);
    assert!(rows.expand("target", &fake));
    assert_eq!(fake.loads(), ["target"]);
    assert_eq!(rows.position("target").map(|at| rows.len() - at), Some(4));
    // Revealing a file inside it opens the folders on the way, asking for each as it comes.
    assert_eq!(rows.reveal("target/debug/app", &fake), Reveal::Pending);
    let listing = fake.put("target", &["!debug/", "!CACHEDIR.TAG"]);
    assert!(rows.apply(&update("target", listing), &fake));
    assert_eq!(fake.loads(), ["target/debug"]);
    let listing = fake.put("target/debug", &["!app"]);
    rows.apply(&update("target/debug", listing), &fake);
    let Reveal::Shown(at) = rows.reveal("target/debug/app", &fake) else {
        panic!("target/debug/app isn't shown");
    };
    assert_eq!(
        shown(&rows)[at - 2..=at],
        ["!target/-", "  !debug/-", "    !app"]
    );
    // A link to a folder isn't opened: nothing is read through one.
    fake.put("", &["link@", "src/"]);
    let mut rows = rows_of(&fake);
    assert!(!rows.expand("link", &fake));
}
