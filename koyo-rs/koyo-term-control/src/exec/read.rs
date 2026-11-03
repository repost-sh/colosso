//! The operations that read and wait (`docs/terminals.md` 8.5): the screen or the stream,
//! input sent with its wait and read in one call, and waits on a terminal's state or screen.

use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::{Value, json};

use super::{Done, Executor, invalid, shape, unexpected};
use crate::catalog::args::{ReadArgs, ReadMode, ReadSpec, SendArgs, WaitArgs, WaitFor, WaitSpec};
use crate::{
    Controller, ErrorCode, Handle, LogKind, ProtocolError, RequestBody, ResponseBody, ScreenRead,
    SendRequest, Status,
};

/// How often a wait looks again.
const POLL: Duration = Duration::from_millis(100);
/// How long after input a wait for the prompt trusts a status that hasn't moved yet.
const SETTLE: Duration = Duration::from_secs(2);

/// What a wait saw.
pub(super) struct Waited {
    met: bool,
    until: WaitFor,
    elapsed_ms: u64,
    status: Status,
    last_exit: Option<i32>,
    found: Option<Value>,
    saw_running: bool,
}

impl Waited {
    pub(super) fn json(&self) -> Value {
        json!({
            "met": self.met,
            "condition": self.until,
            "elapsedMs": self.elapsed_ms,
            "status": self.status,
            "lastExit": self.last_exit,
            "match": self.found,
        })
    }

    pub(super) fn words(&self) -> String {
        let condition = serde_json::to_value(self.until)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default();
        let met = if self.met { "met" } else { "not met" };
        format!("{condition} {met} after {} ms", self.elapsed_ms)
    }
}

impl Executor {
    pub(super) fn read(&self, args: &ReadArgs) -> Result<Done, ProtocolError> {
        let terminal = self.terminal(args.terminal.as_deref())?;
        let (structured, summary) = self.read_spec(&terminal.record.handle, &args.spec())?;
        Ok(Done {
            structured,
            summary,
        })
    }

    /// The number of the screen's frame, which moves on when what it shows changes.
    pub fn frame(&self, handle: &Handle) -> Result<u64, ProtocolError> {
        Ok(self.screen(handle)?.frame)
    }

    fn screen(&self, handle: &Handle) -> Result<ScreenRead, ProtocolError> {
        let read = RequestBody::ReadScreen {
            handle: handle.clone(),
        };
        match self.request(read)? {
            ResponseBody::Screen(screen) => Ok(*screen),
            other => Err(unexpected(&other)),
        }
    }

    pub(super) fn read_spec(
        &self,
        handle: &Handle,
        spec: &ReadSpec,
    ) -> Result<(Value, String), ProtocolError> {
        match spec.mode.unwrap_or_default() {
            ReadMode::Screen => {
                let screen = self.screen(handle)?;
                let (from, to) = rows(spec.rows.as_deref(), screen.lines.len())?;
                let mut lines = screen.lines[from..to].to_vec();
                if spec.fold_blank == Some(true) {
                    lines.dedup_by(|line, before| line.is_empty() && before.is_empty());
                }
                let cursor_line = (screen.cursor)
                    .and_then(|cursor| screen.lines.get(usize::from(cursor.row)))
                    .cloned();
                let summary = format!(
                    "{}: {}×{}, {}{}",
                    screen.name,
                    screen.cols,
                    screen.rows,
                    screen.status.words(),
                    if screen.alt_screen {
                        ", full screen"
                    } else {
                        ""
                    }
                );
                let structured = json!({
                    "source": "screen",
                    "header": {
                        "name": screen.name,
                        "title": screen.title,
                        "cols": screen.cols,
                        "rows": screen.rows,
                        "altScreen": screen.alt_screen,
                        "mouse": screen.mouse,
                        "cursor": screen.cursor.map(|cursor| json!({
                            "row": cursor.row,
                            "col": cursor.col,
                            "visible": cursor.visible,
                        })),
                        "frame": screen.frame,
                        "status": screen.status,
                    },
                    "lines": lines,
                    "cursorLine": cursor_line,
                });
                Ok((structured, summary))
            }
            ReadMode::Stream => {
                let read = RequestBody::ReadStream {
                    handle: handle.clone(),
                    since: spec.cursor,
                    max_bytes: spec.limit,
                };
                let ResponseBody::Stream {
                    text,
                    next,
                    oldest,
                    truncated,
                } = self.request(read)?
                else {
                    return Err(ProtocolError::new(ErrorCode::Internal, "not a stream"));
                };
                let summary = format!("{} bytes of output, up to cursor {next}", text.len());
                let structured = json!({
                    "source": "stream",
                    "text": text,
                    "nextCursor": next,
                    "oldestCursor": oldest,
                    "truncated": truncated,
                });
                Ok((structured, summary))
            }
        }
    }

