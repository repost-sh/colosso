//! Responsive mode in a pane's view: its state, the person's changes and an agent's, the
//! page's new size sent to the host at once (a tab out of sight resizes too) or, for a size
//! the person drags or types, as the pane is next drawn, its emulation sent to the page's
//! session, and the one mapping between the window and the page shown scaled in it.

use gpui_kit::{Bounds, Context, Pixels, Point, Size, Window, point, px, size};
use koyo_browser_ipc::{self as ipc, Viewport};

use super::bar::HEIGHT as BAR;
use super::devices::{self, Device};
use super::{Emulation, Responsive, SetBy, Zoom, commands, place, to_page, to_window};
use crate::browser::control::{Actor, Refused};
use crate::browser::view::BrowserView;
use crate::browser::view::sized::Dragging;
use crate::responsive::Drag;
use crate::responsive::bar::Fields;

/// The scale a pane assumes when it knows no window's, as a last resort: it's made with the
/// window's.
const SCALE_UNDRAWN: f32 = 2.;
/// The size a pane assumes for "the pane's" before it's drawn.
const PANE_UNDRAWN: (u32, u32) = (1280, 800);

/// A view's responsive mode, on or off, and how its page shows.
pub struct State {
    /// The mode while it's on.
    pub mode: Option<Responsive>,
    /// What the page's `DevTools` session emulates, as told since the page opened.
    pub(super) emulated: Emulation,
    /// How much of its true size the page shows at, and the room it shows in, as last
    /// drawn, with the window's scale then and whether the device bar took its height.
    pub fit: f32,
    pub room: Size<Pixels>,
    pub window_scale: f32,
    barred: bool,
    /// A grip in hand.
    pub drag: Option<Drag>,
    /// The bar's width and height fields, made with the window once the mode is on.
    pub(super) fields: Option<Fields>,
    /// Whether the bar offers a reload, the user agent having changed on a loaded page;
    /// and whether it has ever offered one, as it does only the first time.
    pub reload_hint: bool,
    hinted: bool,
}

impl Default for State {
    fn default() -> Self {
        State {
            mode: None,
            emulated: Emulation::default(),
            fit: 1.,
            room: Size::default(),
            window_scale: 0.,
            barred: false,
            drag: None,
            fields: None,
            reload_hint: false,
            hinted: false,
        }
    }
}

/// What an agent asks responsive mode for; what it leaves out stays as it is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Change {
    /// End the mode; nothing else goes with it.
    pub fill: bool,
    /// A free size, or a preset.
    pub device: Option<Choice>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub scale: Option<f32>,
    pub mobile: Option<bool>,
    pub touch: Option<bool>,
    pub user_agent: Option<String>,
    pub landscape: Option<bool>,
    pub zoom: Option<Zoom>,
}

