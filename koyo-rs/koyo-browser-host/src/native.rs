//! A pane's page: a Chrome-style browser whose own views the app shows in its window,
//! Chromium presenting its frames itself and `AppKit` handing it its input
//! (`docs/native-pages.md`).
//!
//! CEF makes a Chrome-style browser only inside a window of its own (with a parent view it
//! makes it Alloy style), so each page has one, its **holder**: frameless, never key or main,
//! clear and click-through, kept over the pane so Chromium places its menus and bubbles right.
//! Chromium draws the whole holder with one compositor, whose layers live in its
//! `ViewsCompositorSuperview`; the page's `WebContentsViewCocoa` over it only takes input. Both
//! move into a view the app gives ([`place`]), compositor first, as Cra's do
//! (`EmbeddedPages.swift`).
//!
//! Where the views are is also whether the page is on screen for the engine: in the app's
//! view, or parked in the holder with the holder moved off the screen's corner, it is (frames
//! run, as a page an agent drives out of sight needs); out of every window it's hidden.
//!
//! All of it happens on the main thread, which is CEF's UI thread here. No page's state is
//! borrowed across a call into `AppKit` or CEF, which call back.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::CStr;
use std::sync::atomic::{AtomicUsize, Ordering};

use cef::{BrowserSettings, BrowserView, ImplPanel, ImplWindow, RequestContext, View, Window};
use koyo_browser_ipc::{PaneId, Viewport};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView, NSWindow};
use objc2_foundation::{NSPoint, NSRect, NSSize};

mod holder;
mod parts;
mod report;

use holder::{closing, hold};
pub(crate) use parts::stray_client;
use parts::{HolderDelegate, PageViewDelegate};
pub use report::describe;

/// Chromium's view that hosts a window's compositor, the page's view over it, and the view in
/// that which takes the page's keys.
const COMPOSITOR: &CStr = c"ViewsCompositorSuperview";
const WEB_CONTENTS: &CStr = c"WebContentsViewCocoa";
const KEYS: &CStr = c"RenderWidgetHostViewCocoa";

struct Entry {
    /// CEF's window the page is made in, and the `NSWindow` under it.
    holder: Option<(Window, Retained<NSWindow>)>,
    /// The page's views, compositor first, once CEF has made them.
    views: Option<(Retained<NSView>, Retained<NSView>)>,
    /// The app's view the page is shown in while a pane draws it.
    shown_in: Option<Retained<NSView>>,
    /// The app's own view in the window the page was last shown in, which takes the keys the
    /// page leaves.
    home: Option<Retained<NSView>>,
    /// Whether the page is on screen for the engine.
    on_screen: bool,
    /// The page's size in points, for its holder while no pane says where it is.
    size: NSSize,
}

/// What waits for a profile's request context, or that it's ready.
enum Profile {
    Ready,
    Waiting(Vec<Box<dyn FnOnce()>>),
}

thread_local! {
    static PAGES: RefCell<HashMap<PaneId, Entry>> = RefCell::new(HashMap::new());
    static PROFILES: RefCell<HashMap<String, Profile>> = RefCell::new(HashMap::new());
    /// What the app's keyboard hears of a page's views moving ([`set_keyboard`]).
    static KEYBOARD: Cell<Option<Hear>> = const { Cell::new(None) };
}

/// How many windows CEF has made for pages and not yet destroyed: it mustn't stop before
/// they're gone.
static WINDOWS: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn windows() -> usize {
    WINDOWS.load(Ordering::Acquire)
}

/// `pane`'s window of CEF's is destroyed.
fn window_destroyed(pane: PaneId) {
    holder::gone(pane);
    let _ = WINDOWS.fetch_update(Ordering::AcqRel, Ordering::Acquire, |open| open.checked_sub(1));
}

/// Runs `with` on `pane`'s page, if there is one.
fn page<R>(pane: PaneId, with: impl FnOnce(&mut Entry) -> R) -> Option<R> {
    PAGES.with(|pages| pages.borrow_mut().get_mut(&pane).map(with))
}

/// A profile's request context is ready: browser views that waited for it are made.
pub(crate) fn context_ready(profile: &str) {
    let was = PROFILES.with(|profiles| profiles.borrow_mut().insert(profile.to_owned(), Profile::Ready));
    if let Some(Profile::Waiting(makes)) = was {
        for make in makes {
            make();
        }
    }
}

/// The profiles were cleared: each is a new request context from now on, ready when it says.
pub(crate) fn profiles_cleared() {
    PROFILES.with(|profiles| profiles.borrow_mut().clear());
}

