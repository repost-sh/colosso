//! What a person does to a terminal, turned into the bytes its program reads: keys through
//! Ghostty's key encoder (legacy, modifyOtherKeys or the kitty protocol, whichever the
//! program asked for), the mouse through its mouse encoder when the program tracks it, and
//! selections made on the engine's own grid so they follow the text as it scrolls.

use ghostty_vt::key;
use ghostty_vt::mouse;
use ghostty_vt::selection::{FormatOptions, SelectLineOptions, SelectWordOptions, Selection};
use ghostty_vt::terminal::{Mode, Point, PointCoordinate};

pub use ghostty_vt::key::{Key, Mods};

use crate::{Engine, Error, Result};

/// The encoders and the events they read, made once per engine and reused.
pub(crate) struct Encoders {
    key: key::Encoder,
    key_event: key::Event,
    mouse: mouse::Encoder,
    mouse_event: mouse::Event,
}

impl Encoders {
    pub(crate) fn new() -> Result<Encoders> {
        Ok(Encoders {
            key: key::Encoder::new()?,
            key_event: key::Event::new()?,
            mouse: mouse::Encoder::new()?,
            mouse_event: mouse::Event::new()?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    Press,
    Release,
    Repeat,
}

/// How macOS's Option key types: as itself (accented letters) or as Alt for the program.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OptionAsAlt {
    #[default]
    No,
    Both,
    Left,
    Right,
}

/// One key event, as the window saw it.
#[derive(Clone, Copy, Debug)]
pub struct KeyInput<'a> {
    pub key: Key,
    pub action: KeyAction,
    pub mods: Mods,
    /// Modifiers the platform used up producing `text`, Shift for a capital letter.
    pub consumed_mods: Mods,
    /// What the key types, when it types something.
    pub text: Option<&'a str>,
    /// The character the key makes with no modifiers, for the kitty protocol.
    pub unshifted: Option<char>,
    /// Part of an input method's composition, not yet committed.
    pub composing: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseAction {
    Press,
    Release,
    Motion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    /// The wheel, up.
    WheelUp,
    /// The wheel, down.
    WheelDown,
    WheelLeft,
    WheelRight,
}

/// One mouse event, in pixels from the grid's top left.
#[derive(Clone, Copy, Debug)]
pub struct MouseInput {
    pub action: MouseAction,
    pub button: Option<MouseButton>,
    pub mods: Mods,
    pub x: f32,
    pub y: f32,
    /// Whether any button is held, for motion reports in button-event mode.
    pub any_button_pressed: bool,
}

/// The grid's size in pixels, for turning mouse positions into cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    pub width: u32,
    pub height: u32,
    pub cell_width: u32,
    pub cell_height: u32,
}

/// A cell of the viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridPoint {
    pub col: u16,
    pub row: u16,
}

/// What a selection grows by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectUnit {
    Cell,
    /// A double click: the word under the pointer.
    Word,
    /// A triple click: the line under the pointer.
    Line,
}

impl Engine {
    /// The bytes `input` sends the program, empty when it sends none (a lone modifier).
    pub fn encode_key(
        &mut self,
        input: &KeyInput<'_>,
        option_as_alt: OptionAsAlt,
    ) -> Result<Vec<u8>> {
        let encoders = &mut self.encoders;
        encoders
            .key
            .set_options_from_terminal(&self.term)
            .set_macos_option_as_alt(match option_as_alt {
                OptionAsAlt::No => key::OptionAsAlt::False,
                OptionAsAlt::Both => key::OptionAsAlt::True,
                OptionAsAlt::Left => key::OptionAsAlt::Left,
                OptionAsAlt::Right => key::OptionAsAlt::Right,
            });
        encoders
            .key_event
            .set_action(match input.action {
                KeyAction::Press => key::Action::Press,
                KeyAction::Release => key::Action::Release,
                KeyAction::Repeat => key::Action::Repeat,
            })
            .set_key(input.key)
            .set_mods(input.mods)
            .set_consumed_mods(input.consumed_mods)
            .set_composing(input.composing)
            .set_utf8(input.text)
            .set_unshifted_codepoint(input.unshifted.unwrap_or('\0'));
        let mut out = Vec::new();
        encoders.key.encode_to_vec(&encoders.key_event, &mut out)?;
        Ok(out)
    }

