//! Responsive mode, Chrome's device toolbar for a browser pane (`docs/responsive.md` 2): the
//! page laid out at a chosen width × height, or as a phone or tablet (`devices`), and shown
//! scaled down to fit the pane. The page's viewport is the emulated one, not its element's
//! bounds, so the page's `innerWidth`, `devicePixelRatio` and media queries are the
//! device's; what a preset adds (mobile layout, touch, its user agent) goes over the page's
//! `DevTools` protocol session, sent by the app itself, and again whenever the host opens
//! the page afresh. The pane shows the page in responsive mode's chrome, which terminals
//! share (`crate::responsive`): centred under the device bar (`bar`), with grips on its
//! edges to drag it to a size (`grips`); the view's side of it is in `pane`.
//!
//! Nothing of it is remembered across restarts: every pane the person opens fills its space,
//! and a page an agent opens starts at the size the person set for those (`opening`).

pub mod bar;
pub mod devices;
pub mod grips;
mod opening;
mod pane;

use gpui_kit::{Bounds, Pixels, Point, Size, point};
use koyo_browser_ipc::{PixelSize, Viewport};
use serde_json::{Value, json};

pub use crate::responsive::{Grip, Zoom};
pub use devices::Device;
use devices::Override;
pub use opening::opening;
pub use pane::{Change, Choice, State, ViewportRefused};

/// Who set the page's size or device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetBy {
    Person,
    /// An agent, by the name the person knows it by.
    Agent(String),
}

impl SetBy {
    /// The agent's name, when an agent set it.
    pub fn agent(&self) -> Option<&str> {
        match self {
            SetBy::Agent(agent) => Some(agent),
            SetBy::Person => None,
        }
    }
}

/// A page's responsive mode while it's on.
#[derive(Clone, Debug, PartialEq)]
pub struct Responsive {
    /// The preset, or `None` for a free size.
    pub device: Option<&'static Device>,
    /// The page's size in logical pixels, turned as it shows.
    pub width: u32,
    pub height: u32,
    /// A free size's own scale; the window's when `None`. A preset has its device's.
    pub scale: Option<f32>,
    pub mobile: bool,
    pub touch: bool,
    /// A free size's own user agent. A preset has its device's.
    pub user_agent: Option<String>,
    pub zoom: Zoom,
    pub set_by: SetBy,
}

impl Responsive {
    /// A free size, as a desktop.
    pub fn free(width: u32, height: u32, zoom: Zoom, set_by: SetBy) -> Self {
        Responsive {
            device: None,
            width,
            height,
            scale: None,
            mobile: false,
            touch: false,
            user_agent: None,
            zoom,
            set_by,
        }
    }

    /// `device`, turned `landscape` or not.
    pub fn preset(device: &'static Device, landscape: bool, zoom: Zoom, set_by: SetBy) -> Self {
        let (width, height) = device.size(landscape);
        Responsive {
            device: Some(device),
            width,
            height,
            scale: Some(device.scale),
            mobile: true,
            touch: true,
            user_agent: None,
            zoom,
            set_by,
        }
    }

    pub fn landscape(&self) -> bool {
        self.width > self.height
    }

    /// The page's device pixels per logical pixel, in a window at `window`'s: a preset's is
    /// its device's, kept in `scale` as it's made.
    pub fn scale_at(&self, window: f32) -> f32 {
        self.scale.unwrap_or(window)
    }

    /// The mode brought within the largest page a window at scale `window` shows, when it
    /// isn't: a free size at the window's scale outgrows it on a denser screen.
    pub fn kept_within(&self, window: f32) -> Option<Responsive> {
        let scale = self.scale_at(window);
        let width = devices::clamp(self.width, scale);
        let height = devices::clamp(self.height, scale);
        ((width, height) != (self.width, self.height)).then(|| Responsive {
            width,
            height,
            ..self.clone()
        })
    }

    /// The viewport the page is laid out at, in a window at scale `window`.
    pub fn viewport(&self, window: f32) -> Viewport {
        Viewport {
            width: self.width,
            height: self.height,
            scale: self.scale_at(window),
        }
    }

    /// How much of its true size the page shows at in `room`, the area under the bar.
    pub fn fit(&self, room: Size<Pixels>) -> f32 {
        match self.zoom {
            Zoom::Percent(percent) => percent as f32 / 100.,
            Zoom::Fit => {
                let room = crate::responsive::room(room);
                let wide = room.width.as_f32() / self.width.max(1) as f32;
                let tall = room.height.as_f32() / self.height.max(1) as f32;
                // A pane too small for any page still shows a sliver of it.
                wide.min(tall).clamp(0.05, 1.)
            }
        }
    }

