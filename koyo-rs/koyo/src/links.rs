//! Links macOS hands Koyo to open (`docs/browser.md` 9.4). Koyo's bundle says it opens `http`
//! and `https` addresses and HTML documents, as a web browser's does (`script/bundle`), so
//! macOS hands it a link clicked in another app or a page's file opened in the Finder while
//! Koyo is the person's browser, or one it's asked to open by name. A file comes as its
//! `file` address. They come on the platform's say, before the app runs for the one Koyo is
//! started for, so they're kept in order until the app takes them. How a page is opened is
//! the caller's: the crate root opens each as an unpinned page (`main.rs`).

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui_kit::App;

/// Schemes of a page's address: a web page's, and that of a file, which is how macOS hands
/// over a document. The bundle says Koyo opens these and no other.
const PAGE_SCHEMES: [&str; 3] = ["http://", "https://", "file://"];

/// Where macOS's links are put as they come.
pub struct Handed(UnboundedSender<Vec<String>>);

/// The links handed over, in the order they came, those from before the app ran among them.
pub struct Links(UnboundedReceiver<Vec<String>>);

/// The two ends: one for the platform's callback, which runs before the app does, and one
/// for the app once it runs.
pub fn handed() -> (Handed, Links) {
    let (said, heard) = unbounded();
    (Handed(said), Links(heard))
}

impl Handed {
    /// Keeps `links` for the app to open. With the app gone there's nothing to open them in.
    pub fn take(&self, links: Vec<String>) {
        let _ = self.0.unbounded_send(links);
    }
}

/// Has `open` open the pages among the links, batch by batch as they come, those that came
/// before now first. What isn't a page's address is left out: the bundle asks for no other.
pub fn open_as_they_come(
    links: Links,
    open: impl Fn(Vec<String>, &mut App) + 'static,
    cx: &mut App,
) {
    cx.spawn(async move |cx| {
        let mut links = links.0;
        while let Some(handed) = links.next().await {
            let pages: Vec<String> = handed.into_iter().filter(|link| page(link)).collect();
            if !pages.is_empty() {
                cx.update(|cx| open(pages, cx));
            }
        }
    })
    .detach();
}

/// Whether `link` is a page's address, whatever its letters' case.
fn page(link: &str) -> bool {
    let lower = link.to_ascii_lowercase();
    PAGE_SCHEMES.iter().any(|scheme| lower.starts_with(scheme))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use gpui_kit::TestAppContext;

    use super::{handed, open_as_they_come, page};

    #[test]
    fn only_a_page_s_address_is_one() {
        assert!(page("https://example.com/") && page("http://localhost:3000/a?b=c"));
        assert!(page("HTTPS://Example.com/"));
        assert!(page("file:///Users/me/Sites/a%20page.html"));
        for other in [
            "",
            "mailto:someone@example.com",
            "koyo://x",
            "chrome://settings/",
            "example.com",
        ] {
            assert!(!page(other), "{other}");
        }
    }

    #[gpui_kit::test]
    fn links_open_in_the_order_they_came_those_from_before_the_app_ran_first(
        cx: &mut TestAppContext,
    ) {
        let (said, links) = handed();
        said.take(vec!["https://example.com/first".into()]);
        said.take(vec!["mailto:someone@example.com".into()]);
        let opened = Rc::new(RefCell::new(Vec::new()));
        let record = opened.clone();
        cx.update(|cx| {
            open_as_they_come(links, move |pages, _| record.borrow_mut().push(pages), cx);
        });
        cx.run_until_parked();
        assert_eq!(*opened.borrow(), [["https://example.com/first"]]);

        said.take(vec![
            "https://example.com/b".into(),
            "koyo://x".into(),
            "file:///Users/me/page.html".into(),
        ]);
        cx.run_until_parked();
        assert_eq!(
            *opened.borrow(),
            [
                vec!["https://example.com/first"],
                vec!["https://example.com/b", "file:///Users/me/page.html"]
            ],
            "what isn't a page's address was opened, or a batch of none was"
        );
    }
}
