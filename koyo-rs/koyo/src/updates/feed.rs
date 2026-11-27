//! What the release pipeline publishes, as the app reads it (`docs/updates.md` 2): where a
//! bundle's feed is, the feed's three fields, and the image beside it. The whole published
//! layout is known here and nowhere else in the app.

use serde::Deserialize;

use super::fetch::Fetch;
use super::version::Version;
use crate::blocking::Blocking;

/// The most a feed may weigh: three fields, with room for those the pipeline adds.
const MOST: u64 = 64 * 1024;

/// The most an image may weigh, 8 GiB: about forty times today's 200 MB, so no release comes
/// near it, and a feed that says more is malformed.
const HEAVIEST: u64 = 8 * 1024 * 1024 * 1024;

/// Where a bundle's releases are published: the address of its feed file, which
/// `script/bundle --feed` writes into the bundle's `Info.plist` and the signature seals. Made
/// only of an `https://` address of a file, so nothing is ever read over anything else, and
/// the image is always a file in the feed's own folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Feed {
    address: String,
    /// Where the feed file's name starts in `address`: what comes before is its folder.
    name: usize,
}

impl Feed {
    /// The `Info.plist` key a released bundle names its feed under.
    pub(super) const KEY: &str = "KoyoUpdateFeed";

    /// The feed at `address`, if that's the plain `https://` address of a file: a host, a
    /// path that ends in a name, and neither a query nor a fragment.
    pub(super) fn at(address: &str) -> Option<Feed> {
        let rest = address.strip_prefix("https://")?;
        let (folder, name) = rest.rsplit_once('/')?;
        let plain = !rest.contains(['?', '#']) && !rest.contains(char::is_whitespace);
        (plain && !folder.is_empty() && !folder.starts_with('/') && !name.is_empty()).then(|| Feed {
            address: address.to_owned(),
            name: address.len() - name.len(),
        })
    }

    pub(super) fn address(&self) -> &str {
        &self.address
    }

    /// Where the image of release `version` is: `Koyo-<version>.dmg`, beside the feed. Both
    /// sides derive the name from the version, so the feed carries no file name and no URL.
    pub(super) fn image(&self, version: Version) -> String {
        format!("{}Koyo-{version}.dmg", &self.address[..self.name])
    }
}

/// The feed as published. Unknown fields are ignored, so the pipeline may add one; these three
/// are read by every Koyo ever shipped and are never renamed, retyped or removed.
#[derive(Deserialize)]
struct Published {
    version: String,
    minimum_macos: String,
    bytes: u64,
}

/// A release, as the feed says it. Its fields decide only whether to offer it and how far a
/// download has come; what gets installed is decided by the image (`staged.rs`). A feed
/// isn't trusted and `bytes` bounds what a download writes to the disk, so no release is made
/// of a feed that says more than [`HEAVIEST`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub(super) version: Version,
    /// The image's size, which progress is shown against and a download may not pass.
    pub(super) bytes: u64,
    pub(super) minimum_macos: Version,
}

/// What the feed says to this Koyo on this Mac.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Answer {
    Newer(Release),
    NeedsMacos(Release),
    Current,
}

/// What `body`, the feed as fetched, says to Koyo `running` on macOS `macos`. Pure.
pub(super) fn answer(body: &[u8], running: Version, macos: Version) -> Result<Answer, String> {
    let published: Published =
        serde_json::from_slice(body).map_err(|error| format!("the feed can't be read: {error}"))?;
    if published.bytes > HEAVIEST {
        let bytes = published.bytes;
        return Err(format!("the feed says a release of {bytes} bytes, more than any weighs"));
    }
    let release = Release {
        version: published.version.parse()?,
        bytes: published.bytes,
        minimum_macos: published.minimum_macos.parse()?,
    };
    Ok(if release.version <= running {
        Answer::Current
    } else if release.minimum_macos > macos {
        Answer::NeedsMacos(release)
    } else {
        Answer::Newer(release)
    })
}

/// Reads `feed` and says what it holds for this Koyo on macOS `macos`. Blocks.
pub(super) fn look(
    fetch: &dyn Fetch,
    feed: &Feed,
    macos: Version,
    may: Blocking<'_>,
) -> Result<Answer, String> {
    let body = fetch.read(feed.address(), MOST, may)?;
    answer(&body, Version::RUNNING, macos)
}

#[cfg(test)]
mod tests {
    use super::{Answer, Feed, Release, answer};
    use crate::updates::version::Version;

    fn version(text: &str) -> Version {
        text.parse().unwrap()
    }

    fn release(number: &str, minimum_macos: &str) -> Release {
        Release {
            version: version(number),
            bytes: 199_876_543,
            minimum_macos: version(minimum_macos),
        }
    }

