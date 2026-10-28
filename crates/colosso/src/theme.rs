//! Shell sizes and colors. Dark only for now: the glass rails read poorly over light desktops.

use gpui_kit::{Hsla, hsla};

pub const TITLE_BAR_HEIGHT: f32 = 40.;
pub const TRAFFIC_LIGHT_SIZE: f32 = 14.;
/// Room left of the top-rail buttons for the traffic lights.
pub const TRAFFIC_LIGHTS_WIDTH: f32 = 78.;
pub const ICON_RAIL_WIDTH: f32 = 48.;
pub const GUTTER: f32 = 8.;
pub const CARD_RADIUS: f32 = 10.;
pub const SIDEBAR_WIDTH: f32 = 260.;
pub const TAB_STRIP_HEIGHT: f32 = 38.;
pub const ROW_HEIGHT: f32 = 30.;

/// The window's blur shows through this, so its alpha sets how glassy the rails look.
pub fn rail_tint() -> Hsla {
    hsla(0., 0., 0.08, 0.55)
}

pub fn card() -> Hsla {
    hsla(0., 0., 0.11, 1.)
}

pub fn sunken() -> Hsla {
    hsla(0., 0., 0.085, 1.)
}

pub fn raised() -> Hsla {
    hsla(0., 0., 0.15, 1.)
}

pub fn hairline() -> Hsla {
    hsla(0., 0., 1., 0.08)
}

pub fn text() -> Hsla {
    hsla(0., 0., 1., 0.9)
}

pub fn muted() -> Hsla {
    hsla(0., 0., 1., 0.5)
}

pub fn hover() -> Hsla {
    hsla(0., 0., 1., 0.05)
}

pub fn selected() -> Hsla {
    hsla(0., 0., 1., 0.1)
}
