//! A page the person closes is asked first, as Chrome asks a tab it closes (`docs/browser.md`
//! 17): one that asks to confirm leaving asks them, shown as it does, and its tab or pane goes
//! only once the page went, as it goes when nothing asks; when they stay, nothing changes. A
//! close that takes more than one page, or an agent's, lets them go unasked.

use gpui_kit::{App, Context, Entity};

use super::Shell;
use crate::browser::BrowserView;

impl Shell {
    /// Does `close` once `page` let the person go (`BrowserView::leave`), at once with none.
    pub(super) fn ask_page_then(
        &mut self,
        page: Option<Entity<BrowserView>>,
        close: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(page) = page else {
            return close(self, cx);
        };
        let shell = cx.entity().downgrade();
        let close = move |cx: &mut App| {
            (shell.update(cx, close)).ok();
        };
        page.update(cx, |page, cx| page.leave(Box::new(close), cx));
    }
}

#[cfg(test)]
mod tests;