    pub(super) fn send(&self, args: SendArgs) -> Result<Done, ProtocolError> {
        let terminal = self.terminal(args.terminal.as_deref())?;
        let handle = terminal.record.handle.clone();
        let request = SendRequest {
            handle: handle.clone(),
            text: args.text,
            paste: args.paste,
            keys: args.keys.unwrap_or_default(),
            enter: args.enter.unwrap_or(false),
            interrupt: args.interrupt.unwrap_or(false),
        };
        let commands_before = self.log_len(&handle, LogKind::Commands);
        let free = terminal.record.control.holder.is_none();
        let sent = self.request_claiming(&handle, free, || RequestBody::Send(request.clone()))?;
        let ResponseBody::Sent { bytes } = sent else {
            return Err(unexpected(&sent));
        };
        self.remember(&handle);
        let then = args.then.unwrap_or_default();
        let waited = (then.wait.as_ref())
            .map(|spec| self.wait_for(&handle, spec, Some(commands_before)))
            .transpose()?;
        let submission = waited.as_ref().map(|waited| {
            let ran =
                waited.saw_running || self.log_len(&handle, LogKind::Commands) > commands_before;
            if ran { "observed" } else { "unobserved" }
        });
        let read = (then.read.as_ref())
            .map(|spec| self.read_spec(&handle, spec))
            .transpose()?;
        let now = self.record(&handle)?;
        let mut summary = format!("sent {bytes} bytes to {}", terminal.record.name);
        if let Some(waited) = &waited {
            summary = format!("{summary}; {}", waited.words());
        }
        Ok(Done {
            structured: json!({
                "accepted": true,
                "bytes": bytes,
                "control": shape::control(&now.record.control, &self.actor),
                "submission": submission,
                "wait": waited.as_ref().map(Waited::json),
                "read": read.map(|(read, _)| read),
            }),
            summary,
        })
    }

    /// Asks the daemon `body` for `handle`; a terminal no one holds (`free`) that needs
    /// control for it is claimed first, as an agent takes one to write to it, and asked again.
    pub(super) fn request_claiming(
        &self,
        handle: &Handle,
        free: bool,
        body: impl Fn() -> RequestBody,
    ) -> Result<ResponseBody, ProtocolError> {
        match self.request(body()) {
            Err(error) if error.code == ErrorCode::ControlRequired && free => {
                let claim = RequestBody::Claim {
                    handle: handle.clone(),
                    takeover: false,
                    reason: None,
                };
                self.request(claim)?;
                self.request(body())
            }
            answer => answer,
        }
    }

    pub(super) fn wait(&self, args: &WaitArgs) -> Result<Done, ProtocolError> {
        let terminal = self.terminal(args.terminal.as_deref())?;
        let handle = terminal.record.handle;
        let waited = self.wait_for(&handle, &args.spec(), None)?;
        let read = (args.read.as_ref())
            .map(|spec| self.read_spec(&handle, spec))
            .transpose()?;
        let mut structured = waited.json();
        if let Some((read, _)) = read {
            structured["read"] = read;
        }
        Ok(Done {
            summary: format!("{}: {}", terminal.record.name, waited.words()),
            structured,
        })
    }

