//! The window's layout, Orca's or Arc's, the menu that switches it, and how much of the window's
//! glass the sidebar lets through and how blurred the desktop behind it is, in either layout.

use gpui_kit::Action;
use gpui_kit::component::native_menu::NativeMenu;
use serde::{Deserialize, Serialize};

use super::{Settings, checked_menu};

/// The window's layout: Orca's, which lists every project, or Arc's sidebar
/// (`docs/arc-sidebar.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Layout {
    #[default]
    Orca,
    Arc,
}

/// Switches the window to a layout, from a menu or a key.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetLayout(pub Layout);

impl Layout {
    pub const ALL: [Layout; 2] = [Layout::Orca, Layout::Arc];

    pub fn label(self) -> &'static str {
        match self {
            Layout::Orca => "Orca",
            Layout::Arc => "Arc",
        }
    }

    pub fn other(self) -> Self {
        match self {
            Layout::Orca => Layout::Arc,
            Layout::Arc => Layout::Orca,
        }
    }

    /// A menu of the layouts with `self` checked.
    pub fn menu(self) -> NativeMenu {
        checked_menu(Layout::ALL, self, Layout::label, |layout| {
            Box::new(SetLayout(layout))
        })
    }
}

/// The stops of the Sidebar glass and Sidebar blur sliders (Settings → Appearance): the glass
/// from clear through the card's own at the middle to solid, the blur from none through macOS's
/// own at the middle to twice it.
pub const GLASS_STOPS: [f32; 5] = [0., 0.25, 0.5, 0.75, 1.];
/// Where along either slider the card's own or macOS's own is: where the blur rests.
pub const GLASS_OWN: f32 = 0.5;
/// Where the glass rests: clear, the sidebar on the window's glass as Arc's has always been, and
/// in Orca's card apart from the content beside it, which is on the card's own.
pub const GLASS_CLEAR: f32 = 0.;

impl Settings {
    /// How opaque the sidebar's fill of the card's colour is where the theme has the card `own`,
    /// as the person set it: from none through the card's own to solid.
    pub fn sidebar_opacity(&self, own: f32) -> f32 {
        let at = self.sidebar_glass.clamp(0., 1.);
        if at <= GLASS_OWN {
            own * at / GLASS_OWN
        } else {
            own + (1. - own) * (at - GLASS_OWN) / GLASS_OWN
        }
    }

    /// How many times as strongly as its material the sidebar's glass blurs, as the person set
    /// it: from none through macOS's own to twice it. None at macOS's own, where the sidebar
    /// blurs as the rest of the window does, by the window's own blur.
    pub fn sidebar_blur_scale(&self) -> Option<f64> {
        let scale = f64::from(self.sidebar_blur.clamp(0., 1.) / GLASS_OWN);
        Some(scale).filter(|scale| (scale - 1.).abs() > f64::EPSILON)
    }
}

#[cfg(test)]
mod tests {
    use super::GLASS_STOPS;
    use crate::settings::Settings;

    fn set(sidebar_glass: f32, sidebar_blur: f32) -> Settings {
        Settings {
            sidebar_glass,
            sidebar_blur,
            ..Settings::default()
        }
    }

    #[test]
    fn the_blur_is_none_at_the_left_the_windows_own_at_the_middle_and_twice_at_the_right() {
        let scales = GLASS_STOPS.map(|at| set(0.5, at).sidebar_blur_scale());
        assert_eq!(scales, [Some(0.), Some(0.5), None, Some(1.5), Some(2.)]);
        assert_eq!(Settings::default().sidebar_blur_scale(), None, "the window's own by default");
    }

    #[test]
    fn the_sidebar_is_clear_by_default_and_at_the_left_the_cards_own_at_the_middle_and_solid() {
        for own in [0.6, 0.65] {
            assert!(Settings::default().sidebar_opacity(own).abs() < 1e-6);
            assert!(set(0., 0.5).sidebar_opacity(own).abs() < 1e-6);
            assert!((set(0.5, 0.5).sidebar_opacity(own) - own).abs() < 1e-6);
            assert!((set(1., 0.5).sidebar_opacity(own) - 1.).abs() < 1e-6);
            let stops = GLASS_STOPS.map(|at| set(at, 0.5).sidebar_opacity(own));
            assert!(stops.windows(2).all(|pair| pair[0] < pair[1]), "{stops:?}");
        }
    }
}
