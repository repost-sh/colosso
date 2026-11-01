//! What each operation takes (`docs/terminals.md` 8.5): camelCase, unknown fields refused,
//! each field described for the schema agents read.

use std::collections::BTreeMap;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{LogKind, Signal, Status};

type TerminalArg = Option<String>;
type WorktreeArg = Option<String>;

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListArgs {
    /// The worktree: `current` (the default: where you run), `main`, a path or a branch.
    pub worktree: WorktreeArg,
    /// Only terminals in these states.
    pub status: Option<Vec<Status>>,
    /// Only terminals with every one of these labels.
    pub labels: Option<Vec<String>>,
    /// Where the previous page ended, its `nextCursor`.
    pub cursor: Option<String>,
    /// At most this many; 50 unless given.
    pub limit: Option<u32>,
}

/// A program: its argv, or a command line the login shell runs.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum CommandArg {
    Argv(Vec<String>),
    Line(String),
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
pub struct CellsArg {
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenArgs {
    /// The worktree: `current` (the default: where you run), `main`, a path or a branch.
    pub worktree: WorktreeArg,
    /// The terminal's name, unique in its worktree: `dev`, `tests`.
    pub name: String,
    /// What to run; left out, the login shell.
    pub command: Option<CommandArg>,
    /// Where to start; the worktree's root unless given.
    pub cwd: Option<PathBuf>,
    pub env: Option<BTreeMap<String, String>>,
    pub size: Option<CellsArg>,
    pub labels: Option<Vec<String>>,
    /// When the name is taken: `attach` to that terminal (the default) or `error`.
    pub if_exists: Option<IfExistsArg>,
    /// Start the program again when the terminal exists but its program ended.
    pub respawn: Option<bool>,
    /// Take control when it's free; true unless given.
    pub claim: Option<bool>,
    /// Sent again with the same arguments, the first answer comes back and nothing is redone.
    pub request_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IfExistsArg {
    Attach,
    Error,
}

/// What to read: the screen as it looks, or the output stream since a cursor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReadMode {
    #[default]
    Screen,
    Stream,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadSpec {
    /// `screen` (the default) when the answer depends on how output looks; `stream` for the
    /// text written since `cursor`, repaints showing as stacked fragments.
    pub mode: Option<ReadMode>,
    /// Stream only: `nextCursor` from the previous read; left out, the newest output.
    pub cursor: Option<u64>,
    /// Stream only: at most this many bytes, 64 KiB unless given.
    pub limit: Option<u32>,
    /// Screen only: rows `a:b`, from row a up to but not including b, counted from 0.
    pub rows: Option<String>,
    /// Screen only: runs of blank lines as one.
    pub fold_blank: Option<bool>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadArgs {
    /// The terminal: its handle (`term_…`) or its name in your worktree; left out, the one
    /// you last acted on there, else its only terminal.
    pub terminal: TerminalArg,
    pub mode: Option<ReadMode>,
    pub cursor: Option<u64>,
    pub limit: Option<u32>,
    pub rows: Option<String>,
    pub fold_blank: Option<bool>,
}

impl ReadArgs {
    pub fn spec(&self) -> ReadSpec {
        ReadSpec {
            mode: self.mode,
            cursor: self.cursor,
            limit: self.limit,
            rows: self.rows.clone(),
            fold_blank: self.fold_blank,
        }
    }
}

/// What to wait for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WaitFor {
    /// The program is quiet: a prompt, or no output for a while.
    Idle,
    Running,
    /// The shell's prompt is back (needs shell integration).
    Prompt,
    Exit,
    NeedsInput,
    /// `pattern` appears on the screen.
    Text,
    /// `pattern` is no longer on the screen.
    Gone,
    /// The screen stops changing for `stableMs`.
    Stable,
    /// No person holds the terminal.
    Control,
    StatusChange,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WaitSpec {
    #[serde(rename = "for")]
    pub until: WaitFor,
    /// A regular expression, for `text` and `gone`.
    pub pattern: Option<String>,
    /// Rows `a:b` the pattern is looked for in.
    pub rows: Option<String>,
    /// For `stable`: how long the screen holds still, 1500 ms unless given.
    pub stable_ms: Option<u64>,
    /// 25 s unless given, 10 minutes at most; running out answers `met: false`.
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WaitArgs {
    /// The terminal: its handle (`term_…`) or its name in your worktree; left out, the one
    /// you last acted on there, else its only terminal.
    pub terminal: TerminalArg,
    #[serde(rename = "for")]
    pub until: WaitFor,
    pub pattern: Option<String>,
    pub rows: Option<String>,
    pub stable_ms: Option<u64>,
    pub timeout_ms: Option<u64>,
    /// Read the terminal once the wait ends.
    pub read: Option<ReadSpec>,
}

impl WaitArgs {
    pub fn spec(&self) -> WaitSpec {
        WaitSpec {
            until: self.until,
            pattern: self.pattern.clone(),
            rows: self.rows.clone(),
            stable_ms: self.stable_ms,
            timeout_ms: self.timeout_ms,
        }
    }
}

/// What to do once the input is written: wait, then read, in the same call.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Then {
    pub wait: Option<WaitSpec>,
    pub read: Option<ReadSpec>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SendArgs {
    /// The terminal: its handle (`term_…`) or its name in your worktree; left out, the one
    /// you last acted on there, else its only terminal.
    pub terminal: TerminalArg,
    /// Typed as is; a newline is Enter.
    pub text: Option<String>,
    /// Keys by name, with modifiers: `enter`, `ctrl+c`, `escape`, `up`, `tab`, `shift+tab`,
    /// `f1`…`f12`, `ctrl+shift+p`; encoded for the terminal's current modes.
    pub keys: Option<Vec<String>>,
    /// Pasted, bracketed when the program asked for bracketed paste.
    pub paste: Option<String>,
    /// Press Enter after the text.
    pub enter: Option<bool>,
    /// Press Ctrl-C first.
    pub interrupt: Option<bool>,
    pub then: Option<Then>,
    /// Sent again with the same arguments, the first answer comes back and nothing is resent.
    pub request_id: Option<String>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClaimArgs {
    /// The terminal: its handle (`term_…`) or its name in your worktree; left out, the one
    /// you last acted on there, else its only terminal.
    pub terminal: TerminalArg,
    /// Displace another agent that holds it. Never displaces a person.
    pub takeover: Option<bool>,
    /// Why, shown to whoever loses it.
    pub reason: Option<String>,
    pub request_id: Option<String>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalArgs {
    /// The terminal: its handle (`term_…`) or its name in your worktree; left out, the one
    /// you last acted on there, else its only terminal.
    pub terminal: TerminalArg,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenameArgs {
    /// The terminal: its handle (`term_…`) or its name in your worktree; left out, the one
    /// you last acted on there, else its only terminal.
    pub terminal: TerminalArg,
    pub name: Option<String>,
    pub title: Option<String>,
    pub labels: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignalArgs {
    /// The terminal: its handle (`term_…`) or its name in your worktree; left out, the one
    /// you last acted on there, else its only terminal.
    pub terminal: TerminalArg,
    /// Sent to the program's process group.
    pub signal: Signal,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloseArgs {
    /// The terminal: its handle (`term_…`) or its name in your worktree; left out, the one
    /// you last acted on there, else its only terminal.
    pub terminal: TerminalArg,
    pub request_id: Option<String>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LogArgs {
    /// The terminal: its handle (`term_…`) or its name in your worktree; left out, the one
    /// you last acted on there, else its only terminal.
    pub terminal: TerminalArg,
    /// `activity` (the default: what people and agents did), `commands` (what the shell
    /// ran, with exit codes) or `screenshots`.
    pub kind: Option<LogKind>,
    /// `nextCursor` from the previous call; left out, the newest entries.
    pub cursor: Option<u64>,
    /// At most this many entries, 200 unless given.
    pub limit: Option<u32>,
    /// Commands only: just the ones that failed.
    pub failed: Option<bool>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorktreeListArgs {}
