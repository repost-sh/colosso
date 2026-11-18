use std::collections::BTreeSet;

use crate::shipped::{Code, File};
use crate::text::character_column;

use super::graph::{Edge, Graph, Place, ROOT};
use super::paths::{Base, Reach, Segment, reaches};

/// One file read: the top-level modules if it's the crate root, and each path in it with the
/// module path it's written in and its place.
pub struct Read {
    top_level: Vec<String>,
    sites: Vec<Vec<String>>,
    reaches: Vec<(Reach, Place)>,
}

/// The import graph of the koyo crate the shipped build compiles, from each of its files read.
pub fn graph(reads: Vec<Read>) -> Graph {
    let mut graph = Graph::default();
    graph.modules.insert(ROOT.to_owned());
    for read in &reads {
        graph.modules.extend(read.top_level.iter().cloned());
    }
    for read in reads {
        for (reach, place) in read.reaches {
            let site = &read.sites[reach.site];
            if let Some(edge) = edge(site, reach.base, &reach.segment, &graph.modules) {
                graph.add(edge, place);
            }
        }
    }
    graph
}

pub fn read(file: &File, text: &str, code: &Code<'_>) -> Read {
    let display = file.path.display().to_string();
    let top_level = if file.module.is_empty() {
        code.modules
            .iter()
            .filter(|declared| declared.within.is_empty())
            .map(|declared| declared.name.clone())
            .collect()
    } else {
        Vec::new()
    };
    let sites = code
        .sites
        .iter()
        .map(|within| [file.module.as_slice(), within].concat())
        .collect();
    let reaches = reaches(&code.tokens)
        .into_iter()
        .map(|reach| {
            let place = Place {
                file: display.clone(),
                line: reach.line,
                column: character_column(text, reach.start),
            };
            (reach, place)
        })
        .collect();
    Read {
        top_level,
        sites,
        reaches,
    }
}

/// The edge a path written in module `site` makes, if it reaches the crate root and from
/// there another top-level module, or the root's own items.
fn edge(
    site: &[String],
    base: Base,
    segment: &Segment,
    modules: &BTreeSet<String>,
) -> Option<Edge> {
    let at_root = match base {
        Base::Crate => true,
        Base::Up(levels) => levels == site.len(),
    };
    if !at_root {
        return None;
    }
    let from = site.first().map_or(ROOT, String::as_str);
    let to = match segment {
        Segment::Item(name) if modules.contains(name) => name.as_str(),
        Segment::Item(_) | Segment::Itself => ROOT,
    };
    (from != to).then(|| Edge {
        from: from.to_owned(),
        to: to.to_owned(),
    })
}
