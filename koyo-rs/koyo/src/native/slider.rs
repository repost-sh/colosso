//! An `NSSlider` over a few stops, numbered from 0 at the left, with its tick marks under it:
//! macOS gives it its thumb, Liquid Glass while it's held on macOS 26.

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSSlider, NSTickMarkPosition};

/// A slider over `stops`, which sends each move as it happens, the thumb still held.
pub fn over_stops(stops: usize, main: MainThreadMarker) -> Retained<NSSlider> {
    let slider = NSSlider::new(main);
    slider.setMinValue(0.);
    slider.setMaxValue(stops.saturating_sub(1) as f64);
    slider.setNumberOfTickMarks(stops as isize);
    slider.setTickMarkPosition(NSTickMarkPosition::Below);
    slider.setAllowsTickMarkValuesOnly(true);
    slider.setContinuous(true);
    slider
}

/// The stop the thumb is at.
pub fn stop(slider: &NSSlider) -> usize {
    slider.doubleValue().round().max(0.) as usize
}

/// Moves the thumb only if it's not at `at` already: the move it's reporting, it may still be held.
pub fn show_stop(slider: &NSSlider, at: usize) {
    if stop(slider) != at {
        slider.setDoubleValue(at as f64);
    }
}
