//! How far in what's in a sidebar's row starts from the row it's under (`docs/layout.md`,
//! Settings).

use super::Settings;

/// The stops of the Sidebar indentation slider (Settings → Appearance), in points.
pub const INDENT_STOPS: [f32; 5] = [0., 8., 16., 24., 32.];
/// The indentation at rest, in points.
pub const INDENT: f32 = 16.;

impl Settings {
    /// How far in what's in a branch's row starts from its project's, and a tab's from its
    /// branch's, in points: as the person set it, between the slider's ends.
    pub fn sidebar_step(&self) -> f32 {
        self.sidebar_indent.clamp(INDENT_STOPS[0], INDENT_STOPS[INDENT_STOPS.len() - 1])
    }
}

#[cfg(test)]
mod tests {
    use crate::settings::Settings;

    #[test]
    fn the_sidebars_step_is_as_set_between_the_sliders_ends() {
        for (set, step) in [(0., 0_f32), (10., 10.), (16., 16.), (-4., 0.), (900., 32.)] {
            let settings = Settings { sidebar_indent: set, ..Settings::default() };
            assert_eq!(settings.sidebar_step().to_bits(), step.to_bits(), "{set}");
        }
        assert_eq!(Settings::default().sidebar_step().to_bits(), 16_f32.to_bits());
    }
}
