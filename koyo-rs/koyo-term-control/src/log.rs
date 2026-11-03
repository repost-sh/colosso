//! A terminal's logs (`docs/terminals.md` 5.8): what was done to it, the commands its shell
//! ran, and the screenshots taken of it, one JSON object a line in `activity.jsonl`,
//! `commands.jsonl` and `screenshots.jsonl` under its folder. The daemon appends; the app and
//! the MCP read on from the byte offset they reached, so an idle log costs nothing.

use std::fs::File;
use std::io::{self, Read as _, Seek as _, SeekFrom};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::event::ControlChange;
use crate::protocol::ErrorCode;
use crate::terminal::Actor;

/// What's kept of a call's or a command's output; the rest is counted, not kept.
pub const KEPT_OUTPUT_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LogKind {
    Activity,
    Commands,
    Screenshots,
}

impl LogKind {
    pub const ALL: [LogKind; 3] = [LogKind::Activity, LogKind::Commands, LogKind::Screenshots];

    pub fn file_name(self) -> &'static str {
        match self {
            LogKind::Activity => "activity.jsonl",
            LogKind::Commands => "commands.jsonl",
            LogKind::Screenshots => "screenshots.jsonl",
        }
    }
}

/// One thing an agent or the person did to the terminal, or a change of who controls it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActivityEntry {
    /// RFC 3339 in UTC, to the millisecond.
    pub time: String,
    pub actor: Actor,
    /// The call as a line that does it again: `type "pnpm test"`, `claim --takeover`.
    pub command: String,
    /// What it did, in words: `typed "pnpm test"`, `pressed enter 3 times`.
    pub action: String,
    #[serde(default)]
    pub duration_ms: u64,
    /// 0 done; 4 a person has control; 1 refused otherwise (agent-terminal's classes).
    #[serde(default)]
    pub exit: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorCode>,
    /// How far the terminal's output had come when it happened, to read on from there.
    pub offset: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub screenshots: Vec<String>,
    /// What the call gave back, kept up to [`KEPT_OUTPUT_BYTES`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// The person's input, in full.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub human: Option<HumanInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control: Option<ControlChange>,
}

/// Input as the activity log keeps it: text typed, keys pressed, text pasted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HumanInput {
    Typed(String),
    Pressed {
        key: String,
        times: u32,
    },
    Pasted(String),
    /// Bytes no key or text accounts for, as a mouse report or an unknown sequence.
    Raw(u32),
}

/// A command the shell ran, whoever typed it, from its OSC 133 marks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandRecord {
    /// Counts up from 1 over the terminal's life.
    pub id: u64,
    pub command: String,
    pub cwd: PathBuf,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
    pub duration_ms: u64,
    pub exit_code: Option<i32>,
    /// The output's span in the terminal's byte stream.
    pub first_offset: u64,
    pub last_offset: u64,
    /// Who wrote last before it ran, `None` when no one did since the prompt.
    pub by: Option<Actor>,
    /// Its output as plain text, kept up to [`KEPT_OUTPUT_BYTES`].
    pub output: String,
}

/// A capture of the terminal's screen, its file under the terminal's folder.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScreenshotEntry {
    pub time: String,
    pub offset: u64,
    /// Relative to the terminal's folder: `screenshots/2026-10-30T14-22-05.006.png`.
    pub path: String,
    pub by: Actor,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub cols: u16,
    pub rows: u16,
}

/// The complete lines a log gained past an offset.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LogRead {
    pub lines: Vec<String>,
    /// Where to read on from next time.
    pub next: u64,
    /// The file was rotated since `offset`: these lines start from its beginning.
    pub rotated: bool,
}