impl Change {
    /// The mode `current` becomes by `by`'s change, in a pane whose room is `pane` logical
    /// pixels and a window at scale `window`: `None` ends it. A free size customised keeps
    /// what isn't changed; a side not given is the other one kept, or the pane's.
    pub fn resolve(
        &self,
        current: Option<&Responsive>,
        pane: (u32, u32),
        window: f32,
        by: SetBy,
    ) -> Result<Option<Responsive>, String> {
        if self.fill {
            if *self
                != (Change {
                    fill: true,
                    ..Change::default()
                })
            {
                return Err("`fill` ends responsive mode: give nothing else with it.".into());
            }
            return Ok(None);
        }
        let sized = self.width.is_some() || self.height.is_some();
        let customised = self.scale.is_some()
            || self.mobile.is_some()
            || self.touch.is_some()
            || self.user_agent.is_some();
        if *self == Change::default() {
            return Err(
                "Say what to show: a `device`, a `width` or `height`, an `orientation` or a \
                 `zoom`; or `fill: true`."
                    .into(),
            );
        }
        let zoom = (self.zoom.or(current.map(|mode| mode.zoom))).unwrap_or(Zoom::Fit);
        let preset = match self.device {
            Some(Choice::Free) => None,
            Some(Choice::Preset(device)) => Some(device),
            None => (current.and_then(|mode| mode.device)).filter(|_| !sized && !customised),
        };
        if let Some(device) = preset {
            if sized || customised {
                return Err(
                    "A preset has its own size, scale, touch and user agent: give `width`, \
                     `height`, `scale`, `mobile`, `touch` and `userAgent` with `device: \
                     \"responsive\"` instead."
                        .into(),
                );
            }
            let turned = current.filter(|mode| mode.device == Some(device));
            let landscape =
                (self.landscape).unwrap_or_else(|| turned.is_some_and(Responsive::landscape));
            return Ok(Some(Responsive::preset(device, landscape, zoom, by)));
        }
        let mut free = match current {
            Some(mode) if mode.device.is_none() => mode.clone(),
            Some(mode) => Responsive::free(mode.width, mode.height, zoom, by.clone()),
            None => Responsive::free(pane.0, pane.1, zoom, by.clone()),
        };
        (free.zoom, free.set_by) = (zoom, by);
        free.width = self.width.unwrap_or(free.width);
        free.height = self.height.unwrap_or(free.height);
        if let Some(scale) = self.scale {
            if !(0.5..=5.).contains(&scale) {
                return Err("`scale` is 0.5 to 5.".into());
            }
            free.scale = Some(scale);
        }
        free.mobile = self.mobile.unwrap_or(free.mobile);
        free.touch = self.touch.unwrap_or(free.touch);
        if let Some(user_agent) = &self.user_agent {
            if user_agent.trim().is_empty() {
                return Err("`userAgent` is empty.".into());
            }
            free.user_agent = Some(user_agent.clone());
        }
        if self
            .landscape
            .is_some_and(|landscape| landscape != free.landscape())
        {
            (free.width, free.height) = (free.height, free.width);
        }
        let scale = free.scale_at(window);
        for (side, value) in [("width", free.width), ("height", free.height)] {
            if !devices::within(value, scale) {
                return Err(format!(
                    "`{side}` is {} to {} logical pixels at scale {scale}.",
                    devices::MIN_SIDE,
                    devices::max_side(scale)
                ));
            }
        }
        Ok(Some(free))
    }
}

/// What responsive mode shows: a free size, or a preset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Choice {
    Free,
    Preset(&'static Device),
}

/// Why an agent's change to a page's viewport didn't happen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViewportRefused {
    /// The page's lease keeps the agent out.
    Refused(Refused),
    /// The change makes no sense; the message says why.
    Invalid(String),
}

impl BrowserView {
    /// A point of the window as a point of the page, in its own logical pixels: every
    /// pointer position the page is told goes through here.
    pub(in crate::browser) fn page_point(&self, position: Point<Pixels>) -> Point<Pixels> {
        to_page(position, self.origin, self.responsive.fit)
    }

    /// A point of the page, as the host names one, as a point of the window.
    pub(in crate::browser) fn window_point(&self, at: ipc::Point) -> Point<Pixels> {
        to_window(point(px(at.x), px(at.y)), self.origin, self.responsive.fit)
    }

    /// The viewport the page is laid out at in `area` of a window at scale `window`, where
    /// it shows there and how much of its true size it shows at.
    pub(in crate::browser) fn lay_out(
        &self,
        area: Bounds<Pixels>,
        window: f32,
    ) -> (Viewport, Bounds<Pixels>, f32) {
        let Some(mode) = &self.responsive.mode else {
            return (filling(area.size, window), area, 1.);
        };
        let kept = mode.kept_within(window);
        let mode = kept.as_ref().unwrap_or(mode);
        // A grip in hand keeps the scale it was taken at, a pixel's size then, so the edge
        // follows the pointer.
        let fit =
            (self.responsive.drag.as_ref()).map_or_else(|| mode.fit(area.size), |drag| drag.unit.0);
        let shown = place(area, mode.width, mode.height, fit);
        (mode.viewport(window), shown, fit)
    }

    /// Whether the page is placed as `laid` lays it out in `room`, at the window's `scale`.
    pub(in crate::browser) fn is_placed(
        &self,
        laid: &(Viewport, Bounds<Pixels>, f32),
        room: Size<Pixels>,
        scale: f32,
    ) -> bool {
        let (viewport, shown, fit) = laid;
        let state = &self.responsive;
        self.origin == shown.origin
            && self.viewport == Some(*viewport)
            && (state.fit - fit).abs() < f32::EPSILON
            && state.room == room
            && (state.window_scale - scale).abs() < f32::EPSILON
            && state.barred == state.mode.is_some()
    }

