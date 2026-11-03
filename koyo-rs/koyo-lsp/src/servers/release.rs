//! A pinned release of a server: where this machine's download is, its digest, and what's
//! in it.

/// A pinned release: a download per platform, each checked against its digest.
pub struct Release {
    pub version: &'static str,
    /// Where it's downloaded from, `{version}` and `{target}` filled in.
    pub url: &'static str,
    /// Each platform the release has a download for.
    pub assets: &'static [Asset],
    /// What the download is.
    pub archive: Archive,
    /// Where the server's binary is, under the folder the download is unpacked into.
    pub binary: &'static str,
}

/// One platform's download: the machine as Rust names its architecture and OS, the platform
/// as the release names it in `{target}`, and the SHA-256 of the download in hex.
pub type Asset = ((&'static str, &'static str), &'static str, &'static str);

/// What a release's download is.
pub enum Archive {
    /// The one binary, gzipped, as rust-analyzer's releases are.
    Gzip,
    /// A gzipped tar, unpacked whole, as an npm package or a release of several files is.
    Tarball,
}

impl Release {
    /// This machine's platform as the release names it, if it has a download for it.
    pub(super) fn target(&self) -> Option<&'static str> {
        self.asset().map(|(_, target, _)| *target)
    }

    /// This machine's download: its URL, and the digest it must have.
    pub(super) fn download(&self) -> Option<(String, &'static str)> {
        let (_, target, sha256) = self.asset()?;
        let url = self
            .url
            .replace("{version}", self.version)
            .replace("{target}", target);
        Some((url, sha256))
    }

    fn asset(&self) -> Option<&Asset> {
        let host = host();
        self.assets.iter().find(|(platform, ..)| *platform == host)
    }
}

/// This machine, as Rust names its architecture and OS: `("aarch64", "macos")`.
pub(super) fn host() -> (&'static str, &'static str) {
    (std::env::consts::ARCH, std::env::consts::OS)
}
