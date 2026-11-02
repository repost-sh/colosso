//! A terminal pinned at a size of the person's choosing (`docs/responsive.md` 3.2, 3.3): the
//! pin as the daemon says it, pins asked for as a grip is dragged (one at a time: the latest
//! once the daemon has answered the last), the font the pinned grid is drawn at, and where it was drawn. The pin
//! is the daemon's, so every view of the terminal shows the same one; what's drawn round it
//! (the bar, the grips) is the app's.

use colosso_term_control::{Pinned, RequestBody, Size, clamp_pin_cells};
use gpui_kit::{Bounds, Context, Pixels};

use super::{TerminalEvent, TerminalView};
use crate::fit::{self, Scale};

/// What the daemon is to be asked next: a pin at these cells, or none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Wanted {
    At(u16, u16),
    Fill,
}

impl TerminalView {
    /// The pin, while there is one.
    pub fn pinned(&self) -> Option<&Pinned> {
        self.pinned.as_ref()
    }

    /// Where the pinned grid was last drawn, and the size of its cells there.
    pub fn pinned_grid(&self) -> Option<(Bounds<Pixels>, (f32, f32))> {
        self.pinned.as_ref()?;
        let grid = self.grid.get()?;
        let cell = (
            f32::from(grid.metrics.cell_width),
            f32::from(grid.metrics.cell_height),
        );
        let drawn = fit::extent(grid.origin, (grid.cols, grid.rows), grid.metrics);
        Some((drawn, cell))
    }

    /// The terminal's size in the daemon, once it's said.
    pub fn cells(&self) -> Option<(u16, u16)> {
        self.size.map(|size| (size.cols, size.rows))
    }

    /// The bounds the terminal was last laid out in.
    pub fn laid_out_in(&self) -> Option<Bounds<Pixels>> {
        self.pane.get()
    }

    /// The font the pinned grid was last fitted at, as a share of the person's: 100 when it
    /// wasn't fitted smaller.
    pub fn font_percent(&self) -> u16 {
        self.fitting.get().percent()
    }

    /// The most cells a room `width` × `height` holds at the person's font, once the
    /// terminal has been laid out once to know its cells.
    pub fn cells_in(&self, width: Pixels, height: Pixels) -> Option<(u16, u16)> {
        let fill = self.fill.get()?;
        let room = (f32::from(width).max(0.), f32::from(height).max(0.));
        Some(fit::cells_in(room, fill.metrics))
    }

    /// How large the pinned grid's font is to be.
    pub fn set_scale(&mut self, scale: Scale, cx: &mut Context<Self>) {
        if self.scale != scale {
            self.scale = scale;
            cx.notify();
        }
    }

    /// Pins the terminal at `cols` × `rows`, kept within the daemon's limits: the cells it
    /// asks for.
    pub fn pin(&mut self, cols: u16, rows: u16, cx: &mut Context<Self>) -> (u16, u16) {
        let (cols, rows) = clamp_pin_cells(cols, rows);
        self.send_pin(Wanted::At(cols, rows), cx);
        (cols, rows)
    }

    /// Unpins it: the pane's size is its size again, whoever pinned it.
    pub fn unpin(&mut self, cx: &mut Context<Self>) {
        self.send_pin(Wanted::Fill, cx);
    }

    /// The daemon said the terminal's size, and the pin it comes from.
    pub(crate) fn sized(&mut self, size: Size, pinned: Option<Pinned>, cx: &mut Context<Self>) {
        self.size = Some(size);
        if self.pinned != pinned {
            self.pinned = pinned;
            cx.emit(TerminalEvent::Responsive);
        }
        cx.notify();
    }

    /// The element fitted the font anew, or forgot it.
    pub(crate) fn fitted(&mut self, cx: &mut Context<Self>) {
        cx.emit(TerminalEvent::Responsive);
    }

    /// Sends `wanted` to the daemon now, or, while another pin is being sent, once that's
    /// answered: whichever is the latest then.
    fn send_pin(&mut self, wanted: Wanted, cx: &mut Context<Self>) {
        let request = |_: &mut Self, handle, wanted| {
            let size = match wanted {
                Wanted::At(cols, rows) => Some(Size::cells(cols, rows)),
                Wanted::Fill => None,
            };
            Some(RequestBody::Pin { handle, size })
        };
        self.send_latest(wanted, |this| &mut this.pin, request, "size", cx);
    }
}
