//! The element a terminal paints itself with (`docs/terminals.md` 7.2, step 3): where the
//! grid sits, the mouse and the input method wired to the view, and the order things are
//! painted in: the frame's backgrounds in runs, the selection, every cell's glyphs, underlines
//! and strikes, then the cursor, and an input method's composition over it. How each layer
//! is drawn is `paint`'s.

use std::cell::{Cell as Shared, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use colosso_term_engine::{CursorShape, Frame, Geometry, GridPoint, Width};
use gpui_kit::{
    App, BorderStyle, Bounds, ContentMask, CursorStyle, DispatchPhase, Element, ElementId,
    ElementInputHandler, Entity, FocusHandle, GlobalElementId, Hitbox, HitboxBehavior,
    InspectorElementId, IntoElement, LayoutId, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, ScrollEvent, SharedString, Style, Window, fill, outline, point, px, relative,
    size,
};

use crate::glyphs::{Glyphs, Look, Metrics};
use crate::paint::{Painter, hsla};
use crate::view::TerminalView;

/// Where the grid sits and how big its cells are, as last laid out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub origin: Point<Pixels>,
    pub metrics: Metrics,
    pub cols: u16,
    pub rows: u16,
    pub scale: f32,
}

impl Grid {
    /// The cell under `position`, clamped to the grid.
    pub fn cell_at(&self, position: Point<Pixels>) -> GridPoint {
        let x = f32::from(position.x - self.origin.x) / f32::from(self.metrics.cell_width);
        let y = f32::from(position.y - self.origin.y) / f32::from(self.metrics.cell_height);
        GridPoint {
            col: (x.max(0.) as u16).min(self.cols.saturating_sub(1)),
            row: (y.max(0.) as u16).min(self.rows.saturating_sub(1)),
        }
    }

    /// `position` in the grid's device pixels, as the mouse encoder reads it.
    pub fn device(&self, position: Point<Pixels>) -> (f32, f32) {
        (
            f32::from(position.x - self.origin.x) * self.scale,
            f32::from(position.y - self.origin.y) * self.scale,
        )
    }

    pub fn geometry(&self) -> Geometry {
        let cell_width = (f32::from(self.metrics.cell_width) * self.scale).round() as u32;
        let cell_height = (f32::from(self.metrics.cell_height) * self.scale).round() as u32;
        Geometry {
            width: cell_width * u32::from(self.cols),
            height: cell_height * u32::from(self.rows),
            cell_width: cell_width.max(1),
            cell_height: cell_height.max(1),
        }
    }
}

pub struct TerminalElement {
    pub view: Entity<TerminalView>,
    pub frame: Option<Arc<Frame>>,
    pub look: Rc<Look>,
    pub glyphs: Rc<RefCell<Glyphs>>,
    pub grid: Rc<Shared<Option<Grid>>>,
    pub focus: FocusHandle,
    pub focused: bool,
    /// The cursor's blink phase: `false` while a blinking cursor is off.
    pub cursor_on: bool,
    /// What an input method is composing, shown at the cursor.
    pub marked: Option<SharedString>,
}

impl IntoElement for TerminalElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

pub struct Prepainted {
    grid: Grid,
    hitbox: Hitbox,
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepainted;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Prepainted {
        let metrics = self.glyphs.borrow_mut().prepare(&self.look, window);
        let padding = self.look.padding;
        let (width, height) = (
            bounds.size.width - padding * 2.,
            bounds.size.height - padding * 2.,
        );
        let cols = (f32::from(width) / f32::from(metrics.cell_width))
            .floor()
            .max(1.) as u16;
        let rows = (f32::from(height) / f32::from(metrics.cell_height))
            .floor()
            .max(1.) as u16;
        let grid = Grid {
            origin: bounds.origin + point(padding, padding),
            metrics,
            cols,
            rows,
            scale: window.scale_factor(),
        };
        let before = self.grid.replace(Some(grid));
        let resized = before.is_none_or(|before| {
            (before.cols, before.rows, before.metrics) != (cols, rows, metrics)
        });
        if resized {
            let view = self.view.clone();
            // Not while drawing: the view sends the daemon its new size.
            cx.defer(move |cx| view.update(cx, |view, cx| view.grid_changed(grid, cx)));
        }
        Prepainted {
            grid,
            hitbox: window.insert_hitbox(bounds, HitboxBehavior::Normal),
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut (),
        prepainted: &mut Prepainted,
        window: &mut Window,
        cx: &mut App,
    ) {
        let tracking = self
            .frame
            .as_ref()
            .is_some_and(|frame| frame.mouse_tracking);
        window.set_cursor_style(
            if tracking {
                CursorStyle::Arrow
            } else {
                CursorStyle::IBeam
            },
            &prepainted.hitbox,
        );
        window.handle_input(
            &self.focus,
            ElementInputHandler::new(bounds, self.view.clone()),
            cx,
        );
        self.listen(prepainted, window);
        let background = self
            .frame
            .as_ref()
            .map_or(self.look.background, |frame| frame.background);
        window.paint_quad(fill(bounds, hsla(background)));
        let Some(frame) = self.frame.clone() else {
            return;
        };
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            let painter = Painter {
                frame: &frame,
                grid: prepainted.grid,
                look: &self.look,
                glyphs: &self.glyphs,
            };
            painter.backgrounds(window);
            painter.text(window);
            self.paint_cursor(&painter, window, cx);
        });
    }
}

