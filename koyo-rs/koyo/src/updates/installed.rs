//! What this copy of Koyo is: a release run from its bundle, or why not.

use std::path::{Path, PathBuf};

use super::feed::Feed;
use super::signature::Requirement;
use super::version::Version;
use crate::blocking::Blocking;

/// A release run from its app bundle: everything an update needs to know about this copy.
pub(super) struct Installed {
    /// Where its releases are published, as its bundle says.
    pub(super) feed: Feed,
    /// The bundle this process runs from, which an update replaces.
    pub(super) bundle: PathBuf,
    /// This process's executable, which the installer is run from: app and installer are one
    /// executable, so the two ends of a hand-over can't be of two versions.
    pub(super) exe: PathBuf,
    /// This process's designated requirement: what any image and any copy must satisfy.
    pub(super) trust: Requirement,
    /// This Mac's macOS, which a release must run on.
    pub(super) macos: Version,
}

/// Why this copy doesn't update itself, which About says.
#[derive(Debug)]
pub enum Unreleased {
    /// Its bundle names no feed: only `script/bundle --feed` writes one, for a release.
    NoFeed,
    /// Not run from `<name>.app/Contents/MacOS/`.
    NotBundled,
    /// Its own signature can't be read, so nothing could be checked against it.
    Unsigned(String),
}

/// Finds what this copy is: its bundle from the path of its executable, the feed that
/// bundle's `Info.plist` names, and the requirement its own signature designates.
pub(super) fn find(may: Blocking<'_>) -> Result<Installed, Unreleased> {
    let (exe, bundle) = bundled().ok_or(Unreleased::NotBundled)?;
    let feed = feed_of(&bundle, may).ok_or(Unreleased::NoFeed)?;
    let trust = Requirement::of_self(may).map_err(Unreleased::Unsigned)?;
    Ok(Installed {
        feed,
        bundle,
        exe,
        trust,
        macos: Version::macos(),
    })
}

/// This process's executable and the bundle it runs from, if it runs from one: the one
/// answer, the app's and the installer's, to which Koyo this process is.
pub(super) fn bundled() -> Option<(PathBuf, PathBuf)> {
    let exe = std::env::current_exe().ok()?;
    let bundle = bundle_of(&exe)?;
    Some((exe, bundle))
}

/// The bundle `exe` runs from, if it runs from `<bundle>.app/Contents/MacOS/<exe>`. Pure.
fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let named = macos.file_name()? == "MacOS" && contents.file_name()? == "Contents";
    (named && bundle.extension()? == "app").then(|| bundle.to_owned())
}

/// The feed `bundle` names in its `Info.plist`, if it names one over HTTPS. A value that
/// isn't the address of a feed is logged: the bundle was made wrongly.
fn feed_of(bundle: &Path, may: Blocking<'_>) -> Option<Feed> {
    let info = info(bundle, may)
        .inspect_err(|error| log::warn!("updates: {error}"))
        .ok()?;
    let address = info.get(Feed::KEY)?.as_string()?;
    let feed = Feed::at(address);
    if feed.is_none() {
        log::warn!(
            "updates: the bundle's {} is {address:?}, not the https address of a feed",
            Feed::KEY
        );
    }
    feed
}

/// What the bundle at `bundle` says of itself: its `Contents/Info.plist`. The one reader for
/// the bundle that runs and for a staged one.
pub(super) fn info(bundle: &Path, _: Blocking<'_>) -> Result<plist::Dictionary, String> {
    let path = bundle.join("Contents/Info.plist");
    let cant = |why: &dyn std::fmt::Display| format!("{} can't be read: {why}", path.display());
    let bytes = std::fs::read(&path).map_err(|error| cant(&error))?;
    let value: plist::Value = plist::from_bytes(&bytes).map_err(|error| cant(&error))?;
    value
        .into_dictionary()
        .ok_or_else(|| cant(&"it isn't a dictionary"))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{bundle_of, feed_of};
    use crate::blocking::in_test;
    use crate::test_support::{scratch, write};
    use crate::updates::feed::Feed;

    #[test]
    fn a_bundle_is_found_from_the_executable_in_it() {
        let found = |exe: &str| bundle_of(Path::new(exe));
        assert_eq!(
            found("/Applications/Koyo.app/Contents/MacOS/koyo").as_deref(),
            Some(Path::new("/Applications/Koyo.app"))
        );
        assert_eq!(found("/work/koyo-rs/target/debug/koyo"), None);
        assert_eq!(found("/Applications/Koyo.app/Contents/Helpers/koyo"), None);
        assert_eq!(found("/Applications/Koyo/Contents/MacOS/koyo"), None);
        assert_eq!(found("/koyo"), None);
    }

    fn plist(feed: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>codes.koyo.desktop</string>
  {feed}
</dict></plist>"#
        )
    }

    #[test]
    fn a_bundle_names_its_feed_in_its_info_plist() {
        let bundle = scratch("update-feed-key").join("Koyo.app");
        let named = |feed: &str| {
            write(&bundle, "Contents/Info.plist", plist(feed).as_bytes());
            in_test(|may| feed_of(&bundle, may))
        };
        let address = "https://dl.koyo.codes/releases/aarch64/feed.json";
        let key = format!("<key>KoyoUpdateFeed</key><string>{address}</string>");
        assert_eq!(named(&key), Feed::at(address));
        assert!(Feed::at(address).is_some());
        assert_eq!(named(""), None, "a bundle made here names no feed");
        let plain = "<key>KoyoUpdateFeed</key><string>http://dl.koyo.codes/feed.json</string>";
        assert_eq!(named(plain), None, "a feed is read over https alone");
        assert_eq!(named("<key>KoyoUpdateFeed</key><true/>"), None);
    }

    #[test]
    fn a_folder_with_no_info_plist_names_no_feed() {
        let bundle = scratch("update-feed-none").join("Koyo.app");
        assert_eq!(in_test(|may| feed_of(&bundle, may)), None);
    }
}