/// Makes `pane`'s page, once the profile it's of is ready (a new one initializes on disk
/// first, and CEF makes no browser view before): a Chrome-style browser view with the pane's
/// client, in a holder of its own. No profile is `DevTools`, which use the engine's own
/// context. Its browser arrives in `on_after_created`. The page is `viewport`'s size until
/// a pane places it, and on screen for the engine until the app says it isn't.
pub(crate) fn create(
    pane: PaneId,
    url: &str,
    viewport: Viewport,
    profile: Option<(&str, RequestContext)>,
) {
    let entry = Entry {
        holder: None,
        views: None,
        shown_in: None,
        home: None,
        on_screen: true,
        size: NSSize::new(f64::from(viewport.width.max(1)), f64::from(viewport.height.max(1))),
    };
    PAGES.with(|pages| pages.borrow_mut().insert(pane, entry));
    let url = url.to_owned();
    let Some((name, context)) = profile else {
        make(pane, &url, None);
        return;
    };
    let make = move || make(pane, &url, Some(context));
    let now = PROFILES.with(|profiles| {
        match profiles.borrow_mut().entry(name.to_owned()).or_insert_with(|| Profile::Waiting(Vec::new())) {
            Profile::Ready => Some(make),
            Profile::Waiting(makes) => {
                makes.push(Box::new(make));
                None
            }
        }
    });
    if let Some(make) = now {
        make();
    }
}

fn make(pane: PaneId, url: &str, mut context: Option<RequestContext>) {
    if page(pane, |_| ()).is_none() {
        return;
    }
    let devtools = context.is_none();
    let mut client = crate::pane::client(pane, devtools);
    let mut delegate = PageViewDelegate::new(pane);
    let settings = BrowserSettings {
        background_color: 0xFFFF_FFFF,
        ..BrowserSettings::default()
    };
    let made = cef::browser_view_create(
        Some(&mut client),
        Some(&url.into()),
        Some(&settings),
        None,
        context.as_mut(),
        Some(&mut delegate),
    )
    .is_some_and(|view| {
        let mut holder = HolderDelegate::new(pane, view);
        cef::window_create_top_level(Some(&mut holder)).is_some()
    });
    if !made {
        PAGES.with(|pages| pages.borrow_mut().remove(&pane));
        crate::host::failed_to_open(pane, devtools);
    }
}

/// CEF's window is made: the page goes in it, and it's shown without the app coming forward.
fn window_created(pane: PaneId, window: &Window, view: &BrowserView) {
    WINDOWS.fetch_add(1, Ordering::AcqRel);
    let mut as_view = View::from(view);
    window.add_child_view(Some(&mut as_view));
    // SAFETY: on macOS a CEF window's handle is its content `NSView`, which lives with it.
    let content: Option<&NSView> = unsafe { window.window_handle().cast::<NSView>().as_ref() };
    if let Some(holder) = content.and_then(NSView::window) {
        hold(pane, &holder);
        if let Some(size) = page(pane, |entry| {
            entry.holder = Some((window.clone(), holder.clone()));
            entry.size
        }) {
            park(&holder, size);
        }
    }
    crate::embedded::showing(|| window.show());
    settle(pane);
}

/// Puts a holder of `size` where it is while no pane shows its page: all but a point of it
/// past the screen's bottom left corner, where it still counts as on screen.
fn park(holder: &NSWindow, size: NSSize) {
    let parked = NSRect::new(NSPoint::new(1. - size.width, 1. - size.height), size);
    if holder.frame() != parked {
        holder.setFrame_display(parked, false);
    }
}