/// Reads the complete lines `path` holds past `offset`, at most about `max_bytes` of them; a
/// line still being written is left for the next read. A missing file has no lines.
pub fn read_since(path: &Path, offset: u64, max_bytes: usize) -> io::Result<LogRead> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(LogRead::default()),
        Err(error) => return Err(error),
    };
    let len = file.metadata()?.len();
    // A file shorter than where the reader was has been rotated under it.
    let (offset, rotated) = if offset > len {
        (0, true)
    } else {
        (offset, false)
    };
    file.seek(SeekFrom::Start(offset))?;
    let mut buf = Vec::new();
    file.take(max_bytes as u64).read_to_end(&mut buf)?;
    let Some(end) = buf.iter().rposition(|&b| b == b'\n') else {
        return Ok(LogRead {
            lines: Vec::new(),
            next: offset,
            rotated,
        });
    };
    let lines = (buf[..end].split(|&b| b == b'\n'))
        .filter(|line| !line.is_empty())
        .map(|line| String::from_utf8_lossy(line).into_owned())
        .collect();
    Ok(LogRead {
        lines,
        next: offset + end as u64 + 1,
        rotated,
    })
}

/// Where to read `path` from for its newest lines, about `bytes` of them: the start of the
/// first whole line among its last `bytes`. A missing file starts at 0.
pub fn tail_start(path: &Path, bytes: u64) -> io::Result<u64> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let len = file.metadata()?.len();
    if len <= bytes {
        return Ok(0);
    }
    // The byte before too, so a line starting right at the cut counts as whole.
    let before = len - bytes - 1;
    file.seek(SeekFrom::Start(before))?;
    let mut buf = Vec::new();
    file.take(bytes + 1).read_to_end(&mut buf)?;
    Ok((buf.iter().position(|&b| b == b'\n')).map_or(len, |at| before + at as u64 + 1))
}

/// `text` cut to [`KEPT_OUTPUT_BYTES`] on a character boundary, saying how much went.
pub fn kept(text: &str) -> String {
    if text.len() <= KEPT_OUTPUT_BYTES {
        return text.to_owned();
    }
    let mut end = KEPT_OUTPUT_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… {} more bytes not kept", &text[..end], text.len() - end)
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = Path::new(env!("KOYO_TARGET_DIR")).join("test-logs");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn the_tail_starts_at_a_whole_line() {
        let path = scratch("tail-start.jsonl");
        assert_eq!(tail_start(&path, 4).unwrap(), 0, "no file yet");
        std::fs::write(&path, b"aa\nbb\ncc\n").unwrap();
        assert_eq!(tail_start(&path, 4).unwrap(), 6);
        assert_eq!(
            tail_start(&path, 6).unwrap(),
            3,
            "a line starting at the cut is whole"
        );
        assert_eq!(tail_start(&path, 100).unwrap(), 0);
        let lines = read_since(&path, tail_start(&path, 4).unwrap(), 1024)
            .unwrap()
            .lines;
        assert_eq!(lines, ["cc"]);
    }

    #[test]
    fn a_reader_gets_only_new_complete_lines() {
        let path = scratch("read-since.jsonl");
        assert_eq!(
            read_since(&path, 0, 1024).unwrap(),
            LogRead::default(),
            "no file yet"
        );
        let mut file = File::create(&path).unwrap();
        file.write_all(b"{\"a\":1}\n{\"a\":2}\n{\"a\":").unwrap();
        let first = read_since(&path, 0, 1024).unwrap();
        assert_eq!(first.lines, ["{\"a\":1}", "{\"a\":2}"]);
        assert_eq!(
            read_since(&path, first.next, 1024).unwrap().lines,
            Vec::<String>::new()
        );
        file.write_all(b"3}\n").unwrap();
        let second = read_since(&path, first.next, 1024).unwrap();
        assert_eq!(second.lines, ["{\"a\":3}"]);
        assert!(!second.rotated);
        std::fs::write(&path, b"{\"b\":1}\n").unwrap();
        let after = read_since(&path, second.next, 1024).unwrap();
        assert!(after.rotated);
        assert_eq!(after.lines, ["{\"b\":1}"]);
    }

    #[test]
    fn long_output_is_cut_on_a_character_and_counted() {
        let text = "é".repeat(KEPT_OUTPUT_BYTES);
        let cut = kept(&text);
        assert!(cut.ends_with(&format!("… {KEPT_OUTPUT_BYTES} more bytes not kept")));
        assert_eq!(kept("short"), "short");
    }
}
