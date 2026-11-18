use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::Path;

use crate::failure::Failure;
use crate::files::{git, git_lookup};

use super::graph::Edge;

/// The list of the edges that lie on a cycle, by its path in the checkout.
pub const FILE: &str = "koyo-rs/module-edges.txt";

/// An edge on the list, with its line in the file.
pub struct Listed {
    pub edge: Edge,
    pub line: usize,
}

pub struct List {
    pub text: String,
    pub edges: Vec<Listed>,
}

impl List {
    pub fn read(root: &Path) -> Result<Self, Failure> {
        let text = match fs::read_to_string(root.join(FILE)) {
            Ok(text) => text,
            Err(failure) if failure.kind() == io::ErrorKind::NotFound => {
                return Err(Failure::new(
                    FILE,
                    format!("missing: restore it from git, as `git checkout HEAD -- {FILE}` does"),
                ));
            }
            Err(failure) => return Err(Failure::new(FILE, failure)),
        };
        Self::parse(text, FILE)
    }

    /// The list as the last commit holds it, or `None` before the commit that adds it.
    pub fn at_head(root: &Path) -> Result<Option<Self>, Failure> {
        let name = format!("HEAD:{FILE}");
        if git_lookup(root, &["rev-parse", "--verify", "-q", &name])?.is_none() {
            return Ok(None);
        }
        let blob = git(Some(root), &["cat-file", "blob", &name])?;
        let text = String::from_utf8(blob).map_err(|failure| Failure::new(&name, failure))?;
        Self::parse(text, &name).map(Some)
    }

    /// The list without the lines of the edges `keep` turns down, everything else as written.
    pub fn keeping(&self, keep: impl Fn(&Edge) -> bool) -> Result<Self, Failure> {
        let gone: BTreeSet<usize> = self
            .edges
            .iter()
            .filter(|listed| !keep(&listed.edge))
            .map(|listed| listed.line)
            .collect();
        let text = self
            .text
            .split_inclusive('\n')
            .enumerate()
            .filter(|(index, _)| !gone.contains(&(index + 1)))
            .map(|(_, line)| line)
            .collect();
        Self::parse(text, FILE)
    }

    pub fn write(&self, root: &Path) -> Result<(), Failure> {
        fs::write(root.join(FILE), &self.text).map_err(|failure| Failure::new(FILE, failure))
    }

    pub fn edges(&self) -> BTreeSet<&Edge> {
        self.edges.iter().map(|listed| &listed.edge).collect()
    }

    fn parse(text: String, name: &str) -> Result<Self, Failure> {
        let mut edges = Vec::new();
        for (index, line) in text.lines().enumerate() {
            let content = line.trim();
            if content.is_empty() || content.starts_with('#') {
                continue;
            }
            let mut words = content.split_whitespace();
            let (Some(from), Some(to), None) = (words.next(), words.next(), words.next()) else {
                return Err(Failure::new(
                    format!("{name}:{}", index + 1),
                    format!("expected a `from to` line, found {content:?}"),
                ));
            };
            edges.push(Listed {
                edge: Edge {
                    from: from.to_owned(),
                    to: to.to_owned(),
                },
                line: index + 1,
            });
        }
        Ok(Self { text, edges })
    }
}