/// Puts `pane`'s views where they belong now. While the page is on screen for the engine:
/// in the app's view if a pane draws it, else parked in the holder off the screen. Else out
/// of every window, which is what hides it: Chromium is told not to judge by what covers a
/// window (`app.rs`), so a parked page is awake whatever is in front.
fn settle(pane: PaneId) {
    let settled = page(pane, |entry| {
        let (_, holder) = entry.holder.clone()?;
        let views = entry.views.clone();
        Some((holder, views, entry.shown_in.clone(), entry.on_screen, entry.size))
    });
    let Some(Some((holder, known, shown_in, on_screen, size))) = settled else {
        return;
    };
    let made = || {
        let content = holder.contentView()?;
        Some((find_view(&content, COMPOSITOR)?, find_view(&content, WEB_CONTENTS)?))
    };
    let Some((compositor, web)) = known.clone().or_else(made) else {
        return;
    };
    if known.is_none() {
        page(pane, |entry| entry.views = Some((compositor.clone(), web.clone())));
    }
    let into = |parent: &NSView| {
        let bounds = parent.bounds();
        for view in [&compositor, &web] {
            if !view.isDescendantOf(parent) {
                parent.addSubview(view);
                view.setAutoresizingMask(
                    NSAutoresizingMaskOptions::ViewWidthSizable
                        | NSAutoresizingMaskOptions::ViewHeightSizable,
                );
            }
            if view.frame() != bounds {
                view.setFrame(bounds);
            }
        }
    };
    let to = match (&shown_in, on_screen, holder.contentView()) {
        (Some(container), true, _) => Some(container.clone()),
        (None, true, Some(content)) => Some(content),
        _ => None,
    };
    let to_window = to.as_ref().and_then(|parent| parent.window());
    let leaving = web.window().is_some_and(|now| {
        (to_window.as_ref()).is_none_or(|to| !std::ptr::eq(&raw const *now, &raw const **to))
    });
    if leaving {
        tell_keyboard(pane, Views::Leaving);
    }
    let arriving = to_window.is_some();
    if let Some(parent) = to {
        into(&parent);
    } else {
        compositor.removeFromSuperview();
        web.removeFromSuperview();
    }
    // Its holder holds no place while no pane shows the page.
    if shown_in.is_none() {
        park(&holder, size);
    }
    if arriving {
        tell_keyboard(pane, Views::Arrived);
    }
}

/// Shows `pane`'s page in `container`, a view of the app's, which the page's views fill, and
/// puts its holder over `screen`, where the container is on screen (in points from the bottom
/// left): the page is that size. Whole points, both: Chromium lays the page out by its
/// holder's size, a window's, and the views resize with the container, so the two have to
/// agree, and a view between points puts Chromium's picture off it (`docs/native-pages.md`
/// 6). `home` is the app's own view in that window, which takes the keys the page leaves.
/// The app calls this as it draws the pane, each time.
pub fn place(pane: PaneId, container: &NSView, home: &NSView, screen: NSRect) {
    let Some((moved, holder)) = page(pane, |entry| {
        let same = (entry.shown_in.as_ref())
            .is_some_and(|held| std::ptr::eq(&raw const **held, container));
        if !same {
            entry.shown_in = Some(Retained::from(container));
            entry.home = Some(Retained::from(home));
        }
        entry.size = screen.size;
        // Settled again until the page's views are there to move.
        let moved = !same || entry.views.is_none();
        (moved, entry.holder.as_ref().map(|(_, holder)| holder.clone()))
    }) else {
        return;
    };
    if moved {
        settle(pane);
    }
    let Some(holder) = holder else {
        return;
    };
    debug_assert!(
        screen.size.width.fract() == 0. && screen.size.height.fract() == 0.,
        "a native page between points: {} by {}",
        screen.size.width,
        screen.size.height
    );
    // Not a child of the app's window, which would move it along with it: Chromium turns
    // down the pointer over a page while a child of the page's window lies over that place.
    if holder.frame() != screen {
        holder.setFrame_display(screen, false);
    }
}

/// `container` no longer shows `pane`'s page: its views are parked, or taken out of every
/// window, by whether the page is on screen for the engine.
pub fn release(pane: PaneId, container: &NSView) {
    let left = page(pane, |entry| {
        (entry.shown_in.take_if(|held| std::ptr::eq(&raw const **held, container))).is_some()
    });
    if left == Some(true) {
        settle(pane);
    }
}

/// The page is `width` by `height` points though no pane says so: an agent's size for a page
/// out of sight.
pub(crate) fn size(pane: PaneId, width: u32, height: u32) {
    let parked = page(pane, |entry| {
        let parked = entry.shown_in.is_none();
        if parked {
            entry.size = NSSize::new(f64::from(width.max(1)), f64::from(height.max(1)));
        }
        parked
    });
    if parked == Some(true) {
        settle(pane);
    }
}

/// Whether `pane`'s page is on screen for the engine.
pub(crate) fn on_screen(pane: PaneId, visible: bool) {
    let changed = page(pane, |entry| std::mem::replace(&mut entry.on_screen, visible) != visible);
    if changed == Some(true) {
        settle(pane);
    }
}

