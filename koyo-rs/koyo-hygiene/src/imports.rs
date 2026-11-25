mod graph;
mod list;
mod moves;
mod paths;
mod ratchet;
mod stats;
mod walk;

use std::io::Write;
use std::path::Path;

use crate::failure::Failure;
use crate::list_file::Edit;
use crate::process::FileReport;

use graph::Cycles;
pub use graph::Graph;
pub use list::FILE as LIST;
use list::List;
pub use walk::{graph, read};

/// The import check of the checkout at `root`, whose shipped build makes `graph`: each edge
/// between the koyo crate's top-level modules that lies on a cycle and isn't on the list,
/// each listed edge that no longer does, and each line the list gained since the last commit.
pub fn check(root: &Path, graph: &Graph) -> Result<Vec<FileReport>, Failure> {
    let list = List::read(root)?;
    let gained = moves::gained(root, graph, &list)?;
    Ok(ratchet::compare(&Cycles::of(graph), &list, &gained))
}

/// Takes the edges that no longer lie on a cycle off the list, and returns how many went and
/// the import check's findings with the list that's left.
pub fn prune(root: &Path, graph: &Graph) -> Result<(usize, Vec<FileReport>), Failure> {
    let cycles = Cycles::of(graph);
    let (kept, pruned) = List::read(root)?.edited(|edge| {
        if cycles.on_cycle(edge) {
            Edit::Keep
        } else {
            Edit::Drop
        }
    })?;
    if pruned > 0 {
        kept.write(root)?;
    }
    let gained = moves::gained(root, graph, &kept)?;
    Ok((pruned, ratchet::compare(&cycles, &kept, &gained)))
}

/// The import graph's size, its largest cycle and its cheapest cuts.
pub fn stats(graph: &Graph, out: &mut impl Write) -> Result<(), Failure> {
    stats::write(out, graph)?;
    Ok(())
}
