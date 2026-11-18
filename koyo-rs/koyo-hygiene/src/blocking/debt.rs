use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::failure::Failure;
use crate::list_file::{self, Line};

/// The files that still block, by its path in the checkout.
pub const FILE: &str = "koyo-rs/blocking-debt.txt";

/// What the list's paths are relative to: the Cargo workspace, as in `hygiene-allow.txt`.
pub const WORKSPACE: &str = "koyo-rs/";

/// A file on the list: its path in the workspace, and the blocking calls it may hold.
pub struct Owed {
    pub path: String,
    pub count: usize,
}

impl Line for Owed {
    const FILE: &'static str = FILE;
    const SHAPE: &'static str = "`path count`";

    fn parse(path: &str, count: &str) -> Result<Self, String> {
        let Ok(count) = count.parse() else {
            return Err(format!("{count:?} isn't a count"));
        };
        Ok(Self {
            path: path.to_owned(),
            count,
        })
    }
}

pub type Debt = list_file::List<Owed>;

/// The list at `root`, each file on it once.
pub fn read(root: &Path) -> Result<Debt, Failure> {
    let debt = Debt::read(root)?;
    let mut seen = BTreeSet::new();
    for entry in &debt.entries {
        if !seen.insert(entry.value.path.as_str()) {
            let place = format!("{FILE}:{}", entry.line);
            return Err(Failure::new(
                place,
                format!("{} is listed twice", entry.value.path),
            ));
        }
    }
    Ok(debt)
}

/// Each file's allowance, by its path in the workspace.
pub fn counts(debt: &Debt) -> BTreeMap<&str, usize> {
    (debt.entries.iter())
        .map(|entry| (entry.value.path.as_str(), entry.value.count))
        .collect()
}