    /// The bytes a mouse event sends the program; empty when it doesn't track the mouse.
    pub fn encode_mouse(&mut self, input: &MouseInput, geometry: Geometry) -> Result<Vec<u8>> {
        if geometry.cell_width == 0 || geometry.cell_height == 0 {
            return Ok(Vec::new());
        }
        let encoders = &mut self.encoders;
        encoders
            .mouse
            .set_options_from_terminal(&self.term)
            .set_any_button_pressed(input.any_button_pressed)
            .set_size(mouse::EncoderSize {
                screen_width: geometry.width,
                screen_height: geometry.height,
                cell_width: geometry.cell_width,
                cell_height: geometry.cell_height,
                padding_top: 0,
                padding_bottom: 0,
                padding_right: 0,
                padding_left: 0,
            });
        encoders
            .mouse_event
            .set_action(match input.action {
                MouseAction::Press => mouse::Action::Press,
                MouseAction::Release => mouse::Action::Release,
                MouseAction::Motion => mouse::Action::Motion,
            })
            .set_button(input.button.map(|button| match button {
                MouseButton::Left => mouse::Button::Left,
                MouseButton::Right => mouse::Button::Right,
                MouseButton::Middle => mouse::Button::Middle,
                MouseButton::WheelUp => mouse::Button::Four,
                MouseButton::WheelDown => mouse::Button::Five,
                MouseButton::WheelLeft => mouse::Button::Six,
                MouseButton::WheelRight => mouse::Button::Seven,
            }))
            .set_mods(input.mods)
            .set_position(mouse::Position {
                x: input.x,
                y: input.y,
            });
        let mut out = Vec::new();
        encoders
            .mouse
            .encode_to_vec(&encoders.mouse_event, &mut out)?;
        Ok(out)
    }

    /// What gaining or losing focus sends, when the program asked to hear (mode 1004).
    pub fn encode_focus(&self, focused: bool) -> Result<Vec<u8>> {
        if !self.term.mode(Mode::FOCUS_EVENT)? {
            return Ok(Vec::new());
        }
        let event = if focused {
            ghostty_vt::focus::Event::Gained
        } else {
            ghostty_vt::focus::Event::Lost
        };
        let mut buf = [0u8; 8];
        let len = event.encode(&mut buf)?;
        Ok(buf[..len].to_vec())
    }

    /// Whether the wheel, over a full-screen program that doesn't track the mouse, sends it
    /// arrow keys (mode 1007, on unless the program turned it off).
    pub fn wheel_sends_arrows(&self) -> bool {
        self.is_alt_screen()
            && !self.is_mouse_tracking()
            && self.term.mode(Mode::ALT_SCROLL).unwrap_or(false)
    }

    /// Whether the cursor keys send their application form (DECCKM), for the wheel's arrows.
    pub fn application_cursor_keys(&self) -> bool {
        self.term.mode(Mode::DECCKM).unwrap_or(false)
    }

    /// Selects from `anchor` to `head`, or the word or line at `anchor`.
    pub fn select(
        &mut self,
        anchor: GridPoint,
        head: GridPoint,
        unit: SelectUnit,
        rectangle: bool,
    ) -> Result<()> {
        let at = |point: GridPoint| {
            Point::Viewport(PointCoordinate {
                x: point.col,
                y: u32::from(point.row),
            })
        };
        let term = &self.term;
        let selection = match unit {
            SelectUnit::Cell => Some(Selection::new(
                term.grid_ref(at(anchor))?,
                term.grid_ref(at(head))?,
                rectangle,
            )),
            SelectUnit::Word => {
                term.select_word(SelectWordOptions::new(term.grid_ref(at(anchor))?))?
            }
            SelectUnit::Line => {
                term.select_line(SelectLineOptions::new(term.grid_ref(at(anchor))?))?
            }
        };
        term.set_selection(selection.as_ref())?;
        Ok(())
    }

    pub fn clear_selection(&mut self) -> Result<()> {
        self.term.set_selection(None)?;
        Ok(())
    }

    pub fn has_selection(&self) -> bool {
        self.term
            .selection()
            .is_ok_and(|selection| selection.is_some())
    }

    /// The selected text, lines unwrapped and trailing blanks trimmed; `None` with nothing
    /// selected.
    pub fn selection_text(&self) -> Result<Option<String>> {
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            match self
                .term
                .format_selection_buf(FormatOptions::new(), &mut buf)
            {
                Ok(Some(len)) => {
                    return Ok(Some(String::from_utf8_lossy(&buf[..len]).into_owned()));
                }
                Ok(None) => return Ok(None),
                Err(Error::OutOfSpace { required }) if required > buf.len() => {
                    buf.resize(required, 0);
                }
                Err(error) => return Err(error),
            }
        }
    }
}

