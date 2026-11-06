//! A page's address as the history shows it and tells it apart: as Firefox's address bar shows
//! one, `https://` and then `www.` left out; the host it's on; and the key two addresses of the
//! same page share.

/// An address as Firefox shows it: `https://` and then `www.` left out, where `http://` stays to
/// say the page isn't secure, and a lone trailing slash dropped.
pub fn display_url(url: &str) -> String {
    let secure = url
        .get(..8)
        .is_some_and(|start| start.eq_ignore_ascii_case("https://"));
    let shown = if secure { strip_www(&url[8..]) } else { url };
    let bare = strip_scheme(shown);
    match bare.split_once('/') {
        Some((_, "")) => shown.trim_end_matches('/').to_owned(),
        _ => shown.to_owned(),
    }
}

/// The host of the web page at `url`, lowercased, without `www.`, its port kept.
pub fn host(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return None;
    }
    let bare = strip_www(strip_scheme(&lower));
    let authority = bare.split(['/', '?', '#']).next()?;
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    (!host.is_empty()).then(|| host.to_owned())
}

pub(super) fn strip_scheme(url: &str) -> &str {
    ["https://", "http://"]
        .iter()
        .find_map(|scheme| {
            url.get(..scheme.len())
                .filter(|start| start.eq_ignore_ascii_case(scheme))
                .map(|_| &url[scheme.len()..])
        })
        .unwrap_or(url)
}

fn strip_www(url: &str) -> &str {
    match url.get(..4) {
        Some(start) if start.eq_ignore_ascii_case("www.") => &url[4..],
        _ => url,
    }
}

/// What tells two addresses apart as pages, as Firefox's switch-to-tab and deduping do: not their
/// scheme, `www.`, case, or an empty query or trailing slash.
pub fn same_page_key(url: &str) -> String {
    let bare = strip_www(strip_scheme(url)).to_lowercase();
    bare.trim_end_matches(['/', '?']).to_owned()
}
