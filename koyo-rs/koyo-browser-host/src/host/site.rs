//! What a page's site is allowed and blocked from, as Chromium keeps it in the profile: the
//! site's content settings (`docs/browser.md` 12.3). The app lists them and changes them from
//! the address field's popover; the host keeps the person's answers about the camera and the
//! microphone here too, which CEF hands to it every time and Chromium so doesn't keep itself.

use cef::{
    CefString, ContentSettingTypes, ContentSettingValues, ImplBrowserHost, ImplRequestContext,
    RequestContext,
};
use koyo_browser_ipc::{HostEvent, PaneId, Permission, SiteSetting};

use super::{host_of, send, with_pane};
use crate::dialogs::origin_of;

/// The permissions Chromium asks the person about and keeps as a content setting, in the
/// order the app lists them. The location is `GEOLOCATION`: the newer
/// `GEOLOCATION_WITH_OPTIONS` is a website setting, and asked for as a content setting stops
/// the host (`location.rs`).
const KEPT: [(Permission, ContentSettingTypes); 9] = [
    (Permission::Camera, ContentSettingTypes::MEDIASTREAM_CAMERA),
    (Permission::Microphone, ContentSettingTypes::MEDIASTREAM_MIC),
    (Permission::Location, ContentSettingTypes::GEOLOCATION),
    (Permission::Notifications, ContentSettingTypes::NOTIFICATIONS),
    (Permission::Clipboard, ContentSettingTypes::CLIPBOARD_READ_WRITE),
    (Permission::Midi, ContentSettingTypes::MIDI_SYSEX),
    (Permission::Fonts, ContentSettingTypes::LOCAL_FONTS),
    (Permission::Windows, ContentSettingTypes::WINDOW_MANAGEMENT),
    (Permission::Downloads, ContentSettingTypes::AUTOMATIC_DOWNLOADS),
];

/// The site of a page at `url`, which settings are kept for: none for a page that isn't the
/// web's, as a start page or a file.
fn site_of(url: &str) -> Option<String> {
    (url.starts_with("https://") || url.starts_with("http://")).then(|| origin_of(url))
}

/// `pane`'s profile, and the site of the page it shows.
fn of(pane: PaneId) -> Option<(RequestContext, String)> {
    let site = with_pane(pane, |p| site_of(&p.page.url)).flatten()?;
    Some((host_of(pane)?.request_context()?, site))
}

/// Whether `site` is allowed what `kind` is, or blocked from it; neither while the person
/// would be asked.
fn decided(context: &RequestContext, site: &str, kind: ContentSettingTypes) -> Option<bool> {
    let site = CefString::from(site);
    let value = context.content_setting(Some(&site), Some(&site), kind);
    if value == ContentSettingValues::ALLOW {
        Some(true)
    } else if value == ContentSettingValues::BLOCK {
        Some(false)
    } else {
        None
    }
}

fn decide(context: &RequestContext, site: &str, kind: ContentSettingTypes, allowed: bool) {
    let site = CefString::from(site);
    let value = if allowed {
        ContentSettingValues::ALLOW
    } else {
        ContentSettingValues::BLOCK
    };
    context.set_content_setting(Some(&site), Some(&site), kind, value);
}

/// Tells the app what `pane`'s site is allowed and blocked from.
pub(super) fn tell(pane: PaneId) {
    // None for a page with no site, as a start page.
    let settings = of(pane).map_or_else(Vec::new, |(context, site)| {
        KEPT.iter()
            .filter_map(|(permission, kind)| {
                let allowed = decided(&context, &site, *kind)?;
                Some(SiteSetting {
                    permission: *permission,
                    allowed,
                })
            })
            .collect()
    });
    send(&HostEvent::SiteSettings { pane, settings });
}

