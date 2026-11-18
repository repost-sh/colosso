//! A browser pane: the page the host shows in it, what the person does with it, and the
//! toolbar over it.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, Image, Pixels,
    Point, Subscription, Task, WeakEntity, Window,
};
use koyo_browser_ipc::{self as ipc, AppRequest, Cursor, PageState, PaneId, Viewport};

use super::control::Lease;
use super::dialog::Question;
use super::dock::{Asked, Devtools};
use super::find::{ACTUAL_SIZE, FindBar};
use super::host::Browsers;
use super::profiles::By;
use crate::history::How;
use crate::keyboard::{Cause, Intent, Part, Surface};

mod about;
mod extensions;
pub(super) mod lifecycle;
mod navigation;
mod profile;
pub(super) mod sight;
use super::menu::MenuRequest;
use sight::Shown;
pub use navigation::{Controls, Visit};

pub enum BrowserEvent {
    /// The address or title changed.
    Changed,
    /// Open an address in a new tab beside this one, without leaving it.
    OpenTab(String),
    /// Open an address in a new tab beside this one and show it: a place the person asked
    /// to go to (an extension's options, the store), not a link opened for later.
    GoToTab(String),
    /// Put the page's `DevTools` in a pane of their own beside this one.
    Detach(Entity<BrowserView>),
    /// Take the page's `DevTools` out of their pane: they're docked again, or closed.
    Undock(Entity<BrowserView>),
    /// The host closed this browser: `DevTools` whose page went away.
    Closed,
    /// An extension opened a tab from this page (`chrome.tabs.create`): its page is made
    /// already, as the host's pane `pane`, and wants a tab beside this one, in front when
    /// `active` (`docs/extensions.md` 3).
    Adopted {
        pane: PaneId,
        url: String,
        active: bool,
    },
    /// An extension asked for this page's tab in front (`chrome.tabs.update`).
    Activate,
    /// The page closed without the app asking: an extension closed its tab
    /// (`chrome.tabs.remove`), or a page a script opened closed itself. Its tab goes.
    ClosedItself,
    /// The person searched the web for this from the address.
    Searched(String),
    /// A page loaded, as the history keeps it (`navigation.rs`).
    Visited(Visit),
}

/// What a pane's browser shows.
enum Kind {
    /// A page of `project`'s, opened by `by` in `profile`, the one its pages used then
    /// (`browser::profile`).
    Page {
        profile: String,
        project: PathBuf,
        by: By,
    },
    /// The `DevTools` of another pane's page. They open once the page is open, and the
    /// host closes them with it.
    DevTools {
        inspected: Entity<BrowserView>,
        /// A point of the page to reveal when the inspector opens.
        inspect: Option<ipc::Point>,
        _inspected: Subscription,
    },
}

pub(super) enum Notice {
    /// The page's renderer went away.
    Gone(String),
    /// The page could not be opened.
    Failed(String),
}

/// Host ids for views, never reused while the app runs.
static NEXT_PANE: AtomicU64 = AtomicU64::new(1);

/// What a render settles with the window: focus the page, after an address was entered;
/// show the page's URL in the address again, once the person stops editing it.
#[derive(Default)]
struct Settle {
    focus_page: bool,
    address_stale: bool,
}

