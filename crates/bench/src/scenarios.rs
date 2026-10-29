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
}

impl Kind {
    const ALL: [Self; 7] = [
        Self::Open,
        Self::Scroll,
        Self::Type,
        Self::TypeWrap,
        Self::Resize,
        Self::SearchQuery,
        Self::SearchEdit,
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
        }
    }

    pub fn soft_wrap(self) -> bool {
        matches!(self, Self::TypeWrap | Self::Resize)
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

/// Whether `kind` is measured on `fixture`: the single line is opened, scrolled and typed
/// in; search runs on the fixtures marked for it; everything else runs on every file.
fn applies(kind: Kind, fixture: &Fixture) -> bool {
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
    /// blocking over 5 ms, and the first paint of a mebibyte under 50 ms.
    pub fn budget(self, fixture_bytes: usize) -> Option<Budget> {
        let (ms, stat) = match self.kind {
            Kind::Type | Kind::TypeWrap | Kind::SearchQuery | Kind::SearchEdit => (4., Stat::P95),
            Kind::Scroll => (8.3, Stat::P95),
            Kind::Resize => (5., Stat::Max),
            // The open budget is written for a mebibyte; bigger files have none.
            Kind::Open if fixture_bytes <= 1 << 20 => (50., Stat::Max),
            Kind::Open => return None,
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
        assert_eq!(all.len(), 48);
        let last: Vec<String> = all[all.len() - 3..].iter().map(|s| s.id()).collect();
        assert_eq!(last, ["open/json-1mb", "scroll/json-1mb", "type/json-1mb"]);
    }
}
