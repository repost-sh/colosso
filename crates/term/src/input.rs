//! A person's input to a terminal view (`docs/terminals.md` 7.3): keys the encoder turns into
//! the program's bytes, text an input method composes and commits, the mouse reported to a
//! program that tracks it or selecting text otherwise, the wheel, copy and paste.

use std::ops::Range;
use std::time::Duration;

use koyo_term_engine::{
    GridPoint, KeyAction, MouseAction, MouseButton, MouseInput, Scroll, SelectUnit,
};
use gpui_kit::{
    AppContext as _, Bounds, ClipboardItem, Context, EntityInputHandler, KeyDownEvent, KeyUpEvent,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, ScrollEvent, UTF16Selection, Window,
    point, size,
};

use crate::element::Grid;
use crate::keys;
use crate::view::{
    Copy, Link, Paste, ScrollPageDown, ScrollPageUp, ScrollToBottom, ScrollToTop, TerminalView,
};
use crate::vt::Command;

/// A press being dragged: reported to the program, or growing a selection.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Drag {
    anchor: GridPoint,
    unit: SelectUnit,
    rectangle: bool,
    /// The program tracks the mouse and got the press.
    reported: Option<MouseButton>,
    moved: bool,
}

fn button(button: gpui_kit::MouseButton) -> Option<MouseButton> {
    match button {
        gpui_kit::MouseButton::Left => Some(MouseButton::Left),
        gpui_kit::MouseButton::Right => Some(MouseButton::Right),
        gpui_kit::MouseButton::Middle => Some(MouseButton::Middle),
        gpui_kit::MouseButton::Navigate(_) => None,
    }
}

impl TerminalView {
    fn is_attached(&self) -> bool {
        matches!(self.link, Link::Attached { .. })
    }

    pub(crate) fn key_down(
        &mut self,
        event: &KeyDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_attached() || keys::types_text(&event.keystroke, self.option_as_alt) {
            // Text comes through the input method, which composes it first.
            return;
        }
        let action = if event.is_held {
            KeyAction::Repeat
        } else {
            KeyAction::Press
        };
        let Some(key) = keys::encode(&event.keystroke, action, self.option_as_alt) else {
            return;
        };
        self.send(Command::Key(key));
        self.typed(cx);
        cx.stop_propagation();
    }

    pub(crate) fn key_up(&mut self, event: &KeyUpEvent, _: &mut Window, _: &mut Context<Self>) {
        if !self.is_attached() || keys::types_text(&event.keystroke, self.option_as_alt) {
            return;
        }
        // The encoder sends releases only to programs that asked for them (kitty protocol).
        if let Some(key) = keys::encode(&event.keystroke, KeyAction::Release, self.option_as_alt) {
            self.send(Command::Key(key));
        }
    }