pub struct BrowserView {
    /// The page's id in the host.
    pub(super) pane: PaneId,
    pub(super) browsers: Entity<Browsers>,
    kind: Kind,
    pub(super) page: PageState,
    /// Who drives the page, and when a page the person is only using goes back to nobody.
    pub(super) lease: Lease,
    /// The page the pane shows once the host is ready; empty for the start page, which
    /// needs no browser.
    target: String,
    pub(super) viewport: Option<Viewport>,
    /// The host epoch the page was opened in.
    opened: Option<u64>,
    /// Whether the glass of the bar over the page is dark, as the page under it last said; none
    /// while it's the theme's.
    pub(super) bar_dark: Option<bool>,
    /// How light the page is, as it last said, and the task that asks it while it's drawn, and
    /// how many times that's woken, for a test to read (`tone.rs`).
    pub(super) tone: Option<super::tone::Tone>,
    pub(super) tone_watch: Option<Task<()>>,
    #[cfg(test)]
    pub(super) tone_turns: usize,
    /// What the page's site is allowed and blocked from, as the host last said, the button
    /// its popover opens from, and when its address was last copied (`site.rs`).
    pub(super) site: Option<Vec<koyo_browser_ipc::SiteSetting>>,
    pub(super) site_trigger: crate::menu::Trigger,
    pub(super) link_copied: Option<Instant>,
    /// Where the question on screen (`asked`) was put.
    pub(super) asked_in: super::dialog::Put,
    /// Whether the page is full screen, and where (`fullscreen.rs`).
    pub(super) fullscreen: super::fullscreen::Fullscreen,
    /// The page element's state while the page is drawn: let go, the page is out of sight.
    pub(super) shown: Option<WeakEntity<Shown>>,
    /// Whether the host was last told the page is on screen.
    pub(super) visible: Option<bool>,
    /// Whether the person saw the page when its sight was last told ([`Self::tell_sight`]).
    pub(super) seen: bool,
    /// Whether the page's window is on screen: minimized or on another desktop it draws
    /// nothing, so the page isn't seen however its pane is laid out (`sight.rs`).
    pub(super) window_visible: bool,
    /// Until when the page is on screen for the engine though no pane draws it, an agent
    /// having used it, and the task that ends that then (`sight.rs`).
    pub(super) awake: Option<(Instant, Task<()>)>,
    pub(super) cursor: Cursor,
    pub(super) focus: FocusHandle,
    pub(super) address: Entity<InputState>,
    pub(super) notice: Option<Notice>,
    /// What the next render, which has the window, settles.
    settle: Settle,
    /// Where the page is in the window, for menus and an agent's pointer at page positions.
    pub(super) origin: Point<Pixels>,
    /// Questions the page waits on, the first one asked.
    pub(super) dialogs: VecDeque<Question>,
    /// The question in the dialog on screen.
    pub(super) asked: Option<u64>,
    pub(super) menu_request: Option<MenuRequest>,
    /// The context menu shown and not yet picked from, with where in the page it opened.
    pub(super) open_menu: Option<(u64, ipc::Point)>,
    /// The page's `DevTools`, while they're open.
    pub(super) devtools: Option<Devtools>,
    /// `DevTools` asked for and not made yet, with the point of the page to reveal.
    pub(super) open_devtools: Option<Asked>,
    /// The size of docked `DevTools` while their divider is dragged.
    pub(super) dock_drag: Option<f32>,
    /// A press on back or forward, until it's a click or a hold that lists the pages.
    pub(super) history_hold: Option<super::history::Hold>,
    /// The find bar, while it's open, and the zoom, in percent.
    pub(super) find: Option<FindBar>,
    pub(super) zoom: u32,
    /// The page's own icon, once the host has fetched it.
    pub(super) favicon: Option<Arc<Image>>,
    /// How the page being opened came, which the history's visit of it says, `None` back or
    /// forward; and the page last told as visited (`navigation.rs`).
    came: Option<How>,
    visited: Option<String>,
    /// Responsive mode: the page at a chosen size or as a device, and how it shows.
    pub(super) responsive: super::responsive::State,
    /// What shows an agent at work on the page: its pointer, and the glow while it holds it.
    pub(super) marks: super::pointer::Marks,
    /// The passkey the page asked for, while it waits on the system's sheet.
    pub(super) passkey: super::passkeys::Passkey,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<BrowserEvent> for BrowserView {}

impl Focusable for BrowserView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl BrowserView {
    /// A page of `project`'s profile, at `url` (empty for the start page).
    pub fn new(url: String, project: &Path, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let kind = Kind::Page {
            profile: super::profile(project, By::Person, cx).to_owned(),
            project: project.to_owned(),
            by: By::Person,
        };
        Self::with_kind(None, url, kind, window, cx)
    }

    /// The `DevTools` of `inspected`'s page, as a pane of their own.
    pub fn inspector(
        inspected: Entity<BrowserView>,
        inspect: Option<ipc::Point>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let changes = cx.observe(&inspected, |this, _, cx| this.ensure_open(cx));
        let kind = Kind::DevTools {
            inspected,
            inspect,
            _inspected: changes,
        };
        Self::with_kind(None, "devtools:".into(), kind, window, cx)
    }

