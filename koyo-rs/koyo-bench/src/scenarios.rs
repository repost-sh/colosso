//! What is measured, on which fixture, and the budget each measurement is held to.

use serde::Serialize;

use crate::fixtures::{FIXTURES, Fixture};
use crate::stats::Summary;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The text set and the first frame drawn.
    Open,
    /// Half a page per frame, down the file (sideways through a single line).
    Scroll,
    /// A keystroke at the middle of the file, soft wrap off.
    Type,
    /// The same keystrokes with soft wrap on.
    TypeWrap,
    /// The editor narrowed and widened with soft wrap on.
    Resize,
    /// The open search panel's query typed and erased.
    SearchQuery,
    /// Keystrokes into the file while a search query is active.
    SearchEdit,
    /// A checkout opened until every folder git doesn't ignore is listed.
    Scan,
    /// A file made or removed, until the listing of its folder shows it.
    ScanTouch,
    /// Files written into an ignored folder: the work it costs, which should be none.
    BurstIgnored,
    /// Files written into tracked folders, until the listings show them all.
    BurstTracked,
    /// The file tree's 10,000-file folder opened, among half a million rows open.
    RowsExpand,
    /// The same folder closed.
    RowsCollapse,
    /// A folder near the top of the open tree listed anew, with a file more or less.
    RowsUpdate,
    /// A file 25 folders deep revealed, every folder above it closed.
    RowsReveal,
    /// Go to File's query typed a letter at a time over the tree's files, then erased.
    FinderQuery,
    /// A folder of files added while Go to File's query is typed.
    FinderUpdate,
    /// A keystroke into a program in a terminal, until its echo is in the client's engine.
    TermEcho,
    /// A large file `cat` in a terminal, until the client's engine has read it all.
    TermCat,
}

impl Kind {
    const ALL: [Self; 19] = [
        Self::Open,
        Self::Scroll,
        Self::Type,
        Self::TypeWrap,
        Self::Resize,
        Self::SearchQuery,
        Self::SearchEdit,
        Self::Scan,
        Self::ScanTouch,
        Self::BurstIgnored,
        Self::BurstTracked,
        Self::RowsExpand,
        Self::RowsCollapse,
        Self::RowsUpdate,
        Self::RowsReveal,
        Self::FinderQuery,
        Self::FinderUpdate,
        Self::TermEcho,
        Self::TermCat,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Scroll => "scroll",
            Self::Type => "type",
            Self::TypeWrap => "type-wrap",
            Self::Resize => "resize",
            Self::SearchQuery => "search-query",
            Self::SearchEdit => "search-edit",
            Self::Scan => "scan",
            Self::ScanTouch => "scan-touch",
            Self::BurstIgnored => "burst-ignored",
            Self::BurstTracked => "burst-tracked",
            Self::RowsExpand => "rows-expand",
            Self::RowsCollapse => "rows-collapse",
            Self::RowsUpdate => "rows-update",
            Self::RowsReveal => "rows-reveal",
            Self::FinderQuery => "finder-query",
            Self::FinderUpdate => "finder-update",
            Self::TermEcho => "term-echo",
            Self::TermCat => "term-cat",
        }
    }

    pub fn soft_wrap(self) -> bool {
        matches!(self, Self::TypeWrap | Self::Resize)
    }

    /// Whether it reads a checkout's files rather than editing one of them.
    pub fn scans(self) -> bool {
        matches!(
            self,
            Self::Scan | Self::ScanTouch | Self::BurstIgnored | Self::BurstTracked
        )
    }

    /// Whether it runs a program in a terminal, through the terminal daemon.
    pub fn terminal(self) -> bool {
        matches!(self, Self::TermEcho | Self::TermCat)
    }

    /// Whether it works the tree held in memory, its rows or its files searched, rather
    /// than a file or a checkout.
    pub fn lists(self) -> bool {
        matches!(
            self,
            Self::RowsExpand
                | Self::RowsCollapse
                | Self::RowsUpdate
                | Self::RowsReveal
                | Self::FinderQuery
                | Self::FinderUpdate
        )
    }
}

/// How much longer than a bare PTY 150 MiB of `cat` may take through the terminal. The bare
/// PTY is read by nothing else, on a machine otherwise idle; the daemon also parses and sends
/// on, and takes 1.14 to 1.25 times as long (`docs/terminals.md` 7.5). A blocking reader, as
/// the daemon had before, takes 1.5 times.
const TERM_CAT_SLACK: f64 = 0.30;

/// Which statistic a budget is held against.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stat {
    P50,
    P95,
    Max,
}