/// Whether `text` can be pasted without asking: no control characters a shell would run,
/// Ghostty's unsafe-paste rule.
pub fn paste_is_safe(text: &str) -> bool {
    ghostty_vt::paste::is_safe(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Size;

    #[test]
    fn focus_reports_only_when_asked() {
        let mut engine = Engine::new(Size::cells(20, 4), 1 << 20).unwrap();
        assert!(engine.encode_focus(true).unwrap().is_empty());
        engine.write(b"\x1b[?1004h");
        assert_eq!(engine.encode_focus(true).unwrap(), b"\x1b[I");
        assert_eq!(engine.encode_focus(false).unwrap(), b"\x1b[O");
        assert!(paste_is_safe("ls -la"));
        assert!(!paste_is_safe("rm -rf x\n"));
    }

    fn press(key: Key, text: Option<&str>, mods: Mods) -> KeyInput<'_> {
        KeyInput {
            key,
            action: KeyAction::Press,
            mods,
            consumed_mods: Mods::empty(),
            text,
            unshifted: None,
            composing: false,
        }
    }

    #[test]
    fn keys_encode_for_the_programs_mode() {
        let mut engine = Engine::new(Size::cells(20, 4), 1 << 20).unwrap();
        assert_eq!(
            engine
                .encode_key(&press(Key::A, Some("a"), Mods::empty()), OptionAsAlt::No)
                .unwrap(),
            b"a"
        );
        assert_eq!(
            engine
                .encode_key(&press(Key::Enter, None, Mods::empty()), OptionAsAlt::No)
                .unwrap(),
            b"\r"
        );
        assert_eq!(
            engine
                .encode_key(&press(Key::C, Some("c"), Mods::CTRL), OptionAsAlt::No)
                .unwrap(),
            b"\x03"
        );
        assert_eq!(
            engine
                .encode_key(&press(Key::ArrowUp, None, Mods::empty()), OptionAsAlt::No)
                .unwrap(),
            b"\x1b[A"
        );
        // Application cursor keys (DECCKM), as vim and less turn on.
        engine.write(b"\x1b[?1h");
        assert_eq!(
            engine
                .encode_key(&press(Key::ArrowUp, None, Mods::empty()), OptionAsAlt::No)
                .unwrap(),
            b"\x1bOA"
        );
        // A lone modifier sends nothing.
        assert!(
            engine
                .encode_key(&press(Key::ShiftLeft, None, Mods::SHIFT), OptionAsAlt::No)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn the_mouse_reports_only_when_tracked() {
        let mut engine = Engine::new(Size::cells(20, 4), 1 << 20).unwrap();
        let geometry = Geometry {
            width: 200,
            height: 80,
            cell_width: 10,
            cell_height: 20,
        };
        let click = MouseInput {
            action: MouseAction::Press,
            button: Some(MouseButton::Left),
            mods: Mods::empty(),
            x: 35.,
            y: 25.,
            any_button_pressed: true,
        };
        assert!(engine.encode_mouse(&click, geometry).unwrap().is_empty());
        // Normal tracking with SGR coordinates.
        engine.write(b"\x1b[?1000h\x1b[?1006h");
        assert_eq!(
            engine.encode_mouse(&click, geometry).unwrap(),
            b"\x1b[<0;4;2M"
        );
    }

    #[test]
    fn selections_copy_their_text() {
        let mut engine = Engine::new(Size::cells(20, 3), 1 << 20).unwrap();
        engine.write(b"hello brave\r\nnew world");
        assert!(!engine.has_selection());
        assert_eq!(engine.selection_text().unwrap(), None);
        engine
            .select(
                GridPoint { col: 6, row: 0 },
                GridPoint { col: 2, row: 1 },
                SelectUnit::Cell,
                false,
            )
            .unwrap();
        assert!(engine.has_selection());
        assert_eq!(
            engine.selection_text().unwrap().as_deref(),
            Some("brave\nnew")
        );
        let frame = engine.frame().unwrap();
        assert!(
            frame.rows[0].cells[6]
                .attrs
                .contains(crate::Attrs::SELECTED)
        );
        assert!(
            !frame.rows[0].cells[0]
                .attrs
                .contains(crate::Attrs::SELECTED)
        );
        engine
            .select(
                GridPoint { col: 7, row: 1 },
                GridPoint { col: 7, row: 1 },
                SelectUnit::Word,
                false,
            )
            .unwrap();
        assert_eq!(engine.selection_text().unwrap().as_deref(), Some("world"));
        engine
            .select(
                GridPoint { col: 1, row: 0 },
                GridPoint { col: 1, row: 0 },
                SelectUnit::Line,
                false,
            )
            .unwrap();
        assert_eq!(
            engine.selection_text().unwrap().as_deref(),
            Some("hello brave")
        );
        engine.clear_selection().unwrap();
        assert!(!engine.has_selection());
    }
}
