//! A page the person closes is asked first, as Chrome asks a tab it closes (`docs/browser.md`
//! 17): one that asks to confirm leaving asks them, shown as it does. What they close goes once
//! every page of it may be left, as it goes when nothing asks; when they stay on one, nothing
//! changes. An agent's close lets pages go unasked.

use gpui_kit::{AsyncApp, Context, Entity, WeakEntity, Window};

use super::Shell;
use crate::browser::{BrowserView, Leave};
use crate::model::{Pane, Spot, Tab};

/// The pages `panes` show, but `DevTools`.
pub(super) fn pages<'a>(panes: impl IntoIterator<Item = &'a Pane>) -> Vec<WeakEntity<BrowserView>> {
    (panes.into_iter())
        .filter(|pane| !pane.is_inspector())
        .filter_map(Pane::view)
        .map(Entity::downgrade)
        .collect()
}

impl Shell {
    /// The pages of the tabs where `held` is true.
    pub(crate) fn pages_of(&self, held: impl Fn(Spot) -> bool) -> Vec<WeakEntity<BrowserView>> {
        let tabs = (self.workspace.everywhere()).filter(|visited| held(visited.spot));
        pages(tabs.map(|visited| visited.tab).flat_map(Tab::panes))
    }

    /// Does `then` once the person may leave each of `pages`, asked one after the other
    /// (`BrowserView::leave`), at once when none is open: never when they stay on one, nor when
    /// one asks them already, which is shown, nor when one is asked for a later close instead.
    pub(crate) fn ask_pages_then(
        &mut self,
        pages: Vec<WeakEntity<BrowserView>>,
        then: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let mut pages = pages.into_iter();
        while let Some(page) = pages.next() {
            let Some(page) = page.upgrade() else {
                continue;
            };
            let answer = match page.update(cx, BrowserView::leave) {
                Leave::NotOpen => continue,
                Leave::Asks => return,
                Leave::Asked(answer) => answer,
            };
            let ask_the_rest = async move |shell: WeakEntity<Self>, cx: &mut AsyncApp| {
                if answer.await != Ok(false) {
                    (shell.update(cx, |shell, cx| shell.ask_pages_then(pages.collect(), then, cx))).ok();
                }
            };
            return cx.spawn(ask_the_rest).detach();
        }
        then(self, cx);
    }

    /// The page `page` asks the person whether to leave it: they see it.
    pub(super) fn page_asks_to_leave(
        &mut self,
        page: &Entity<BrowserView>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        window.activate_window();
        self.tab_asked_for(page, cx);
    }
}

#[cfg(test)]
mod tests;
