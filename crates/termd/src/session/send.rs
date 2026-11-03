//! `Send` and `ReadScreen` (`docs/terminals.md` 8.5, `terminal_send` and `terminal_read`):
//! text, named keys, a paste, Enter and Ctrl-C written as the caller without attaching, under
//! the rules typing follows (5.6), and the screen read off the daemon's own engine.

use koyo_term_control::{
    ErrorCode, ProtocolError, ResponseBody, ScreenCursor, ScreenRead, SendRequest, TerminalRecord,
};
use koyo_term_engine::{Engine, KeyAction, KeyInput, OptionAsAlt, chord};

use super::Session;
use super::activity::quoted;
use super::cmd::{Reply, exited};

impl Session {
    pub(super) fn send(&mut self, reply: &Reply, send: &SendRequest) {
        // A key it can't name refuses the whole send, before anything is written.
        let bytes = match self.encode_send(send) {
            Ok(bytes) => bytes,
            Err(error) => return reply.err(error),
        };
        if self.pty.is_none() {
            return reply.err(exited());
        }
        let actor = reply.client.actor.clone();
        let (command, action) = describe(send);
        if let Err(error) = self.may_act(&actor) {
            let action = format!("tried to write: {action}");
            self.log(&actor, command, action, Err(&error));
            return reply.err(error);
        }
        self.status.input();
        self.commands.last_writer = Some(actor.clone());
        self.log(&actor, command, action, Ok(()));
        self.write_pty(&bytes);
        reply.ok(ResponseBody::Sent {
            bytes: u32::try_from(bytes.len()).unwrap_or(u32::MAX),
        });
    }

    fn encode_send(&mut self, send: &SendRequest) -> Result<Vec<u8>, ProtocolError> {
        let mut bytes = Vec::new();
        if send.interrupt {
            bytes.extend(self.encode_chord("ctrl+c")?);
        }
        if let Some(text) = &send.text {
            bytes.extend(text.replace("\r\n", "\r").replace('\n', "\r").bytes());
        }
        if let Some(text) = &send.paste {
            bytes.extend(self.paste_bytes(text));
        }
        for key in &send.keys {
            bytes.extend(self.encode_chord(key)?);
        }
        if send.enter {
            bytes.extend(self.encode_chord("enter")?);
        }
        if bytes.is_empty() {
            return Err(ProtocolError::new(
                ErrorCode::InvalidArgument,
                "nothing to send: give text, paste, keys, enter or interrupt",
            ));
        }
        Ok(bytes)
    }

    /// The bytes `written` (`ctrl+c`, `up`) sends the program in the terminal's modes now.
    fn encode_chord(&mut self, written: &str) -> Result<Vec<u8>, ProtocolError> {
        let chord = chord(written).ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::InvalidArgument,
                format!("no key is named {written:?}; try enter, ctrl+c, shift+tab, up, f5"),
            )
        })?;
        let input = KeyInput {
            key: chord.key,
            action: KeyAction::Press,
            mods: chord.mods,
            consumed_mods: chord.consumed_mods,
            text: chord.text.as_deref(),
            unshifted: chord.unshifted,
            composing: false,
        };
        // An agent's alt is the Meta key, as a terminal user's Option set to Alt is.
        self.engine
            .encode_key(&input, OptionAsAlt::Both)
            .map_err(|error| {
                ProtocolError::new(
                    ErrorCode::Internal,
                    format!("couldn't encode {written}: {error}"),
                )
            })
    }

    pub(super) fn read_screen(&mut self, reply: &Reply) {
        match screen_read(&mut self.engine, &self.record) {
            Ok(screen) => reply.ok(ResponseBody::Screen(Box::new(screen))),
            Err(error) => reply.err(ProtocolError::new(
                ErrorCode::Internal,
                format!("couldn't read the screen: {error}"),
            )),
        }
    }
}

/// The send as the activity log words it: the command, as a CLI would take it, and what it
/// did.
fn describe(send: &SendRequest) -> (String, String) {
    let mut command = vec!["send".to_owned()];
    let mut action = Vec::new();
    if send.interrupt {
        command.push("--interrupt".into());
        action.push("pressed ctrl+c".to_owned());
    }
    if let Some(text) = &send.text {
        let text = quoted(text, 80);
        command.push(format!("--text {text}"));
        action.push(format!("typed {text}"));
    }
    if let Some(text) = &send.paste {
        let text = quoted(text, 80);
        command.push(format!("--paste {text}"));
        action.push(format!("pasted {text}"));
    }
    if !send.keys.is_empty() {
        command.push(format!("--keys {}", send.keys.join(",")));
        action.push(format!("pressed {}", send.keys.join(", ")));
    }
    if send.enter {
        command.push("--enter".into());
        action.push("pressed enter".to_owned());
    }
    (command.join(" "), action.join(", "))
}

/// What `engine` shows, as `ReadScreen` answers for `record`'s terminal.
pub(super) fn screen_read(
    engine: &mut Engine,
    record: &TerminalRecord,
) -> Result<ScreenRead, String> {
    let frame = engine.frame().map_err(|error| error.to_string())?;
    let rows = u16::try_from(frame.rows.len()).unwrap_or(u16::MAX);
    let lines = (0..rows)
        .map(|row| frame.row_text(row, 0).trim_end().to_owned())
        .collect();
    Ok(ScreenRead {
        name: record.name.clone(),
        title: frame.title.clone(),
        cols: frame.cols,
        rows,
        alt_screen: frame.alt_screen,
        mouse: frame.mouse_tracking,
        cursor: frame.cursor.map(|cursor| ScreenCursor {
            row: cursor.row,
            col: cursor.col,
            visible: cursor.visible(),
        }),
        frame: frame.sequence,
        status: record.status,
        lines,
    })
}
