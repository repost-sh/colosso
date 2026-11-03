//! A browser page at a chosen size, or as a phone or tablet: responsive mode
//! (`docs/responsive.md` 4.1). Setting it acts on the page, under its lease; reading it is
//! `browser_list`'s `viewport`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ops::shared::PaneId;
use crate::{Hints, Operation, Scope};

/// A device preset, or `responsive` for a free size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DeviceName {
    /// A free width and height, desktop behaviour.
    Responsive,
    IphoneSe,
    #[serde(rename = "iphone-15")]
    Iphone15,
    #[serde(rename = "iphone-15-pro-max")]
    Iphone15ProMax,
    #[serde(rename = "pixel-8")]
    Pixel8,
    #[serde(rename = "galaxy-s24")]
    GalaxyS24,
    IpadMini,
    #[serde(rename = "ipad-pro-11")]
    IpadPro11,
}

impl DeviceName {
    /// Every name, `responsive` first, then the presets in the device menu's order.
    pub const ALL: [DeviceName; 8] = [
        DeviceName::Responsive,
        DeviceName::IphoneSe,
        DeviceName::Iphone15,
        DeviceName::Iphone15ProMax,
        DeviceName::Pixel8,
        DeviceName::GalaxyS24,
        DeviceName::IpadMini,
        DeviceName::IpadPro11,
    ];

    /// The name as tools write it: `iphone-15`.
    pub fn id(self) -> &'static str {
        match self {
            DeviceName::Responsive => "responsive",
            DeviceName::IphoneSe => "iphone-se",
            DeviceName::Iphone15 => "iphone-15",
            DeviceName::Iphone15ProMax => "iphone-15-pro-max",
            DeviceName::Pixel8 => "pixel-8",
            DeviceName::GalaxyS24 => "galaxy-s24",
            DeviceName::IpadMini => "ipad-mini",
            DeviceName::IpadPro11 => "ipad-pro-11",
        }
    }

    /// The name `id` writes, if it's one.
    pub fn from_id(id: &str) -> Option<DeviceName> {
        DeviceName::ALL.into_iter().find(|name| name.id() == id)
    }
}

/// Which way up the page is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    Portrait,
    Landscape,
}

/// Fit the page to the pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FitWord {
    Fit,
}

/// How large the page shows in the pane: `fit` (as large as fits, at most its true size),
/// or a percent of its true size: 50, 75 or 100.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ViewportZoom {
    Fit(FitWord),
    Percent(u32),
}

/// What the page is shown as.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ViewportMode {
    /// Responsive mode is off: the page fills its pane.
    Fill,
    /// A free width and height.
    Responsive,
    /// A device preset.
    Device,
}

/// The size and device a page is laid out at, and how it shows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Viewport {
    /// `fill` (off), `responsive` (a free size) or `device` (a preset).
    pub mode: ViewportMode,
    /// The preset, in `device` mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<DeviceName>,
    /// The page's width in CSS pixels.
    pub width: u32,
    /// The page's height in CSS pixels.
    pub height: u32,
    /// Device pixels per CSS pixel: the page's `devicePixelRatio`.
    pub scale: f64,
    /// Whether the page is laid out as on a phone: `<meta viewport>`, overlay scrollbars.
    pub mobile: bool,
    /// Whether the page has touch, the mouse sending touch events.
    pub touch: bool,
    /// The user agent the page is given, when it isn't the engine's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    /// `portrait` or `landscape`.
    pub orientation: Orientation,
    /// How large it shows: `fit`, or 50, 75 or 100 percent.
    pub zoom: ViewportZoom,
    /// How large it shows now, in percent of its true size.
    pub shown_percent: u32,
    /// Who set it: `person`, or the agent's name; absent in `fill` mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set_by: Option<String>,
}

impl Viewport {
    /// "iPhone 15, 393 × 852 at 3x, shown at 83%".
    pub fn summary(&self) -> String {
        let size = format!("{} × {}", self.width, self.height);
        match self.mode {
            ViewportMode::Fill => format!("Filling the pane, {size}"),
            ViewportMode::Responsive | ViewportMode::Device => format!(
                "{}, {size} at {}x, shown at {}%",
                self.device.map_or("Responsive", DeviceName::id),
                self.scale,
                self.shown_percent
            ),
        }
    }
}

