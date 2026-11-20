//! A page printed (`docs/browser.md` 16): Chrome's own print preview, which the page's
//! `window.print()` opens as well, or the page as a PDF, which CEF writes off its UI thread.

use std::os::raw::c_int;

use cef::{
    BrowserHost, CefString, ImplBrowserHost, ImplPdfPrintCallback, PdfPrintCallback,
    PdfPrintSettings, WrapPdfPrintCallback, rc::Rc, wrap_pdf_print_callback,
};
use koyo_browser_ipc::{HostEvent, PaneId, Print};

/// Prints `pane`'s page, whose browser `host` is.
pub(crate) fn print(pane: PaneId, host: &BrowserHost, print: Print) {
    match print {
        Print::Preview => host.print(),
        Print::Pdf { path } => {
            let mut saved = Saved::new(pane, path.clone());
            let path = CefString::from(path.as_str());
            host.print_to_pdf(Some(&path), Some(&pdf()), Some(&mut saved));
        }
    }
}

/// How the page is laid out as a PDF: as it prints on the Mac's own paper, its backgrounds
/// and colours kept as the page shows them.
fn pdf() -> PdfPrintSettings {
    let (paper_width, paper_height) = paper();
    PdfPrintSettings {
        print_background: 1,
        paper_width,
        paper_height,
        ..PdfPrintSettings::default()
    }
}

/// The paper the Mac prints on unless asked otherwise, in inches: its Printers settings'
/// default size, A4 or Letter by the region.
fn paper() -> (f64, f64) {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::NSSize;

    // SAFETY: AppKit's shared print info, on the main thread, where the host runs: a class
    // method that always answers an object, and its paper size.
    let size: NSSize = unsafe {
        let info: Retained<AnyObject> = msg_send![class!(NSPrintInfo), sharedPrintInfo];
        msg_send![&*info, paperSize]
    };
    let inches = |points: f64| points / 72.;
    (inches(size.width), inches(size.height))
}

wrap_pdf_print_callback! {
    struct Saved {
        pane: PaneId,
        path: String,
    }

    impl PdfPrintCallback {
        fn on_pdf_print_finished(&self, _path: Option<&CefString>, ok: c_int) {
            crate::host::send(&HostEvent::PdfSaved {
                pane: self.pane,
                path: self.path.clone(),
                saved: ok != 0,
            });
        }
    }
}
