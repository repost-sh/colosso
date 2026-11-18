use std::collections::BTreeSet;

use crate::list_file::{self, Line};

use super::graph::Edge;

/// The list of the edges that lie on a cycle, by its path in the checkout.
pub const FILE: &str = "koyo-rs/module-edges.txt";

pub type List = list_file::List<Edge>;

impl Line for Edge {
    const FILE: &'static str = FILE;
    const SHAPE: &'static str = "`from to`";

    fn parse(from: &str, to: &str) -> Result<Self, String> {
        Ok(Self {
            from: from.to_owned(),
            to: to.to_owned(),
        })
    }
}

pub fn edges(list: &List) -> BTreeSet<&Edge> {
    list.entries.iter().map(|entry| &entry.value).collect()
}
