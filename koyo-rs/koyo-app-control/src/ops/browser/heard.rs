//! What a page said to its console and the requests it made, kept for agents from when one
//! first asks: `browser_console` and `browser_network`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ops::shared::{PaneId, count};
use crate::{Hints, Operation, Scope};

/// How many of a page's console messages, and of its requests, are kept: past it, the oldest
/// go, and `dropped` counts them.
pub const KEPT: usize = 500;

/// Which page's console or requests to read.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserHeardInput {
    /// The browser pane. Left out, the page you acted on last, else the shown tab's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// Lets go of what this call answers, so the next one says only what came since.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clear: Option<bool>,
}

/// How much a console message matters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConsoleLevel {
    Debug,
    Log,
    Info,
    Warning,
    Error,
}

/// One message of a page's console.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleMessage {
    pub level: ConsoleLevel,
    /// `console` for the page's own `console.log` and the rest, `exception` for an error nothing
    /// caught, else what the engine says it's of: `network`, `security`, `violation`,
    /// `intervention`, `deprecation` and others.
    pub source: String,
    pub text: String,
    /// The script or page it came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Its line there, counted from 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// When, in milliseconds since 1970.
    pub timestamp: f64,
}

/// A page's console.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BrowserConsoleOutput {
    pub pane: PaneId,
    /// The page's address now.
    pub url: String,
    /// Oldest first.
    pub messages: Vec<ConsoleMessage>,
    /// How many older messages were let go, past the 500 kept.
    pub dropped: u64,
}

/// `browser_console`: a page's console messages.
#[derive(Debug)]
pub struct BrowserConsole;

impl Operation for BrowserConsole {
    const NAME: &'static str = "browser_console";
    const TITLE: &'static str = "Read a page's console";
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "A browser page's console messages, oldest first: its `console.log` and the rest, errors nothing caught, and what the engine says of it (a blocked request, a security warning). The first call on a page starts keeping them, with those the page logged before it; then they're kept until the page closes, the last 500. `clear` lets go of what the call answers, so the next says only what's new. The page's frames of another site, which run apart, keep theirs. Reading needs no lease.";
    const HINTS: Hints = Hints::READ;
    type Input = BrowserHeardInput;
    type Output = BrowserConsoleOutput;

    fn summary(output: &BrowserConsoleOutput) -> String {
        count(output.messages.len(), "console message", "console messages")
    }
}

/// One request a page made.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NetworkRequest {
    /// The engine's id for it; a redirect keeps it, each hop listed.
    pub id: String,
    /// `GET`, `POST` and the others.
    pub method: String,
    pub url: String,
    /// What it loads, as the engine says: `Document`, `Script`, `XHR`, `Fetch`, `Image` and others.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_type: Option<String>,
    /// The response's HTTP status, once it came.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Why it failed, as the engine says (`net::ERR_CONNECTION_REFUSED`, `canceled`, what
    /// blocked it); absent when it didn't.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<String>,
    /// Whether it's over: loaded, failed or redirected.
    pub finished: bool,
    /// When it was sent, in milliseconds since 1970.
    pub timestamp: f64,
}

/// A page's requests.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BrowserNetworkOutput {
    pub pane: PaneId,
    /// The page's address now.
    pub url: String,
    /// Oldest first.
    pub requests: Vec<NetworkRequest>,
    /// How many older requests were let go, past the 500 kept.
    pub dropped: u64,
}

/// `browser_network`: the requests a page made.
#[derive(Debug)]
pub struct BrowserNetwork;

impl Operation for BrowserNetwork {
    const NAME: &'static str = "browser_network";
    const TITLE: &'static str = "Read a page's requests";
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "The requests a browser page made, oldest first: method, address, what it loads, status, type, and why one failed. The first call on a page starts recording, and answers none: what the page requests after it, browser_go reload for its own load, until the page closes, the last 500. `clear` lets go of what the call answers, so the next says only what's new. The page's frames of another site, which run apart, keep theirs. Reading needs no lease.";
    const HINTS: Hints = Hints::READ;
    type Input = BrowserHeardInput;
    type Output = BrowserNetworkOutput;

    fn summary(output: &BrowserNetworkOutput) -> String {
        count(output.requests.len(), "request", "requests")
    }
}
