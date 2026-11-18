use std::io::{self, Write};

use crate::report::counted;

use super::graph::{Cycles, Edge, Graph, Sites};

/// How many of the edges that lie on a cycle, those with the fewest import sites, to name.
const CHEAPEST: usize = 15;

/// The graph's size and its largest cycle on one line, then the cheapest cuts: the edges on a
/// cycle with the fewest import sites, fewest first.
pub fn write(out: &mut impl Write, graph: &Graph) -> io::Result<()> {
    let cycles = Cycles::of(graph);
    let mut on_cycle: Vec<(&Edge, &Sites)> = cycles.edges().collect();
    writeln!(
        out,
        "modules {}, edges {}, edges on a cycle {}, largest cycle {} modules",
        graph.modules.len(),
        graph.edges.len(),
        on_cycle.len(),
        cycles.largest()
    )?;
    on_cycle.sort_by_key(|(edge, sites)| (sites.count, *edge));
    for (edge, sites) in on_cycle.iter().take(CHEAPEST) {
        writeln!(out, "{edge}: {}", counted(sites.count, "site"))?;
    }
    Ok(())
}
