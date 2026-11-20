//! A page that stands in for one of a history brought back until it's gone back or forward
//! to: a small document of its own, at an address that says the page it stands for, which
//! then replaces itself with that page (`location.replace`, which keeps the history around it).
//!
//! Each is loaded as the browser's own navigation, one after another (`plan`), because
//! Chromium skips, on its back and forward buttons, the entries a page's script adds without a
//! click of the person's (its history manipulation intervention); and a page's script may
//! open no `data:` address, which the browser may. One loaded so is left as it is, unless
//! it's the last of a history with pages ahead of the one shown: that one goes `back` that
//! many steps, to the stand-in of the page shown, which replaces itself as any other does
//! once gone to (`docs/browser.md` 7).
//!
//! A page can make a `data:` document too, and open it in a tab of its own, and anything that
//! reads a page (an extension, an agent's tools) can read a stand-in's address: only a stand-in
//! this host issued for the pane's own history ([`StandIns`]), word for word, is shown as the
//! page it stands for, so no page is ever shown at another's address.
use koyo_browser_ipc::SessionEntry;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt::Write as _;

/// How every stand-in's address starts.
const SCHEME: &str = "data:text/html;charset=utf-8,";
/// The document, before what it stands for.
const HEAD: &str = "<!doctype html><meta charset=\"utf-8\">\
    <script type=\"application/json\" id=\"koyo-kept\">";
/// The document, after what it stands for: what it does once shown.
const TAIL: &str = "</script><script>(() => {\
    const kept = JSON.parse(document.getElementById('koyo-kept').textContent);\
    document.title = kept.title || kept.url;\
    addEventListener('pageshow', (event) => {\
      const [timing] = performance.getEntriesByType('navigation');\
      const loaded = !event.persisted && timing !== undefined && timing.type === 'navigate';\
      if (!loaded) location.replace(kept.url);\
      else if (kept.back > 0) history.go(-kept.back);\
    });\
  })();</script>";

/// What a stand-in stands for, as its document holds it.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Kept {
    url: String,
    #[serde(default)]
    title: String,
    /// Its place in the history, so two stand-ins for one address are two documents.
    #[serde(default)]
    at: usize,
    /// How many steps back to go once it's loaded; 0 for none.
    #[serde(default)]
    back: usize,
}

/// The stand-ins the host issued for one pane's history, by their addresses: the only ones it
/// shows as the pages they stand for.
#[derive(Debug, Default)]
pub(crate) struct StandIns(HashSet<String>);

impl StandIns {
    /// The address of a new stand-in of the pane's for `entry`, at place `at` of its history,
    /// which goes `back` steps once loaded.
    pub(crate) fn issue(&mut self, entry: &SessionEntry, at: usize, back: usize) -> String {
        let address = address(entry, at, back);
        self.0.insert(address.clone());
        address
    }

    /// The page the stand-in at `url` stands for; `None` for any address the host didn't issue
    /// the pane.
    pub(crate) fn entry_of(&self, url: &str) -> Option<SessionEntry> {
        if !self.0.contains(url) {
            return None;
        }
        let document = decode(url.strip_prefix(SCHEME)?)?;
        let json = document.strip_prefix(HEAD)?.strip_suffix(TAIL)?;
        let kept: Kept = serde_json::from_str(json).ok()?;
        Some(SessionEntry {
            url: kept.url,
            title: kept.title,
        })
    }

    /// Whether `url` is a stand-in the host issued the pane.
    pub(crate) fn has(&self, url: &str) -> bool {
        self.0.contains(url)
    }
}

/// The address of the stand-in for `entry`, at place `at` of its history, which goes `back`
/// steps once loaded.
pub(super) fn address(entry: &SessionEntry, at: usize, back: usize) -> String {
    let kept = Kept {
        url: entry.url.clone(),
        title: entry.title.clone(),
        at,
        back,
    };
    let json = serde_json::to_string(&kept)
        .unwrap_or_default()
        .replace('<', "\\u003c");
    format!("{SCHEME}{}", encode(&format!("{HEAD}{json}{TAIL}")))
}

