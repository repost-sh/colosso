//! `file://` URIs, the protocol's names for files, made from paths and read back.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use lsp_types::Uri;

/// The `file://` URI of an absolute path, every byte but letters, digits, `-._~` and `/`
/// percent-encoded; `None` for a relative path or one that isn't UTF-8.
///
/// Encoded here because `fluent-uri`, which parses lsp-types' URIs, offers its encoder only
/// behind its `unstable` feature.
pub fn from_path(path: &Path) -> Option<Uri> {
    let text = path.to_str().filter(|_| path.is_absolute())?;
    let mut uri = String::with_capacity(text.len() + 8);
    uri.push_str("file://");
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            uri.push(char::from(byte));
        } else {
            let _ = write!(uri, "%{byte:02X}");
        }
    }
    Uri::from_str(&uri).ok()
}

/// The path a `file://` URI names, or `None` for another scheme, another host, or a path
/// that doesn't decode to UTF-8.
pub fn to_path(uri: &Uri) -> Option<PathBuf> {
    if !uri.scheme()?.as_str().eq_ignore_ascii_case("file") {
        return None;
    }
    if let Some(authority) = uri.authority() {
        let host = authority.host().as_str();
        if !(host.is_empty() || host.eq_ignore_ascii_case("localhost")) {
            return None;
        }
    }
    let path = uri.path().as_estr().decode().into_string().ok()?;
    Some(PathBuf::from(path.into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_round_trip_through_uris() {
        let path = Path::new("/Users/me/my project/naïve 中文/a+b#c.rs");
        let uri = from_path(path).unwrap();
        assert_eq!(
            uri.as_str(),
            "file:///Users/me/my%20project/na%C3%AFve%20%E4%B8%AD%E6%96%87/a%2Bb%23c.rs"
        );
        assert_eq!(to_path(&uri).unwrap(), path);
    }

    #[test]
    fn uris_spelled_by_servers_are_read() {
        let uri = Uri::from_str("file://localhost/tmp/a%3a.rs").unwrap();
        assert_eq!(to_path(&uri).unwrap(), Path::new("/tmp/a:.rs"));
        let uri = Uri::from_str("untitled:Untitled-1").unwrap();
        assert_eq!(to_path(&uri), None);
        let uri = Uri::from_str("file://server/share/a.rs").unwrap();
        assert_eq!(to_path(&uri), None);
    }

    #[test]
    fn relative_paths_are_refused() {
        assert_eq!(from_path(Path::new("src/lib.rs")), None);
    }
}
