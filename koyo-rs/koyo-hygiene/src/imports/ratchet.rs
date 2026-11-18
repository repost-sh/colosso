use std::collections::BTreeMap;

use crate::check::Finding;
use crate::process::FileReport;
use crate::rule::{ADDED_EDGE, GONE_EDGE, NEW_EDGE, Rule};

use super::graph::Cycles;
use super::list::{FILE, List};

/// Where the source and the list disagree: each edge on a cycle the list lacks, at the first
/// place the source makes it; each listed edge no longer on one; and each listed edge the
/// list at `head` lacks.
pub fn compare(cycles: &Cycles<'_>, list: &List, head: Option<&List>) -> Vec<FileReport> {
    let listed = list.edges();
    let at_head = head.map(List::edges);
    let mut found: BTreeMap<&str, Vec<Finding>> = BTreeMap::new();
    let mut add = |file, line, column, rule: &'static Rule, detail| {
        found.entry(file).or_default().push(Finding {
            line,
            column,
            rule,
            detail,
        });
    };
    for (edge, sites) in cycles.edges().filter(|(edge, _)| !listed.contains(edge)) {
        let cycle = cycles.closed_by(edge).join(" -> ");
        let place = &sites.first;
        add(
            &place.file,
            place.line,
            place.column,
            &NEW_EDGE,
            format!("{edge} closes {cycle}"),
        );
    }
    for entry in &list.edges {
        if !cycles.on_cycle(&entry.edge) {
            add(FILE, entry.line, 1, &GONE_EDGE, entry.edge.to_string());
        }
        if at_head
            .as_ref()
            .is_some_and(|before| !before.contains(&entry.edge))
        {
            add(FILE, entry.line, 1, &ADDED_EDGE, entry.edge.to_string());
        }
    }
    found
        .into_iter()
        .map(|(path, mut findings)| {
            findings.sort_by_key(|finding| (finding.line, finding.column));
            FileReport {
                path: path.to_owned(),
                findings,
                deleted: 0,
            }
        })
        .collect()
}