    /// A view of `kind` at `url`: `adopted` is its number in the host when the host gave it
    /// one, a page it made itself; else the app gives it the next of its own.
    fn with_kind(
        adopted: Option<PaneId>,
        url: String,
        kind: Kind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let browsers = Browsers::global(cx);
        let address =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search or enter an address"));
        if !url.is_empty() {
            address.update(cx, |address, cx| address.set_value(url.clone(), window, cx));
        }
        let pane = adopted.unwrap_or_else(|| NEXT_PANE.fetch_add(1, Ordering::Relaxed));
        let focus = cx.focus_handle();
        let subscriptions = vec![
            cx.observe_window_visibility(window, |this, visibility, _, cx| {
                this.window_visible = visibility.is_visible();
                this.tell_sight(cx);
            }),
            cx.on_focus(&focus, window, |this, window, cx| this.focus_moved(true, window, cx)),
            cx.on_blur(&focus, window, |this, window, cx| this.focus_moved(false, window, cx)),
            cx.subscribe_in(
                &address,
                window,
                |this, address, event, _, cx| match event {
                    InputEvent::PressEnter { .. } => {
                        let text = address.read(cx).value();
                        this.go_to_typed(&text, cx);
                    }
                    InputEvent::Blur => {
                        this.settle.address_stale = true;
                        cx.notify();
                    }
                    InputEvent::Change | InputEvent::Focus => {}
                },
            ),
        ];
        let (weak, id) = (cx.weak_entity(), cx.entity_id());
        browsers.update(cx, |browsers, _| browsers.attach(pane, weak));
        let releasing = browsers.clone();
        cx.on_release(move |_, cx| {
            releasing.update(cx, |browsers, _| browsers.detach(pane, id));
            crate::keyboard::page_closed(pane);
        })
        .detach();
        let mut responsive = super::responsive::State::default();
        responsive.window_scale = window.scale_factor();
        Self {
            pane,
            browsers,
            kind,
            page: PageState {
                url: url.clone(),
                ..PageState::default()
            },
            lease: Lease::default(),
            target: url,
            viewport: None,
            opened: None,
            bar_dark: None,
            tone: None,
            tone_watch: None,
            #[cfg(test)]
            tone_turns: 0,
            site: None,
            site_trigger: super::site::trigger(pane),
            link_copied: None,
            asked_in: super::dialog::Put::Dialog,
            fullscreen: super::fullscreen::Fullscreen::default(),
            shown: None,
            visible: None,
            seen: false,
            window_visible: window.is_visible(),
            awake: None,
            cursor: Cursor::Arrow,
            focus,
            address,
            notice: None,
            settle: Settle::default(),
            origin: Point::default(),
            dialogs: VecDeque::new(),
            asked: None,
            menu_request: None,
            open_menu: None,
            devtools: None,
            open_devtools: None,
            dock_drag: None,
            history_hold: None,
            find: None,
            zoom: ACTUAL_SIZE,
            favicon: None,
            came: Some(How::Followed),
            visited: None,
            responsive,
            marks: super::pointer::Marks::default(),
            passkey: super::passkeys::Passkey::default(),
            _subscriptions: subscriptions,
        }
    }

    /// The host closed this browser.
    pub(super) fn closed(&mut self, cx: &mut Context<Self>) {
        self.opened = None;
        self.forget_passkey();
        if self.is_fullscreen() {
            self.fullscreen_changed(false, cx);
        }
        cx.emit(BrowserEvent::Closed);
        cx.notify();
    }

    /// Shows the element at `at` of the inspected page, `page`, in the open inspector, or
    /// once it opens; `dock` is the side the page has its tools at. The page asks this as
    /// it's updated, so it says both itself.
    pub fn reveal(
        &mut self,
        at: ipc::Point,
        page: PaneId,
        dock: ipc::DevToolsDock,
        cx: &mut Context<Self>,
    ) {
        let Kind::DevTools { inspect, .. } = &mut self.kind else {
            return;
        };
        let (Some(viewport), true) = (self.viewport, self.opened.is_some()) else {
            *inspect = Some(at);
            return;
        };
        let request = AppRequest::OpenDevTools {
            pane: page,
            devtools: self.pane,
            viewport,
            inspect: Some(at),
            dock,
        };
        self.send(request, cx);
    }

