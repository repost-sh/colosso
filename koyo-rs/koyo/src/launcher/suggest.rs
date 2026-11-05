//! What the search engine suggests for what's typed in a browser launcher, as Firefox asks it
//! (`docs/new-tab.md` 4.6): Google's suggest endpoint in Firefox's format, once typing pauses,
//! given half a second, without cookies, and not for an address. Fetched with the system's
//! `curl`, as the language servers are (koyo-lsp's `install.rs`), so the app links no TLS stack
//! of its own for it.

use std::io::Read as _;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use gpui_kit::{App, Task};

use crate::{browser, settings};

/// Google's suggestions in Firefox's format: `["query", ["suggestion", …], …]`.
const ENDPOINT: &str = "https://www.google.com/complete/search?client=firefox&ie=utf-8&oe=utf-8&q=";
/// How long typing pauses before it's asked, so a word typed fast is asked about once.
const PAUSE: Duration = Duration::from_millis(50);
/// How long an answer has, Firefox's `browser.search.suggest.timeout`; how often `curl` is looked
/// at meanwhile, and how many times before it's given up on.
const TIMEOUT: &str = "0.5";
const POLL: Duration = Duration::from_millis(10);
const POLLS: usize = 100;
/// The shortest and longest text it's asked about.
const SHORTEST: usize = 2;
const LONGEST: usize = 100;

/// Whether to ask about `typed`: the person lets the app, and it's words rather than an address.
pub fn wanted(typed: &str, cx: &App) -> bool {
    let len = typed.chars().count();
    !cfg!(test)
        && settings::get(cx).search_suggestions
        && (SHORTEST..=LONGEST).contains(&len)
        && !is_address(typed)
}

/// Whether `typed` loads as a page rather than searched for.
pub fn is_address(typed: &str) -> bool {
    matches!(browser::read_address(typed), Some(browser::Address::Page(_)))
}

/// Asks what's suggested for `query` once typing pauses; nothing when it fails or is late.
/// Dropped, it isn't asked, or what's asking is stopped: no thread waits on an answer no one
/// wants.
pub fn fetch(query: String, cx: &App) -> Task<Option<Vec<String>>> {
    let executor = cx.background_executor().clone();
    cx.background_executor().spawn(async move {
        executor.timer(PAUSE).await;
        let encoded: String = form_urlencoded::byte_serialize(query.as_bytes()).collect();
        let child = Command::new("curl")
            // First, so no `.curlrc` changes what's asked.
            .arg("--disable")
            .args(["--fail", "--silent", "--compressed", "--proto", "=https"])
            .args(["--max-time", TIMEOUT])
            .args(["--header", "Accept: application/json"])
            .arg(format!("{ENDPOINT}{encoded}"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .inspect_err(|error| log::debug!("couldn't ask for search suggestions: {error}"))
            .ok()?;
        let mut asking = Asking(child);
        let mut polls = 0;
        // An answer is a few hundred bytes, well within what the pipe holds till it's read.
        let status = loop {
            if let Some(status) = asking.0.try_wait().ok()? {
                break status;
            }
            polls += 1;
            if polls > POLLS {
                return None;
            }
            executor.timer(POLL).await;
        };
        let mut body = Vec::new();
        asking.0.stdout.take()?.read_to_end(&mut body).ok()?;
        status.success().then(|| parse(&query, &body))?
    })
}

/// `curl` asking: stopped if it's still at it when what wanted its answer goes.
struct Asking(Child);

impl Drop for Asking {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// The suggestions in Firefox's format, if they answer `query`: the first element has to be it.
pub fn parse(query: &str, body: &[u8]) -> Option<Vec<String>> {
    let json: serde_json::Value = serde_json::from_slice(body).ok()?;
    let answered = json.get(0)?.as_str()?;
    if answered.to_lowercase() != query.to_lowercase() {
        return None;
    }
    let suggested = json.get(1)?.as_array()?;
    Some(
        (suggested.iter())
            .filter_map(|suggestion| suggestion.as_str())
            .map(str::trim)
            .filter(|suggestion| !suggestion.is_empty())
            .map(str::to_owned)
            .collect(),
    )
}
