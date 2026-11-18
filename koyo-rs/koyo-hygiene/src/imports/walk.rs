use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::failure::Failure;
use crate::pool::map_in_order;
use crate::text::character_column;

use super::code::{Body, Declared, read};
use super::graph::{Edge, Graph, Place, ROOT};
use super::paths::{Base, Reach, Segment, reaches};

pub const CRATE_ROOT: &str = "koyo-rs/koyo/src/main.rs";

/// A file of the crate's module tree, by its path in the checkout.
struct Pending {
    path: PathBuf,
    module: Vec<String>,
    /// Where `mod x;` in the file looks for `x.rs` and `x/mod.rs`.
    owns: PathBuf,
}

/// One file read: the modules it declares, the files those live in, and each path in it with
/// the module path it's written in and its place.
struct Read {
    declared: Vec<Declared>,
    files: Vec<Pending>,
    sites: Vec<Vec<String>>,
    reaches: Vec<(Reach, Place)>,
}

/// The import graph of the koyo crate the shipped build compiles, read from the checkout at
/// `root`: the module tree walked from the crate root as rustc does, a level of files at a
/// time, each level's files read in parallel.
pub fn graph(root: &Path) -> Result<Graph, Failure> {
    let crate_root = PathBuf::from(CRATE_ROOT);
    let owns = crate_root
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let mut level = vec![Pending {
        path: crate_root,
        module: Vec::new(),
        owns,
    }];
    let mut graph = Graph::default();
    graph.modules.insert(ROOT.to_owned());
    while !level.is_empty() {
        let outcomes = map_in_order(&level, |file| read_file(root, file))
            .map_err(|failure| Failure::new("threads", failure))?;
        let mut next = Vec::new();
        for (file, outcome) in level.iter().zip(outcomes) {
            let read = outcome?;
            if file.module.is_empty() {
                let top_level = read
                    .declared
                    .iter()
                    .filter(|declared| declared.within.is_empty());
                graph
                    .modules
                    .extend(top_level.map(|declared| declared.name.clone()));
            }
            for (reach, place) in read.reaches {
                let site = &read.sites[reach.site];
                if let Some(edge) = edge(site, reach.base, &reach.segment, &graph.modules) {
                    graph.add(edge, place);
                }
            }
            next.extend(read.files);
        }
        level = next;
    }
    Ok(graph)
}

fn read_file(root: &Path, file: &Pending) -> Result<Read, Failure> {
    let display = file.path.display().to_string();
    let text = fs::read_to_string(root.join(&file.path))
        .map_err(|failure| Failure::new(&display, failure))?;
    let code = read(&text).map_err(|failure| Failure::new(&display, failure))?;
    let files = code
        .modules
        .iter()
        .filter_map(|declared| match &declared.body {
            Body::File { path } => Some(locate(root, file, declared, path.as_deref())),
            Body::Inline => None,
        })
        .collect::<Result<_, _>>()?;
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
                column: character_column(&text, reach.start),
            };
            (reach, place)
        })
        .collect();
    Ok(Read {
        declared: code.modules,
        files,
        sites,
        reaches,
    })
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

fn locate(
    root: &Path,
    file: &Pending,
    declared: &Declared,
    path: Option<&str>,
) -> Result<Pending, Failure> {
    let module = [
        file.module.as_slice(),
        &declared.within,
        std::slice::from_ref(&declared.name),
    ]
    .concat();
    let inline = declared
        .within
        .iter()
        .fold(file.owns.clone(), |folder, name| folder.join(name));
    if let Some(path) = path {
        let base = if declared.within.is_empty() {
            file.path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default()
        } else {
            inline
        };
        let path = base.join(path);
        let owns = path.parent().map(Path::to_path_buf).unwrap_or_default();
        return Ok(Pending { path, module, owns });
    }
    let owns = inline.join(&declared.name);
    let candidates = [
        inline.join(format!("{}.rs", declared.name)),
        owns.join("mod.rs"),
    ];
    match candidates
        .iter()
        .find(|candidate| root.join(candidate).is_file())
    {
        Some(path) => Ok(Pending {
            path: path.clone(),
            module,
            owns,
        }),
        None => Err(Failure::new(
            format!("{}:{}", file.path.display(), declared.line),
            format!(
                "mod {} has no file: neither {} nor {} exists",
                declared.name,
                candidates[0].display(),
                candidates[1].display()
            ),
        )),
    }
}
