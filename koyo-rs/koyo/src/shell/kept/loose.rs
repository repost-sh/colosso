//! The loose pages as the window keeps them (`docs/arc-sidebar.md` 11, `docs/browser.md` 2): each
//! space's, by the space's id, in the order of their rows, each page by the address it showed,
//! its title and the project whose profile it keeps, with the one the space showed. They're
//! open tabs like a checkout's, so they're in the window's file, and come back as it's read.

use std::path::PathBuf;

use gpui_kit::{App, Context};
use serde::{Deserialize, Serialize};

use crate::browser;
use crate::model::{Loose, Tab};
use crate::settings::{self, Layout};
use crate::shell::Shell;

/// A loose page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptLoose {
    /// The address it showed; empty for a start page.
    pub url: String,
    /// Its title, until its page says otherwise.
    #[serde(default)]
    pub title: String,
    /// The project whose profile its page keeps, or what stands for none.
    #[serde(default)]
    pub project: PathBuf,
}

/// A space's loose pages.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptSpace {
    /// The space, by its id (`settings.json`, `spaces`).
    pub space: u64,
    /// The one shown in place of the selected checkout's tab, by its place among `tabs`.
    #[serde(default)]
    pub shown: Option<usize>,
    #[serde(default)]
    pub tabs: Vec<KeptLoose>,
}

impl Shell {
    /// Each space's loose pages as they're kept: as their panes show them. Not a space with
    /// none.
    pub(super) fn kept_spaces(&self, cx: &App) -> Vec<KeptSpace> {
        (self.workspace.loose_by_space())
            .filter(|(_, pages, _)| !pages.is_empty())
            .map(|(space, pages, shown)| {
                let tabs = pages.iter().map(|page| {
                    let pane = page.tab.first();
                    KeptLoose {
                        url: pane.address(cx).unwrap_or_default(),
                        title: pane.title.to_string(),
                        project: page.project.clone(),
                    }
                });
                KeptSpace {
                    space,
                    shown,
                    tabs: tabs.collect(),
                }
            })
            .collect()
    }

    /// Brings the pages that are no checkout's back as the files have them, once, as the window
    /// opens: the pinned ones (`pinned.rs`), and each space's loose ones, in the order of their
    /// rows, titled as they were, the one each space showed shown. Those of a space that's gone
    /// since are the space shown's, rather than lost. No page is made until it's shown.
    pub(crate) fn restore_pages(&mut self, cx: &mut Context<Self>) {
        self.restore_pinned(cx);
        let settings = settings::get(cx);
        let known: Vec<u64> = settings.spaces.iter().map(|space| space.id).collect();
        let here = settings.shown_space_id();
        let orca = settings.layout != Layout::Arc;
        let (spaces, workspace) = (&self.kept.kept.spaces, &mut self.workspace);
        for kept in spaces {
            let pages = (kept.tabs.iter())
                .map(|tab| Loose {
                    tab: Tab::Single(browser::page_brought_back(&tab.url, &tab.title, cx)),
                    project: tab.project.clone(),
                })
                .collect();
            let (space, shown) = if known.contains(&kept.space) {
                (kept.space, kept.shown)
            } else {
                (here, None)
            };
            workspace.bring_back_loose(space, pages, shown);
        }
        // The Orca layout lists no pinned or loose pages to show one of.
        if orca {
            self.workspace.leave_apart();
        }
    }
}
