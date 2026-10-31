//! A browser pane: the page the host renders, what the person does with it, and the
//! toolbar over it.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use colosso_browser_ipc::{self as ipc, AppRequest, Cursor, Go, PageState, PaneId, Viewport};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Div, ElementId, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, ParentElement as _, Pixels, Point, Stateful, Styled as _,
    Subscription, WeakEntity, Window, div, white,
};

use super::control::{Control, Mode};
use super::dialog::Question;
use super::dock::{Asked, Devtools};
use super::find::{ACTUAL_SIZE, FindBar};
use super::frames::{Frame, Shown};
use super::host::{Browsers, HostStatus};

mod lifecycle;
use super::menu::MenuRequest;
use super::page::PageSurface;
use super::{input, toolbar};
use crate::theme;

pub enum BrowserEvent {
    /// The address or title changed.
    Changed,
    /// Open an address in a new tab beside this one, without leaving it.
    OpenTab(String),
    /// Put the page's `DevTools` in a pane of their own beside this one.
    Detach(Entity<BrowserView>),
    /// Take the page's `DevTools` out of their pane: they're docked again, or closed.
    Undock(Entity<BrowserView>),
    /// The host closed this browser: `DevTools` whose page went away.
    Closed,
}

/// What a pane's browser shows.
enum Kind {
    /// A page of the project's profile.
    Page { profile: String },
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
    /// Who drives the page.
    pub(super) control: Control,
    /// The page the pane shows once the host is ready; empty for the start page, which
    /// needs no browser.
    target: String,
    pub(super) viewport: Option<Viewport>,
    /// The host epoch the page was opened in.
    opened: Option<u64>,
    pub(super) frame: Option<Frame>,
    /// The slot shown before the frame, kept from the host until the next one shows.
    pub(super) behind: Option<(u32, u8)>,
    /// How many frames the page has shown, for measuring its rate.
    pub(super) frames_shown: u64,
    /// The layer the page shows in while it's drawn, which new frames go straight to.
    pub(super) shown: Option<WeakEntity<Shown>>,
    /// Whether the host was last told the page is on screen.
    pub(super) visible: Option<bool>,
    pub(super) cursor: Cursor,
    pub(super) focus: FocusHandle,
    pub(super) address: Entity<InputState>,
    pub(super) notice: Option<Notice>,
    /// Text the input method is composing.
    pub(super) marked: Option<String>,
    /// The modifier keys held, as the page was last told.
    pub(super) held: gpui_kit::Modifiers,
    pub(super) pressed: Option<ipc::MouseButton>,
    pub(super) hovered: bool,
    /// What the next render, which has the window, settles.
    settle: Settle,
    /// Where the page is in the window, for input and menus at page positions.
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
            profile: super::profile(project),
        };
        Self::with_kind(url, kind, window, cx)
    }

    /// The `DevTools` of `inspected`'s page, as a pane of their own.
    pub fn inspector(
        inspected: Entity<BrowserView>,
        inspect: Option<ipc::Point>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // The inspector opens once its page is open, and again after the host restarts and
        // the page reopens.
        let changes = cx.observe(&inspected, |this, _, cx| this.ensure_open(cx));
        let kind = Kind::DevTools {
            inspected,
            inspect,
            _inspected: changes,
        };
        Self::with_kind("devtools:".into(), kind, window, cx)
    }

    fn with_kind(url: String, kind: Kind, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let browsers = Browsers::global(cx);
        let address =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search or enter an address"));
        if !url.is_empty() {
            address.update(cx, |address, cx| address.set_value(url.clone(), window, cx));
        }
        // Each view is a page of its own in the host, even when it reuses a closed pane's
        // id while the host is still closing that page.
        let pane = NEXT_PANE.fetch_add(1, Ordering::Relaxed);
        let focus = cx.focus_handle();
        let subscriptions = vec![
            cx.on_focus(&focus, window, |this, _, cx| this.set_focused(true, cx)),
            cx.on_blur(&focus, window, |this, _, cx| this.set_focused(false, cx)),
            cx.subscribe_in(
                &address,
                window,
                |this, address, event, _, cx| match event {
                    InputEvent::PressEnter { .. } => {
                        let text = address.read(cx).value();
                        if let Some(url) = super::address_to_url(&text) {
                            this.person_acts(Mode::Implicit, cx);
                            this.navigate(url, cx);
                            this.settle.focus_page = true;
                        }
                    }
                    InputEvent::Blur => {
                        this.settle.address_stale = true;
                        cx.notify();
                    }
                    InputEvent::Change | InputEvent::Focus => {}
                },
            ),
        ];
        let weak = cx.weak_entity();
        browsers.update(cx, |browsers, _| browsers.attach(pane, weak));
        let releasing = browsers.clone();
        cx.on_release(move |_, cx| {
            releasing.update(cx, |browsers, _| browsers.detach(pane));
        })
        .detach();
        Self {
            pane,
            browsers,
            kind,
            page: PageState {
                url: url.clone(),
                ..PageState::default()
            },
            control: Control::default(),
            target: url,
            viewport: None,
            opened: None,
            frame: None,
            behind: None,
            frames_shown: 0,
            shown: None,
            visible: None,
            cursor: Cursor::Arrow,
            focus,
            address,
            notice: None,
            marked: None,
            held: gpui_kit::Modifiers::default(),
            pressed: None,
            hovered: false,
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
            _subscriptions: subscriptions,
        }
    }

    /// What the strip calls the pane.
    pub fn title(&self) -> String {
        match &self.kind {
            Kind::Page { .. } => toolbar::tab_title(&self.page.title, &self.page.url),
            Kind::DevTools { .. } => "DevTools".into(),
        }
    }

    /// The address the page shows, or the one it's opening.
    pub fn address(&self) -> String {
        if self.page.url.is_empty() {
            self.target.clone()
        } else {
            self.page.url.clone()
        }
    }

    /// The `DevTools` of another pane's page.
    pub fn is_inspector(&self) -> bool {
        matches!(self.kind, Kind::DevTools { .. })
    }

    /// Whether the pane draws the frames it gets. While the `DevTools` frontend loads it
    /// paints an empty document and then its UI unstyled, white whatever the theme; it
    /// shows from the moment it names the page it is attached to, which it does once it
    /// has styled itself.
    pub(super) fn shows_frames(&self) -> bool {
        !self.is_inspector() || self.page.title.starts_with("DevTools - ")
    }

    pub fn is_start_page(&self) -> bool {
        self.target.is_empty() && self.page.url.is_empty()
    }

    /// The page `DevTools` inspect.
    pub fn inspected(&self) -> Option<&Entity<BrowserView>> {
        match &self.kind {
            Kind::DevTools { inspected, .. } => Some(inspected),
            Kind::Page { .. } => None,
        }
    }

    /// The host closed this browser.
    pub(super) fn closed(&mut self, cx: &mut Context<Self>) {
        self.opened = None;
        self.frame = None;
        self.behind = None;
        cx.emit(BrowserEvent::Closed);
        cx.notify();
    }

    /// Shows the element at `at` of the inspected page in the open inspector, or once it
    /// opens.
    pub fn reveal(&mut self, at: ipc::Point, cx: &mut Context<Self>) {
        let Kind::DevTools {
            inspected, inspect, ..
        } = &mut self.kind
        else {
            return;
        };
        let (Some(viewport), true) = (self.viewport, self.opened.is_some()) else {
            *inspect = Some(at);
            return;
        };
        let request = AppRequest::OpenDevTools {
            pane: inspected.read(cx).pane,
            devtools: self.pane,
            viewport,
            inspect: Some(at),
        };
        self.send(request, cx);
    }

    pub fn navigate(&mut self, url: String, cx: &mut Context<Self>) {
        if self.is_inspector() {
            return;
        }
        if matches!(self.status(cx), HostStatus::Failed(_)) {
            // Asking for a page again is the way to retry.
            self.restart_host(cx);
        }
        self.notice = None;
        self.target.clone_from(&url);
        self.page.url.clone_from(&url);
        (self.page.loading, self.page.progress) = (true, 0);
        self.send_or_open(
            AppRequest::Navigate {
                pane: self.pane,
                url,
            },
            cx,
        );
        cx.emit(BrowserEvent::Changed);
        cx.notify();
    }

    /// The person moves through the page's history, reloads or stops it.
    pub(super) fn go(&mut self, to: Go, cx: &mut Context<Self>) {
        self.person_acts(Mode::Implicit, cx);
        if to == Go::Reload && matches!(self.notice, Some(Notice::Gone(_))) {
            // A crashed page needs a new browser, not a reload.
            self.notice = None;
            self.opened = None;
            self.ensure_open(cx);
            return;
        }
        let pane = self.pane;
        self.send_to_page(AppRequest::Go { pane, to }, cx);
    }

    pub(super) fn page_changed(&mut self, page: PageState, cx: &mut Context<Self>) {
        let changed = page.url != self.page.url || page.title != self.page.title;
        if self.page.loading && !page.loading && self.zoom != ACTUAL_SIZE {
            self.send_zoom(cx);
        }
        self.page = page;
        if !self.page.url.is_empty() {
            self.target.clone_from(&self.page.url);
        }
        self.settle.address_stale = true;
        if changed {
            cx.emit(BrowserEvent::Changed);
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

    /// Shows a new frame; returns the slot it replaces, for the host.
    pub(super) fn show_frame(
        &mut self,
        mut frame: Frame,
        cx: &mut Context<Self>,
    ) -> Option<(u32, u8)> {
        let shown = (frame.generation, frame.slot);
        self.frames_shown += 1;
        frame.serial = self.frames_shown;
        // A frame needs no layout: once the page's layer is up, it goes straight to it, and
        // the window isn't drawn again for it.
        match self.shown.as_ref().and_then(WeakEntity::upgrade) {
            Some(layer) if self.frame.is_some() => layer.update(cx, |layer, _| layer.show(&frame)),
            _ => cx.notify(),
        }
        let previous = (self.frame.replace(frame))
            .map(|previous| (previous.generation, previous.slot))
            .filter(|previous| *previous != shown);
        // The slot shown until now stays on screen until Core Animation draws this one, so
        // the host paints into it again only once the next frame comes.
        std::mem::replace(&mut self.behind, previous).filter(|behind| *behind != shown)
    }

    /// Tells the host whether the page is on screen, when that changed; out of sight it
    /// stops painting.
    pub(super) fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible.replace(visible) != Some(visible) {
            let pane = self.pane;
            self.send_to_page(AppRequest::Visible { pane, visible }, cx);
        }
    }

    /// Puts the keyboard in the address, all of it selected.
    pub fn focus_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.address.update(cx, |address, cx| {
            address.focus(window, cx);
            address.select_all(window, cx);
        });
    }

    fn set_focused(&mut self, focused: bool, cx: &mut Context<Self>) {
        let pane = self.pane;
        self.send_to_page(AppRequest::Focus { pane, focused }, cx);
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

    /// The page itself under its notices, taking the keys bound to it.
    pub(super) fn page_element(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let p = theme::palette(cx);
        let status = self.status(cx);
        let start = self.is_start_page();
        let inspector = self.is_inspector();
        let overlay = toolbar::overlay(self, &status, cx);
        let bar = toolbar::control_bar(self, cx);
        let page = div()
            .id(ElementId::NamedInteger("browser-page".into(), self.pane))
            .key_context("BrowserPage")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .overflow_hidden()
            // DevTools wait in the pane's own colour until they are drawn.
            .bg(if start || inspector { p.card } else { white() })
            .child(PageSurface {
                view: cx.entity(),
                focused: self.focus.is_focused(window),
                covered: overlay.is_some(),
            })
            .when(start, |page| page.child(toolbar::start_page(p)))
            .children(bar)
            .children(overlay);
        input::listen(page, cx)
    }
}

#[cfg(test)]
mod tests;
