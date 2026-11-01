//! A terminal's logs as `terminal_log` reads them (`docs/terminals.md` 8.5): from the tail
//! or on from a cursor, the failed commands alone when asked, in the answers' casing.

use serde_json::{Value, json};

use super::{Done, Executor};
use crate::catalog::args::LogArgs;
use crate::log::{read_since, tail_start};
use crate::{CommandRecord, ErrorCode, Handle, LogKind, ProtocolError};

/// How much of a log a read from its tail looks through.
const TAIL_BYTES: u64 = 4 << 20;
const READ_BYTES: usize = 8 << 20;

impl Executor {
    pub(super) fn log(&self, args: &LogArgs) -> Result<Done, ProtocolError> {
        let terminal = self.terminal(args.terminal.as_deref())?;
        let kind = args.kind.unwrap_or(LogKind::Activity);
        let limit = args.limit.unwrap_or(200).clamp(1, 1000) as usize;
        let failed_only = args.failed == Some(true) && kind == LogKind::Commands;
        let keep = |line: &str| !failed_only || failed(line);
        let path = self.dir.journal(&terminal.record.handle, kind);
        let (lines, next, rotated) = match args.cursor {
            None => {
                let from = tail_start(&path, TAIL_BYTES).map_err(io)?;
                let read = read_since(&path, from, READ_BYTES).map_err(io)?;
                let mut lines: Vec<String> = read.lines.into_iter().filter(|l| keep(l)).collect();
                let over = lines.len().saturating_sub(limit);
                lines.drain(..over);
                (lines, read.next, false)
            }
            Some(cursor) => {
                let read = read_since(&path, cursor, READ_BYTES).map_err(io)?;
                let mut at = if read.rotated { 0 } else { cursor };
                let mut lines = Vec::new();
                for line in read.lines {
                    if lines.len() == limit {
                        break;
                    }
                    at += line.len() as u64 + 1;
                    if keep(&line) {
                        lines.push(line);
                    }
                }
                let next = if lines.len() == limit { at } else { read.next };
                (lines, next, read.rotated)
            }
        };
        let entries: Vec<Value> = (lines.iter())
            .filter_map(|line| serde_json::from_str(line).ok().map(camel))
            .collect();
        let summary = format!(
            "{} {} entries of {}",
            entries.len(),
            kind.file_name(),
            terminal.record.name
        );
        Ok(Done {
            structured: json!({
                "entries": entries,
                "pending": [],
                "nextCursor": next,
                "oldestDropped": rotated,
            }),
            summary,
        })
    }

    /// The newest `count` entries of a log.
    pub(super) fn log_tail(
        &self,
        handle: &Handle,
        kind: LogKind,
        count: usize,
    ) -> Result<Vec<Value>, ProtocolError> {
        let path = self.dir.journal(handle, kind);
        let from = tail_start(&path, 64 * 1024).map_err(io)?;
        let read = read_since(&path, from, 1 << 20).map_err(io)?;
        let skip = read.lines.len().saturating_sub(count);
        Ok((read.lines.iter().skip(skip))
            .filter_map(|line| serde_json::from_str(line).ok().map(camel))
            .collect())
    }

    pub(super) fn log_len(&self, handle: &Handle, kind: LogKind) -> u64 {
        let path = self.dir.journal(handle, kind);
        std::fs::metadata(path).map_or(0, |metadata| metadata.len())
    }
}

/// A log entry in the answers' casing: keys in camelCase, `…_at_ms` times as `…At`.
fn camel(value: Value) -> Value {
    match value {
        Value::Object(map) => (map.into_iter())
            .map(|(key, value)| (camel_key(&key), camel(value)))
            .collect(),
        Value::Array(items) => items.into_iter().map(camel).collect(),
        other => other,
    }
}

fn camel_key(key: &str) -> String {
    let key = key
        .strip_suffix("_ms")
        .filter(|stem| stem.ends_with("_at"))
        .unwrap_or(key);
    let mut out = String::with_capacity(key.len());
    let mut upper = false;
    for c in key.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn failed(line: &str) -> bool {
    serde_json::from_str::<CommandRecord>(line)
        .is_ok_and(|record| record.exit_code.is_some_and(|code| code != 0))
}

#[expect(clippy::needless_pass_by_value, reason = "it's `map_err`'s function")]
fn io(error: std::io::Error) -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Internal,
        format!("couldn't read the log: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_read_in_camel_case() {
        let entry = json!({
            "started_at_ms": 1, "duration_ms": 2, "exit_code": 0,
            "by": { "kind": "agent" }, "human": { "pressed": { "key": "a" } },
        });
        let expected = json!({
            "startedAt": 1, "durationMs": 2, "exitCode": 0,
            "by": { "kind": "agent" }, "human": { "pressed": { "key": "a" } },
        });
        assert_eq!(camel(entry), expected);
    }
}
