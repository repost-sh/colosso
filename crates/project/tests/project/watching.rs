//! Following writes as the platform reports them: listings kept up to date, ignore rules
//! edited, bursts inside ignored folders dropped, and bursts too big to name collapsed.

use std::sync::Arc;

use colosso_project::{Feed, Folder, Git, Options, Subscription, Update, Written};

use crate::common::{Checkout, ignored, names, until, until_listed};

/// Whether `update` says `path` was written.
fn wrote(update: &Update, path: &str) -> bool {
    matches!(&update.written, Written::Paths(paths) if paths.contains(path))
}

/// Whether `update` says `path` may have been written: by name, or as part of a folder
/// written in or of everything, as when the platform lost track of what changed.
fn covers(update: &Update, path: &str) -> bool {
    match &update.written {
        Written::Paths(paths) => paths.contains(path),
        Written::Folders(folders) => (folders.iter())
            .any(|folder| folder.is_empty() || path.starts_with(&format!("{folder}/"))),
        Written::Everything => true,
    }
}

/// Writes `marker` and takes `written`'s updates until one says so, returning every update
/// taken, the marker's included. The platform drops reports when it falls behind, and then
/// says only that it lost track: the marker is then reported as part of something broader.
fn through_marker(checkout: &Checkout, written: &Subscription, marker: &str) -> Vec<Update> {
    checkout.write(marker, "");
    let mut taken = Vec::new();
    let seen = until(written, |update| {
        taken.push(update.clone());
        covers(update, marker)
    });
    assert!(seen, "{marker} was never reported");
    taken
}

#[test]
fn created_removed_and_renamed_files_and_folders_update_their_listings() {
    let checkout = Checkout::new("updates");
    checkout.write("src/a.rs", "");
    let (project, updates) = checkout.open();
    let written = project.subscribe(Feed::Written);

    checkout.write("src/b.rs", "");
    assert!(until_listed(&updates, "src", |listing| listing
        .is_some_and(|listing| names(listing) == ["a.rs", "b.rs"])));
    assert!(until(&written, |update| wrote(update, "src/b.rs")));

    checkout.remove("src/a.rs");
    assert!(until_listed(&updates, "src", |listing| listing
        .is_some_and(|listing| names(listing) == ["b.rs"])));

    checkout.write("src/new/deep/c.rs", "");
    assert!(until_listed(&updates, "src/new/deep", |listing| listing
        .is_some_and(|listing| names(listing) == ["c.rs"])));

    checkout.rename("src/new", "src/old");
    assert!(until_listed(&updates, "src/old/deep", |listing| listing
        .is_some_and(|listing| names(listing) == ["c.rs"])));
    assert!(until_listed(&updates, "src/new", |listing| listing.is_none()));
    assert!(project.listing("src/new/deep").is_none());

    checkout.rename("src/b.rs", "src/d.rs");
    assert!(until_listed(&updates, "src", |listing| listing
        .is_some_and(|listing| names(listing) == ["old", "d.rs"])));

    checkout.remove("src/old");
    assert!(until_listed(&updates, "src/old/deep", |listing| listing.is_none()));
    assert!(until_listed(&updates, "src", |listing| listing
        .is_some_and(|listing| names(listing) == ["d.rs"])));
}

#[test]
fn editing_a_gitignore_flips_what_it_ignores() {
    let checkout = Checkout::new("flip");
    checkout.write("out/a.txt", "");
    checkout.write("out/sub/b.txt", "");
    checkout.write("keep.txt", "");
    let (project, updates) = checkout.open();
    assert!(project.listing("out/sub").is_some());

    checkout.write(".gitignore", "out/\n");
    assert!(until_listed(&updates, "", |listing| listing
        .is_some_and(|listing| ignored(listing) == ["out"])));
    assert!(until_listed(&updates, "out", |listing| listing.is_none()));
    assert!(project.listing("out/sub").is_none());

    checkout.write(".gitignore", "");
    assert!(until_listed(&updates, "out/sub", |listing| listing
        .is_some_and(
            |listing| names(listing) == ["b.txt"] && ignored(listing).is_empty()
        )));

    // A folder's own rules reach the folders listed under it.
    checkout.write("out/.gitignore", "*.txt\n");
    assert!(until_listed(&updates, "out/sub", |listing| listing
        .is_some_and(|listing| ignored(listing) == ["b.txt"])));
    assert_eq!(ignored(&project.listing("out").unwrap()), ["a.txt"]);
}

#[test]
fn editing_the_repositorys_excludes_flips_what_they_ignore() {
    let checkout = Checkout::new("excludes");
    checkout.write(".git/info/exclude", "");
    checkout.write("secret.txt", "");
    checkout.write("src/secret.txt", "");
    let (_project, updates) = checkout.open();
    checkout.write(".git/info/exclude", "secret.txt\n");
    assert!(until_listed(&updates, "src", |listing| listing
        .is_some_and(|listing| ignored(listing) == ["secret.txt"])));
    assert!(until_listed(&updates, "", |listing| listing
        .is_some_and(|listing| ignored(listing) == ["secret.txt"])));
}

#[test]
fn only_the_git_files_consumers_read_are_reported() {
    let checkout = Checkout::new("git");
    checkout.write(".git/HEAD", "ref: refs/heads/main\n");
    let (project, _updates) = checkout.open();
    let written = project.subscribe(Feed::Written);
    // Whatever the setup left for the watcher to report is taken first.
    through_marker(&checkout, &written, "before");
    checkout.write(".git/objects/ab/cdef", "x");
    checkout.write(".git/refs/heads/main.lock", "x");
    checkout.write(".git/index", "x");
    let mut moved = Git::default();
    assert!(until(&written, |update| {
        assert!(update.written.is_empty(), "{:?}", update.written);
        moved.index |= update.git.index;
        moved.head |= update.git.head;
        moved.index
    }));
    assert!(!moved.head, "an object or a lock file moved HEAD");
    checkout.write(".git/refs/heads/main", "x");
    assert!(until(&written, |update| update.git.head));
}