/// `pane`'s site is allowed `permission` from now on, or blocked from it; the app is told how
/// its settings stand.
pub(super) fn set(pane: PaneId, permission: Permission, allowed: bool) {
    let kind = KEPT.iter().find(|(kept, _)| *kept == permission);
    if let (Some((context, site)), Some((_, kind))) = (of(pane), kind) {
        decide(&context, &site, *kind, allowed);
        // Chromium reads the position only once macOS lets the host know it.
        #[cfg(target_os = "macos")]
        if allowed && permission == Permission::Location {
            crate::location::allowed();
        }
    }
    tell(pane);
}

/// The content settings behind a media request's `bits`: the microphone's and the camera's.
/// None for a request that takes in the screen, which is asked for every time.
fn media(bits: u32) -> Option<Vec<ContentSettingTypes>> {
    use cef::sys::cef_media_access_permission_types_t as Media;
    let has = |bit: Media| bits & bit as u32 != 0;
    if has(Media::CEF_MEDIA_PERMISSION_DESKTOP_AUDIO_CAPTURE)
        || has(Media::CEF_MEDIA_PERMISSION_DESKTOP_VIDEO_CAPTURE)
    {
        return None;
    }
    let mut kinds = Vec::new();
    if has(Media::CEF_MEDIA_PERMISSION_DEVICE_AUDIO_CAPTURE) {
        kinds.push(ContentSettingTypes::MEDIASTREAM_MIC);
    }
    if has(Media::CEF_MEDIA_PERMISSION_DEVICE_VIDEO_CAPTURE) {
        kinds.push(ContentSettingTypes::MEDIASTREAM_CAMERA);
    }
    (!kinds.is_empty()).then_some(kinds)
}

/// What the person decided before about a media request of `bits` from `origin` in `pane`:
/// allowed when every device asked for is, blocked when any is, and none while they'd be
/// asked.
pub(crate) fn media_decided(pane: PaneId, origin: &str, bits: u32) -> Option<bool> {
    let context = host_of(pane)?.request_context()?;
    let site = site_of(origin)?;
    let decided: Vec<Option<bool>> = (media(bits)?.into_iter())
        .map(|kind| decided(&context, &site, kind))
        .collect();
    if decided.contains(&Some(false)) {
        Some(false)
    } else if decided.iter().all(|one| *one == Some(true)) {
        Some(true)
    } else {
        None
    }
}

/// Keeps that the person allowed a media request of `bits` from `origin` in `pane`.
pub(crate) fn keep_media(pane: PaneId, origin: &str, bits: u32) {
    let Some(context) = host_of(pane).and_then(|host| host.request_context()) else {
        return;
    };
    let (Some(site), Some(kinds)) = (site_of(origin), media(bits)) else {
        return;
    };
    for kind in kinds {
        decide(&context, &site, kind, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_webs_pages_have_a_site() {
        assert_eq!(
            site_of("https://meet.example.com/abc?x=1").as_deref(),
            Some("https://meet.example.com")
        );
        assert_eq!(
            site_of("http://localhost:3000/").as_deref(),
            Some("http://localhost:3000")
        );
        assert_eq!(site_of("about:blank"), None);
        assert_eq!(site_of("file:///tmp/a.html"), None);
        assert_eq!(site_of(""), None);
    }

    #[test]
    fn a_media_request_names_its_devices_and_the_screen_is_always_asked_for() {
        use cef::sys::cef_media_access_permission_types_t as Media;
        let audio = Media::CEF_MEDIA_PERMISSION_DEVICE_AUDIO_CAPTURE as u32;
        let video = Media::CEF_MEDIA_PERMISSION_DEVICE_VIDEO_CAPTURE as u32;
        let screen = Media::CEF_MEDIA_PERMISSION_DESKTOP_VIDEO_CAPTURE as u32;
        assert_eq!(media(audio), Some(vec![ContentSettingTypes::MEDIASTREAM_MIC]));
        assert_eq!(
            media(audio | video),
            Some(vec![
                ContentSettingTypes::MEDIASTREAM_MIC,
                ContentSettingTypes::MEDIASTREAM_CAMERA
            ])
        );
        assert_eq!(media(video | screen), None);
        assert_eq!(media(0), None);
    }
}
