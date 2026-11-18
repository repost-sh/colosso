//! A site's icon in the launcher's rows: a globe while it's read the first time its host is
//! asked for, then the site's own as soon as the read lands, and the row picked stays picked
//! when an icon changes under it.

use std::sync::Arc;

use gpui_kit::{Entity, Image, ImageFormat, TestAppContext};

use super::{Launcher, launch, place, shell};
use crate::files::tests::press;
use crate::history::{self, How};
use crate::launcher::rows::{Act, Glyph};
use crate::test_support::scratch;

/// The icons of the rows that visit a page: each site's PNG bytes, or `None` for the globe.
fn glyphs(launcher: &Entity<Launcher>, cx: &TestAppContext) -> Vec<Option<Vec<u8>>> {
    cx.read(|cx| {
        (launcher.read(cx).rows.iter())
            .filter(|row| matches!(row.act, Act::Visit(_)))
            .map(|row| match &row.glyph {
                Glyph::Image(image) => Some(image.bytes.clone()),
                Glyph::Site(url) => history::icon(url, cx).map(|icon| icon.bytes.clone()),
                Glyph::Icon(_) => None,
            })
            .collect()
    })
}

#[gpui_kit::test]
fn a_sites_icon_replaces_the_globe_once_read_and_the_row_picked_stays_picked(
    cx: &mut TestAppContext,
) {
    let (window, _, root) = shell("launcher-icons", cx);
    let folder = scratch("launcher-icons-kept");
    std::fs::write(folder.join("example.com.png"), b"com").unwrap();
    std::fs::write(folder.join("example.org.png"), b"org").unwrap();
    cx.update(|cx| {
        history::keep_icons_in(&folder, cx);
        for page in ["https://example.com/", "https://example.org/"] {
            history::visited(page, "Example", How::Typed, &place(&root), cx);
        }
    });
    let (launcher_window, launcher) = launch(window, cx);
    let sorted = |cx: &TestAppContext| {
        let mut shown = glyphs(&launcher, cx);
        shown.sort();
        shown
    };
    assert_eq!(
        sorted(cx),
        [Some(b"com".to_vec()), Some(b"org".to_vec())],
        "the top sites' rows, made as the launcher opened, show their icons once read"
    );

    press(launcher_window, "down", cx);
    let picked = cx.read(|cx| launcher.read(cx).selected);
    assert_eq!(picked, Some(0));
    let fetched = Arc::new(Image::from_bytes(ImageFormat::Png, b"fetched".to_vec()));
    cx.update(|cx| history::keep_icon("https://example.com/", &fetched, cx));
    cx.run_until_parked();
    assert_eq!(
        sorted(cx),
        [Some(b"fetched".to_vec()), Some(b"org".to_vec())],
        "an icon fetched since shows at once"
    );
    assert_eq!(
        cx.read(|cx| launcher.read(cx).selected),
        picked,
        "the row picked stays picked"
    );
}

/// The launcher open on a page leaves the browser engine idle: it starts once a page is asked
/// for, as the page's view is made (`BrowserView::ensure_open`), not while the address is typed.
#[gpui_kit::test]
fn the_launcher_on_a_page_leaves_the_browser_engine_idle_until_a_page_is_asked_for(
    cx: &mut TestAppContext,
) {
    let (window, _, _) = shell("launcher-engine", cx);
    let (_, launcher) = launch(window, cx);
    assert_eq!(cx.read(|cx| launcher.read(cx).kind), super::Kind::Browser);
    assert!(
        cx.update(crate::browser::engine_idle),
        "the launcher alone asked for it"
    );
}
