//! What app control asks of a browser page for an agent: what the page is, who holds it,
//! and the ways an agent moves it; and the Chrome `DevTools` protocol commands it runs on
//! it, under the page's lease when they act.

use std::time::Duration;

use gpui_kit::{App, Context, Entity, Task};
use koyo_app_control::ops::browser::{
    DeviceName, Orientation, Viewport as ViewportFacts, ViewportMode,
};
use koyo_browser_ipc::AppRequest;
pub use koyo_browser_ipc::{Go, Viewport};

use super::control::Holder;
pub use super::control::{Actor, Mode, Refused};
pub use super::host::Answer;
use super::host::{Browsers, HostStatus};
use super::responsive;
pub use super::responsive::devices::find as find_device;
pub use super::responsive::{
    Change as ViewportChange, Choice, SetBy, ViewportRefused, Zoom, opening,
};
use super::view::BrowserView;
pub(crate) use super::view::sight::AWAKE_FOR;

/// Why a protocol command didn't run for an agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentError {
    /// The page's lease keeps the agent out.
    Refused(Refused),
    /// The engine hasn't the page open, for the reason given.
    NotOpen(NotOpen),
}

/// Why the engine hasn't a page open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotOpen {
    /// The engine is starting, or yet to start: it opens the page once it runs.
    Starting,
    /// The engine failed, as the message says, and stays down until the person restarts it.
    Failed(String),
    /// The pane shows the start page, which has no page until an address is loaded.
    StartPage,
    /// Its pane has never been drawn, and no agent opened it: it opens once its tab is shown.
    Unshown,
}

/// The viewport of a page that fills its pane, at `viewport`, shown at `shown_percent`.
fn filling(viewport: Viewport, shown_percent: u32) -> ViewportFacts {
    ViewportFacts {
        mode: ViewportMode::Fill,
        device: None,
        width: viewport.width,
        height: viewport.height,
        scale: f64::from(viewport.scale),
        mobile: false,
        touch: false,
        user_agent: None,
        orientation: orientation(viewport.width > viewport.height),
        zoom: Zoom::Fit.into(),
        shown_percent,
        set_by: None,
    }
}

/// The viewport of a page brought back at launch whose tab wasn't shown yet: it fills a pane
/// not laid out, 0 × 0.
pub fn unshown_viewport() -> ViewportFacts {
    let viewport = Viewport {
        width: 0,
        height: 0,
        scale: 1.,
    };
    filling(viewport, 100)
}

/// A page as an agent sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct PageFacts {
    pub url: String,
    pub title: String,
    pub loading: bool,
    /// Who drives it: `person`, or the agent's name.
    pub controller: Option<String>,
    /// Whether the host has it open, so commands reach it.
    pub open: bool,
    /// The size and device it's laid out at.
    pub viewport: ViewportFacts,
}

impl BrowserView {
    /// The page as an agent sees it.
    pub fn facts(&self, cx: &App) -> PageFacts {
        PageFacts {
            url: self.page.url.clone(),
            title: self.title(),
            loading: self.page.loading,
            controller: match &self.control.holder {
                Some(Holder::Human { .. }) => Some("person".to_owned()),
                Some(Holder::Agent { actor, .. }) => Some(actor.label.clone()),
                None => None,
            },
            open: self.is_open(cx),
            viewport: self.viewport_facts(cx),
        }
    }

    /// The page's viewport as an agent sees it: responsive mode's, or the pane's size.
    pub fn viewport_facts(&self, cx: &App) -> ViewportFacts {
        let window = self.window_scale();
        let shown_percent = self.shown_percent();
        let Some(mode) = &self.responsive.mode else {
            let viewport = self.viewport.unwrap_or(Viewport {
                width: 0,
                height: 0,
                scale: window,
            });
            return filling(viewport, shown_percent);
        };
        let major = self.browsers.read(cx).chromium_major().unwrap_or_default();
        let emulated = mode.emulation(&major);
        ViewportFacts {
            mode: if mode.device.is_some() {
                ViewportMode::Device
            } else {
                ViewportMode::Responsive
            },
            device: mode
                .device
                .and_then(|device| DeviceName::from_id(device.id)),
            width: mode.width,
            height: mode.height,
            scale: f64::from(mode.scale_at(self.window_scale())),
            mobile: mode.mobile,
            touch: mode.touch,
            user_agent: emulated.agent.map(|agent| agent.user_agent),
            orientation: orientation(mode.landscape()),
            zoom: mode.zoom.into(),
            shown_percent,
            set_by: Some(mode.set_by.agent().unwrap_or("person").to_owned()),
        }
    }

