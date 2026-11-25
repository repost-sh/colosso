use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::failure::Failure;
use crate::files::git_on;
use crate::list_file::Entry;
use crate::renames::Renames;
use crate::scratch::Scratch;
use crate::shipped::{self, KOYO, KOYO_FILES};

use super::graph::{Edge, Graph};
use super::list::{List, edges};
use super::walk::{graph, read};

/// The lines `list` gained since the last commit, for the source that makes `now`; none before
/// the commit that adds the list.
///
/// A line the last commit's list lacks isn't gained where it takes the place of one that went
/// with an import that moved: an import in a file git finds renamed into another module, or of a
/// file git finds renamed into another. A line that went gives its place to one line, so the
/// list never grows.
pub fn gained<'list>(
    root: &Path,
    now: &Graph,
    list: &'list List,
) -> Result<Vec<&'list Entry<Edge>>, Failure> {
    let Some(head) = List::at_head(root)? else {
        return Ok(Vec::new());
    };
    let (listed, at_head) = (edges(list), edges(&head));
    let mut lines: Vec<_> = list.entries.iter().collect();
    lines.retain(|line| !at_head.contains(&line.value));
    if lines.is_empty() {
        return Ok(lines);
    }
    let moves = Moves {
        renames: Renames::since_head(root)?,
        before: graph_at_head(root)?,
        now,
    };
    let sources: Vec<Vec<&Edge>> = lines
        .iter()
        .map(|line| {
            let mut sources: Vec<&Edge> = at_head.difference(&listed).copied().collect();
            sources.retain(|source| moves.carried(source, &line.value));
            sources
        })
        .collect();
    let mut taken = BTreeMap::new();
    Ok(lines
        .into_iter()
        .enumerate()
        .filter(|(line, _)| !pair(*line, &sources, &mut taken, &mut BTreeSet::new()))
        .map(|(_, entry)| entry)
        .collect())
}

/// The koyo crate's import graph as the last commit's source makes it.
fn graph_at_head(root: &Path) -> Result<Graph, Failure> {
    let scratch = Scratch::new(root, "head")?;
    let index = scratch.path("index");
    let here = format!("--prefix={KOYO_FILES}/");
    let files = format!("HEAD:{KOYO_FILES}");
    git_on(root, &index, &["read-tree", &here, &files])?;
    let there = format!("--prefix={}/", scratch.name("tree"));
    git_on(root, &index, &["checkout-index", "--all", &there])?;
    let reads = shipped::walk(&scratch.path("tree"), KOYO, read)
        .map_err(|failure| Failure::new(format!("HEAD:{}", failure.subject), failure.reason))?;
    Ok(graph(reads))
}

/// The import graph at the last commit and now, and the files renamed in between.
struct Moves<'now> {
    renames: Renames,
    before: Graph,
    now: &'now Graph,
}

impl Moves<'_> {
    /// Whether an import that made `source` at the last commit moved, and makes `edge` now: a
    /// file made the one and makes the other, under whichever name it has; it was renamed if it
    /// imports from another module now; and if it imports from another module's files, one of
    /// them was renamed there since it imported it.
    fn carried(&self, source: &Edge, edge: &Edge) -> bool {
        let (Some(before), Some(now)) = (self.before.edges.get(source), self.now.edges.get(edge))
        else {
            return false;
        };
        now.files.iter().any(|(file, imported)| {
            let renamed = self.renames.before(file);
            let Some(imported_before) = before.files.get(renamed.unwrap_or(file)) else {
                return false;
            };
            let from_moved = renamed.is_some();
            let to_moved = || {
                imported
                    .iter()
                    .filter_map(|file| self.renames.before(file))
                    .any(|file| imported_before.contains(file))
            };
            (source.from == edge.from || from_moved) && (source.to == edge.to || to_moved())
        })
    }
}

/// Gives `line` one of its `sources` no other line has taken, or one whose line takes another
/// instead, and says whether there is one.
fn pair<'head>(
    line: usize,
    sources: &[Vec<&'head Edge>],
    taken: &mut BTreeMap<&'head Edge, usize>,
    tried: &mut BTreeSet<&'head Edge>,
) -> bool {
    for source in &sources[line] {
        if tried.insert(source)
            && taken
                .get(source)
                .copied()
                .is_none_or(|other| pair(other, sources, taken, tried))
        {
            taken.insert(source, line);
            return true;
        }
    }
    false
}
