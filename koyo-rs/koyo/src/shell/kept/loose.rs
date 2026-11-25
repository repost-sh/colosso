//! The loose pages as the window keeps them (`docs/sidebar.md` 11, `docs/browser.md` 2): in
//! the order of their rows, by the id of the space each is filed under, each page by the address
//! it showed, its title and the project whose profile it keeps, with the one each space showed.
//! They're open tabs like a checkout's, so they're in the window's file, and come back as it's
//! read.

use std::collections::BTreeMap;
use std::path::PathBuf;

use gpui_kit::{App, Context};
use koyo_browser_ipc::Session;
use serde::{Deserialize, Serialize};

use crate::browser;
use crate::browser::profiles::By;
use crate::model::{Loose, Tab};
use crate::settings;
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
    /// The pages back and forward of it, and where it was scrolled (`docs/browser.md` 7).
    #[serde(default, skip_serializing_if = "Session::is_empty")]
    pub session: Session,
    /// The spaces besides its own that show it, as a space may while loose pages are shared:
    /// its own says so in `shown`, which a build before this one reads.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shown_in: Vec<u64>,
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
    /// The loose pages as they're kept, as their panes show them: in the order of their rows,
    /// each run of those filed under one space an entry of that space, the place of the one it
    /// shows in that entry's `shown`, so a build before this one reads them; and each space
    /// that shows one filed under another in that page's `shown_in`.
    pub(super) fn kept_spaces(&self, cx: &App) -> Vec<KeptSpace> {
        let shown_by: BTreeMap<u64, u64> = self.workspace.loose_shown_by().collect();
        let mut spaces: Vec<KeptSpace> = Vec::new();
        for (space, page) in self.workspace.loose_filed() {
            let pane = page.tab.first();
            let shown_in = (shown_by.iter())
                .filter(|(by, key)| **key == page.key() && **by != space)
                .map(|(by, _)| *by);
            let tab = KeptLoose {
                url: pane.address().unwrap_or_default().to_owned(),
                title: pane.title.to_string(),
                project: page.project.clone(),
                session: pane.session(cx),
                shown_in: shown_in.collect(),
            };
            if spaces.last().is_none_or(|run| run.space != space) {
                spaces.push(KeptSpace {
                    space,
                    shown: None,
                    tabs: Vec::new(),
                });
            }
            let Some(run) = spaces.last_mut() else {
                continue;
            };
            if shown_by.get(&space) == Some(&page.key()) {
                run.shown = Some(run.tabs.len());
            }
            run.tabs.push(tab);
        }
        spaces
    }

    /// Brings the pages that are no checkout's back as the files have them, once, as the window
    /// opens: the pinned ones (`pinned.rs`), and each space's loose ones, in the order of their
    /// rows, titled as they were, the one each space showed shown, filed under its own space or
    /// shown by another. Those of a space that's gone since are the space shown's, rather than
    /// lost. No page is made until it's shown.
    pub(crate) fn restore_pages(&mut self, cx: &mut Context<Self>) {
        self.restore_pinned(cx);
        let settings = settings::get(cx);
        let known: Vec<u64> = settings.spaces.iter().map(|space| space.id).collect();
        let here = settings.shown_space_id();
        let (spaces, workspace) = (&self.kept.kept.spaces, &mut self.workspace);
        for kept in spaces {
            let pages: Vec<Loose> = (kept.tabs.iter())
                .map(|tab| Loose {
                    tab: Tab::Single(browser::page_brought_back(
                        &tab.url,
                        &tab.title,
                        By::Person,
                        tab.session.clone(),
                        cx,
                    )),
                    project: tab.project.clone(),
                })
                .collect();
            let (space, shown) = if known.contains(&kept.space) {
                (kept.space, kept.shown)
            } else {
                (here, None)
            };
            let shown_in: Vec<(u64, u64)> = (kept.tabs.iter().zip(&pages))
                .flat_map(|(tab, page)| tab.shown_in.iter().map(move |by| (*by, page.key())))
                .filter(|(by, _)| known.contains(by))
                .collect();
            workspace.bring_back_loose(space, pages, shown);
            for (by, key) in shown_in {
                workspace.bring_back_loose_shown(by, key);
            }
        }
    }
}