    /// Places the page as `laid` lays it out in `window`, telling the host its size when it
    /// changed, as `dragging` drags the pane's edge.
    pub(in crate::browser) fn place_page(
        &mut self,
        laid: (Viewport, Bounds<Pixels>, f32),
        room: Size<Pixels>,
        window: &Window,
        dragging: Dragging,
        cx: &mut Context<Self>,
    ) {
        let (viewport, shown, fit) = laid;
        let scale = window.scale_factor();
        self.origin = shown.origin;
        (self.responsive.fit, self.responsive.room) = (fit, room);
        self.responsive.window_scale = scale;
        self.window_size = Some(window.viewport_size());
        // The bar is there exactly while the mode is on.
        self.responsive.barred = self.responsive.mode.is_some();
        // The size laid out at a new scale is the mode's from now on, and the bar's.
        if let Some(kept) = (self.responsive.mode.as_ref()).and_then(|mode| mode.kept_within(scale))
        {
            self.responsive.mode = Some(kept);
            cx.notify();
        }
        self.viewport_changed(viewport, dragging, cx);
    }

    /// The window's scale as last drawn, or a likely one before.
    pub(in crate::browser) fn window_scale(&self) -> f32 {
        let scale = self.responsive.window_scale;
        if scale > 0. { scale } else { SCALE_UNDRAWN }
    }

    /// The page's room as last drawn, under the device bar (`barred`) or with none: what it
    /// is either way, the bar taking its height off the pane while the mode is on.
    fn room(&self, barred: bool) -> Size<Pixels> {
        let state = &self.responsive;
        let bar = match (state.barred, barred) {
            (true, false) => px(BAR),
            (false, true) => px(-BAR),
            _ => px(0.),
        };
        size(state.room.width, (state.room.height + bar).max(px(0.)))
    }

    /// The pane's room for a page at its true size, in logical pixels: the area under the
    /// device bar, less the gutters.
    pub(super) fn pane_size(&self) -> (u32, u32) {
        let room = self.room(true);
        if room.width <= px(0.) || room.height <= px(0.) {
            return PANE_UNDRAWN;
        }
        let inside = crate::responsive::room(room);
        let side = |side: Pixels| (side.as_f32().floor().max(1.) as u32).max(devices::MIN_SIDE);
        (side(inside.width), side(inside.height))
    }

    /// The viewport the page is wanted at now: the mode's, or with it off the pane's, the
    /// bar gone with it; `None` before the pane is drawn with it off.
    pub fn wanted_viewport(&self) -> Option<Viewport> {
        if let Some(mode) = &self.responsive.mode {
            return Some(mode.viewport(self.window_scale()));
        }
        let room = self.room(false);
        let scale = self.responsive.window_scale;
        (scale > 0. && room.width > px(0.)).then(|| filling(room, scale))
    }

    /// Turns responsive mode on, at the pane's size, or off. Inspectors have none.
    pub(in crate::browser) fn toggle_responsive(&mut self, cx: &mut Context<Self>) {
        let mode = self.responsive.mode.is_none().then(|| {
            let (width, height) = self.pane_size();
            Responsive::free(width, height, Zoom::Fit, SetBy::Person)
        });
        self.set_responsive(mode, cx);
    }

    /// The person picks a preset, or a free size (`None`), from the device menu.
    pub(in crate::browser) fn pick_device(
        &mut self,
        device: Option<&'static Device>,
        cx: &mut Context<Self>,
    ) {
        let change = Change {
            device: Some(device.map_or(Choice::Free, Choice::Preset)),
            ..Change::default()
        };
        self.person_changes(&change, cx);
    }

    /// The person changes the mode, which takes nothing from an agent holding the page.
    pub(in crate::browser) fn person_changes(&mut self, change: &Change, cx: &mut Context<Self>) {
        let (pane, window) = (self.pane_size(), self.window_scale());
        let current = self.responsive.mode.as_ref();
        match change.resolve(current, pane, window, SetBy::Person) {
            Ok(mode) => self.set_responsive(mode, cx),
            Err(why) => log::warn!("responsive mode: {why}"),
        }
    }

