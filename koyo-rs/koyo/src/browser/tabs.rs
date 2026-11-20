//! Browser tabs across a window's close and the app's restart (`docs/browser.md`). The window
//! keeps a page pane with its other panes (`shell/kept.rs`), by its address and title; it
//! comes back as a pane with no page, and its page is made, at that address, only once its tab
//! is shown.

use std::path::Path;

use gpui_kit::{App, Context, Window};
use koyo_browser_ipc::Session;

use super::profiles::By;
use super::toolbar::tab_title;
use crate::history;
use crate::model::{Browser, Content, Pane, Unloaded};
use crate::shell::Shell;

/// The pane a page the window kept comes back as, `by`'s: titled as it was, with its site's
/// icon if one is kept, and no page until its tab is shown, which then opens with `session`,
/// the history around it and its scroll.
pub(crate) fn page_brought_back(
    url: &str,
    title: &str,
    by: By,
    session: Session,
    cx: &mut App,
) -> Pane {
    let unloaded = Unloaded {
        favicon: history::icon(url, cx),
        address: url.to_owned(),
        restored: true,
        session,
    };
    Pane {
        title: tab_title(title, url).into(),
        ..Pane::of(Content::Browser(Browser::waiting(unloaded, by)))
    }
}

impl Shell {
    /// Makes the pages the shown tab's browser panes in `missing` lack, each at the address its
    /// pane kept and as whose it was, so the panes draw them (`Shell::make_shown`), an agent's
    /// page an agent's again. Each opens its page once laid out; its first load is no new visit
    /// when it was visited before.
    pub(crate) fn make_pages(
        &mut self,
        missing: Vec<(u64, Unloaded, By)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if missing.is_empty() {
            return;
        }
        let workspace = &self.workspace;
        let apart = workspace.shown_apart_project().map(Path::to_owned);
        let selected = || Some(workspace.project()?.path.clone());
        let Some(project) = apart.or_else(selected) else {
            return;
        };
        for (id, page, by) in missing {
            let view = self.browser_view(page.address, &project, window, cx);
            view.update(cx, |view, _| {
                if page.restored {
                    view.mark_restored();
                }
                view.bring_back(page.session);
                view.made_by(by);
            });
            let pane = self.workspace.pane_mut(id);
            if let Some(Content::Browser(browser)) = pane.map(|pane| &mut pane.content) {
                browser.show(view, cx);
            }
        }
    }
}
