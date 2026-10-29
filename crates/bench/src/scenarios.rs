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
}

impl Kind {
    const ALL: [Self; 15] = [
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

    /// Whether it works the file tree's rows rather than a file or a checkout.
    pub fn lists(self) -> bool {
        matches!(
            self,
            Self::RowsExpand | Self::RowsCollapse | Self::RowsUpdate | Self::RowsReveal
        )
    }
}

/// Which statistic a budget is held against.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stat {
    P95,
    Max,
}

impl Stat {
    pub fn name(self) -> &'static str {
        match self {
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
/// rows on the tree and only there; the single line is opened, scrolled and typed in; search
/// runs on the fixtures marked for it; everything else runs on every file.
fn applies(kind: Kind, fixture: &Fixture) -> bool {
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
    /// tree's rows never take 2 ms, well inside a frame.
    pub fn budget(self, fixture_bytes: usize) -> Option<Budget> {
        let (ms, stat) = match self.kind {
            Kind::RowsExpand | Kind::RowsCollapse | Kind::RowsUpdate | Kind::RowsReveal => {
                (2., Stat::Max)
            }
            Kind::Scan => (3000., Stat::Max),
            Kind::ScanTouch => (100., Stat::P95),
            Kind::Type | Kind::TypeWrap | Kind::SearchQuery | Kind::SearchEdit => (4., Stat::P95),
            Kind::Scroll => (8.3, Stat::P95),
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
        assert_eq!(all.len(), 56);
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
                "rows-reveal/tree-500k"
            ]
        );
        let last: Vec<String> = all[all.len() - 3..].iter().map(|s| s.id()).collect();
        assert_eq!(last, ["open/json-1mb", "scroll/json-1mb", "type/json-1mb"]);
    }
}
