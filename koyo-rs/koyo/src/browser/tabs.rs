//! Browser tabs across a window's close and the app's restart (`docs/browser.md`). The window
//! keeps a page pane with its other panes (`shell/kept.rs`), by its address and title; it
//! comes back as a pane with no page, and its page is made, at that address, only once its tab
//! is shown.

use std::path::Path;

use gpui_kit::{App, Context, Window};

use super::profiles::By;
use super::toolbar::tab_title;
use crate::history;
use crate::model::{Pane, Unloaded};
use crate::shell::Shell;

/// The pane a page the window kept comes back as: titled as it was, with its site's icon if
/// one is kept, and no page until its tab is shown.
pub(crate) fn page_brought_back(url: &str, title: &str, cx: &mut App) -> Pane {
    let unloaded = Unloaded {
        favicon: history::icon(url, cx),
        address: url.to_owned(),
    };
    Pane::unloaded(unloaded, tab_title(title, url).into())
}

impl Shell {
    /// Makes the pages the shown tab's browser panes don't have yet, at the addresses they
    /// kept, so the panes draw them, an agent's page an agent's again. Each opens its page once
    /// laid out; its first load is no new visit, as it was visited before.
    pub fn make_shown_browsers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let workspace = &self.workspace;
        let missing: Vec<(u64, String, By)> = (workspace.tab_shown())
            .map(|tab| {
                (tab.panes().into_iter())
                    .filter_map(|pane| {
                        Some((pane.id, pane.unloaded_address()?.to_owned(), pane.by()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if missing.is_empty() {
            return;
        }
        let apart = workspace.shown_apart_project().map(Path::to_owned);
        let selected = || Some(workspace.project()?.path.clone());
        let Some(project) = apart.or_else(selected) else {
            return;
        };
        for (id, address, by) in missing {
            let view = self.browser_view(address, &project, window, cx);
            view.update(cx, |view, _| {
                view.mark_restored();
                view.made_by(by);
            });
            if let Some(pane) = self.workspace.pane_mut(id) {
                pane.set_view(view, cx);
            }
        }
    }
}
