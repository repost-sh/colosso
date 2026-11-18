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
use crate::shipped::{CRATE_ROOT, Code, File};
use crate::text::character_column;

use calls::Standing;
pub use debt::FILE as DEBT;
use debt::{Debt, WORKSPACE, counts};

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
    let sites: Vec<Site> = calls::of(code, path == CRATE_ROOT)
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

    /// How many calls the shipped build makes in code that may block, how many it owes and
    /// in how many files, and how many `blocking::at_launch` outside the crate root.
    pub fn write_stats(&self, out: &mut impl Write) -> io::Result<()> {
        let count = |standing| {
            (self.0.values().flatten())
                .filter(|site| site.standing == standing)
                .count()
        };
        let owing = (self.0.keys())
            .filter(|path| self.owed(path).next().is_some())
            .count();
        writeln!(
            out,
            "Blocking calls: {} on a Blocking, {} owed in {} ({DEBT}), {} at_launch outside main.rs.",
            count(Standing::Lent),
            count(Standing::Debt),
            counted(owing, "file"),
            count(Standing::Launch),
        )
    }
}

/// The blocking check of the checkout at `root`, whose shipped build makes `calls`: each
/// blocking call in code that may not block that the debt list doesn't allow, each
/// `blocking::at_launch` outside the crate root, each listed count above what its file holds,
/// and each line the list gained or raised since the last commit.
pub fn check(root: &Path, calls: &Calls) -> Result<Vec<FileReport>, Failure> {
    let debt = debt::read(root)?;
    let head = Debt::at_head(root)?;
    Ok(compare(calls, &debt, head.as_ref()))
}

/// Lowers each count on the list to what its file holds, taking off the files that hold
/// none, and returns how many lines changed and the check's findings with the list that's
/// left. A count is never raised.
pub fn prune(root: &Path, calls: &Calls) -> Result<(usize, Vec<FileReport>), Failure> {
    let (lowered, changed) = debt::read(root)?.edited(|owed| {
        let holds = calls.owed(&format!("{WORKSPACE}{}", owed.path)).count();
        match holds {
            0 => Edit::Drop,
            holds if holds < owed.count => Edit::Write(format!("{} {holds}", owed.path)),
            _ => Edit::Keep,
        }
    })?;
    if changed > 0 {
        lowered.write(root)?;
    }
    let head = Debt::at_head(root)?;
    Ok((changed, compare(calls, &lowered, head.as_ref())))
}

fn workspace_path(path: &str) -> &str {
    path.strip_prefix(WORKSPACE).unwrap_or(path)
}

fn compare(calls: &Calls, debt: &Debt, head: Option<&Debt>) -> Vec<FileReport> {
    let allowed = counts(debt);
    let at_head = head.map(counts);
    let mut found = Found::default();
    for (path, sites) in &calls.0 {
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
                    "{} ({} in the file, {DEBT} allows {count})",
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
            found.add(DEBT, entry.line, 1, &GONE_CALL, detail);
        }
        let before = at_head
            .as_ref()
            .map(|counts| counts.get(owed.path.as_str()).copied());
        let detail = match before {
            Some(None) => format!(
                "{} {}: not on the last commit's list",
                owed.path, owed.count
            ),
            Some(Some(was)) if was < owed.count => {
                format!(
                    "{} {}: {was} on the last commit's list",
                    owed.path, owed.count
                )
            }
            _ => continue,
        };
        found.add(DEBT, entry.line, 1, &ADDED_CALL, detail);
    }
    found.reports()
}
