//! HTTP through the system `curl`, as Koyo's other requests go (`docs/claude-accounts.md` 3).
//!
//! claude-swap uses `urllib`; Koyo uses `/usr/bin/curl`, which trusts the Keychain's
//! certificates and so works behind a TLS-inspecting proxy (pitfall 34). Everything about a
//! request but `--disable` and the fixed flags goes to curl's `--config -` on stdin: the URL,
//! the headers and the body, so a token never shows in `ps`. The answer comes back with its
//! headers (`--include`), for the status and `Retry-After`.
//!
//! [`Http`] stands for the transport, so tests run on a fake (`testing::FakeHttp`) and never
//! reach the network.

use std::io::{Read as _, Write as _};
use std::process::{Command, Stdio};
use std::time::Duration;

pub const CURL: &str = "/usr/bin/curl";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub timeout: Duration,
}

impl Request {
    pub fn get(url: impl Into<String>, timeout: Duration) -> Request {
        Request {
            method: "GET",
            url: url.into(),
            headers: Vec::new(),
            body: None,
            timeout,
        }
    }

    pub fn post(url: impl Into<String>, body: String, timeout: Duration) -> Request {
        Request {
            method: "POST",
            url: url.into(),
            headers: Vec::new(),
            body: Some(body),
            timeout,
        }
    }

    #[must_use]
    pub fn header(mut self, name: &str, value: impl Into<String>) -> Request {
        self.headers.push((name.to_owned(), value.into()));
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    /// As received, names as sent.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    /// The first header called `name`, whatever its case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// A request that got no HTTP answer, by what kept it from one: the kinds cswap's
/// `_classify_usage_error` tells apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HttpError {
    /// Past the request's time.
    Timeout,
    /// The server's certificate was refused: a TLS-inspecting proxy, often.
    Tls(String),
    /// No route, no name, no connection.
    Network(String),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::Timeout => f.write_str("timed out"),
            HttpError::Tls(why) => write!(f, "TLS refused: {why}"),
            HttpError::Network(why) => write!(f, "network: {why}"),
        }
    }
}

impl std::error::Error for HttpError {}

pub trait Http: Send + Sync {
    /// # Errors
    /// No HTTP answer; an error status is a [`Response`].
    fn send(&self, request: &Request) -> Result<Response, HttpError>;
}

/// The system `curl`.
pub struct Curl;

impl Http for Curl {
    fn send(&self, request: &Request) -> Result<Response, HttpError> {
        let mut child = Command::new(CURL)
            // First, so no `.curlrc` changes what's asked.
            .arg("--disable")
            .args(["--silent", "--show-error", "--include", "--compressed"])
            .args(["--proto", "=https", "--config", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| HttpError::Network(format!("couldn't run {CURL}: {error}")))?;
        let config = config(request);
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(config.as_bytes())
                .map_err(|error| HttpError::Network(format!("couldn't talk to curl: {error}")))?;
        }
        let mut stdout = Vec::new();
        let mut stderr = String::new();
        if let Some(mut out) = child.stdout.take() {
            out.read_to_end(&mut stdout).ok();
        }
        if let Some(mut err) = child.stderr.take() {
            err.read_to_string(&mut stderr).ok();
        }
        let status = child
            .wait()
            .map_err(|error| HttpError::Network(format!("curl: {error}")))?;
        match status.code() {
            Some(0) => parse(&stdout).ok_or_else(|| HttpError::Network("unreadable answer".into())),
            Some(code) => Err(failure(code, stderr.trim())),
            None => Err(HttpError::Network("curl was stopped".into())),
        }
    }
}

/// What curl's exit `code` says kept the request from an answer.
fn failure(code: i32, said: &str) -> HttpError {
    match code {
        28 => HttpError::Timeout,
        // A certificate refused: the peer's, the CA store, its issuer, a pinned key, its status.
        // Other TLS failures (a handshake with a plaintext port) are the network's, as cswap
        // tells only a certificate verification apart.
        60 | 77 | 83 | 90 | 91 => HttpError::Tls(said.to_owned()),
        _ => HttpError::Network(if said.is_empty() {
            format!("curl exited {code}")
        } else {
            said.to_owned()
        }),
    }
}

/// `request` as curl's config file: one option a line, values quoted and escaped.
pub(crate) fn config(request: &Request) -> String {
    let mut lines = vec![
        format!("url = {}", quoted(&request.url)),
        format!("request = {}", quoted(request.method)),
        format!("max-time = {}", request.timeout.as_secs_f64()),
    ];
    for (name, value) in &request.headers {
        lines.push(format!("header = {}", quoted(&format!("{name}: {value}"))));
    }
    if let Some(body) = &request.body {
        lines.push(format!("data-raw = {}", quoted(body)));
    }
    let mut config = lines.join("\n");
    config.push('\n');
    config
}

/// `value` in double quotes, as curl's config parser reads them back.
fn quoted(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The answer `--include` printed: header blocks, interim ones (`100 Continue`) skipped, then
/// the body.
pub(crate) fn parse(raw: &[u8]) -> Option<Response> {
    let mut rest = raw;
    loop {
        let end = rest.windows(4).position(|window| window == b"\r\n\r\n")?;
        let head = std::str::from_utf8(&rest[..end]).ok()?;
        rest = &rest[end + 4..];
        let mut lines = head.split("\r\n");
        let status: u16 = lines.next()?.split_whitespace().nth(1)?.parse().ok()?;
        if (100..200).contains(&status) {
            continue;
        }
        let headers = lines
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| (name.trim().to_owned(), value.trim().to_owned()))
            .collect();
        return Some(Response {
            status,
            headers,
            body: rest.to_vec(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_goes_to_curl_on_stdin_with_its_secrets() {
        let request = Request::post(
            "https://platform.claude.com/v1/oauth/token",
            r#"{"refresh_token":"sk-ant-ort-\"x"}"#.to_owned(),
            Duration::from_secs(10),
        )
        .header("Authorization", "Bearer sk-ant-oat-1");
        let config = config(&request);
        assert!(config.contains(r#"header = "Authorization: Bearer sk-ant-oat-1""#));
        assert!(config.contains(r#"data-raw = "{\"refresh_token\":\"sk-ant-ort-\\\"x\"}""#));
        assert!(config.contains(r#"request = "POST""#));
    }

    #[test]
    fn an_answer_is_read_past_an_interim_block() {
        let raw = b"HTTP/1.1 100 Continue\r\n\r\nHTTP/2 429\r\nretry-after: 3600\r\nx: y\r\n\r\n{\"e\":1}";
        let response = parse(raw).unwrap();
        assert_eq!(response.status, 429);
        assert_eq!(response.header("Retry-After"), Some("3600"));
        assert_eq!(response.body, b"{\"e\":1}");
    }

    #[test]
    fn curl_failures_are_told_apart() {
        assert_eq!(failure(28, ""), HttpError::Timeout);
        assert!(matches!(
            failure(60, "SSL certificate problem"),
            HttpError::Tls(_)
        ));
        assert!(matches!(
            failure(6, "Could not resolve host"),
            HttpError::Network(_)
        ));
    }
}