    pub(super) fn page_changed(&mut self, page: PageState, cx: &mut Context<Self>) {
        let gone = self.favicon_named(&page.favicon);
        let loaded = self.page.loading && !page.loading;
        let moved = page.url != self.page.url;
        let changed = gone || loaded || moved || page.title != self.page.title;
        if loaded && self.zoom != ACTUAL_SIZE {
            self.send_zoom(cx);
        }
        self.responsive.reload_hint &= !page.loading;
        self.page = page;
        if !self.page.url.is_empty() {
            self.target.clone_from(&self.page.url);
        }
        self.settle.address_stale = true;
        if changed {
            cx.emit(BrowserEvent::Changed);
        }
        if loaded || (moved && !self.page.loading) {
            self.loaded(cx);
        }
        cx.notify();
    }

    pub(super) fn cursor_changed(&mut self, cursor: Cursor, cx: &mut Context<Self>) {
        if self.cursor != cursor {
            self.cursor = cursor;
            cx.notify();
        }
    }

    pub(super) fn gone(&mut self, reason: String, cx: &mut Context<Self>) {
        self.notice = Some(Notice::Gone(reason));
        cx.notify();
    }

    pub(super) fn failed(&mut self, message: String, cx: &mut Context<Self>) {
        self.notice = Some(Notice::Failed(message));
        cx.notify();
    }

    /// The address field, which the page's toolbar shows, or the Arc sidebar in its place.
    pub fn address_field(&self) -> &Entity<InputState> {
        &self.address
    }

    /// Puts the keyboard in the address, all of it selected, as the person asked with `intent`.
    pub fn focus_address(&mut self, intent: Intent, window: &mut Window, cx: &mut Context<Self>) {
        let bar = Surface::Gpui(Part::Bar(self.pane));
        crate::keyboard::set(window, bar, Cause::Intent(intent));
        self.address.update(cx, |address, cx| {
            address.focus(window, cx);
            address.select_all(window, cx);
        });
    }

    /// GPUI's focus came to the page, or left it: the keyboard's holder hears so, and where it
    /// went, its address or find bar or elsewhere. The holder moves the window's first
    /// responder in or out of the page (`docs/keyboard.md`).
    fn focus_moved(&mut self, into: bool, window: &mut Window, cx: &mut Context<Self>) {
        if into {
            crate::keyboard::entered_page(window, self.pane);
            self.tell_active(cx);
        } else if let Some(part) = self.part_with_focus(window, cx) {
            crate::keyboard::left_page(window, self.pane, part);
        }
    }

    /// Where GPUI's focus is, as the keyboard's holder names it, unless it's the page's own:
    /// the page's address or find bar, or another part of Koyo's. Still the page's while the
    /// window only stopped being key, as its views hear it leave all the same.
    pub fn part_with_focus(&self, window: &Window, cx: &App) -> Option<Part> {
        if self.focus.is_focused(window) {
            return None;
        }
        let address = self.address.read(cx).focus_handle(cx).contains_focused(window, cx);
        let bar = address || self.find.as_ref().is_some_and(|find| find.has_focus(window, cx));
        Some(if bar { Part::Bar(self.pane) } else { Part::Other })
    }

    /// What the render has to do first, with the window: focus, menus and dialogs asked
    /// for since the last one, and the address once the person is done editing it.
    pub(super) fn settle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.settle.focus_page) {
            window.focus(&self.focus, cx);
        }
        if let Some(request) = self.menu_request.take() {
            self.show_menu(&request, window, cx);
        }
        self.ask_next(window, cx);
        self.ask_passkey(window, cx);
        let editing = self.address.read(cx).focus_handle(cx).is_focused(window);
        if self.settle.address_stale && !editing {
            self.settle.address_stale = false;
            let url = self.page.url.clone();
            if self.address.read(cx).value().as_ref() != url {
                self.address
                    .update(cx, |address, cx| address.set_value(url, window, cx));
            }
        }
    }
}

#[cfg(test)]
mod tests;
