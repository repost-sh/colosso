//! What the first scan lists, and how: the order, what's never listed, what git ignores, and
//! the ignored folders read only when asked for.

use std::sync::Arc;

use colosso_project::{Excludes, Feed, Kind, Options};

use crate::common::{Checkout, ignored, names, until_listed};

#[test]
fn listings_put_folders_first_then_names_as_people_sort_them() {
    let checkout = Checkout::new("order");
    for name in ["file10", "file2", "File1", "z.txt", "_x"] {
        checkout.write(name, "");
    }
    checkout.mkdir("b-dir");
    checkout.mkdir("A-dir");
    let (project, _updates) = checkout.open();
    let root = project.listing("").unwrap();
    assert_eq!(
        names(&root),
        ["A-dir", "b-dir", "_x", "File1", "file2", "file10", "z.txt"]
    );
}

#[test]
fn git_and_finder_files_are_never_listed() {
    let checkout = Checkout::new("never");
    checkout.write(".git/HEAD", "ref: refs/heads/main\n");
    checkout.write(".DS_Store", "");
    checkout.write("sub/.DS_Store", "");
    checkout.write("sub/a", "");
    checkout.write("nested/.git", "gitdir: elsewhere\n");
    let (project, _updates) = checkout.open();
    assert_eq!(names(&project.listing("").unwrap()), ["nested", "sub"]);
    assert_eq!(names(&project.listing("sub").unwrap()), ["a"]);
    assert!(project.listing("nested").unwrap().is_empty());
    assert!(project.listing(".git").is_none());
}

#[test]
fn nested_negated_excluded_and_global_rules_are_followed() {
    let checkout = Checkout::new("rules");
    checkout.write(
        ".gitignore",
        "*.log\n!keep.log\nbuild/\n!build/kept.txt\n/top-only.txt\n",
    );
    checkout.write("sub/.gitignore", "secret.txt\n!a.log\n");
    for file in [
        "a.log",
        "keep.log",
        "build/kept.txt",
        "top-only.txt",
        "excluded.txt",
        "global.txt",
        "sub/a.log",
        "sub/b.log",
        "sub/secret.txt",
        "sub/top-only.txt",
        "sub/excluded.txt",
    ] {
        checkout.write(file, "");
    }
    checkout.write(".git/info/exclude", "excluded.txt\n");
    checkout.write(".git/global-excludes", "global.txt\n");
    let (project, _updates) = checkout.open_with(Options {
        excludes: Excludes::File(checkout.root.join(".git/global-excludes")),
        ..checkout.options()
    });
    let root = project.listing("").unwrap();
    assert_eq!(
        ignored(&root),
        [
            "build",
            "a.log",
            "excluded.txt",
            "global.txt",
            "top-only.txt"
        ]
    );
    let sub = project.listing("sub").unwrap();
    assert_eq!(ignored(&sub), ["b.log", "excluded.txt", "secret.txt"]);
    // A file can't be brought back from inside a folder that's ignored, as in git.
    assert!(project.listing("build").is_none());
}

#[test]
fn ignored_folders_are_read_only_when_asked_one_level_at_a_time() {
    let checkout = Checkout::new("lazy");
    checkout.write(".gitignore", "node_modules/\n");
    checkout.write("node_modules/react/index.js", "");
    checkout.write("node_modules/react/lib/a.js", "");
    checkout.write("node_modules/.bin/react", "");
    let (project, updates) = checkout.open();
    assert_eq!(ignored(&project.listing("").unwrap()), ["node_modules"]);
    assert!(project.listing("node_modules").is_none());

    project.load("node_modules");
    assert!(until_listed(&updates, "node_modules", |listing| listing
        .is_some_and(
            |listing| names(listing) == [".bin", "react"] && ignored(listing) == [".bin", "react"]
        )));
    assert!(project.listing("node_modules/react").is_none());

    project.load("node_modules/react");
    assert!(until_listed(&updates, "node_modules/react", |listing| {
        listing.is_some_and(|listing| ignored(listing) == ["lib", "index.js"])
    }));
    assert!(project.listing("node_modules/react/lib").is_none());
    assert_eq!(&*project.files(), [Arc::from(".gitignore")]);
}

#[cfg(unix)]
#[test]
fn links_are_listed_with_where_they_lead_and_never_followed() {
    use std::os::unix::fs::symlink;

    let checkout = Checkout::new("links");
    checkout.write("real/a.txt", "");
    symlink(checkout.root.join("real"), checkout.root.join("link")).unwrap();
    symlink(
        checkout.root.join("real/a.txt"),
        checkout.root.join("file-link"),
    )
    .unwrap();
    symlink(
        checkout.root.join("nowhere"),
        checkout.root.join("dangling"),
    )
    .unwrap();
    let (project, _updates) = checkout.open();
    let root = project.listing("").unwrap();
    let kinds: Vec<(&str, Kind)> = (root.iter())
        .map(|entry| (&*entry.name, entry.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            ("link", Kind::Symlink { to_dir: true }),
            ("real", Kind::Dir),
            ("dangling", Kind::Symlink { to_dir: false }),
            ("file-link", Kind::Symlink { to_dir: false }),
        ]
    );
    assert!(project.listing("link").is_none());
}

#[test]
fn the_file_list_holds_every_file_git_does_not_ignore() {
    let checkout = Checkout::new("files");
    checkout.write(".gitignore", "target/\n*.tmp\n");
    for file in ["src/main.rs", "src/lib/a.rs", "x.tmp", "target/out"] {
        checkout.write(file, "");
    }
    let (project, _updates) = checkout.open();
    let files = project.files();
    let mut sorted: Vec<&str> = files.iter().map(|path| &**path).collect();
    sorted.sort_unstable();
    assert_eq!(sorted, [".gitignore", "src/lib/a.rs", "src/main.rs"]);
    // Nothing changed, so the same list is handed out again.
    assert!(Arc::ptr_eq(&files, &project.files()));
    assert_eq!(
        project.scanned().map(|scanned| scanned.counts.files),
        Some(3)
    );
}

#[test]
fn a_late_subscriber_hears_what_is_known_already() {
    let checkout = Checkout::new("late");
    checkout.write("src/main.rs", "");
    let (project, _updates) = checkout.open();
    let everything = project.subscribe(Feed::Everything);
    let first = everything.try_next().unwrap();
    let mut listed: Vec<&str> = first.listings.keys().map(|dir| &**dir).collect();
    listed.sort_unstable();
    assert_eq!(listed, ["", "src"]);
    assert!(first.scanned.is_some());
    // Only what's written from here on, and never a listing.
    let written = project.subscribe(Feed::Written);
    assert!(
        written
            .try_next()
            .is_none_or(|update| update.listings.is_empty() && update.scanned.is_none())
    );
}
