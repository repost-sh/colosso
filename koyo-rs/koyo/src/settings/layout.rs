//! The window's layout, Orca's or Arc's, the menu that switches it, and, in Orca's, how much of
//! the window's glass its one card lets through and how blurred that glass is.

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

/// The stops of the Orca layout's Card glass and Glass blur sliders (Settings → Appearance),
/// whose middle is the theme's own card and macOS's own blur.
pub const GLASS_STOPS: [f32; 5] = [0., 0.25, 0.5, 0.75, 1.];
/// Where along either slider the theme's or macOS's own is.
pub const GLASS_OWN: f32 = 0.5;
/// How opaque the card is at the Card glass slider's clear end.
const CLEAREST: f32 = 0.2;

impl Settings {
    /// How opaque the card is where the theme has it `own`: in the Orca layout as the person set
    /// it, from the clearest through the theme's own to solid; the theme's own in Arc's.
    pub fn card_opacity(&self, own: f32) -> f32 {
        let at = match self.layout {
            Layout::Orca => self.card_glass.clamp(0., 1.),
            Layout::Arc => GLASS_OWN,
        };
        if at <= GLASS_OWN {
            CLEAREST + (own - CLEAREST) * at / GLASS_OWN
        } else {
            own + (1. - own) * (at - GLASS_OWN) / GLASS_OWN
        }
    }

    /// How many times as strongly as its material the window's glass blurs: in the Orca layout
    /// as the person set it, from none through macOS's own to twice it; macOS's own in Arc's.
    pub fn blur_scale(&self) -> f64 {
        let at = match self.layout {
            Layout::Orca => self.glass_blur.clamp(0., 1.),
            Layout::Arc => GLASS_OWN,
        };
        f64::from(at / GLASS_OWN)
    }
}

#[cfg(test)]
mod tests {
    use super::{GLASS_STOPS, Layout};
    use crate::settings::Settings;

    fn orca(card_glass: f32, glass_blur: f32) -> Settings {
        Settings {
            layout: Layout::Orca,
            card_glass,
            glass_blur,
            ..Settings::default()
        }
    }

    #[test]
    #[expect(clippy::float_cmp, reason = "the stops' scales are exact in binary")]
    fn the_blur_is_none_at_the_left_macos_own_at_the_middle_and_twice_at_the_right() {
        let scales = GLASS_STOPS.map(|at| orca(0.5, at).blur_scale());
        assert_eq!(scales, [0., 0.5, 1., 1.5, 2.]);
        let arc = Settings {
            layout: Layout::Arc,
            ..orca(0.5, 0.)
        };
        assert_eq!(arc.blur_scale(), 1., "Arc keeps macOS's own");
    }

    #[test]
    fn the_card_is_the_themes_own_at_the_middle_the_clearest_and_solid_at_the_ends() {
        for own in [0.6, 0.65] {
            assert!((orca(0.5, 0.5).card_opacity(own) - own).abs() < 1e-6);
            assert!((orca(1., 0.5).card_opacity(own) - 1.).abs() < 1e-6);
            assert!((orca(0., 0.5).card_opacity(own) - 0.2).abs() < 1e-6);
            let stops = GLASS_STOPS.map(|at| orca(at, 0.5).card_opacity(own));
            assert!(stops.windows(2).all(|pair| pair[0] < pair[1]), "{stops:?}");
        }
    }
}