    /// Why the engine hasn't the page open; none when it has.
    pub fn not_open(&self, cx: &App) -> Option<NotOpen> {
        if self.is_open(cx) {
            return None;
        }
        Some(match self.status(cx) {
            HostStatus::Idle | HostStatus::Starting => NotOpen::Starting,
            HostStatus::Failed(message) => NotOpen::Failed(message),
            HostStatus::Running if self.is_start_page() => NotOpen::StartPage,
            HostStatus::Running => NotOpen::Unshown,
        })
    }

    /// Whether the page can be asked the size it lays out at, as at `target`: it was told
    /// `target`, and it shows a frame of it, unless the engine has it out of sight, where
    /// the host paints nothing and the page's own report is all there is. A page an agent
    /// uses is on screen for the engine (`view/sight.rs`), so it paints.
    pub fn ready_to_measure(&self, target: Viewport) -> bool {
        self.viewport == Some(target)
            && (self.visible == Some(false) || self.shows_frame_of(target.pixels()))
    }

    /// Whether a page reporting `inner` (`innerWidth`, `innerHeight`) has laid out at
    /// `target`, at the page's zoom.
    pub fn laid_out_at(&self, target: Viewport, inner: (f64, f64)) -> bool {
        let mobile = self
            .responsive
            .mode
            .as_ref()
            .is_some_and(|mode| mode.mobile);
        responsive::laid_out(target.width, target.height, self.zoom, mobile, inner)
    }

    /// Loads `url` for `actor`, which must hold the page or find it free.
    pub fn agent_navigate(
        &mut self,
        actor: &Actor,
        url: String,
        cx: &mut Context<Self>,
    ) -> Result<(), Refused> {
        self.agent_claims(actor.clone(), false, None, cx)?;
        self.navigate(url, cx);
        Ok(())
    }

    /// Moves through the page's history or reloads it for `actor`, which must hold the page
    /// or find it free.
    pub fn agent_go(
        &mut self,
        actor: &Actor,
        to: Go,
        cx: &mut Context<Self>,
    ) -> Result<(), Refused> {
        self.agent_claims(actor.clone(), false, None, cx)?;
        let pane = self.pane;
        self.page.loading = true;
        self.send_to_page(AppRequest::Go { pane, to }, cx);
        cx.notify();
        Ok(())
    }

    /// Claims the page for `actor` and sends its pointer on its way to `(x, y)` of the page,
    /// in CSS pixels: answers how long it takes to get there, which a click waits out so it
    /// lands as the pointer does. A page the host hasn't opened yet takes no click, and the
    /// pointer stays put.
    pub fn agent_points(
        &mut self,
        actor: &Actor,
        (x, y): (f64, f64),
        cx: &mut Context<Self>,
    ) -> Result<Duration, Refused> {
        self.agent_claims(actor.clone(), false, None, cx)?;
        if !self.is_open(cx) {
            return Ok(Duration::ZERO);
        }
        Ok(self.point_to(x, y, cx))
    }

    /// The agent's click landed where its pointer is.
    pub fn agent_clicked(&mut self, cx: &mut Context<Self>) {
        self.pointer_clicked(cx);
    }
}

/// `portrait`, or `landscape` when wider than tall.
fn orientation(landscape: bool) -> Orientation {
    if landscape {
        Orientation::Landscape
    } else {
        Orientation::Portrait
    }
}

/// Runs a protocol command on `view`'s page: under its lease for `actor` when it acts, freely
/// when it only reads (`actor` is `None`). Its answer comes from the returned task, which
/// waits for it at most `limit` ([`Browsers::ask`]).
pub fn cdp(
    view: &Entity<BrowserView>,
    actor: Option<&Actor>,
    method: &str,
    params: serde_json::Value,
    limit: Duration,
    cx: &mut App,
) -> Result<Task<Answer>, AgentError> {
    if let Some(why) = view.read(cx).not_open(cx) {
        return Err(AgentError::NotOpen(why));
    }
    if let Some(actor) = actor {
        (view.update(cx, |view, cx| {
            view.agent_claims(actor.clone(), false, None, cx)
        }))
        .map_err(AgentError::Refused)?;
    }
    view.update(cx, BrowserView::agent_uses);
    let pane = view.read(cx).pane;
    let browsers = Browsers::global(cx);
    Ok(browsers.update(cx, |browsers, cx| {
        browsers.ask(pane, method, params, limit, cx)
    }))
}