/// The document of the stand-in at `url`, for a test to read.
#[cfg(test)]
pub(super) fn document_of(url: &str) -> String {
    decode(url.strip_prefix(SCHEME).unwrap_or_default()).unwrap_or_default()
}

/// The address of a page a page made, with `json` where a stand-in holds what it stands for,
/// for a test to try passing off as one.
#[cfg(test)]
pub(super) fn made_by_a_page(json: &str) -> String {
    format!("{SCHEME}{}", encode(&format!("{HEAD}{json}{TAIL}")))
}

/// `text` with every byte but the URL standard's unreserved ones percent-encoded.
fn encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len() * 2);
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

/// `text` with its percent-encoded bytes decoded; `None` when they aren't UTF-8.
fn decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = bytes
            .get(at + 1..at + 3)
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (bytes[at], hex) {
            (b'%', Some(byte)) => {
                decoded.push(byte);
                at += 3;
            }
            (byte, _) => {
                decoded.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8(decoded).ok()
}

#[cfg(test)]
mod tests {
    use super::{StandIns, address};
    use koyo_browser_ipc::SessionEntry;

    fn entry(url: &str, title: &str) -> SessionEntry {
        SessionEntry {
            url: url.into(),
            title: title.into(),
        }
    }

    #[test]
    fn a_stand_in_says_the_page_it_stands_for() {
        let page = entry(
            "https://example.com/a?b=c#d",
            "Ünïcode </script> & “quotes”",
        );
        let mut issued = StandIns::default();
        let stand_in = issued.issue(&page, 3, 0);
        assert!(stand_in.starts_with("data:text/html;charset=utf-8,"));
        assert_eq!(issued.entry_of(&stand_in), Some(page));
        assert!(issued.has(&stand_in));
    }

    #[test]
    fn two_stand_ins_for_one_address_are_two_documents() {
        let page = entry("https://example.com/", "");
        let mut issued = StandIns::default();
        assert_ne!(issued.issue(&page, 0, 0), issued.issue(&page, 2, 0));
    }

    #[test]
    fn no_other_address_is_a_stand_in() {
        let mut issued = StandIns::default();
        issued.issue(&entry("https://example.com/", ""), 0, 0);
        for url in [
            "https://example.com/",
            "data:text/html;charset=utf-8,%3Ch1%3Ehi%3C%2Fh1%3E",
            "data:text/html,<p>",
            "",
        ] {
            assert_eq!(issued.entry_of(url), None, "{url}");
            assert!(!issued.has(url), "{url}");
        }
    }

    #[test]
    fn a_stand_in_made_word_for_word_but_issued_elsewhere_or_by_a_page_is_none() {
        let bank = entry("https://bank.test/", "Bank");
        let mut elsewhere = StandIns::default();
        let made_for_another = elsewhere.issue(&bank, 0, 0);
        let issued = StandIns::default();
        assert_eq!(issued.entry_of(&made_for_another), None, "another pane's");
        assert_eq!(issued.entry_of(&address(&bank, 0, 0)), None, "made as the host makes them");
        for json in [
            r#"{"url":"https://bank.test/","title":"Bank","at":0,"back":0,"x":"</script><h1>Sign in</h1><script>"}"#,
            r#"{"url":"https://bank.test/"}"#,
        ] {
            let url = super::made_by_a_page(json);
            assert_eq!(issued.entry_of(&url), None, "{json}");
        }
    }

    #[test]
    fn its_document_holds_no_closing_script_tag_but_its_own() {
        let stand_in = address(&entry("https://x.test/", "</script><script>alert(1)"), 0, 0);
        let document = super::decode(stand_in.strip_prefix(super::SCHEME).unwrap()).unwrap();
        assert_eq!(document.matches("</script>").count(), 2);
    }
}
