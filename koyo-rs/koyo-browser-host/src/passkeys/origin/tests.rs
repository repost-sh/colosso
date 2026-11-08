use super::{Allowed, check, origin_of, suffix_of_or_equal};
use koyo_browser_ipc::PasskeyError;

fn allowed(address: &str, rp_id: &str) -> Allowed {
    check(address, &[], rp_id)
        .unwrap_or_else(|refusal| panic!("{address} for {rp_id:?}: {refusal:?}"))
}

fn refused(address: &str, ancestors: &[&str], rp_id: &str) -> PasskeyError {
    let ancestors: Vec<String> = ancestors.iter().map(|a| (*a).to_owned()).collect();
    match check(address, &ancestors, rp_id) {
        Ok(allowed) => panic!("{address} for {rp_id:?} was allowed: {allowed:?}"),
        Err(refusal) => refusal.error,
    }
}

#[test]
fn a_page_with_no_id_of_its_own_asks_for_its_domain() {
    assert_eq!(
        allowed("https://accounts.google.com/v3/signin/challenge/pk?x=1", ""),
        Allowed {
            origin: "https://accounts.google.com".into(),
            rp_id: "accounts.google.com".into(),
        }
    );
    // Ports stay in the origin, never in the id.
    assert_eq!(
        allowed("https://example.com:8443/", ""),
        Allowed {
            origin: "https://example.com:8443".into(),
            rp_id: "example.com".into(),
        }
    );
}

#[test]
fn an_id_may_be_the_domain_or_a_registrable_suffix_of_it() {
    assert_eq!(
        allowed("https://accounts.google.com/", "google.com").rp_id,
        "google.com"
    );
    assert_eq!(
        allowed("https://www.github.com/", "github.com").rp_id,
        "github.com"
    );
    assert_eq!(
        allowed("https://login.example.co.uk/", "example.co.uk").rp_id,
        "example.co.uk"
    );
    assert_eq!(
        allowed("https://me.github.io/", "me.github.io").rp_id,
        "me.github.io"
    );
    // Named as the page likes, compared as the URL standard writes domains.
    assert_eq!(
        allowed("https://www.example.com/", "Example.COM").rp_id,
        "example.com"
    );
    assert_eq!(
        allowed("https://bücher.example/", "").rp_id,
        "xn--bcher-kva.example"
    );
    assert_eq!(
        allowed("https://www.bücher.example/", "bücher.example").rp_id,
        "xn--bcher-kva.example"
    );
}

#[test]
fn an_id_that_is_a_public_suffix_or_another_site_is_a_security_error() {
    for (address, rp_id) in [
        ("https://example.co.uk/", "co.uk"),
        ("https://me.github.io/", "github.io"),
        ("https://www.example.com/", "com"),
        ("https://example.com/", "www.example.com"),
        ("https://example.com/", "notexample.com"),
        ("https://badexample.com/", "example.com"),
        ("https://bucket.s3.amazonaws.com/", "amazonaws.com"),
        ("https://example.com/", "1.2.3.4"),
        ("https://example.com/", "exa mple.com"),
    ] {
        assert_eq!(
            refused(address, &[], rp_id),
            PasskeyError::Security,
            "{address} {rp_id}"
        );
    }
    // A site on an address has no domain for an id.
    assert_eq!(
        refused("https://192.168.1.10/", &[], ""),
        PasskeyError::Security
    );
    assert_eq!(
        refused("http://127.0.0.1:8080/", &[], ""),
        PasskeyError::Security
    );
}

#[test]
fn only_secure_origins_may_ask() {
    for address in [
        "http://example.com/",
        "about:blank",
        "data:text/html,hi",
        "file:///Users/ada/page.html",
        "ftp://example.com/",
        "",
    ] {
        assert_eq!(
            refused(address, &[], ""),
            PasskeyError::NotAllowed,
            "{address}"
        );
    }
    assert_eq!(allowed("http://localhost:5173/", "").rp_id, "localhost");
    assert_eq!(allowed("http://app.localhost/", "").rp_id, "app.localhost");
    // An unlisted top-level name is a public suffix by the list's own rule.
    assert_eq!(
        refused("http://app.localhost/", &[], "localhost"),
        PasskeyError::Security
    );
    // A blob's origin is the one inside it.
    assert_eq!(
        allowed("blob:https://example.com/4a5c-11e8", "").origin,
        "https://example.com"
    );
}

#[test]
fn a_frame_under_another_origins_frame_may_not_ask() {
    let ancestors = ["https://example.com/"];
    assert_eq!(
        check(
            "https://example.com/embedded",
            &ancestors.map(String::from),
            ""
        )
        .map(|a| a.rp_id),
        Ok("example.com".into())
    );
    assert_eq!(
        refused("https://login.example.com/", &["https://example.com/"], ""),
        PasskeyError::NotAllowed
    );
    // Same origin as its parent, under a frame of another: still another origin above it.
    assert_eq!(
        refused(
            "https://example.com/inner",
            &["https://example.com/middle", "https://ads.example.net/"],
            ""
        ),
        PasskeyError::NotAllowed
    );
}

#[test]
fn opaque_addresses_have_no_origin() {
    assert!(origin_of("about:blank").is_none());
    assert!(origin_of("data:text/html,x").is_none());
    assert!(origin_of("not a url").is_none());
    assert!(origin_of("https://example.com/").is_some());
}

#[test]
fn suffixes_are_cut_at_a_labels_edge() {
    assert!(suffix_of_or_equal("example.com", "a.b.example.com"));
    assert!(!suffix_of_or_equal("ample.com", "example.com"));
    assert!(!suffix_of_or_equal("", "example.com"));
    // A host may be its own id even where it's a public suffix's whole name.
    assert!(suffix_of_or_equal("localhost", "localhost"));
}
