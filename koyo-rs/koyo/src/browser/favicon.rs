//! The page's icon, shown on its tab in place of the globe: its own, which the host fetches through
//! the engine each time the page names a new one, and sends as a PNG; and its site's, as the history
//! keeps it (`history::icon`), while it has none of its own, as before it has loaded.

use std::sync::Arc;

use gpui_kit::{App, Context, Image, ImageFormat};

use super::view::{BrowserEvent, BrowserView};
use crate::history;

/// An icon a page named, fetched.
pub(super) struct Favicon {
    /// The address of the page that named it.
    page: String,
    image: Arc<Image>,
}

impl BrowserView {
    /// The page's icon: its own, once it's fetched, or else its site's as the history keeps it.
    pub fn favicon(&self, cx: &App) -> Option<Arc<Image>> {
        match &self.favicon {
            Some(own) => Some(own.image.clone()),
            None => history::icon(&self.address(), cx),
        }
    }

    /// The icon a page here named last, once it's fetched, with that page's address: the page shown
    /// may be a later one, which hasn't named its own yet.
    pub fn named_favicon(&self) -> Option<(&str, &Arc<Image>)> {
        (self.favicon.as_ref()).map(|own| (own.page.as_str(), &own.image))
    }

    /// The icons kept changed: the page shows its site's while it has none of its own.
    pub(super) fn kept_favicons_changed(&mut self, cx: &mut Context<Self>) {
        if self.favicon.is_none() {
            cx.emit(BrowserEvent::Changed);
        }
    }

    /// The page names another icon, or none: with none, its own goes, and whether it did; another
    /// shows once it's fetched, the one shown staying till then.
    pub(super) fn favicon_named(&mut self, url: &str) -> bool {
        url.is_empty() && self.favicon.take().is_some()
    }

    /// The icon at `url` was fetched, as a PNG; empty when it couldn't be.
    pub(super) fn favicon_fetched(&mut self, url: &str, png: Vec<u8>, cx: &mut Context<Self>) {
        if url != self.page.favicon {
            return;
        }
        self.favicon = (!png.is_empty()).then(|| Favicon {
            page: self.page.url.clone(),
            image: Arc::new(Image::from_bytes(ImageFormat::Png, png)),
        });
        cx.emit(BrowserEvent::Changed);
        cx.notify();
    }

    /// The page is at `url`, showing the icon at `icon` once it's fetched, or none: as the host
    /// says, for tests outside the browser.
    #[cfg(test)]
    pub(crate) fn pretend_page(&mut self, url: &str, icon: Option<&str>, cx: &mut Context<Self>) {
        let page = koyo_browser_ipc::PageState {
            url: url.into(),
            favicon: icon.unwrap_or_default().into(),
            ..koyo_browser_ipc::PageState::default()
        };
        self.page_changed(page, cx);
        if let Some(icon) = icon {
            self.favicon_fetched(icon, vec![1], cx);
        }
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
        cx.update(crate::test_support::init);
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
                view.favicon(cx).is_none(),
                "one the page no longer names is stale"
            );
            view.favicon_fetched(a, vec![1], cx);
            assert!(view.favicon(cx).is_some());
            view.page_changed(naming(b), cx);
            assert!(view.favicon(cx).is_some(), "kept till the next one is in");
            view.favicon_fetched(b, Vec::new(), cx);
            assert!(view.favicon(cx).is_none(), "the next one couldn't be fetched");
            view.page_changed(naming(a), cx);
            view.favicon_fetched(a, vec![1], cx);
            view.page_changed(naming(""), cx);
            assert!(
                view.favicon(cx).is_none(),
                "a page that names none shows none"
            );
        });
    }

    /// Before it names one, and when the one it names can't be fetched, a page shows its site's icon
    /// as kept; its own once it's fetched. An icon is the page's that named it, though another page
    /// shows by then.
    #[gpui_kit::test]
    fn a_page_shows_its_sites_kept_icon_until_it_has_its_own(cx: &mut TestAppContext) {
        cx.update(crate::test_support::init);
        let kept = Arc::new(Image::from_bytes(ImageFormat::Png, vec![7]));
        let (_, view) = in_window(size(px(800.), px(600.)), cx, |window, cx| {
            let url = "https://example.com/start".to_owned();
            cx.new(|cx| BrowserView::new(url, Path::new("/work/app"), window, cx))
        });
        let shows = |cx: &mut TestAppContext| {
            view.read_with(cx, |view, cx| view.favicon(cx).map(|icon| icon.bytes.clone()))
        };
        assert_eq!(shows(cx), None, "none kept");
        cx.update(|cx| history::keep_icon("https://example.com/", &kept, cx));
        assert_eq!(shows(cx), Some(vec![7]), "its site's");

        let page = |url: &str, favicon: &str| PageState {
            url: url.into(),
            favicon: favicon.into(),
            ..PageState::default()
        };
        let (here, icon) = ("https://example.com/start", "https://example.com/a.ico");
        view.update(cx, |view, cx| {
            view.page_changed(page(here, icon), cx);
            view.favicon_fetched(icon, Vec::new(), cx);
        });
        assert_eq!(shows(cx), Some(vec![7]), "its own couldn't be fetched");
        view.update(cx, |view, cx| view.favicon_fetched(icon, vec![1], cx));
        assert_eq!(shows(cx), Some(vec![1]), "its own");

        view.update(cx, |view, cx| {
            view.page_changed(page("https://example.org/", icon), cx);
            let named = view.named_favicon().map(|(page, _)| page.to_owned());
            assert_eq!(named.as_deref(), Some(here), "the page that named it");
        });
    }
}
