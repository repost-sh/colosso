//! Where the computer is, which macOS tells a page only once it has asked the person whether
//! Koyo may know.
//!
//! A page asks Chromium, Chromium asks the person in the pane, and once they allow it Chromium
//! reads the position from macOS. macOS answers only an app it has asked the person about,
//! and until it has, Chromium waits for that answer without asking: in Chrome it is Chrome's
//! own window that asks, and the host has none of Chrome's. So the page would wait forever,
//! never hearing back. The host asks macOS itself instead, when the person allows a page to
//! know where they are, and when a page opens from a site they allowed before macOS was asked,
//! which Chromium doesn't ask the person about again: on a new install, or after a build signed
//! ad hoc, whose new signature macOS takes for another app's and forgets what it was told.
//! macOS asks once, with the reason the host's Info.plist gives (`script/browser-engine`);
//! Chromium hears the answer and reads the position, or tells the page it is denied.
use cef::{
    Browser, CefString, ContentSettingTypes, ContentSettingValues, ImplBrowser, ImplBrowserHost,
    ImplRequestContext,
};
use objc2::rc::Retained;
use objc2_core_location::{CLAuthorizationStatus, CLLocationManager};

thread_local! {
    /// The host's own location manager, made on the UI thread when first needed and kept, so
    /// macOS keeps asking while the person thinks it over. Chromium keeps one of its own,
    /// which hears the answer too.
    // SAFETY: `new` makes a location manager with no delegate, which the host never needs.
    static MANAGER: Retained<CLLocationManager> = unsafe { CLLocationManager::new() };
}

/// The person allowed a page to know where they are.
pub(crate) fn allowed() {
    MANAGER.with(|manager| ask(manager));
}

/// A page opened at `url` in `browser`: macOS is asked about the host now if the person
/// allowed the page's site to know where they are while macOS hadn't been, as the page may
/// ask any moment, and Chromium wouldn't ask the person about it again.
pub(crate) fn opened(browser: Option<&mut Browser>, url: &str) {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return;
    }
    MANAGER.with(|manager| {
        // SAFETY: reads what macOS decided about the host, which the manager keeps.
        if unsafe { manager.authorizationStatus() } != CLAuthorizationStatus::NotDetermined {
            return;
        }
        let Some(context) = browser
            .and_then(|browser| browser.host())
            .and_then(|host| host.request_context())
        else {
            return;
        };
        // Chromium keeps the person's answer as the site's `GEOLOCATION` content setting.
        // `GEOLOCATION_WITH_OPTIONS`, which it is moving to, is a website setting: asked
        // for as a content setting, Chromium stops the host.
        let url = CefString::from(url);
        if context.content_setting(Some(&url), Some(&url), ContentSettingTypes::GEOLOCATION)
            == ContentSettingValues::ALLOW
        {
            ask(manager);
        }
    });
}

/// Asks the person, unless macOS has asked them already: it asks only while it hasn't
/// decided, and tells every location manager of the host what was decided.
fn ask(manager: &CLLocationManager) {
    // SAFETY: takes nothing and returns nothing; macOS asks on its own.
    unsafe { manager.requestWhenInUseAuthorization() }
}