impl TerminalElement {
    /// The mouse goes to the view: presses over the grid, and moves and releases anywhere,
    /// so a drag that leaves the grid keeps selecting.
    fn listen(&self, prepainted: &Prepainted, window: &mut Window) {
        let grid = prepainted.grid;
        let (view, hitbox) = (self.view.clone(), prepainted.hitbox.clone());
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                view.update(cx, |view, cx| view.mouse_down(event, grid, window, cx));
                cx.stop_propagation();
            }
        });
        let (view, hitbox) = (self.view.clone(), prepainted.hitbox.clone());
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble {
                let hovered = hitbox.is_hovered(window);
                view.update(cx, |view, cx| view.mouse_move(event, grid, hovered, cx));
            }
        });
        let view = self.view.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble {
                view.update(cx, |view, cx| view.mouse_up(event, grid, cx));
            }
        });
        let (view, hitbox) = (self.view.clone(), prepainted.hitbox.clone());
        window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && hitbox.should_handle_scroll(window) {
                view.update(cx, |view, cx| view.scroll_wheel(event, grid, cx));
                cx.stop_propagation();
            }
        });
    }

    fn paint_cursor(&self, painter: &Painter<'_>, window: &mut Window, cx: &mut App) {
        let Some(cursor) = painter.frame.cursor else {
            return;
        };
        let grid = painter.grid;
        let (cw, ch) = (grid.metrics.cell_width, grid.metrics.cell_height);
        let row = painter.frame.rows.get(usize::from(cursor.row));
        let cell = row.and_then(|row| row.cells.get(usize::from(cursor.col)));
        let wide = cell.is_some_and(|cell| cell.width == Width::Wide);
        let origin = grid.origin + point(cw * f32::from(cursor.col), ch * f32::from(cursor.row));
        let area = Bounds::new(origin, size(if wide { cw * 2. } else { cw }, ch));
        if let Some(marked) = &self.marked {
            painter.composition(marked, area, window, cx);
            return;
        }
        if !cursor.visible() {
            return;
        }
        let color = painter.frame.cursor_color.map_or(self.look.cursor, hsla);
        if !self.focused {
            window.paint_quad(outline(area, color, BorderStyle::Solid));
            return;
        }
        if cursor.blinking() && !self.cursor_on {
            return;
        }
        let stroke = grid.metrics.stroke;
        match cursor.shape {
            CursorShape::Block => {
                window.paint_quad(fill(area, color));
                if let Some(cell) = cell {
                    painter.grapheme(cell, area, hsla(painter.frame.background), window);
                }
            }
            CursorShape::Bar => window.paint_quad(fill(
                Bounds::new(origin, size((stroke * 2.).max(px(1.)), ch)),
                color,
            )),
            CursorShape::Underline => {
                let height = (stroke * 2.).max(px(1.));
                window.paint_quad(fill(
                    Bounds::new(
                        point(origin.x, origin.y + ch - height),
                        size(area.size.width, height),
                    ),
                    color,
                ));
            }
            CursorShape::HollowBlock => window.paint_quad(outline(area, color, BorderStyle::Solid)),
        }
    }
}
