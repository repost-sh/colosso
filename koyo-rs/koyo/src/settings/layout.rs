//! How much of the window's glass the sidebar lets through (`docs/layout.md`, Settings).

use super::Settings;

/// The stops of the Sidebar glass slider (Settings → Appearance), from clear through the card's
/// own at the middle to solid.
pub const GLASS_STOPS: [f32; 5] = [0., 0.25, 0.5, 0.75, 1.];
/// Where along the slider the card's own is.
pub const GLASS_OWN: f32 = 0.5;
/// Where the glass rests: clear, the sidebar on the window's glass, apart from the islands beside
/// it, which are on the card's own.
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
}

#[cfg(test)]
mod tests {
    use super::GLASS_STOPS;
    use crate::settings::Settings;

    fn set(sidebar_glass: f32) -> Settings {
        Settings {
            sidebar_glass,
            ..Settings::default()
        }
    }

    #[test]
    fn the_sidebar_is_clear_by_default_and_at_the_left_the_cards_own_at_the_middle_and_solid() {
        for own in [0.6, 0.65] {
            assert!(Settings::default().sidebar_opacity(own).abs() < 1e-6);
            assert!(set(0.).sidebar_opacity(own).abs() < 1e-6);
            assert!((set(0.5).sidebar_opacity(own) - own).abs() < 1e-6);
            assert!((set(1.).sidebar_opacity(own) - 1.).abs() < 1e-6);
            let stops = GLASS_STOPS.map(|at| set(at).sidebar_opacity(own));
            assert!(stops.windows(2).all(|pair| pair[0] < pair[1]), "{stops:?}");
        }
    }
}
