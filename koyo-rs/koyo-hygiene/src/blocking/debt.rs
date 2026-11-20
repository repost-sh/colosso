use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::failure::Failure;
use crate::list_file::{self, Line};

/// The koyo crate's list of the files that still block, by its path in the checkout.
pub const FILE: &str = "koyo-rs/blocking-debt.txt";

/// What the lists' paths are relative to: the Cargo workspace, as in `hygiene-allow.txt`.
pub const WORKSPACE: &str = "koyo-rs/";

/// A crate's list of the files of its shipped build that still block, and where its files
/// are, both by their paths in the checkout. Each crate keeps a list of its own, seeded in the
/// commit that adds it as the koyo crate's was, and each only shrinks.
pub struct Ledger {
    pub list: &'static str,
    pub source: &'static str,
}

/// The lists: the app's, and the browser host's, which runs on the app's main thread.
pub const LEDGERS: [Ledger; 2] = [
    Ledger {
        list: FILE,
        source: "koyo-rs/koyo/",
    },
    Ledger {
        list: "koyo-rs/blocking-debt-browser-host.txt",
        source: "koyo-rs/koyo-browser-host/",
    },
];

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

/// `ledger`'s list at `root`, each file on it once, and each its crate's.
pub fn read(root: &Path, ledger: &Ledger) -> Result<Debt, Failure> {
    let debt = Debt::read_at(root, ledger.list)?;
    let mut seen = BTreeSet::new();
    for entry in &debt.entries {
        let place = || format!("{}:{}", ledger.list, entry.line);
        let path = &entry.value.path;
        if !seen.insert(path.as_str()) {
            return Err(Failure::new(place(), format!("{path} is listed twice")));
        }
        if !format!("{WORKSPACE}{path}").starts_with(ledger.source) {
            return Err(Failure::new(
                place(),
                format!("{path} isn't in {}, whose list this is", ledger.source),
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
