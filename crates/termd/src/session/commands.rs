//! The commands log's lines (`docs/terminals.md` 5.8): every command the shell ran, whoever
//! typed it, from its OSC 133 marks. Output goes through the engine cut at each mark, so the
//! screen can be read where the mark fell: Ghostty's zsh integration sends a bare `133;C`, so
//! the command is the line above the cursor then, soft wraps followed, less the prompt's
//! width `133;B` left. A multi-line command keeps its last line.

use std::fmt::Write as _;

use colosso_term_control::{Actor, CommandRecord, KEPT_OUTPUT_BYTES, LogKind, log::kept, now_ms};
use colosso_term_engine::Mark;

use super::Session;
use crate::journal::Journal;
use crate::plain::plain;

/// Raw output kept while a command runs; plain text comes out shorter.
const KEPT_RAW: usize = 4 * KEPT_OUTPUT_BYTES;

pub struct Commands {
    next_id: u64,
    /// The column the last prompt left the cursor at, where input starts.
    prompt_col: u16,
    running: Option<Running>,
    /// Who wrote last, whom the next command is put down to.
    pub last_writer: Option<Actor>,
}

struct Running {
    id: u64,
    command: String,
    cwd: std::path::PathBuf,
    started_at_ms: u64,
    first_offset: u64,
    by: Option<Actor>,
    output: Vec<u8>,
    dropped: usize,
}

impl Commands {
    /// Counting on from the last command `journal` has.
    pub fn new(journal: &Journal) -> Commands {
        let next_id = (journal.last_line(LogKind::Commands))
            .and_then(|line| serde_json::from_str::<CommandRecord>(&line).ok())
            .map_or(1, |last| last.id + 1);
        Commands {
            next_id,
            prompt_col: 0,
            running: None,
            last_writer: None,
        }
    }
}

impl Session {
    /// Writes program output starting at `offset` to the engine, cut at the shell's marks and
    /// each mark acted on where it falls; the marks, for the status.
    pub(super) fn write_marked(&mut self, chunk: &[u8], offset: u64) -> Vec<Mark> {
        let mut marks = Vec::new();
        self.scanner.feed(chunk, &mut marks);
        let mut done = 0;
        for &(end, mark) in &marks {
            self.write_engine(&chunk[done..end]);
            done = end;
            self.mark(mark, offset + end as u64);
        }
        self.write_engine(&chunk[done..]);
        marks.into_iter().map(|(_, mark)| mark).collect()
    }

    fn write_engine(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.engine.write(bytes);
        if let Some(running) = &mut self.commands.running {
            let room = KEPT_RAW
                .saturating_sub(running.output.len())
                .min(bytes.len());
            running.output.extend_from_slice(&bytes[..room]);
            running.dropped += bytes.len() - room;
        }
    }

    fn mark(&mut self, mark: Mark, offset: u64) {
        match mark {
            Mark::PromptStart => {}
            Mark::InputStart => {
                if let Some(cursor) = self.engine.frame().ok().and_then(|frame| frame.cursor) {
                    self.commands.prompt_col = cursor.col;
                }
            }
            Mark::OutputStart => self.command_started(offset),
            Mark::CommandDone { exit } => self.command_done(exit, offset),
        }
    }

    fn command_started(&mut self, offset: u64) {
        // One that never said it ended is done now.
        self.command_done(None, offset);
        let command = self.command_line().unwrap_or_default();
        self.commands.running = Some(Running {
            id: self.commands.next_id,
            command,
            cwd: self.record.cwd.clone(),
            started_at_ms: now_ms(),
            first_offset: offset,
            by: self.commands.last_writer.take(),
            output: Vec::new(),
            dropped: 0,
        });
        self.commands.next_id += 1;
    }

    fn command_done(&mut self, exit: Option<i32>, offset: u64) {
        let Some(running) = self.commands.running.take() else {
            return;
        };
        let ended = now_ms();
        let mut output = plain(&running.output);
        if running.dropped > 0 {
            let _ = write!(output, "… {} more bytes not kept", running.dropped);
        }
        let record = CommandRecord {
            id: running.id,
            command: running.command,
            cwd: running.cwd,
            started_at_ms: running.started_at_ms,
            ended_at_ms: ended,
            duration_ms: ended.saturating_sub(running.started_at_ms),
            exit_code: exit,
            first_offset: running.first_offset,
            last_offset: offset,
            by: running.by,
            output: kept(output.trim_end()),
        };
        self.append_log(LogKind::Commands, &record);
    }

    /// The command as the screen shows it when it starts.
    fn command_line(&mut self) -> Option<String> {
        if self.engine.is_alt_screen() {
            return None;
        }
        let frame = match self.engine.frame() {
            Ok(frame) => frame,
            Err(error) => {
                log::debug!("{}: couldn't read the command: {error}", self.record.handle);
                return None;
            }
        };
        let cursor = frame.cursor?;
        let last = if cursor.col == 0 {
            cursor.row.checked_sub(1)?
        } else {
            cursor.row
        };
        let start = frame.line_start(last);
        let mut line = String::new();
        for row in start..=last {
            let from = if row == start {
                self.commands.prompt_col
            } else {
                0
            };
            line.push_str(&frame.row_text(row, from));
        }
        let cols = usize::from(self.engine.cols());
        let reaches_edge = frame.row_text(last, 0).chars().count() + 1 >= cols;
        Some(without_right_prompt(&line, reaches_edge).trim().to_owned())
    }
}

/// `line` without a right prompt (zsh's `RPROMPT`) beside the command: when the line reaches
/// the screen's edge, what follows its last gap of three blanks or more.
fn without_right_prompt(line: &str, reaches_edge: bool) -> &str {
    match line.trim_end().rfind("   ") {
        Some(gap) if reaches_edge => line[..gap].trim_end(),
        _ => line,
    }
}

#[cfg(test)]
mod tests {
    use super::without_right_prompt;

    #[test]
    fn a_right_prompt_at_the_edge_is_left_out() {
        assert_eq!(
            without_right_prompt("git status        main ✓", true),
            "git status"
        );
        assert_eq!(without_right_prompt("echo 'a   b'", false), "echo 'a   b'");
    }
}