/// A page's views moving, as the app's keyboard hears of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Views {
    /// They're about to leave the window they're in, for another or for none. A view that has
    /// the keyboard as it leaves its window leaves the window itself with it, which hands no
    /// key on: after a tab with the keyboard in its page was left, nothing typed reached a
    /// chat's composer or a terminal. So the keyboard is moved off them first.
    Leaving,
    /// They're in a window now, the app's or the page's holder: a page the keyboard is wanted
    /// in takes it once its views are there.
    Arrived,
}

/// What hears a page's views moving.
type Hear = fn(PaneId, Views);

/// Has `hear` hear, from now on, each time a page's views leave a window or come into one.
/// The app's keyboard moves the first responder; the host never does.
pub fn set_keyboard(hear: Hear) {
    KEYBOARD.set(Some(hear));
}

fn tell_keyboard(pane: PaneId, views: Views) {
    if let Some(hear) = KEYBOARD.get() {
        hear(pane, views);
    }
}

/// The page's view, when it's in a window.
fn web_view(pane: PaneId) -> Option<(Retained<NSWindow>, Retained<NSView>)> {
    let web = page(pane, |entry| Some(entry.views.as_ref()?.1.clone()))??;
    Some((web.window()?, web))
}

/// The view of `pane`'s page that takes its keys, and the window it's in, while it's in one.
pub fn key_view(pane: PaneId) -> Option<(Retained<NSWindow>, Retained<NSView>)> {
    let (window, web) = web_view(pane)?;
    let keys = find_view(&web, KEYS).unwrap_or(web);
    Some((window, keys))
}

/// The page `view` is one of the views of, if it's a page's: its keys' view, or another in it.
pub fn page_of(view: &NSView) -> Option<PaneId> {
    PAGES.with(|pages| {
        let pages = pages.try_borrow().ok()?;
        let of = |entry: &Entry| {
            let Some((compositor, web)) = &entry.views else {
                return false;
            };
            view.isDescendantOf(web) || view.isDescendantOf(compositor)
        };
        pages.iter().find(|(_, entry)| of(entry)).map(|(pane, _)| *pane)
    })
}

/// A key `pane`'s page left unhandled, as the `NSEvent` it came as, is offered to the app's
/// own view as a shortcut; whether there was a view to offer it.
pub(crate) fn unhandled_key(pane: PaneId, event: *mut AnyObject) -> bool {
    // Only while a pane shows the page: its keys are the app's then.
    let home = page(pane, |entry| entry.shown_in.as_ref().and(entry.home.clone()));
    let Some(Some(home)) = home else {
        return false;
    };
    if event.is_null() {
        return false;
    }
    // As a shortcut, and not as a key press: the page has had the key, as text where it's
    // text, and a view handed a key press it has no use for passes it on to the text system,
    // which types it into whatever has the keyboard, the page, a second time.
    // SAFETY: `event` is the `NSEvent` CEF is handling, alive through this call; a view is
    // asked whether a key is a shortcut of its own as `AppKit` asks it.
    unsafe {
        let _: objc2::runtime::Bool = objc2::msg_send![&*home, performKeyEquivalent: event];
    }
    true
}

/// `pane`'s browser has closed: its views and its holder go.
pub(crate) fn closed(pane: PaneId) {
    let Some(entry) = PAGES.with(|pages| pages.borrow_mut().remove(&pane)) else {
        return;
    };
    if let Some((compositor, web)) = &entry.views {
        if web.window().is_some() {
            tell_keyboard(pane, Views::Leaving);
        }
        compositor.removeFromSuperview();
        web.removeFromSuperview();
    }
    if let Some((window, holder)) = &entry.holder {
        closing(holder);
        window.close();
    }
}

/// Lets go of every page, as CEF is about to stop.
pub(crate) fn clear() {
    PAGES.with(|pages| pages.borrow_mut().clear());
    PROFILES.with(|profiles| profiles.borrow_mut().clear());
}

/// The class `object` says it is. Not its `isa`, which for an object under key-value observing
/// is a subclass the runtime made (`NSKVONotifying_…`), as windows and views often are.
#[must_use]
pub fn class_of(object: &AnyObject) -> &'static objc2::runtime::AnyClass {
    // SAFETY: every object answers `class` with a registered class, which lives for the
    // process's life.
    unsafe { objc2::msg_send![object, class] }
}

/// The view of class `class` in `root`'s tree, `root` included.
#[must_use]
pub fn find_view(root: &NSView, class: &CStr) -> Option<Retained<NSView>> {
    if class_of(root).name() == class {
        return Some(Retained::from(root));
    }
    root.subviews().iter().find_map(|child| find_view(&child, class))
}
