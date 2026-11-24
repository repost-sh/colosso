//! A page the person closes is asked first, as Chrome asks a tab it closes (`docs/browser.md`
//! 17): one that asks to confirm leaving asks them, shown as it does, and its tab or pane goes
//! only once the page went, as it goes when nothing asks; when they stay, nothing changes. A
//! close that takes more than one page, or an agent's, lets them go unasked.

use std::collections::HashMap;

use gpui_kit::{App, Context, Entity, EntityId, Subscription};

use super::Shell;
use crate::browser::BrowserView;
use crate::model::Pane;

/// The pages the person closed that are still to go, by the page.
#[derive(Default)]
pub(super) struct Leaving(HashMap<EntityId, Closing>);

/// What the person's close of a page does once the page went; let go of with the page, should
/// a close that doesn't ask take it meanwhile.
struct Closing {
    then: Then,
    _released: Subscription,
}

/// A close of the person's, to do.
type Then = Box<dyn FnOnce(&mut Shell, &mut Context<Shell>)>;

impl Shell {
    /// Does `close` once `page` let the person go: at once with no page, or one that's not
    /// asked (`BrowserView::leave`); once it went, when it's asked. A page asked already is
    /// asked again, which the engine takes as the same ask while the first is still out, and
    /// the close it waits for stays the first.
    pub(super) fn ask_page_then(
        &mut self,
        page: Option<Entity<BrowserView>>,
        close: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(page) = page else {
            return close(self, cx);
        };
        let id = page.entity_id();
        let asked = page.update(cx, BrowserView::leave);
        if self.leaving.0.contains_key(&id) {
            return;
        }
        if !asked {
            return close(self, cx);
        }
        let released = cx.observe_release(&page, move |shell, _, _| {
            shell.leaving.0.remove(&id);
        });
        let then = Box::new(close);
        self.leaving.0.insert(id, Closing { then, _released: released });
    }

    /// `view`'s page, which the person closed, went: what their close does happens now.
    pub(super) fn page_left(&mut self, view: &Entity<BrowserView>, cx: &mut Context<Self>) {
        if let Some(closing) = self.leaving.0.remove(&view.entity_id()) {
            (closing.then)(self, cx);
        }
    }

    /// The person stayed on `view`'s page: their close does nothing.
    pub(super) fn page_stayed(&mut self, view: &Entity<BrowserView>) {
        self.leaving.0.remove(&view.entity_id());
    }

    /// The page a close of `panes` asks first: their one page, as Chrome asks a tab's page;
    /// none when they hold none, or more than one, which go unasked as a worktree's tabs
    /// cleared do.
    pub(super) fn page_among(panes: &[&Pane], cx: &App) -> Option<Entity<BrowserView>> {
        let mut found = (panes.iter())
            .filter_map(|pane| pane.view())
            .filter(|view| !view.read(cx).is_inspector());
        let page = found.next()?;
        found.next().is_none().then(|| page.clone())
    }
}

#[cfg(test)]
mod tests;
