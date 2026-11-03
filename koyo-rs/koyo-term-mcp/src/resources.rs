//! The MCP resources: the app's (`docs/app-control.md` 7), each the output of one read
//! operation and followed by one topic, and the terminals' (`docs/terminals.md` 8.6): the
//! worktrees, a worktree's terminals, and a terminal, its screen and its activity, read
//! through the executor.

use std::path::PathBuf;

use koyo_app_control::{ControlError, Topic};
use koyo_term_control::catalog::Scope;
use koyo_term_control::exec::Executor;
use koyo_term_control::{ErrorCode, ProtocolError};
use rmcp::ErrorData;
use rmcp::model::{Resource, ResourceContents, ResourceTemplate};
use serde_json::{Value, json};

pub const JSON: &str = "application/json";
const TEXT: &str = "text/plain";

/// A resource's address.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Uri {
    /// One of the app's resources, read from the running app.
    App(AppResource),
    Worktrees,
    /// A worktree's terminals, by the worktree's path.
    Terminals(PathBuf),
    Terminal(String),
    Screen(String),
    Activity(String),
}

impl Uri {
    pub fn parse(uri: &str) -> Option<Uri> {
        let rest = uri.strip_prefix("koyo://")?;
        if let Some(resource) = AppResource::ALL.into_iter().find(|r| r.path() == rest) {
            return Some(Uri::App(resource));
        }
        if rest == "worktrees" {
            return Some(Uri::Worktrees);
        }
        let worktree =
            (rest.strip_prefix("worktrees/")).and_then(|rest| rest.strip_suffix("/terminals"));
        if let Some(path) = worktree {
            return Some(Uri::Terminals(PathBuf::from(decode(path)?)));
        }
        let rest = rest.strip_prefix("terminals/")?;
        let (handle, part) = rest.split_once('/').unwrap_or((rest, ""));
        if !handle.starts_with("term_") {
            return None;
        }
        let handle = handle.to_owned();
        match part {
            "" => Some(Uri::Terminal(handle)),
            "screen" => Some(Uri::Screen(handle)),
            "activity" => Some(Uri::Activity(handle)),
            _ => None,
        }
    }

    pub fn text(&self) -> String {
        match self {
            Uri::App(resource) => format!("koyo://{}", resource.path()),
            Uri::Worktrees => "koyo://worktrees".to_owned(),
            Uri::Terminals(path) => {
                format!(
                    "koyo://worktrees/{}/terminals",
                    encode(&path.to_string_lossy())
                )
            }
            Uri::Terminal(handle) => format!("koyo://terminals/{handle}"),
            Uri::Screen(handle) => format!("koyo://terminals/{handle}/screen"),
            Uri::Activity(handle) => format!("koyo://terminals/{handle}/activity"),
        }
    }

    /// All but the worktrees can be subscribed to.
    pub fn subscribable(&self) -> bool {
        *self != Uri::Worktrees
    }
}

/// The app's resources.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AppResource {
    App,
    Projects,
    Tabs,
    Settings,
}

impl AppResource {
    pub const ALL: [AppResource; 4] = [
        AppResource::App,
        AppResource::Projects,
        AppResource::Tabs,
        AppResource::Settings,
    ];

