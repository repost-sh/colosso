use crate::process::{FileReport, Found};
use crate::rule::{ADDED_EDGE, GONE_EDGE, NEW_EDGE};

use super::graph::Cycles;
use super::list::{FILE, List, edges};

/// Where the source and the list disagree: each edge on a cycle the list lacks, at the first
/// place the source makes it; each listed edge no longer on one; and each listed edge the
/// list at `head` lacks.
pub fn compare(cycles: &Cycles<'_>, list: &List, head: Option<&List>) -> Vec<FileReport> {
    let listed = edges(list);
    let at_head = head.map(edges);
    let mut found = Found::default();
    for (edge, sites) in cycles.edges().filter(|(edge, _)| !listed.contains(edge)) {
        let cycle = cycles.closed_by(edge).join(" -> ");
        let place = &sites.first;
        let detail = format!("{edge} closes {cycle}");
        found.add(&place.file, place.line, place.column, &NEW_EDGE, detail);
    }
    for entry in &list.entries {
        if !cycles.on_cycle(&entry.value) {
            found.add(FILE, entry.line, 1, &GONE_EDGE, entry.value.to_string());
        }
        if at_head
            .as_ref()
            .is_some_and(|before| !before.contains(&entry.value))
        {
            found.add(FILE, entry.line, 1, &ADDED_EDGE, entry.value.to_string());
        }
    }
    found.reports()
}
