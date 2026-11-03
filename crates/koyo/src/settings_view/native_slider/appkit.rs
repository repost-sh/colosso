//! The `AppKit` half of the native slider: an `NSSlider` over a few stops, its tick marks
//! under it.

use gpui_kit::App;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSSlider, NSTickMarkPosition};

use super::host::{Shown, take_move};
use crate::native::Control;
use crate::settings_view::ticked_slider::Stops;

impl Control for Stops {
    type View = NSSlider;
    type Shown = Shown;

    /// A slider over the stops, numbered from 0 at the left.
    fn make(&self, main: MainThreadMarker) -> Retained<NSSlider> {
        let stops = self.values.len();
        let slider = NSSlider::new(main);
        slider.setMinValue(0.);
        slider.setMaxValue(stops.saturating_sub(1) as f64);
        slider.setNumberOfTickMarks(stops as isize);
        slider.setTickMarkPosition(NSTickMarkPosition::Below);
        slider.setAllowsTickMarkValuesOnly(true);
        // AppKit sends each move as it happens, the thumb still held.
        slider.setContinuous(true);
        slider
    }

    /// Moves the thumb only when the setting has gone to another stop.
    fn show(slider: &NSSlider, shown: Shown) {
        if at(slider) != shown.at {
            slider.setDoubleValue(shown.at as f64);
        }
        if slider.isEnabled() != shown.enabled {
            slider.setEnabled(shown.enabled);
        }
    }

    fn acted(&self, slider: &NSSlider, shown: Option<Shown>, cx: &mut App) {
        take_move(*self, shown, at(slider), cx);
    }
}

/// The stop the thumb is at.
fn at(slider: &NSSlider) -> usize {
    slider.doubleValue().round().max(0.) as usize
}