    pub(crate) fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        let rx = self.selected_text();
        cx.spawn(async move |_, cx| {
            let text = cx
                .background_spawn(
                    async move { rx.recv_timeout(Duration::from_secs(1)).ok().flatten() },
                )
                .await;
            if let Some(text) = text.filter(|text| !text.is_empty()) {
                cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string(text)));
            }
        })
        .detach();
    }

    pub(crate) fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        self.paste_text(text);
    }

    /// Sends `text` as a paste, which the daemon brackets when the program asked for it.
    pub fn paste_text(&mut self, text: String) {
        let Some(handle) = self
            .handle
            .clone()
            .filter(|_| self.is_attached() && !text.is_empty())
        else {
            return;
        };
        self.send(Command::Scroll(Scroll::Bottom));
        self.send(Command::ClearSelection);
        self.send(Command::Paste { handle, text });
    }

    fn page(&self) -> i32 {
        self.grid
            .get()
            .map_or(24, |grid| i32::from(grid.rows.max(2)) - 1)
    }

    pub(crate) fn scroll_page_up(
        &mut self,
        _: &ScrollPageUp,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
        self.send(Command::Scroll(Scroll::Rows(-self.page())));
    }

    pub(crate) fn scroll_page_down(
        &mut self,
        _: &ScrollPageDown,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
        self.send(Command::Scroll(Scroll::Rows(self.page())));
    }

    pub(crate) fn scroll_to_top(&mut self, _: &ScrollToTop, _: &mut Window, _: &mut Context<Self>) {
        self.send(Command::Scroll(Scroll::Top));
    }

    pub(crate) fn scroll_to_bottom(
        &mut self,
        _: &ScrollToBottom,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
        self.send(Command::Scroll(Scroll::Bottom));
    }

    fn mouse(
        &self,
        action: MouseAction,
        button: Option<MouseButton>,
        position: gpui_kit::Point<Pixels>,
        modifiers: gpui_kit::Modifiers,
        grid: Grid,
    ) -> MouseInput {
        let (x, y) = grid.device(position);
        MouseInput {
            action,
            button,
            mods: keys::mods(modifiers),
            x,
            y,
            any_button_pressed: self.drag.is_some(),
        }
    }

    fn tracking(&self) -> bool {
        self.frame
            .as_ref()
            .is_some_and(|frame| frame.mouse_tracking)
    }

    pub(crate) fn mouse_down(
        &mut self,
        event: &MouseDownEvent,
        grid: Grid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        let Some(pressed) = button(event.button) else {
            return;
        };
        let at = grid.cell_at(event.position);
        // Shift selects even over a program that tracks the mouse, as in every terminal.
        if self.tracking() && !event.modifiers.shift {
            let input = self.mouse(
                MouseAction::Press,
                Some(pressed),
                event.position,
                event.modifiers,
                grid,
            );
            self.send(Command::Mouse(input, grid.geometry()));
            self.drag = Some(Drag {
                anchor: at,
                unit: SelectUnit::Cell,
                rectangle: false,
                reported: Some(pressed),
                moved: false,
            });
            return;
        }
        if pressed != MouseButton::Left {
            return;
        }
        let unit = match event.click_count {
            0 | 1 => SelectUnit::Cell,
            2 => SelectUnit::Word,
            _ => SelectUnit::Line,
        };
        let rectangle = event.modifiers.alt;
        if unit == SelectUnit::Cell {
            // A click alone selects nothing; a drag from it does.
            self.send(Command::ClearSelection);
        } else {
            self.send(Command::Select {
                anchor: at,
                head: at,
                unit,
                rectangle,
            });
        }
        self.drag = Some(Drag {
            anchor: at,
            unit,
            rectangle,
            reported: None,
            moved: false,
        });
        cx.notify();
    }

    pub(crate) fn mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        grid: Grid,
        hovered: bool,
        _: &mut Context<Self>,
    ) {
        let Some(drag) = &mut self.drag else {
            // Motion with nothing pressed matters only to a program tracking all of it.
            if hovered && self.tracking() {
                let input = self.mouse(
                    MouseAction::Motion,
                    None,
                    event.position,
                    event.modifiers,
                    grid,
                );
                self.send(Command::Mouse(input, grid.geometry()));
            }
            return;
        };
        if event.pressed_button.is_none() {
            // The release happened outside the window.
            self.drag = None;
            return;
        }
        let head = grid.cell_at(event.position);
        drag.moved = true;
        let drag = *drag;
        if let Some(pressed) = drag.reported {
            let input = self.mouse(
                MouseAction::Motion,
                Some(pressed),
                event.position,
                event.modifiers,
                grid,
            );
            self.send(Command::Mouse(input, grid.geometry()));
        } else {
            self.send(Command::Select {
                anchor: drag.anchor,
                head,
                unit: drag.unit,
                rectangle: drag.rectangle,
            });
        }
    }

    pub(crate) fn mouse_up(&mut self, event: &MouseUpEvent, grid: Grid, _: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else { return };
        if let Some(pressed) = drag.reported {
            let input = self.mouse(
                MouseAction::Release,
                Some(pressed),
                event.position,
                event.modifiers,
                grid,
            );
            self.send(Command::Mouse(input, grid.geometry()));
        }
    }

    pub(crate) fn scroll_wheel(&mut self, event: &ScrollEvent, grid: Grid, _: &mut Context<Self>) {
        let cell_height = grid.metrics.cell_height;
        // Positive is towards the top, into the history: rows up are negative here.
        self.wheel_rest -= f32::from(event.delta.pixel_delta(cell_height).y);
        let rows = (self.wheel_rest / f32::from(cell_height)).trunc();
        if rows == 0. {
            return;
        }
        self.wheel_rest -= rows * f32::from(cell_height);
        let at = self.mouse(
            MouseAction::Press,
            None,
            event.position,
            event.modifiers,
            grid,
        );
        self.send(Command::Wheel {
            rows: rows as i32,
            at,
            geometry: grid.geometry(),
        });
    }

    /// The cursor's cell, for the input method's candidate window.
    fn cursor_bounds(&self) -> Option<Bounds<Pixels>> {
        let grid = self.grid.get()?;
        let cursor = self.frame.as_ref()?.cursor?;
        let (cw, ch) = (grid.metrics.cell_width, grid.metrics.cell_height);
        Some(Bounds::new(
            grid.origin + point(cw * f32::from(cursor.col), ch * f32::from(cursor.row)),
            size(cw, ch),
        ))
    }
}

/// The input method's view of a terminal: no text to read back, only a composition at the
/// cursor and the text it commits.
impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        _: Range<usize>,
        _: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let end = self
            .marked
            .as_ref()
            .map_or(0, |marked| marked.encode_utf16().count());
        Some(UTF16Selection {
            range: end..end,
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked
            .as_ref()
            .map(|marked| 0..marked.encode_utf16().count())
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked = None;
        if self.is_attached() && !text.is_empty() {
            self.send(Command::Text(text.to_owned()));
            self.typed(cx);
        }
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked = (!text.is_empty()).then(|| text.to_owned());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.cursor_bounds()
    }

    fn character_index_for_point(
        &mut self,
        _: gpui_kit::Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }

    fn accepts_text_input(&self, _: &mut Window, _: &mut Context<Self>) -> bool {
        self.is_attached()
    }
}
