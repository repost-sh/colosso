mod attributes;
mod code;
mod graph;
mod list;
mod paths;
mod ratchet;
mod stats;
mod walk;

use std::io::Write;
use std::path::Path;

use crate::failure::Failure;
use crate::process::FileReport;

use graph::Cycles;
pub use list::FILE as LIST;
use list::List;

/// The import check of the checkout at `root`: each edge between the koyo crate's top-level
/// modules that lies on a cycle and isn't on the list, each listed edge that no longer does,
/// and each line the list gained since the last commit.
pub fn check(root: &Path) -> Result<Vec<FileReport>, Failure> {
    let graph = walk::graph(root)?;
    let list = List::read(root)?;
    let head = List::at_head(root)?;
    Ok(ratchet::compare(&Cycles::of(&graph), &list, head.as_ref()))
}

/// Takes the edges that no longer lie on a cycle off the list, and returns how many went and
/// the import check's findings with the list that's left.
pub fn prune(root: &Path) -> Result<(usize, Vec<FileReport>), Failure> {
    let graph = walk::graph(root)?;
    let cycles = Cycles::of(&graph);
    let list = List::read(root)?;
    let kept = list.keeping(|edge| cycles.on_cycle(edge))?;
    let pruned = list.edges.len() - kept.edges.len();
    if pruned > 0 {
        kept.write(root)?;
    }
    let head = List::at_head(root)?;
    Ok((pruned, ratchet::compare(&cycles, &kept, head.as_ref())))
}

/// The import graph's size, its largest cycle and its cheapest cuts.
pub fn stats(root: &Path, out: &mut impl Write) -> Result<(), Failure> {
    stats::write(out, &walk::graph(root)?)?;
    Ok(())
}