    /// Waits until `spec` holds or its time runs out. Right after input (`after_send`, the
    /// commands log's length before it), a wait for the prompt or quiet first sees the
    /// input taken: the program running, a command logged, or a moment gone by.
    pub(super) fn wait_for(
        &self,
        handle: &Handle,
        spec: &WaitSpec,
        after_send: Option<u64>,
    ) -> Result<Waited, ProtocolError> {
        let timeout = Duration::from_millis(spec.timeout_ms.unwrap_or(25_000).min(600_000));
        let pattern = match (spec.until, &spec.pattern) {
            (WaitFor::Text | WaitFor::Gone, Some(pattern)) => {
                Some(Regex::new(pattern).map_err(|error| invalid(format!("pattern: {error}")))?)
            }
            (WaitFor::Text | WaitFor::Gone, None) => {
                return Err(invalid("waiting for text or for it to go needs a pattern"));
            }
            _ => None,
        };
        let stable_for = Duration::from_millis(spec.stable_ms.unwrap_or(1500));
        let started = Instant::now();
        let first = self.record(handle)?.record.status;
        let mut saw_running = false;
        let mut still: Option<(Vec<String>, Instant)> = None;
        loop {
            let record = self.record(handle)?.record;
            saw_running |= record.status == Status::Running;
            let settled = after_send.is_none_or(|before| {
                saw_running
                    || self.log_len(handle, LogKind::Commands) > before
                    || started.elapsed() >= SETTLE
            });
            let mut found = None;
            let met = match spec.until {
                WaitFor::Idle => settled && record.status == Status::Idle,
                WaitFor::Prompt => {
                    settled && record.status == Status::Idle && record.status_reason == "prompt"
                }
                WaitFor::Running => record.status == Status::Running,
                WaitFor::Exit => record.status == Status::Exited,
                WaitFor::NeedsInput => record.status == Status::NeedsInput,
                WaitFor::Control => {
                    !matches!(record.control.holder, Some(Controller::Human { .. }))
                }
                WaitFor::StatusChange => record.status != first,
                WaitFor::Text | WaitFor::Gone | WaitFor::Stable => {
                    let screen = self.screen(handle)?;
                    let (from, to) = rows(spec.rows.as_deref(), screen.lines.len())?;
                    let lines = &screen.lines[from..to];
                    match (spec.until, &pattern) {
                        (WaitFor::Text, Some(pattern)) => {
                            found = find(pattern, lines, from);
                            found.is_some()
                        }
                        (WaitFor::Gone, Some(pattern)) => find(pattern, lines, from).is_none(),
                        _ => match &still {
                            Some((before, since)) if before.as_slice() == lines => {
                                since.elapsed() >= stable_for
                            }
                            _ => {
                                still = Some((lines.to_vec(), Instant::now()));
                                false
                            }
                        },
                    }
                }
            };
            let elapsed = started.elapsed();
            if met || elapsed >= timeout {
                return Ok(Waited {
                    met,
                    until: spec.until,
                    elapsed_ms: u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
                    status: record.status,
                    last_exit: record.last_exit,
                    found,
                    saw_running,
                });
            }
            std::thread::park_timeout(POLL.min(timeout.saturating_sub(elapsed)));
        }
    }
}

/// Rows `a:b` of a screen `len` rows tall, clamped to it.
fn rows(spec: Option<&str>, len: usize) -> Result<(usize, usize), ProtocolError> {
    let Some(spec) = spec else {
        return Ok((0, len));
    };
    let (from, to) = spec
        .split_once(':')
        .ok_or_else(|| invalid("rows is `a:b`, `a:` or `:b`"))?;
    let bound = |text: &str, default: usize| -> Result<usize, ProtocolError> {
        if text.is_empty() {
            Ok(default)
        } else {
            text.trim()
                .parse()
                .map_err(|error| invalid(format!("rows is `a:b`, `a:` or `:b`: {error}")))
        }
    };
    let to = bound(to, len)?.min(len);
    let from = bound(from, 0)?.min(to);
    Ok((from, to))
}

/// Where `pattern` first matches `lines`, which start at screen row `first`.
fn find(pattern: &Regex, lines: &[String], first: usize) -> Option<Value> {
    lines.iter().enumerate().find_map(|(index, line)| {
        let found = pattern.find(line)?;
        let col = line[..found.start()].chars().count();
        Some(json!({ "row": first + index, "col": col, "text": found.as_str() }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_clamped_slices() {
        assert_eq!(rows(None, 10).unwrap(), (0, 10));
        assert_eq!(rows(Some("2:5"), 10).unwrap(), (2, 5));
        assert_eq!(rows(Some("8:"), 10).unwrap(), (8, 10));
        assert_eq!(rows(Some(":3"), 10).unwrap(), (0, 3));
        assert_eq!(rows(Some("4:40"), 10).unwrap(), (4, 10));
        assert_eq!(rows(Some("9:2"), 10).unwrap(), (2, 2));
        assert!(rows(Some("x"), 10).is_err());
    }

    #[test]
    fn a_match_says_where_it_is() {
        let lines = vec!["one".to_owned(), "été done".to_owned()];
        let pattern = Regex::new("done").unwrap();
        let found = find(&pattern, &lines, 3).unwrap();
        assert_eq!(found, json!({ "row": 4, "col": 4, "text": "done" }));
    }
}