    /// What the page's session is told for it, Android's user agent naming the engine's
    /// Chromium `major` version.
    pub fn emulation(&self, major: &str) -> Emulation {
        let agent = match (self.device, &self.user_agent) {
            (Some(device), _) => Some(device.user_agent(major)),
            (None, Some(user_agent)) => Some(Override {
                user_agent: user_agent.clone(),
                platform: None,
                metadata: None,
            }),
            (None, None) => None,
        };
        Emulation {
            mobile: self.mobile,
            touch: self.touch,
            agent,
        }
    }
}

/// What a page's `DevTools` session emulates beyond its size. The default is a page as the
/// engine shows it, which a session just opened is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Emulation {
    pub mobile: bool,
    pub touch: bool,
    pub agent: Option<Override>,
}

/// The protocol commands that take a session from emulating `applied` to `wanted`.
pub fn commands(applied: &Emulation, wanted: &Emulation) -> Vec<(&'static str, Value)> {
    let mut commands = Vec::new();
    if applied.mobile != wanted.mobile {
        commands.push(if wanted.mobile {
            // Zeros keep the real size and scale: only `mobile` is overridden.
            let metrics =
                json!({ "width": 0, "height": 0, "deviceScaleFactor": 0, "mobile": true });
            ("Emulation.setDeviceMetricsOverride", metrics)
        } else {
            ("Emulation.clearDeviceMetricsOverride", json!({}))
        });
    }
    if applied.touch != wanted.touch {
        let touch = if wanted.touch {
            json!({ "enabled": true, "maxTouchPoints": 5 })
        } else {
            json!({ "enabled": false })
        };
        commands.push(("Emulation.setTouchEmulationEnabled", touch));
        let mouse = if wanted.touch {
            json!({ "enabled": true, "configuration": "mobile" })
        } else {
            json!({ "enabled": false })
        };
        commands.push(("Emulation.setEmitTouchEventsForMouse", mouse));
    }
    if applied.agent != wanted.agent {
        let agent = match &wanted.agent {
            Some(agent) => {
                let mut params = json!({ "userAgent": agent.user_agent });
                if let Some(platform) = agent.platform {
                    params["platform"] = json!(platform);
                }
                if let Some(metadata) = &agent.metadata {
                    params["userAgentMetadata"] = metadata.clone();
                }
                params
            }
            // Empty is the engine's own again.
            None => json!({ "userAgent": "" }),
        };
        commands.push(("Emulation.setUserAgentOverride", agent));
    }
    commands
}

/// Where a page `width` × `height` shows at `fit` in `area`: as any content in responsive
/// mode, centred across it at its top (`crate::responsive::place`).
pub fn place(area: Bounds<Pixels>, width: u32, height: u32, fit: f32) -> Bounds<Pixels> {
    crate::responsive::place(area, width as f32 * fit, height as f32 * fit)
}

/// A point of the window as a point of the page shown at `origin` at `fit`, in the page's
/// own logical pixels.
pub fn to_page(position: Point<Pixels>, origin: Point<Pixels>, fit: f32) -> Point<Pixels> {
    let at = position - origin;
    point(at.x / fit, at.y / fit)
}

/// A point of the page shown at `origin` at `fit` as a point of the window.
pub fn to_window(at: Point<Pixels>, origin: Point<Pixels>, fit: f32) -> Point<Pixels> {
    origin + point(at.x * fit, at.y * fit)
}

/// The device pixels per logical pixel of a frame `size` device pixels, the page last told
/// `viewport`: its scale when the frame is of it; else, the frame being one painted before
/// the page was told, the scale of the frame before it (`previous`).
pub fn frame_scale(size: PixelSize, viewport: Option<Viewport>, previous: Option<f32>) -> f32 {
    let told = viewport.map(|viewport| viewport.scale);
    match viewport {
        Some(viewport) if size.near(viewport.pixels()) => viewport.scale,
        _ => previous.or(told).unwrap_or(1.),
    }
}

/// Whether a page that reports `inner` (`innerWidth`, `innerHeight`) has laid out at
/// `width` × `height` logical pixels, zoomed to `zoom` percent. A mobile page that sets no
/// `<meta viewport>` lays out wider, at the device's shape.
pub fn laid_out(width: u32, height: u32, zoom: u32, mobile: bool, inner: (f64, f64)) -> bool {
    let zoom = f64::from(zoom.max(1)) / 100.;
    let (wide, tall) = (f64::from(width) / zoom, f64::from(height) / zoom);
    let (inner_wide, inner_tall) = inner;
    let exact = (inner_wide - wide).abs() <= 1. && (inner_tall - tall).abs() <= 1.;
    let shaped =
        mobile && inner_wide > wide && (inner_tall * wide - inner_wide * tall).abs() <= inner_wide;
    exact || shaped
}

#[cfg(test)]
mod tests;