    /// The address after `koyo://`.
    fn path(self) -> &'static str {
        match self {
            AppResource::App => "app",
            AppResource::Projects => "projects",
            AppResource::Tabs => "tabs",
            AppResource::Settings => "settings",
        }
    }

    /// The topic whose events say it changed.
    pub fn topic(self) -> Topic {
        match self {
            AppResource::App => Topic::App,
            AppResource::Projects | AppResource::Tabs => Topic::Workspace,
            AppResource::Settings => Topic::Settings,
        }
    }

    /// The read operation it shows, and that operation's input.
    pub fn operation(self) -> (&'static str, Value) {
        match self {
            AppResource::App => ("app_status", json!({})),
            AppResource::Projects => ("project_list", json!({})),
            AppResource::Tabs => ("tab_list", json!({ "all": true })),
            AppResource::Settings => ("settings_get", json!({})),
        }
    }

    fn describe(self) -> (&'static str, &'static str) {
        match self {
            AppResource::App => (
                "Koyo",
                "The window: what it shows, its panels and layout, what is selected, and the MCP server",
            ),
            AppResource::Projects => (
                "Projects",
                "The projects in the sidebar, each with its worktrees, as project_list reads them",
            ),
            AppResource::Tabs => (
                "Tabs",
                "Every worktree's tabs with their split panes and pane ids, as tab_list reads them",
            ),
            AppResource::Settings => ("Settings", "The settings agents may read and change"),
        }
    }
}

/// The app's resources, for `resources/list`.
pub fn app_resources() -> Vec<Resource> {
    (AppResource::ALL.into_iter())
        .map(|resource| {
            let (title, description) = resource.describe();
            Resource::new(Uri::App(resource).text(), resource.path())
                .with_title(title)
                .with_description(description)
                .with_mime_type(JSON)
        })
        .collect()
}

/// An app refusal as a JSON-RPC error, its structured form kept as the error's data:
/// resource reads have no `isError` result.
pub fn app_error(error: &ControlError) -> ErrorData {
    use koyo_app_control::ErrorCode;
    let data = serde_json::to_value(error).ok();
    let message = error.message.clone();
    match error.code {
        ErrorCode::NotFound => ErrorData::resource_not_found(message, data),
        ErrorCode::InvalidInput | ErrorCode::Ambiguous | ErrorCode::ScopeDenied => {
            ErrorData::invalid_params(message, data)
        }
        _ => ErrorData::internal_error(message, data),
    }
}

pub fn templates() -> Vec<ResourceTemplate> {
    vec![
        ResourceTemplate::new("koyo://worktrees/{path}/terminals", "worktree terminals")
            .with_description("A worktree's terminals; the path is percent-encoded")
            .with_mime_type(JSON),
        ResourceTemplate::new("koyo://terminals/{handle}", "terminal")
            .with_description("A terminal, who holds it, and its size")
            .with_mime_type(JSON),
        ResourceTemplate::new("koyo://terminals/{handle}/screen", "terminal screen")
            .with_description("What the terminal shows, as text")
            .with_mime_type(TEXT),
        ResourceTemplate::new("koyo://terminals/{handle}/activity", "terminal activity")
            .with_description("The terminal's last 200 activity entries")
            .with_mime_type(JSON),
    ]
}