    /// The person sizes a free page to `width` × `height`, within the limits. The page is told
    /// as the pane is next drawn, so a grip dragged faster than the window draws sends one
    /// size a frame, and the window waits for the page's frame at it (`view/sized.rs`).
    pub(in crate::browser) fn person_sizes(
        &mut self,
        width: u32,
        height: u32,
        cx: &mut Context<Self>,
    ) {
        let window = self.window_scale();
        let Some(mode) = (self.responsive.mode.as_mut()).filter(|mode| mode.device.is_none())
        else {
            return;
        };
        let scale = mode.scale_at(window);
        let sized = (devices::clamp(width, scale), devices::clamp(height, scale));
        if sized == (mode.width, mode.height) && mode.set_by == SetBy::Person {
            return;
        }
        (mode.width, mode.height) = sized;
        mode.set_by = SetBy::Person;
        cx.notify();
    }

    /// `actor` sets the page's viewport, holding the page or finding it free: the viewport
    /// the page is to settle at, when the pane has been drawn to know it.
    pub fn agent_viewport(
        &mut self,
        actor: &Actor,
        change: &Change,
        cx: &mut Context<Self>,
    ) -> Result<Option<Viewport>, ViewportRefused> {
        let by = SetBy::Agent(actor.label.clone());
        let current = self.responsive.mode.as_ref();
        let mode = (change.resolve(current, self.pane_size(), self.window_scale(), by))
            .map_err(ViewportRefused::Invalid)?;
        (self.agent_claims(actor.clone(), false, None, cx)).map_err(ViewportRefused::Refused)?;
        self.set_responsive(mode, cx);
        Ok(self.wanted_viewport())
    }

    /// Puts the page in `mode`: its new size goes to the host now, and what it emulates to
    /// its session.
    pub(in crate::browser) fn set_responsive(
        &mut self,
        mode: Option<Responsive>,
        cx: &mut Context<Self>,
    ) {
        if self.is_inspector() {
            return;
        }
        if mode.is_none() {
            self.responsive.drag = None;
        }
        self.responsive.mode = mode;
        if let Some(viewport) = self.wanted_viewport() {
            self.viewport_changed(viewport, Dragging::No, cx);
        }
        self.emulate(cx);
        cx.notify();
    }

    /// The page's session opened afresh, emulating nothing: it's told the mode again.
    pub(in crate::browser) fn session_opened(&mut self, cx: &mut Context<Self>) {
        self.responsive.emulated = Emulation::default();
        self.emulate(cx);
    }

    /// Tells the page's session what the mode emulates that it doesn't yet. The app sends
    /// these itself, under no agent's lease.
    fn emulate(&mut self, cx: &mut Context<Self>) {
        if self.is_inspector() || !self.opened_now(cx) {
            return;
        }
        let major = self.browsers.read(cx).chromium_major().unwrap_or_default();
        let wanted = (self.responsive.mode.as_ref())
            .map(|mode| mode.emulation(&major))
            .unwrap_or_default();
        let sent = commands(&self.responsive.emulated, &wanted);
        let agent_changed = wanted.agent != self.responsive.emulated.agent;
        self.responsive.emulated = wanted;
        // A site that reads the user agent on its server shows its other pages once the
        // page loads again; the bar says so, the first time.
        if agent_changed && !self.page.url.is_empty() && !self.responsive.hinted {
            (self.responsive.reload_hint, self.responsive.hinted) = (true, true);
        }
        let pane = self.pane;
        for (method, params) in sent {
            let answered = self
                .browsers
                .update(cx, |browsers, _| browsers.call(pane, method, params));
            cx.spawn(async move |_, _| {
                if let Ok(Err(why)) = answered.await {
                    log::warn!("responsive mode's {method} failed: {why}");
                }
            })
            .detach();
        }
    }

    /// How large the page shows now, in percent of its true size.
    pub fn shown_percent(&self) -> u32 {
        let fit = match &self.responsive.mode {
            Some(mode) => mode.fit(self.room(true)),
            None => 1.,
        };
        (fit * 100.).round() as u32
    }
}

/// The viewport of a page filling `size` of a window at scale `scale`.
fn filling(size: Size<Pixels>, scale: f32) -> Viewport {
    Viewport {
        width: size.width.as_f32().round().max(1.) as u32,
        height: size.height.as_f32().round().max(1.) as u32,
        scale,
    }
}
