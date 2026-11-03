//! A free page's grips (`crate::responsive::grips`): they size the page in logical pixels,
//! within the limits at its scale, and a double click fits a side to the pane. The pointer
//! over the page's native view goes to a grip in hand rather than the page.

use std::ops::RangeInclusive;

use gpui_kit::{App, Context, PlatformInput};

use super::devices;
use crate::browser::view::BrowserView;
use crate::responsive::{Drag, Resizable};

impl Resizable for BrowserView {
    fn drag(&mut self) -> &mut Option<Drag> {
        &mut self.responsive.drag
    }

    fn grip_size(&self, _: &App) -> Option<((u32, u32), (f32, f32))> {
        let mode = (self.responsive.mode.as_ref()).filter(|mode| mode.device.is_none())?;
        let fit = self.responsive.fit;
        Some(((mode.width, mode.height), (fit, fit)))
    }

    fn side_limits(&self, _: &App) -> RangeInclusive<u32> {
        let window = self.window_scale();
        let scale = (self.responsive.mode.as_ref()).map_or(window, |mode| mode.scale_at(window));
        devices::MIN_SIDE..=devices::max_side(scale)
    }

    fn filling_size(&self, _: &App) -> Option<(u32, u32)> {
        Some(self.pane_size())
    }

    fn grip_sized(&mut self, width: u32, height: u32, cx: &mut Context<Self>) {
        self.person_sizes(width, height, cx);
    }
}

impl BrowserView {
    /// What the pointer does over the page's native view while a grip is in hand, which
    /// goes to the grip rather than the page: whether it did.
    pub(in crate::browser) fn grip_pointer(
        &mut self,
        event: &PlatformInput,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.responsive.drag.is_none() {
            return false;
        }
        match event {
            PlatformInput::MouseMove(event) => self.drag_grip(event, cx),
            PlatformInput::MouseUp(_) => self.let_go_of_grip(cx),
            _ => {}
        }
        true
    }
}