/// The worktrees, and each terminal of the caller's worktree with its screen.
pub fn list(executor: &Executor) -> Result<Vec<Resource>, ErrorData> {
    let mut resources = vec![
        Resource::new(Uri::Worktrees.text(), "worktrees")
            .with_description("The worktrees with terminals, with their counts")
            .with_mime_type(JSON),
    ];
    let listed = executor
        .call("terminal_list", json!({ "limit": 500 }))
        .map_err(error)?;
    let terminals = listed.structured["terminals"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for terminal in terminals {
        let (Some(handle), Some(name)) = (terminal["terminal"].as_str(), terminal["name"].as_str())
        else {
            continue;
        };
        resources.push(
            Resource::new(Uri::Terminal(handle.to_owned()).text(), name)
                .with_description(format!("Terminal {name}"))
                .with_mime_type(JSON),
        );
        resources.push(
            Resource::new(
                Uri::Screen(handle.to_owned()).text(),
                format!("{name} screen"),
            )
            .with_description(format!("What {name} shows"))
            .with_mime_type(TEXT),
        );
    }
    Ok(resources)
}

pub fn read(executor: &Executor, uri: &str) -> Result<ResourceContents, ErrorData> {
    let Some(parsed) = Uri::parse(uri) else {
        return Err(ErrorData::resource_not_found(
            format!("no resource is {uri}"),
            None,
        ));
    };
    if !executor.grants().contains(&Scope::Read) {
        let denied = ProtocolError::new(ErrorCode::ScopeDenied, "reading needs terminal.read");
        return Err(error(denied));
    }
    let call = |name: &str, args: Value| executor.call(name, args).map_err(error);
    let (text, mime) = match &parsed {
        Uri::Worktrees => (
            call("worktree_list", json!({}))?.structured.to_string(),
            JSON,
        ),
        Uri::Terminals(path) => {
            let worktree = format!("path:{}", path.display());
            let listed = call(
                "terminal_list",
                json!({ "worktree": worktree, "limit": 500 }),
            )?;
            (listed.structured["terminals"].to_string(), JSON)
        }
        Uri::Terminal(handle) => (executor.describe(handle).map_err(error)?.to_string(), JSON),
        Uri::Screen(handle) => {
            let read = call("terminal_read", json!({ "terminal": handle }))?;
            let lines: Vec<&str> = (read.structured["lines"].as_array().into_iter().flatten())
                .filter_map(Value::as_str)
                .collect();
            (lines.join("\n"), TEXT)
        }
        Uri::Activity(handle) => {
            let args = json!({ "terminal": handle, "kind": "activity", "limit": 200 });
            (
                call("terminal_log", args)?.structured["entries"].to_string(),
                JSON,
            )
        }
        // The server reads the app's resources from the app.
        Uri::App(_) => {
            let message = format!("{uri} is the app's, not the terminals'");
            return Err(ErrorData::resource_not_found(message, None));
        }
    };
    Ok(ResourceContents::text(text, uri).with_mime_type(mime))
}

/// A daemon refusal as a JSON-RPC error, its structured form kept as the error's data.
pub fn error(error: ProtocolError) -> ErrorData {
    let data = serde_json::to_value(&error).ok();
    match error.code {
        ErrorCode::TerminalNotFound | ErrorCode::WorktreeNotFound => {
            ErrorData::resource_not_found(error.message, data)
        }
        ErrorCode::InvalidArgument | ErrorCode::ScopeDenied | ErrorCode::AmbiguousTerminal => {
            ErrorData::invalid_params(error.message, data)
        }
        _ => ErrorData::internal_error(error.message, data),
    }
}

/// Percent-encodes all but the unreserved characters, so a path fits in one segment.
fn encode(text: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0xF)]));
        }
    }
    out
}

fn decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let hex = std::str::from_utf8(bytes.get(at + 1..at + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_read_back_as_written() {
        let all = [
            Uri::App(AppResource::App),
            Uri::App(AppResource::Tabs),
            Uri::Worktrees,
            Uri::Terminals(PathBuf::from("/Users/me/work tree/été")),
            Uri::Terminal("term_01".into()),
            Uri::Screen("term_01".into()),
            Uri::Activity("term_01".into()),
        ];
        for uri in all {
            assert_eq!(Uri::parse(&uri.text()), Some(uri.clone()), "{}", uri.text());
        }
        assert_eq!(
            Uri::Terminals(PathBuf::from("/a b")).text(),
            "koyo://worktrees/%2Fa%20b/terminals"
        );
        assert_eq!(Uri::parse("koyo://terminals/term_01/nothing"), None);
        assert_eq!(Uri::parse("koyo://terminals/other"), None);
        assert_eq!(Uri::parse("file:///etc"), None);
    }

    #[test]
    fn each_topic_covers_the_app_resources_that_follow_it() {
        for topic in Topic::ALL {
            let covered: Vec<Uri> = (topic.resources().iter())
                .map(|uri| Uri::parse(uri).unwrap())
                .collect();
            let following: Vec<Uri> = (AppResource::ALL.into_iter())
                .filter(|resource| resource.topic() == topic)
                .map(Uri::App)
                .collect();
            assert_eq!(covered, following, "{topic:?}");
        }
        assert_eq!(app_resources().len(), AppResource::ALL.len());
    }
}
