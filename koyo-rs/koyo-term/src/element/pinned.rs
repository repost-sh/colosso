//! A pinned grid in the element (`docs/responsive.md` 3.2): its font fitted to the bounds the
//! pane gives it, or a share of the person's, and the grid centred across them at their top.
//! The app draws what's round it.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::{App, Bounds, Pixels, Window, px};

use super::{Grid, TerminalElement};
use crate::fit::{self, Layout, Scale};
use crate::glyphs::Look;
use crate::view::TerminalView;

/// The look a pinned grid was last drawn in at a font smaller than the person's, and the
/// person's look it was made from (kept, so it's that very one).
pub type PinnedLook = Rc<RefCell<Option<(Rc<Look>, Rc<Look>)>>>;

impl TerminalElement {
    /// Fits `cells` to `bounds` at the view's scale and lays them out: the grid, the look its
    /// cells are drawn in, and where it's drawn.
    pub(super) fn lay_out_pinned(
        &mut self,
        bounds: Bounds<Pixels>,
        cells: (u16, u16),
        window: &mut Window,
        cx: &mut App,
    ) -> (Grid, Rc<Look>, Bounds<Pixels>) {
        let person = f32::from(self.look.font_size);
        let font = match self.scale {
            Scale::Percent(percent) => fit::font_at(percent, person),
            Scale::Held => (self.fitting.get().font())
                .unwrap_or_else(|| self.fit(bounds, cells, window, cx)),
            Scale::Fit => self.fit(bounds, cells, window, cx),
        };
        let look = if font.to_bits() == person.to_bits() {
            Rc::clone(&self.look)
        } else {
            self.look_at(font)
        };
        let metrics = self.glyphs.borrow_mut().prepare(&look, window);
        let scale = window.scale_factor();
        let origin = fit::origin(bounds, cells, metrics, scale);
        let grid = Grid {
            origin,
            metrics,
            cols: cells.0,
            rows: cells.1,
            scale,
        };
        (grid, look, fit::extent(origin, cells, metrics))
    }

    /// The person's look at `font`, made once and drawn in by every frame at that font.
    fn look_at(&self, font: f32) -> Rc<Look> {
        let mut kept = self.pinned_look.borrow_mut();
        if let Some((from, look)) = kept.as_ref()
            && Rc::ptr_eq(from, &self.look)
            && f32::from(look.font_size).to_bits() == font.to_bits()
        {
            return Rc::clone(look);
        }
        let look = Rc::new(Look {
            font_size: px(font),
            ..(*self.look).clone()
        });
        *kept = Some((Rc::clone(&self.look), Rc::clone(&look)));
        look
    }

    /// The font that fits `cells` in `bounds`, the bar told when its percent changed.
    fn fit(
        &mut self,
        bounds: Bounds<Pixels>,
        cells: (u16, u16),
        window: &mut Window,
        cx: &mut App,
    ) -> f32 {
        let layout = Layout {
            room: (
                f32::from(bounds.size.width).max(1.),
                f32::from(bounds.size.height).max(1.),
            ),
            cells,
            person: f32::from(self.look.font_size),
        };
        let mut fitting = self.fitting.get();
        let shown = fitting.percent();
        let font = {
            let mut glyphs = self.glyphs.borrow_mut();
            fitting.fit(layout, |font| {
                let metrics = glyphs.metrics_at(&self.look, px(font), window);
                (
                    f32::from(metrics.cell_width) * f32::from(cells.0),
                    f32::from(metrics.cell_height) * f32::from(cells.1),
                )
            })
        };
        self.fitting.set(fitting);
        if fitting.percent() != shown {
            self.tell_fitted(cx);
        }
        font
    }

    /// No pin: the fitted font is forgotten, and the bar told when it showed a percent.
    pub(super) fn unfit(&self, cx: &mut App) {
        let mut fitting = self.fitting.get();
        if fitting.font().is_none() {
            return;
        }
        fitting.reset();
        self.fitting.set(fitting);
        self.tell_fitted(cx);
    }

    fn tell_fitted(&self, cx: &mut App) {
        let view = self.view.clone();
        // Not while drawing: the view tells whoever shows its percent.
        cx.defer(move |cx| view.update(cx, TerminalView::fitted));
    }
}
