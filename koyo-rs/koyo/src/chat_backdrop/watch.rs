//! Noticing a change in what a screen's picture is made from (`docs/chat-backdrop.md` 3.3).
//! macOS says nothing when a wallpaper changes, so the wallpaper of each screen a chat shows the
//! backdrop on is asked for once a second, a few calls that cost next to nothing, and again at
//! once when the Space changes, as each Space can have a picture of its own. A change of the
//! backdrop's settings draws the windows again, which make the pictures they then ask for; the
//! appearance, a window moved to another screen and a screen's new size draw them again already.

use std::ptr::NonNull;
use std::time::Duration;

use block2::RcBlock;
use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui_kit::App;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSWorkspace, NSWorkspaceActiveSpaceDidChangeNotification};
use objc2_foundation::{NSNotification, NSObjectProtocol, NSOperationQueue};

use super::screens;
use crate::settings;

/// How often the wallpapers are asked for.
const EVERY: Duration = Duration::from_secs(1);

/// Starts noticing: the settings at once, the wallpapers, the screens and the Space from the
/// main thread, which tests don't run on.
pub fn init(cx: &mut App) {
    let mut last = settings::get(cx).chat_backdrop;
    settings::observe_app(cx, move |cx| {
        let now = settings::get(cx).chat_backdrop;
        if now != last {
            last = now;
            cx.refresh_windows();
        }
    })
    .detach();
    let Some(main) = MainThreadMarker::new() else {
        return;
    };
    let (tell, mut told) = mpsc::unbounded();
    let observer = space_changes(main, tell);
    screens::tick(cx);
    cx.spawn(async move |cx| {
        // Kept for as long as the app listens.
        let _observer = observer;
        loop {
            let timer = cx.background_executor().timer(EVERY);
            futures::future::select(timer, told.next()).await;
            cx.update(screens::tick);
        }
    })
    .detach();
}

/// Has `tell` told of each change of Space, until what this gives is dropped.
fn space_changes(
    _main: MainThreadMarker,
    tell: mpsc::UnboundedSender<()>,
) -> Retained<ProtocolObject<dyn NSObjectProtocol>> {
    let changed = RcBlock::new(move |_: NonNull<NSNotification>| {
        // Unheard only once the app has stopped listening, on its way out.
        tell.unbounded_send(()).ok();
    });
    let center = NSWorkspace::sharedWorkspace().notificationCenter();
    // SAFETY: the name is AppKit's own constant, and the block only sends on a channel, which
    // any thread may do; the main queue runs it on the main thread regardless.
    unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceActiveSpaceDidChangeNotification),
            None,
            Some(&NSOperationQueue::mainQueue()),
            &changed,
        )
    }
}