    fn said(body: &str) -> Result<Answer, String> {
        answer(body.as_bytes(), version("0.2.0"), version("14.5"))
    }

    const NEWER: &str = r#"{ "version": "0.3.0", "minimum_macos": "13.0", "bytes": 199876543 }"#;

    #[test]
    fn a_feed_is_the_https_address_of_a_file() {
        let feed = Feed::at("https://dl.koyo.codes/releases/aarch64/feed.json").unwrap();
        assert_eq!(feed.address(), "https://dl.koyo.codes/releases/aarch64/feed.json");
        for address in [
            "http://dl.koyo.codes/aarch64/feed.json",
            "https://dl.koyo.codes",
            "https://dl.koyo.codes/",
            "https:///feed.json",
            "https://dl.koyo.codes/feed.json?token=1",
            "https://dl.koyo.codes/feed.json#top",
            "https://dl.koyo.codes/a folder/feed.json",
            "file:///tmp/feed.json",
            "",
        ] {
            assert_eq!(Feed::at(address), None, "{address:?} was taken for a feed");
        }
    }

    #[test]
    fn the_image_is_beside_the_feed_and_named_by_the_version() {
        let feed = Feed::at("https://dl.koyo.codes/releases/aarch64/feed.json").unwrap();
        assert_eq!(
            feed.image(version("0.3.0")),
            "https://dl.koyo.codes/releases/aarch64/Koyo-0.3.0.dmg"
        );
        let feed = Feed::at("https://dl.koyo.codes/feed.json").unwrap();
        assert_eq!(feed.image(version("1.0.0")), "https://dl.koyo.codes/Koyo-1.0.0.dmg");
    }

    #[test]
    fn a_newer_release_this_mac_runs_is_offered() {
        assert_eq!(said(NEWER), Ok(Answer::Newer(release("0.3.0", "13.0"))));
    }

    #[test]
    fn the_running_release_or_an_older_one_is_nothing_new() {
        let equal = r#"{ "version": "0.2.0", "minimum_macos": "13.0", "bytes": 1 }"#;
        let older = r#"{ "version": "0.1.9", "minimum_macos": "99.0", "bytes": 1 }"#;
        assert_eq!(said(equal), Ok(Answer::Current));
        assert_eq!(said(older), Ok(Answer::Current));
    }

    #[test]
    fn a_newer_release_for_a_newer_macos_is_held_back() {
        let body = r#"{ "version": "0.3.0", "minimum_macos": "15.0", "bytes": 199876543 }"#;
        assert_eq!(said(body), Ok(Answer::NeedsMacos(release("0.3.0", "15.0"))));
        let exact = r#"{ "version": "0.3.0", "minimum_macos": "14.5", "bytes": 199876543 }"#;
        assert_eq!(said(exact), Ok(Answer::Newer(release("0.3.0", "14.5"))));
    }

    #[test]
    fn fields_the_pipeline_adds_are_ignored() {
        let body = r#"{ "version": "0.3.0", "minimum_macos": "13.0", "bytes": 199876543,
            "notes": "faster", "channel": { "name": "stable" } }"#;
        assert_eq!(said(body), Ok(Answer::Newer(release("0.3.0", "13.0"))));
    }

    #[test]
    fn a_malformed_feed_is_an_error_and_never_an_offer() {
        for body in [
            "",
            "<html>not found</html>",
            r#"{ "version": "0.3.0", "bytes": 1 }"#,
            r#"{ "version": "0.3.0-beta", "minimum_macos": "13.0", "bytes": 1 }"#,
            r#"{ "version": "0.3.0", "minimum_macos": "thirteen", "bytes": 1 }"#,
            r#"{ "version": "0.3.0", "minimum_macos": "13.0", "bytes": "1" }"#,
            r#"{ "version": "0.3.0", "minimum_macos": "13.0", "bytes": -1 }"#,
        ] {
            assert!(said(body).is_err(), "{body:?} was read as a feed");
        }
    }

    #[test]
    fn a_feed_that_says_more_than_any_release_weighs_is_malformed() {
        let weighing = |version: &str, bytes: u64| {
            said(&format!(r#"{{ "version": "{version}", "minimum_macos": "13.0", "bytes": {bytes} }}"#))
        };
        let heaviest = 8 * 1024 * 1024 * 1024;
        let offered = Release { bytes: heaviest, ..release("0.3.0", "13.0") };
        assert_eq!(weighing("0.3.0", heaviest), Ok(Answer::Newer(offered)));
        for (version, bytes) in [("0.3.0", heaviest + 1), ("0.3.0", u64::MAX), ("0.2.0", heaviest + 1)] {
            let refused = weighing(version, bytes).unwrap_err();
            assert!(refused.ends_with("more than any weighs"), "{refused}");
        }
    }
}
