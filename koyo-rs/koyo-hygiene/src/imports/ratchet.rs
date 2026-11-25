use crate::list_file::Entry;
use crate::process::{FileReport, Found};
use crate::rule::{ADDED_EDGE, GONE_EDGE, NEW_EDGE};

use super::graph::{Cycles, Edge};
use super::list::{FILE, List, edges};

/// Where the source and the list disagree: each edge on a cycle the list lacks, at the first
/// place the source makes it; each listed edge no longer on one; and each line the list
/// `gained` since the last commit.
pub fn compare(cycles: &Cycles<'_>, list: &List, gained: &[&Entry<Edge>]) -> Vec<FileReport> {
    let listed = edges(list);
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
    }
    for entry in gained {
        found.add(FILE, entry.line, 1, &ADDED_EDGE, entry.value.to_string());
    }
    found.reports()
}
