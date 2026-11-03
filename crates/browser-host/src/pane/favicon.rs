//! The page's icon: fetched through the engine, from its cache where it can be, near the
//! size a tab shows it at, and sent to the app as a PNG. A page lists its icons best first;
//! the first the engine makes an image of is the one sent, as one may be missing or in a
//! form it can't read.
use crate::host;
use cef::{
    Browser, CefString, DownloadImageCallback, Image, ImplBinaryValue, ImplBrowser,
    ImplBrowserHost, ImplDownloadImageCallback, ImplImage, WrapDownloadImageCallback, rc::Rc,
    wrap_download_image_callback,
};
use koyo_browser_ipc::{HostEvent, PaneId};
use std::os::raw::c_int;

/// The most pixels a side of the icon is fetched at: a tab's 13 points, on a 2x screen,
/// with room to spare.
const SIZE: u32 = 64;

/// Fetches the first of `urls` that makes an image, for `pane`'s page, which `browser`
/// shows. The app hears of it as the icon at the first of `urls`, as the page names it, or
/// that none made one.
pub(super) fn fetch(pane: PaneId, browser: &Browser, urls: Vec<String>) {
    if let Some(first) = urls.first().cloned() {
        try_next(pane, browser.clone(), first, urls);
    }
}

/// Fetches the first of `left`, the rest left for if it makes no image.
fn try_next(pane: PaneId, browser: Browser, first: String, mut left: Vec<String>) {
    let Some(host) = browser.host().filter(|_| !left.is_empty()) else {
        send(pane, first, Vec::new());
        return;
    };
    let url = CefString::from(left.remove(0).as_str());
    let mut fetched = Fetched::new(pane, browser, first, left);
    // An icon: no cookies go with it, and the engine's cache serves it when it can.
    host.download_image(Some(&url), 1, SIZE, 0, Some(&mut fetched));
}

fn send(pane: PaneId, url: String, png: Vec<u8>) {
    host::send(&HostEvent::Favicon { pane, url, png });
}

wrap_download_image_callback! {
    struct Fetched {
        pane: PaneId,
        browser: Browser,
        // The page's first icon, which the app knows its icon by.
        first: String,
        // The icons to try after this one, best first.
        left: Vec<String>,
    }

    impl DownloadImageCallback {
        fn on_download_image_finished(
            &self,
            _image_url: Option<&CefString>,
            _http_status_code: c_int,
            image: Option<&mut Image>,
        ) {
            // The engine answers nothing for a PNG unless it's told where to put its size.
            let (mut width, mut height) = (0, 0);
            let png = image
                .and_then(|image| image.as_png(1., 1, Some(&mut width), Some(&mut height)))
                .map(|png| {
                    let mut bytes = vec![0; png.size()];
                    png.data(Some(&mut bytes), 0);
                    bytes
                });
            match png {
                Some(png) if !png.is_empty() => send(self.pane, self.first.clone(), png),
                _ => try_next(self.pane, self.browser.clone(), self.first.clone(), self.left.clone()),
            }
        }
    }
}
