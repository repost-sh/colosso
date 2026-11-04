//! The `AppKit` half of the native slider: an `NSSlider` over a few stops, its tick marks
//! under it.

use gpui_kit::App;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSSlider;

use super::host::{Shown, take_move};
use crate::native::{Control, slider};
use crate::settings_view::ticked_slider::Stops;

impl Control for Stops {
    type View = NSSlider;
    type Shown = Shown;

    /// A slider over the stops, numbered from 0 at the left.
    fn make(&self, main: MainThreadMarker) -> Retained<NSSlider> {
        slider::over_stops(self.values.len(), main)
    }

    /// Moves the thumb only when the setting has gone to another stop.
    fn show(view: &NSSlider, shown: Shown) {
        slider::show_stop(view, shown.at);
        if view.isEnabled() != shown.enabled {
            view.setEnabled(shown.enabled);
        }
    }

    fn acted(&self, view: &NSSlider, shown: Option<Shown>, cx: &mut App) {
        take_move(*self, shown, slider::stop(view), cx);
    }
}
