mod calls;
mod debt;
mod names;

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::Path;

use crate::failure::Failure;
use crate::list_file::Edit;
use crate::process::{FileReport, Found};
use crate::report::counted;
use crate::rule::{ADDED_CALL, BLOCKING_CALL, GONE_CALL, LAUNCH};
use crate::shipped::{Code, File};
use crate::text::character_column;

use calls::Standing;
use debt::{Debt, LEDGERS, Ledger, WORKSPACE, counts};

/// The file that may block as it pleases: the log's sink, which `log::Log::log` hands no
/// `Blocking`.
const FREE: &str = "koyo-rs/koyo/src/logs/sink.rs";

/// A blocking call's place in its file, what it calls, and whether the code it's in may.
pub struct Site {
    line: usize,
    column: usize,
    what: String,
    standing: Standing,
}

/// The blocking calls of each file of the shipped build that makes one, by its path in the
/// checkout.
#[derive(Default)]
pub struct Calls(BTreeMap<String, Vec<Site>>);

/// The blocking calls of one file of the shipped build, by its path in the checkout.
pub fn read(file: &File, text: &str, code: &Code<'_>) -> Option<(String, Vec<Site>)> {
    let path = file.path.display().to_string();
    if path == FREE {
        return None;
    }
    let sites: Vec<Site> = calls::of(code, file.module.is_empty())
        .into_iter()
        .map(|call| Site {
            line: call.line,
            column: character_column(text, call.start),
            what: call.what,
            standing: call.standing,
        })
        .collect();
    (!sites.is_empty()).then_some((path, sites))
}

impl Calls {
    pub fn of(files: impl IntoIterator<Item = Option<(String, Vec<Site>)>>) -> Self {
        Self(files.into_iter().flatten().collect())
    }

    /// The calls of `path` the debt list counts: those in code that may not block.
    fn owed<'calls>(&'calls self, path: &str) -> impl Iterator<Item = &'calls Site> {
        (self.0.get(path).into_iter().flatten()).filter(|site| site.standing == Standing::Debt)
    }

    /// How many calls each crate's shipped build makes in code that may block, how many it
    /// owes and in how many files, and how many `blocking::at_launch` outside the crate root.
    pub fn write_stats(&self, out: &mut impl Write) -> io::Result<()> {
        for ledger in &LEDGERS {
            let ours = || (self.0.iter()).filter(|(path, _)| path.starts_with(ledger.source));
            let count = |standing| {
                ours()
                    .flat_map(|(_, sites)| sites)
                    .filter(|site| site.standing == standing)
                    .count()
            };
            let owing = ours()
                .filter(|(path, _)| self.owed(path).next().is_some())
                .count();
            writeln!(
                out,
                "Blocking calls in {}: {} on a Blocking, {} owed in {} ({}), {} at_launch outside the crate root.",
                ledger.source,
                count(Standing::Lent),
                count(Standing::Debt),
                counted(owing, "file"),
                ledger.list,
                count(Standing::Launch),
            )?;
        }
        Ok(())
    }
}

/// The blocking check of the checkout at `root`, whose shipped build makes `calls`, against
/// each crate's debt list: each blocking call in code that may not block that the list doesn't
/// allow, each `blocking::at_launch` outside the crate root, each listed count above what its
/// file holds, and each line the list gained or raised since the last commit.
pub fn check(root: &Path, calls: &Calls) -> Result<Vec<FileReport>, Failure> {
    let mut found = Vec::new();
    for ledger in &LEDGERS {
        let debt = debt::read(root, ledger)?;
        let head = Debt::at_head_at(root, ledger.list)?;
        found.extend(compare(calls, ledger, &debt, head.as_ref()));
    }
    Ok(found)
}

/// How many lines of each debt list a prune lowered or took off, by the list's path.
pub type Lowered = Vec<(&'static str, usize)>;

/// Lowers each count on each debt list to what its file holds, taking off the files that hold
/// none, and returns how many lines of which list changed and the check's findings with the
/// lists that are left. A count is never raised.
pub fn prune(root: &Path, calls: &Calls) -> Result<(Lowered, Vec<FileReport>), Failure> {
    let (mut lowered, mut found) = (Vec::new(), Vec::new());
    for ledger in &LEDGERS {
        let (left, changed) = debt::read(root, ledger)?.edited(|owed| {
            let holds = calls.owed(&format!("{WORKSPACE}{}", owed.path)).count();
            match holds {
                0 => Edit::Drop,
                holds if holds < owed.count => Edit::Write(format!("{} {holds}", owed.path)),
                _ => Edit::Keep,
            }
        })?;
        if changed > 0 {
            left.write(root)?;
            lowered.push((ledger.list, changed));
        }
        let head = Debt::at_head_at(root, ledger.list)?;
        found.extend(compare(calls, ledger, &left, head.as_ref()));
    }
    Ok((lowered, found))
}

fn workspace_path(path: &str) -> &str {
    path.strip_prefix(WORKSPACE).unwrap_or(path)
}

fn compare(calls: &Calls, ledger: &Ledger, debt: &Debt, head: Option<&Debt>) -> Vec<FileReport> {
    let list = ledger.list;
    let allowed = counts(debt);
    let at_head = head.map(counts);
    let mut found = Found::default();
    let ours = (calls.0.iter()).filter(|(path, _)| path.starts_with(ledger.source));
    for (path, sites) in ours {
        for site in sites
            .iter()
            .filter(|site| site.standing == Standing::Launch)
        {
            found.add(path, site.line, site.column, &LAUNCH, site.what.clone());
        }
        let owed: Vec<&Site> = calls.owed(path).collect();
        let count = allowed.get(workspace_path(path)).copied();
        if count.is_some_and(|count| owed.len() <= count) {
            continue;
        }
        for site in &owed {
            let detail = match count {
                Some(count) => format!(
                    "{} ({} in the file, {list} allows {count})",
                    site.what,
                    owed.len()
                ),
                None => site.what.clone(),
            };
            found.add(path, site.line, site.column, &BLOCKING_CALL, detail);
        }
    }
    for entry in &debt.entries {
        let owed = &entry.value;
        let holds = calls.owed(&format!("{WORKSPACE}{}", owed.path)).count();
        if holds < owed.count {
            let detail = format!("{} holds {holds}, not {}", owed.path, owed.count);
            found.add(list, entry.line, 1, &GONE_CALL, detail);
        }
        let before = at_head
            .as_ref()
            .map(|counts| counts.get(owed.path.as_str()).copied());
        let detail = match before {
            Some(None) => format!(
                "{} {}: not on the last commit's list",
                owed.path, owed.count
            ),
            Some(Some(was)) if was < owed.count => format!(
                "{} {}: {was} on the last commit's list",
                owed.path, owed.count
            ),
            _ => continue,
        };
        found.add(list, entry.line, 1, &ADDED_CALL, detail);
    }
    found.reports()
}
