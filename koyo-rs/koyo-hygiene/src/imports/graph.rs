use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

/// The node for the crate root's own items and code: `crate::init`, and `settings::init(cx)`
/// written in `main.rs`.
pub const ROOT: &str = "main";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Edge {
    pub from: String,
    pub to: String,
}

impl fmt::Display for Edge {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} -> {}", self.from, self.to)
    }
}

/// A place in a file, ordered by file, then line, then column.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Place {
    pub file: String,
    pub line: usize,
    pub column: usize,
}

/// The places the source makes an edge: the first, how many there are, and the files they're
/// in, each with the files its paths reach there.
pub struct Sites {
    pub first: Place,
    pub count: usize,
    pub files: BTreeMap<String, BTreeSet<String>>,
}

/// The import graph between the crate's top-level modules and its root.
#[derive(Default)]
pub struct Graph {
    /// Every top-level module, and [`ROOT`].
    pub modules: BTreeSet<String>,
    pub edges: BTreeMap<Edge, Sites>,
}

impl Graph {
    /// Adds a path at `place` that makes `edge` and reaches the files `reached`.
    pub fn add(&mut self, edge: Edge, place: Place, reached: impl Iterator<Item = String>) {
        let sites = self.edges.entry(edge).or_insert_with(|| Sites {
            first: place.clone(),
            count: 0,
            files: BTreeMap::new(),
        });
        sites.count += 1;
        let files = sites.files.entry(place.file.clone()).or_default();
        files.extend(reached);
        if place < sites.first {
            sites.first = place;
        }
    }
}

/// The graph with what each module reaches, which tells the edges that lie on a cycle.
pub struct Cycles<'graph> {
    graph: &'graph Graph,
    targets: BTreeMap<&'graph str, Vec<&'graph str>>,
    reach: BTreeMap<&'graph str, BTreeSet<&'graph str>>,
}

impl<'graph> Cycles<'graph> {
    pub fn of(graph: &'graph Graph) -> Self {
        let mut targets: BTreeMap<&str, Vec<&str>> = graph
            .modules
            .iter()
            .map(|module| (module.as_str(), Vec::new()))
            .collect();
        for edge in graph.edges.keys() {
            targets.entry(&edge.from).or_default().push(&edge.to);
        }
        let reach = targets
            .keys()
            .map(|module| (*module, reachable(&targets, module)))
            .collect();
        Self {
            graph,
            targets,
            reach,
        }
    }

    /// Whether the source makes the edge and its target reaches back to its source.
    pub fn on_cycle(&self, edge: &Edge) -> bool {
        self.graph.edges.contains_key(edge) && self.reaches(&edge.to, &edge.from)
    }

    /// The edges that lie on a cycle, in order.
    pub fn edges(&self) -> impl Iterator<Item = (&'graph Edge, &'graph Sites)> {
        self.graph
            .edges
            .iter()
            .filter(|(edge, _)| self.reaches(&edge.to, &edge.from))
    }

    /// How many modules the largest cycle holds: the most that each reach every other, or 0
    /// when no module is on a cycle.
    pub fn largest(&self) -> usize {
        self.reach
            .iter()
            .map(|(module, reached)| {
                reached
                    .iter()
                    .filter(|other| self.reaches(other, module))
                    .count()
            })
            .filter(|size| *size > 1)
            .max()
            .unwrap_or_default()
    }

    /// A shortest cycle the edge closes, as the modules along it from its source back to it.
    pub fn closed_by(&self, edge: &'graph Edge) -> Vec<&'graph str> {
        let (from, to) = (edge.from.as_str(), edge.to.as_str());
        let mut previous: BTreeMap<&str, &str> = BTreeMap::new();
        let mut pending = VecDeque::from([to]);
        while let Some(module) = pending.pop_front() {
            if module == from {
                break;
            }
            for next in self.targets.get(module).into_iter().flatten() {
                if *next != to && !previous.contains_key(next) {
                    previous.insert(next, module);
                    pending.push_back(next);
                }
            }
        }
        let mut back = vec![from];
        while let Some(before) = back.last().and_then(|module| previous.get(module)) {
            back.push(before);
        }
        back.push(from);
        back.reverse();
        back
    }

    fn reaches(&self, from: &str, to: &str) -> bool {
        self.reach
            .get(from)
            .is_some_and(|reached| reached.contains(to))
    }
}

fn reachable<'graph>(
    targets: &BTreeMap<&'graph str, Vec<&'graph str>>,
    start: &'graph str,
) -> BTreeSet<&'graph str> {
    let mut reached = BTreeSet::from([start]);
    let mut pending = vec![start];
    while let Some(module) = pending.pop() {
        for next in targets.get(module).into_iter().flatten() {
            if reached.insert(*next) {
                pending.push(next);
            }
        }
    }
    reached
}
