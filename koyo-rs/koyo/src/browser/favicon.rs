//! The page's own icon, shown on its tab in place of the globe. The host fetches it through
//! the engine each time the page names a new one, and sends it as a PNG.

use std::sync::Arc;

use gpui_kit::{Context, Image, ImageFormat};

use super::view::{BrowserEvent, BrowserView};

impl BrowserView {
    /// The page's own icon, once it has one.
    pub fn favicon(&self) -> Option<Arc<Image>> {
        self.favicon.clone()
    }

    /// The page names another icon, or none: with none, the one shown goes, and whether it
    /// did; another shows once it's fetched, the one shown staying till then.
    pub(super) fn favicon_named(&mut self, url: &str) -> bool {
        url.is_empty() && self.favicon.take().is_some()
    }

    /// The icon at `url` was fetched, as a PNG; empty when it couldn't be.
    pub(super) fn favicon_fetched(&mut self, url: &str, png: Vec<u8>, cx: &mut Context<Self>) {
        // One the page has stopped naming since it was asked for is stale.
        if url != self.page.favicon {
            return;
        }
        self.favicon =
            (!png.is_empty()).then(|| Arc::new(Image::from_bytes(ImageFormat::Png, png)));
        cx.emit(BrowserEvent::Changed);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use gpui_kit::{AppContext as _, TestAppContext, px, size};
    use koyo_browser_ipc::PageState;

    use super::*;
    use crate::test_support::in_window;

    #[gpui_kit::test]
    fn a_page_shows_the_icon_it_names_once_fetched_until_it_names_none(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (_, view) = in_window(size(px(800.), px(600.)), cx, |window, cx| {
            cx.new(|cx| BrowserView::new(String::new(), Path::new("/work/app"), window, cx))
        });
        let naming = |favicon: &str| PageState {
            url: "https://example.com/".into(),
            favicon: favicon.into(),
            ..PageState::default()
        };
        let (a, b) = ("https://example.com/a.ico", "https://example.com/b.svg");
        view.update(cx, |view, cx| {
            view.page_changed(naming(a), cx);
            view.favicon_fetched("https://example.com/old.ico", vec![1], cx);
            assert!(
                view.favicon().is_none(),
                "one the page no longer names is stale"
            );
            view.favicon_fetched(a, vec![1], cx);
            assert!(view.favicon().is_some());
            view.page_changed(naming(b), cx);
            assert!(view.favicon().is_some(), "kept till the next one is in");
            view.favicon_fetched(b, Vec::new(), cx);
            assert!(view.favicon().is_none(), "the next one couldn't be fetched");
            view.page_changed(naming(a), cx);
            view.favicon_fetched(a, vec![1], cx);
            view.page_changed(naming(""), cx);
            assert!(
                view.favicon().is_none(),
                "a page that names none shows none"
            );
        });
    }
}
