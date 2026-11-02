//! The records a terminal is described by, on the wire and in the daemon's registry.
//! See `docs/terminals.md` 5.2, 5.5, 5.6 and 5.7.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A terminal's identity, `term_` and a ULID; issued by the daemon and never reused.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Handle(pub String);

impl Handle {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The checkout a terminal belongs to: its canonical path, its repository's main checkout,
/// and what its head was when last read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeRef {
    /// The checkout's folder, canonicalized, so a path through a link is the same checkout.
    pub path: PathBuf,
    /// The repository's main checkout, canonicalized; equals `path` for the main one.
    pub repo_main: PathBuf,
    /// The branch name without `refs/heads/`, or `None` when detached.
    pub branch: Option<String>,
}

impl WorktreeRef {
    /// What the sidebar and the lists call it: the branch, else the folder's name.
    pub fn display_name(&self) -> String {
        self.branch.clone().unwrap_or_else(|| {
            self.path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
    }

    pub fn is_main(&self) -> bool {
        self.path == self.repo_main
    }
}

/// How a caller names a worktree, the same words the Orca CLI takes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Selector {
    /// The caller's own checkout: its cwd resolved upward to the nearest worktree root.
    Current,
    /// The main checkout of the caller's project.
    Main,
    Path(PathBuf),
    Branch(String),
    /// The checkout folder's name.
    Name(String),
}

impl Selector {
    /// Parses `current`, `main`, `path:<p>`, `branch:<b>`, `name:<n>`; a bare value that
    /// looks like a path is a path, anything else a branch.
    pub fn parse(text: &str) -> Selector {
        match text {
            "current" | "active" => Selector::Current,
            "main" => Selector::Main,
            other => {
                if let Some(path) = other.strip_prefix("path:") {
                    Selector::Path(PathBuf::from(path))
                } else if let Some(branch) = other.strip_prefix("branch:") {
                    Selector::Branch(branch.to_owned())
                } else if let Some(name) = other.strip_prefix("name:") {
                    Selector::Name(name.to_owned())
                } else if other.starts_with('/') || other.starts_with('~') || other.starts_with('.')
                {
                    Selector::Path(PathBuf::from(other))
                } else {
                    Selector::Branch(other.to_owned())
                }
            }
        }
    }
}

/// The grid's size in cells and the cell's size in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
    pub cell_width_px: u32,
    pub cell_height_px: u32,
}

impl Size {
    /// A grid with no cell size known.
    pub const fn cells(cols: u16, rows: u16) -> Size {
        Size {
            cols,
            rows,
            cell_width_px: 0,
            cell_height_px: 0,
        }
    }
}

impl Default for Size {
    fn default() -> Self {
        Size::cells(80, 24)
    }
}

/// The columns and the rows a pinned size may have, each: agent-terminal's limits, so a size
/// that works there works here (`docs/responsive.md` 3.1).
pub const PIN_CELLS: std::ops::RangeInclusive<u16> = 2..=1000;

/// Why `cols` × `rows` can't be pinned, when a side is outside `PIN_CELLS`.
pub fn pin_cells_error(cols: u16, rows: u16) -> Option<String> {
    if PIN_CELLS.contains(&cols) && PIN_CELLS.contains(&rows) {
        return None;
    }
    Some(format!(
        "cols and rows are each {} to {}; {cols}x{rows} isn't",
        PIN_CELLS.start(),
        PIN_CELLS.end()
    ))
}

/// `cols` × `rows`, each side brought within `PIN_CELLS`.
pub fn clamp_pin_cells(cols: u16, rows: u16) -> (u16, u16) {
    let within = |count: u16| count.clamp(*PIN_CELLS.start(), *PIN_CELLS.end());
    (within(cols), within(rows))
}

/// A size pinned on a terminal for every viewer, and who pinned it: the person in the app or
/// an agent through its relay. It lasts as long as the connection that set it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pinned {
    pub cols: u16,
    pub rows: u16,
    pub by: Actor,
}

/// Who did something: a person at the app, an agent through the MCP, or the daemon itself.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Actor {
    pub kind: ActorKind,
    /// `app`, or `<client name>:<connection id>` for an agent.
    pub id: String,
    /// What the UI calls it: "you", "Claude Code".
    pub label: String,
}

impl Actor {
    pub fn system() -> Actor {
        Actor {
            kind: ActorKind::System,
            id: "daemon".into(),
            label: "Colosso".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    Human,
    Agent,
    System,
}

/// What the program in the terminal is up to, derived in the daemon (5.7).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Unknown,
    Running,
    Idle,
    NeedsInput,
    Exited,
}

impl Status {
    /// How a person reads it: in the app's tab and panel, and in what the MCP's tools say.
    pub fn words(self) -> &'static str {
        match self {
            Status::Unknown => "starting",
            Status::Running => "running",
            Status::Idle => "idle",
            Status::NeedsInput => "needs input",
            Status::Exited => "exited",
        }
    }
}