#[test]
fn a_burst_inside_an_ignored_folder_reaches_nobody() {
    let checkout = Checkout::new("ignored-burst");
    checkout.write(".gitignore", "node_modules/\n");
    checkout.write("node_modules/.keep", "");
    checkout.write("src/main.rs", "");
    let (project, _updates) = checkout.open();
    let written = project.subscribe(Feed::Written);
    let start = project.stats();
    // Whatever the setup left for the watcher to report is taken first.
    through_marker(&checkout, &written, "src/before");
    let before = project.stats();
    for n in 0..2_000 {
        checkout.write(&format!("node_modules/pkg{}/f{n}.js", n % 40), "");
    }
    // The platform reports in order: everything about the burst comes before the marker.
    let taken = through_marker(&checkout, &written, "src/after");
    let after = project.stats();
    assert!(after.dropped > before.dropped, "{after:?}");
    assert!(project.listing("node_modules").is_none());
    if after.lost == start.lost {
        assert_eq!(taken.len(), 1, "{taken:?}");
        assert_eq!(
            taken[0].written,
            Written::Paths(["src/after".into()].into())
        );
        return;
    }
    // The platform lost track of the checkout, under load: what was written is then read
    // again whole, and a marker may come late or be part of that. Still, nothing names
    // the ignored folder, nor anything in it.
    for update in &taken {
        let markers = match &update.written {
            Written::Paths(paths) => {
                (paths.iter()).all(|path| ["src/before", "src/after"].contains(&&**path))
            }
            Written::Folders(folders) => {
                (folders.iter()).all(|folder| ["", "src"].contains(&&**folder))
            }
            Written::Everything => true,
        };
        assert!(markers, "{after:?} {taken:?}");
    }
}

#[test]
fn a_burst_past_the_cap_is_taken_up_by_folder_then_whole() {
    let checkout = Checkout::new("capped");
    checkout.write("src/main.rs", "");
    let (project, updates) = checkout.open_with(Options {
        most_written: 8,
        ..checkout.options()
    });
    let written = project.subscribe(Feed::Written);
    for folder in 0..20 {
        for file in 0..20 {
            checkout.write(&format!("burst/d{folder}/f{file}.txt"), "");
        }
    }
    for folder in 0..20 {
        let dir = format!("burst/d{folder}");
        assert!(until_listed(&updates, &dir, |listing| listing
            .is_some_and(|listing| listing.len() == 20)));
    }
    let mut collapsed = false;
    while let Some(update) = written.try_next() {
        collapsed |= matches!(update.written, Written::Folders(_) | Written::Everything);
        assert!(match &update.written {
            Written::Paths(paths) | Written::Folders(paths) => paths.len() <= 8,
            Written::Everything => true,
        });
    }
    assert!(collapsed, "a burst of 400 never went past a cap of 8");
    assert_eq!(project.files().len(), 401);
}

#[test]
fn a_folders_files_stay_shared_until_its_listing_changes() {
    let checkout = Checkout::new("by-folder");
    checkout.write(".gitignore", "*.tmp\n");
    for file in ["src/a.rs", "src/b.tmp", "lib/c.rs"] {
        checkout.write(file, "");
    }
    checkout.mkdir("empty");
    let (project, updates) = checkout.open();
    let folders = project.files_by_folder();
    let files = |folders: &[Folder], dir: &str| {
        let folder = folders.iter().find(|folder| &*folder.path == dir);
        folder.map(|folder| folder.files.clone())
    };
    let mut listed: Vec<&str> = folders.iter().map(|folder| &*folder.path).collect();
    listed.sort_unstable();
    assert_eq!(listed, ["", "lib", "src"]);
    let lib = files(&folders, "lib").unwrap();
    assert_eq!(&*lib, [Arc::from("lib/c.rs")]);

    checkout.write("src/d.rs", "");
    assert!(until_listed(&updates, "src", |listing| listing
        .is_some_and(
            |listing| names(listing) == ["a.rs", "b.tmp", "d.rs"]
        )));
    let after = project.files_by_folder();
    assert!(Arc::ptr_eq(&lib, &files(&after, "lib").unwrap()));
    let src = files(&after, "src").unwrap();
    assert_eq!(&*src, [Arc::from("src/a.rs"), Arc::from("src/d.rs")]);
    assert!(Arc::ptr_eq(&after, &project.files_by_folder()));
}

#[test]
fn a_listings_feed_hears_what_is_listed_and_nothing_written() {
    let checkout = Checkout::new("listings-feed");
    checkout.write("src/a.rs", "");
    let (project, _everything) = checkout.open();
    let listings = project.subscribe(Feed::Listings);
    let first = listings.try_next().expect("what's listed so far");
    assert!(first.scanned.is_some() && first.listings.contains_key("src"));

    checkout.write("src/a.rs", "written over");
    checkout.write("src/b.rs", "");
    let mut written = Vec::new();
    let listed = until(&listings, |update| {
        written.push(update.written.clone());
        let src = update.listings.get("src").cloned().flatten();
        src.is_some_and(|listing| names(&listing) == ["a.rs", "b.rs"])
    });
    assert!(listed, "src/b.rs was never listed");
    assert!(written.iter().all(Written::is_empty), "{written:?}");
}
