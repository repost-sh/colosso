//! macOS notifications, through `UNUserNotificationCenter`. Only an app with a bundle may ask for
//! the center, and it stops any other process that does, so a build run from `target` has none.
//! Each is named for what it's about, so a newer one takes the place of the last, and a click says
//! that name: a chat's as [`chat`] makes it, so a click opens that chat ([`chat_of`]); any other
//! only brings Koyo to the front. The system may call the delegate off the main thread, which the channel doesn't mind.

use std::sync::{Mutex, Once};

use block2::{DynBlock, RcBlock};
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AllocAnyThread as _, define_class, msg_send};
use objc2_foundation::{NSBundle, NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
    UNNotificationResponse, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};

define_class!(
    /// Hears a person click a notification, and says which.
    #[unsafe(super(NSObject))]
    #[name = "KoyoNotificationClicks"]
    struct Clicks;

    unsafe impl NSObjectProtocol for Clicks {}

    unsafe impl UNUserNotificationCenterDelegate for Clicks {
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn clicked(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            done: &DynBlock<dyn Fn()>,
        ) {
            if let Ok(hearing) = HEARING.lock()
                && let Some(hearing) = hearing.as_ref()
            {
                let name = response.notification().request().identifier().to_string();
                // Nobody's listening only as the app goes.
                let _ = hearing.unbounded_send(name);
            }
            done.call(());
        }
    }
);

impl Clicks {
    fn new() -> Retained<Self> {
        unsafe { msg_send![super(Clicks::alloc().set_ivars(())), init] }
    }
}

/// Who hears of the clicks: the window asked last, as one takes the place of another that closed.
static HEARING: Mutex<Option<UnboundedSender<String>>> = Mutex::new(None);

/// Whether this process may ask for the center.
fn has_bundle() -> bool {
    NSBundle::mainBundle().bundleIdentifier().is_some()
}

/// The names of the notifications a person clicks from now on; with no bundle, none.
pub fn clicks() -> Option<UnboundedReceiver<String>> {
    static LISTENING: Once = Once::new();
    if !has_bundle() {
        return None;
    }
    LISTENING.call_once(|| {
        let delegate = Clicks::new();
        let protocol = ProtocolObject::from_ref(&*delegate);
        UNUserNotificationCenter::currentNotificationCenter().setDelegate(Some(protocol));
        // The center's hold on its delegate is weak, and this one's for as long as the app.
        std::mem::forget(delegate);
    });
    let (told, hears) = unbounded();
    *HEARING.lock().ok()? = Some(told);
    Some(hears)
}

/// The name of a notification about the chat of thread `thread`.
pub fn chat(thread: &str) -> String {
    format!("chat:{thread}")
}

/// The thread of a notification named `name`, if it's about a chat.
pub fn chat_of(name: &str) -> Option<&str> {
    name.strip_prefix("chat:")
}

/// Posts a notification named `name`, if the app has a bundle and the person lets it: they're
/// asked the first time.
pub fn post(name: &str, title: &str, body: &str) {
    static ASKED: Once = Once::new();
    if !has_bundle() {
        log::info!("a notification, with no bundle to post it from: {title}: {body}");
        return;
    }
    let center = UNUserNotificationCenter::currentNotificationCenter();
    ASKED.call_once(|| {
        let answered = RcBlock::new(|_: Bool, error: *mut NSError| {
            if let Some(error) = unsafe { error.as_ref() } {
                log::warn!("notifications aren't allowed: {error}");
            }
        });
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert,
            &answered,
        );
    });
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
        &NSString::from_str(name),
        &content,
        None,
    );
    let added = RcBlock::new(|error: *mut NSError| {
        if let Some(error) = unsafe { error.as_ref() } {
            log::warn!("a notification wasn't posted: {error}");
        }
    });
    center.addNotificationRequest_withCompletionHandler(&request, Some(&added));
}

#[cfg(test)]
mod tests {
    use objc2::runtime::ProtocolObject;
    use objc2_user_notifications::UNUserNotificationCenterDelegate;

    use super::{Clicks, clicks, post};

    #[test]
    fn a_process_with_no_bundle_neither_asks_for_the_center_nor_stops() {
        assert!(clicks().is_none());
        post("0001", "A chat", "Finished");
    }

    #[test]
    fn what_hears_the_clicks_is_a_delegate_of_the_center() {
        let delegate = Clicks::new();
        let _: &ProtocolObject<dyn UNUserNotificationCenterDelegate> =
            ProtocolObject::from_ref(&*delegate);
    }
}