/// The size or device to show a page at.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserViewportInput {
    /// The browser pane. Left out, the page you acted on last, else the shown tab's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// A preset (`iphone-se`, `iphone-15`, `iphone-15-pro-max`, `pixel-8`, `galaxy-s24`,
    /// `ipad-mini`, `ipad-pro-11`), which sets its size, scale, user agent, touch and mobile
    /// layout; or `responsive` for a free size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<DeviceName>,
    /// A free width in CSS pixels, from 50; alone it means `responsive` at that width.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    /// A free height in CSS pixels, from 50; alone it means `responsive` at that height.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// Device pixels per CSS pixel for a free size, 0.5 to 5; the window's when left out.
    /// Not with a preset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    /// Lay a free size out as a phone does (`<meta viewport>`). Not with a preset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mobile: Option<bool>,
    /// Give a free size touch, the mouse sending touch events. Not with a preset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub touch: Option<bool>,
    /// A user agent for a free size. Not with a preset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    /// `portrait` or `landscape`: turns a preset (or a free size) that way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orientation: Option<Orientation>,
    /// How large the page shows in the pane: `fit`, or 50, 75 or 100 percent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<ViewportZoom>,
    /// `true` ends responsive mode, the page filling its pane again; nothing else with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<bool>,
    /// Add a screenshot of the page at its new size, as `browser_screenshot` gives one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<bool>,
}

/// The page's viewport after the change, and whether the page has laid out at it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BrowserViewportOutput {
    /// The viewport the page is at now.
    pub viewport: Viewport,
    /// Whether the page reported the new size and showed a frame of it within 5 seconds.
    pub settled: bool,
    /// With `screenshot`, the image's MIME type: `image/png`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// With `screenshot`, the image, base64-encoded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
}

/// `browser_viewport`: responsive mode.
#[derive(Debug)]
pub struct BrowserViewport;

impl Operation for BrowserViewport {
    const NAME: &'static str = "browser_viewport";
    const TITLE: &'static str = "Set a page's viewport";
    const SUBJECT: &'static [&'static str] = &["device", "width", "height"];
    const SCOPE: Scope = Scope::Browser;
    const DESCRIPTION: &'static str = leased!(
        "Shows a browser page at a phone's or tablet's size (`device`, with its user agent, touch and mobile layout) or at a free `width` × `height`, scaled down to fit the pane, as Chrome's device toolbar does; `fill: true` ends it. ",
        "Answers once the page has laid out at the new size (`settled`, 5 seconds at most); `screenshot: true` adds an image of it. ",
        "To check a layout: set the size, look, then `fill: true`. The person sees what you do, and their own change wins."
    );
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = BrowserViewportInput;
    type Output = BrowserViewportOutput;

    fn summary(output: &BrowserViewportOutput) -> String {
        let settled = if output.settled {
            ""
        } else {
            "; the page hadn't laid out at it within 5 seconds"
        };
        format!("{}{settled}", output.viewport.summary())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn device_names_and_zooms_read_as_the_tools_write_them() {
        for name in DeviceName::ALL {
            assert_eq!(serde_json::to_value(name).unwrap(), json!(name.id()));
            assert_eq!(DeviceName::from_id(name.id()), Some(name));
        }
        assert_eq!(
            serde_json::from_value::<ViewportZoom>(json!("fit")).unwrap(),
            ViewportZoom::Fit(FitWord::Fit)
        );
        assert_eq!(
            serde_json::from_value::<ViewportZoom>(json!(75)).unwrap(),
            ViewportZoom::Percent(75)
        );
        assert!(serde_json::from_value::<ViewportZoom>(json!("half")).is_err());
    }

    #[test]
    fn a_screenshot_rides_along_as_browser_screenshot_gives_one() {
        let output = BrowserViewportOutput {
            viewport: Viewport {
                mode: ViewportMode::Device,
                device: Some(DeviceName::Iphone15),
                width: 393,
                height: 852,
                scale: 3.,
                mobile: true,
                touch: true,
                user_agent: Some("Mozilla/5.0 (iPhone…)".into()),
                orientation: Orientation::Portrait,
                zoom: ViewportZoom::Fit(FitWord::Fit),
                shown_percent: 83,
                set_by: Some("claude-code:7f3a".into()),
            },
            settled: true,
            mime_type: Some("image/png".into()),
            data: Some("iVBORw0KGgo=".into()),
        };
        let value = serde_json::to_value(&output).unwrap();
        // A relay makes an image block of the top-level `mimeType` and `data`.
        assert_eq!(value["mimeType"], "image/png");
        assert_eq!(value["data"], "iVBORw0KGgo=");
        assert_eq!(value["viewport"]["device"], "iphone-15");
        assert_eq!(value["viewport"]["zoom"], "fit");
        assert_eq!(value["viewport"]["setBy"], "claude-code:7f3a");
        assert_eq!(
            BrowserViewport::summary(&output),
            "iphone-15, 393 × 852 at 3x, shown at 83%"
        );
        let unsettled = BrowserViewportOutput {
            settled: false,
            mime_type: None,
            data: None,
            ..output
        };
        let value = serde_json::to_value(&unsettled).unwrap();
        assert!(value.get("mimeType").is_none() && value.get("data").is_none());
        assert!(BrowserViewport::summary(&unsettled).ends_with("within 5 seconds"));
    }
}
