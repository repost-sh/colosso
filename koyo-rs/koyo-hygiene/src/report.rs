use std::collections::BTreeSet;
use std::io::{self, Write};

use crate::process::FileReport;
use crate::rule::RULES;

pub fn write_findings(out: &mut impl Write, reports: &[FileReport]) -> io::Result<()> {
    let mut counts = [0_usize; RULES.len()];
    let mut files = BTreeSet::new();
    for report in reports.iter().filter(|report| !report.findings.is_empty()) {
        files.insert(&report.path);
        for finding in &report.findings {
            writeln!(
                out,
                "{}:{}:{}: {}: {}",
                report.path, finding.line, finding.column, finding.rule.name, finding.detail
            )?;
            if let Some(slot) = RULES.iter().position(|rule| *rule == finding.rule) {
                counts[slot] += 1;
            }
        }
    }
    let total: usize = counts.iter().sum();
    if total == 0 {
        return Ok(());
    }
    let by_rule: Vec<String> = RULES
        .iter()
        .zip(counts)
        .filter(|(_, count)| *count > 0)
        .map(|(rule, count)| format!("{count} {}", rule.name))
        .collect();
    writeln!(
        out,
        "\n{} in {} ({}).",
        counted(total, "finding"),
        counted(files.len(), "file"),
        by_rule.join(", ")
    )?;
    for (rule, _) in RULES.iter().zip(counts).filter(|(_, count)| *count > 0) {
        writeln!(out, "\nTo fix {}:\n{}", rule.name, rule.fix)?;
    }
    Ok(())
}

pub fn write_fix_summary(
    out: &mut impl Write,
    deleted: usize,
    files_changed: usize,
) -> io::Result<()> {
    writeln!(
        out,
        "Deleted {} from {}.",
        counted(deleted, "comment"),
        counted(files_changed, "file")
    )
}

pub fn counted(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}