/// A progress bar a program reports (OSC 9;4), shown on the tab until it's removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Progress {
    Percent(u8),
    Indeterminate,
    Error(Option<u8>),
    Paused(Option<u8>),
}

/// Who may write to a terminal (5.6).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Control {
    pub holder: Option<Controller>,
    pub observers: Vec<Actor>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Controller {
    /// A person took the terminal, by typing (`implicit`) or by asking (`explicit`); agents
    /// stay out until the person gives it back.
    Human {
        actor: Actor,
        since_ms: u64,
        implicit: bool,
    },
    /// An agent holds a lease it renews by acting; it lapses at `lease_until_ms`.
    Agent {
        actor: Actor,
        since_ms: u64,
        lease_until_ms: u64,
        reason: Option<String>,
    },
}

impl Controller {
    pub fn actor(&self) -> &Actor {
        match self {
            Controller::Human { actor, .. } | Controller::Agent { actor, .. } => actor,
        }
    }
}

/// How a terminal's process ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exit {
    pub code: Option<i32>,
    pub signal: Option<i32>,
    /// The daemon restarted under it, so nothing is known of how it ended.
    pub daemon_restart: bool,
}

/// Everything the daemon keeps about a terminal; the registry's row and the wire's summary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalRecord {
    pub handle: Handle,
    pub worktree: Option<WorktreeRef>,
    /// Unique within the worktree.
    pub name: String,
    pub title: Option<String>,
    /// Where the shell is, as it last reported, else where it was spawned.
    pub cwd: PathBuf,
    /// The program; empty means the login shell.
    pub command: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub size: Size,
    pub labels: BTreeSet<String>,
    pub created_ms: u64,
    pub created_by: Actor,
    /// Bumps on every spawn of the terminal's process.
    pub incarnation: u32,
    pub pid: Option<u32>,
    pub exit: Option<Exit>,
    pub status: Status,
    /// What the status came from: "prompt", "command running", "bell", "exited 1".
    pub status_reason: String,
    /// The last command's exit code, from the shell's OSC 133 D.
    pub last_exit: Option<i32>,
    pub progress: Option<Progress>,
    pub control: Control,
    pub attached_clients: u32,
    pub last_output_ms: u64,
    /// The size pinned on it, which every viewer shows, while the connection that set it
    /// lasts.
    #[serde(default)]
    pub pinned: Option<Pinned>,
}

impl TerminalRecord {
    /// Whether `cwd` has left the worktree the terminal belongs to.
    pub fn outside_worktree(&self) -> bool {
        self.worktree
            .as_ref()
            .is_some_and(|worktree| !self.cwd.starts_with(&worktree.path))
    }
}

/// A record as a list shows it, with the last non-blank screen line for a preview.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSummary {
    pub record: TerminalRecord,
    pub preview: String,
}

/// The nearest ancestor of `path` holding a `.git` entry (a folder for a main checkout, a
/// file for a linked worktree), canonicalized; `None` outside any checkout.
pub fn worktree_root(path: &Path) -> Option<PathBuf> {
    let start = std::fs::canonicalize(path).ok()?;
    start
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selectors_parse_the_orca_words() {
        assert_eq!(Selector::parse("current"), Selector::Current);
        assert_eq!(Selector::parse("active"), Selector::Current);
        assert_eq!(Selector::parse("main"), Selector::Main);
        assert_eq!(
            Selector::parse("path:/tmp/x"),
            Selector::Path("/tmp/x".into())
        );
        assert_eq!(Selector::parse("/tmp/x"), Selector::Path("/tmp/x".into()));
        assert_eq!(
            Selector::parse("branch:feat/a"),
            Selector::Branch("feat/a".into())
        );
        assert_eq!(Selector::parse("feat/a"), Selector::Branch("feat/a".into()));
        assert_eq!(
            Selector::parse("name:feat-a"),
            Selector::Name("feat-a".into())
        );
    }

    #[test]
    fn worktree_names_itself_by_branch_then_folder() {
        let detached = WorktreeRef {
            path: "/w/feat-x".into(),
            repo_main: "/w/main".into(),
            branch: None,
        };
        assert_eq!(detached.display_name(), "feat-x");
        assert!(!detached.is_main());
        let main = WorktreeRef {
            path: "/w/main".into(),
            repo_main: "/w/main".into(),
            branch: Some("main".into()),
        };
        assert_eq!(main.display_name(), "main");
        assert!(main.is_main());
    }

    #[test]
    fn finds_the_worktree_root_of_this_checkout() {
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        let root = worktree_root(here).expect("this crate is in a checkout");
        assert!(root.join("Cargo.toml").exists());
        assert!(root.join(".git").exists());
        assert_eq!(worktree_root(Path::new("/")), None);
    }
}