impl Stat {
    pub fn name(self) -> &'static str {
        match self {
            Self::P50 => "p50",
            Self::P95 => "p95",
            Self::Max => "max",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Budget {
    pub ms: f64,
    pub stat: Stat,
}

impl Budget {
    /// The statistic of `summary` this budget is held against.
    pub fn measured(self, summary: &Summary) -> f64 {
        match self.stat {
            Stat::P50 => summary.p50,
            Stat::P95 => summary.p95,
            Stat::Max => summary.max,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scenario {
    pub kind: Kind,
    pub fixture: &'static Fixture,
}

/// Whether `kind` is measured on `fixture`: the scans on the repository and only there; the
/// rows on the tree and only there; the terminal's echo on the program typed into and its
/// `cat` on the file shown; the single line is opened, scrolled and typed in; search runs on
/// the fixtures marked for it; everything else runs on every file.
fn applies(kind: Kind, fixture: &Fixture) -> bool {
    if kind.terminal() || fixture.is_terminal() {
        return match kind {
            Kind::TermEcho => fixture.is_terminal() && !fixture.is_shown(),
            Kind::TermCat => fixture.is_shown(),
            _ => false,
        };
    }
    if kind.lists() || fixture.is_tree() {
        return kind.lists() && fixture.is_tree();
    }
    if kind.scans() || fixture.is_repo() {
        return kind.scans() && fixture.is_repo();
    }
    if fixture.single_line {
        return matches!(kind, Kind::Open | Kind::Scroll | Kind::Type);
    }
    match kind {
        Kind::SearchQuery | Kind::SearchEdit => fixture.searched,
        _ => true,
    }
}

impl Scenario {
    /// Every scenario, in the order the table lists them: by kind, then by fixture, with the
    /// single line's scenarios together at the end.
    pub fn all() -> Vec<Self> {
        let mut all: Vec<Self> = Kind::ALL
            .into_iter()
            .flat_map(|kind| FIXTURES.iter().map(move |fixture| Self { kind, fixture }))
            .filter(|scenario| applies(scenario.kind, scenario.fixture))
            .collect();
        // Stable, so each group keeps its kind-by-fixture order.
        all.sort_by_key(|scenario| scenario.fixture.single_line);
        all
    }

    /// `type/rust-10k`: the kind, which is also the table's label, then the fixture.
    pub fn id(self) -> String {
        format!("{}/{}", self.kind.name(), self.fixture.name)
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::all().into_iter().find(|scenario| scenario.id() == id)
    }

    /// Keystroke to paint under 4 ms, a scroll frame under 8.3 ms (120 Hz), a resize never
    /// blocking over 5 ms, and the first paint of a mebibyte under 50 ms. A checkout listed
    /// within 3 s, and a file written shown in its folder's listing within 100 ms. The file
    /// tree's rows never take 2 ms, well inside a frame; Go to File's results are ready
    /// within a frame of a keystroke or a folder added. A keystroke's echo is in the
    /// terminal's engine within a frame at 120 Hz. `reference_ms` is what the run measured
    /// to hold a relative budget to: `cat` through the terminal takes at most
    /// `TERM_CAT_SLACK` more than through a bare PTY, which no terminal beats.
    pub fn budget(self, fixture_bytes: usize, reference_ms: Option<f64>) -> Option<Budget> {
        let (ms, stat) = match self.kind {
            Kind::TermCat => (reference_ms? * (1. + TERM_CAT_SLACK), Stat::P50),
            Kind::FinderQuery | Kind::FinderUpdate => (16., Stat::P95),
            Kind::RowsExpand | Kind::RowsCollapse | Kind::RowsUpdate | Kind::RowsReveal => {
                (2., Stat::Max)
            }
            Kind::Scan => (3000., Stat::Max),
            Kind::ScanTouch => (100., Stat::P95),
            Kind::Type | Kind::TypeWrap | Kind::SearchQuery | Kind::SearchEdit => (4., Stat::P95),
            // A frame at 120 Hz.
            Kind::Scroll | Kind::TermEcho => (8.3, Stat::P95),
            Kind::Resize => (5., Stat::Max),
            // The open budget is written for a mebibyte; bigger files have none.
            Kind::Open if fixture_bytes <= 1 << 20 => (50., Stat::Max),
            // Bursts are measured for what they cost, which nothing promises yet.
            Kind::Open | Kind::BurstIgnored | Kind::BurstTracked => return None,
        };
        Some(Budget { ms, stat })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_name_one_scenario_each() {
        let all = Scenario::all();
        for scenario in &all {
            assert_eq!(Scenario::from_id(&scenario.id()), Some(*scenario));
        }
        assert_eq!(all.len(), 60);
        let terminal: Vec<String> = (all.iter())
            .filter(|scenario| scenario.kind.terminal())
            .map(|scenario| scenario.id())
            .collect();
        assert_eq!(terminal, ["term-echo/pty", "term-cat/cat-150mb"]);
        let scans: Vec<String> = (all.iter())
            .filter(|scenario| scenario.kind.scans())
            .map(|scenario| scenario.id())
            .collect();
        assert_eq!(
            scans,
            [
                "scan/repo-300k",
                "scan-touch/repo-300k",
                "burst-ignored/repo-300k",
                "burst-tracked/repo-300k"
            ]
        );
        let rows: Vec<String> = (all.iter())
            .filter(|scenario| scenario.kind.lists())
            .map(|scenario| scenario.id())
            .collect();
        assert_eq!(
            rows,
            [
                "rows-expand/tree-500k",
                "rows-collapse/tree-500k",
                "rows-update/tree-500k",
                "rows-reveal/tree-500k",
                "finder-query/tree-500k",
                "finder-update/tree-500k"
            ]
        );
        let last: Vec<String> = all[all.len() - 3..].iter().map(|s| s.id()).collect();
        assert_eq!(last, ["open/json-1mb", "scroll/json-1mb", "type/json-1mb"]);
    }
}
