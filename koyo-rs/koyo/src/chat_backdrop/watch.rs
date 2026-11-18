//! Noticing a change in what a screen's picture is made from (`docs/chat-backdrop.md` 3.3).
//! macOS says nothing when a wallpaper changes, so the wallpaper of each screen a chat shows the
//! backdrop on is asked for once a second while a window of Koyo's is seen, a few calls that cost
//! next to nothing, and again at once when the Space changes, as each Space can have a picture of
//! its own, when the system's appearance changes, which a dynamic wallpaper may follow, and when a
//! window is seen again after none was; one that follows the time of day or the sun changes its
//! picture at the next of these asks after its time. Nothing is asked while no chat shows the
//! backdrop or nobody can see a window, and the watch then sleeps until it's told, or until a
//! picture no chat shows is to go (`screens::tick`). A change of the backdrop's settings draws
//! again what draws it, which reads them, as the shell follows the settings, and that asks for
//! the pictures it then needs; the appearance, a window moved to another screen and a screen's
//! new size draw the windows again already.

use std::ptr::NonNull;

use block2::RcBlock;
use futures::StreamExt as _;
use futures::channel::mpsc;
use futures::future::{self, Either};
use gpui_kit::App;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSWorkspace, NSWorkspaceActiveSpaceDidChangeNotification};
use objc2_foundation::{
    NSDistributedNotificationCenter, NSNotification, NSNotificationName, NSObjectProtocol,
    NSOperationQueue, ns_string,
};

use super::screens;

/// Starts noticing the wallpapers, and the Space and the appearance from the main thread, which
/// tests don't run on.
pub fn init(cx: &mut App) {
    let (tell, mut told) = mpsc::unbounded();
    let observers = MainThreadMarker::new().map(|main| {
        [
            space_changes(main, tell.clone()),
            appearance_changes(main, tell.clone()),
        ]
    });
    screens::listen(tell, cx);
    cx.spawn(async move |cx| {
        let _observers = observers;
        loop {
            let closed = match cx.update(screens::tick) {
                Some(after) => {
                    let timer = cx.background_executor().timer(after);
                    let woke = future::select(timer, told.next()).await;
                    matches!(woke, Either::Right((None, _)))
                }
                None => told.next().await.is_none(),
            };
            if closed {
                return;
            }
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

/// Has `tell` told of each change of the system's appearance, until what this gives is dropped.
fn appearance_changes(
    _main: MainThreadMarker,
    tell: mpsc::UnboundedSender<()>,
) -> Retained<ProtocolObject<dyn NSObjectProtocol>> {
    let changed = RcBlock::new(move |_: NonNull<NSNotification>| {
        tell.unbounded_send(()).ok();
    });
    let name: &NSNotificationName = ns_string!("AppleInterfaceThemeChangedNotification");
    // SAFETY: as `space_changes`; the notice is the one the system posts as the appearance
    // changes, delivered on the main queue.
    unsafe {
        NSDistributedNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(name),
            None,
            Some(&NSOperationQueue::mainQueue()),
            &changed,
        )
    }
}
