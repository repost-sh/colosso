//! The browser tabs the builds before this one kept in a file of their own, beside the
//! window's (`docs/browser.md` 2): read the first time the window's file isn't there, so
//! nobody's tabs are lost to the change, and left as it is for a build that still reads it.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{KeptCheckout, KeptPane, KeptTree};
use crate::kept_file;

/// What that file is called.
const FILE: &str = "browser-tabs.json";

#[derive(Deserialize)]
struct Before {
    #[serde(default)]
    checkouts: Vec<CheckoutBefore>,
}

#[derive(Deserialize)]
struct CheckoutBefore {
    project: PathBuf,
    checkout: PathBuf,
    tabs: Vec<TabBefore>,
}

#[derive(Deserialize)]
struct TabBefore {
    url: String,
    #[serde(default)]
    title: String,
}

/// The browser tabs kept beside the window's file at `window`, as the window keeps them: each
/// checkout's pages, a tab each, the first shown.
pub(super) fn tabs_kept_before(window: &Path) -> Vec<KeptCheckout> {
    let before: Option<Before> = kept_file::read(&window.with_file_name(FILE));
    let checkouts = before.map(|before| before.checkouts).unwrap_or_default();
    (checkouts.into_iter())
        .map(|kept| KeptCheckout {
            project: kept.project,
            checkout: kept.checkout,
            shown: 0,
            tabs: (kept.tabs.into_iter())
                .map(|tab| {
                    KeptTree::Pane(KeptPane::Page {
                        url: tab.url,
                        title: tab.title,
                    })
                })
                .collect(),
        })
        .collect()
}
