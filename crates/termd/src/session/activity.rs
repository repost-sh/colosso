//! The activity log's lines (`docs/terminals.md` 5.8): what each writer did to the terminal,
//! refusals included, typing run together into one line until a pause, and every change of
//! control.

use std::time::{Duration, Instant};

use koyo_term_control::{
    ActivityEntry, Actor, ActorKind, ControlChange, ErrorCode, EventKind, HumanInput, LogKind,
    ProtocolError,
};
use serde::Serialize;

use super::Session;
use super::keys::{self, Key};

/// A pause this long ends a line of typing.
const TYPING_PAUSE: Duration = Duration::from_secs(1);

/// Typing, or a key pressed again and again, not logged yet.
pub struct Typing {
    actor: Actor,
    input: HumanInput,
    offset: u64,
    last: Instant,
}

impl Session {
    /// Logs what `actor` wrote, text typed joined to the text before it, a key pressed again
    /// counted.
    pub(super) fn log_input(&mut self, actor: &Actor, bytes: &[u8]) {
        let now = Instant::now();
        for key in keys::describe(bytes) {
            if let Some(typing) = &mut self.typing
                && typing.actor.id == actor.id
            {
                let joined = match (&mut typing.input, &key) {
                    (HumanInput::Typed(text), Key::Typed(more)) => {
                        text.push_str(more);
                        true
                    }
                    (HumanInput::Pressed { key, times }, Key::Pressed(again)) if key == again => {
                        *times += 1;
                        true
                    }
                    _ => false,
                };
                if joined {
                    typing.last = now;
                    continue;
                }
            }
            self.flush_typing();
            self.typing = Some(Typing {
                actor: actor.clone(),
                input: input_of(key),
                offset: self.ring.end(),
                last: now,
            });
        }
    }

    /// Logs input that was refused, as one line.
    pub(super) fn log_refused_input(&mut self, actor: &Actor, bytes: &[u8], error: &ProtocolError) {
        let inputs: Vec<HumanInput> = keys::describe(bytes).into_iter().map(input_of).collect();
        let command: Vec<String> = inputs.iter().map(command_for).collect();
        let action: Vec<String> = inputs.iter().map(action).collect();
        let action = format!("tried to write: {}", action.join(", "));
        self.log(actor, command.join("; "), action, Err(error));
    }

    pub(super) fn log_paste(
        &mut self,
        actor: &Actor,
        text: &str,
        outcome: Result<(), &ProtocolError>,
    ) {
        let input = HumanInput::Pasted(text.to_owned());
        let verb = if outcome.is_ok() {
            ""
        } else {
            "tried to write: "
        };
        let action = format!("{verb}{}", action(&input));
        self.flush_typing();
        let mut entry = self.entry(actor, command_for(&input), action, outcome);
        entry.human = (actor.kind == ActorKind::Human).then_some(input);
        self.append_log(LogKind::Activity, &entry);
    }

    /// Logs a change of control, `from` the actor that held the terminal before.
    pub(super) fn log_control(&mut self, change: &ControlChange, from: Option<&Actor>) {
        let from = from
            .map(|from| format!(" from {}", from.id))
            .unwrap_or_default();
        let (actor, command, action) = match change {
            ControlChange::Claimed { by } => (by.clone(), "claim".into(), "took control".into()),
            ControlChange::Released { by } => {
                (by.clone(), "release".into(), "gave control up".into())
            }
            ControlChange::Taken { by, from, reason } => {
                let why = reason.as_deref().map(|r| quoted(r, 80));
                let flag = why
                    .as_ref()
                    .map(|why| format!(" --reason {why}"))
                    .unwrap_or_default();
                let words = why.map(|why| format!(" ({why})")).unwrap_or_default();
                let action = format!("took control from {}{words}", from.id);
                (by.clone(), format!("claim --takeover{flag}"), action)
            }
            ControlChange::HumanTook { by, implicit: true } => (
                by.clone(),
                "type".into(),
                format!("took control{from} by typing"),
            ),
            ControlChange::HumanTook { by, .. } => (
                by.clone(),
                "take control".into(),
                format!("took control{from}"),
            ),
            ControlChange::HumanReleased { by } => {
                (by.clone(), "give back".into(), "gave back".into())
            }
            ControlChange::Expired { was } => (
                Actor::system(),
                String::new(),
                format!("the lease of {} ended", was.id),
            ),
        };
        self.flush_typing();
        let mut entry = self.entry(&actor, command, action, Ok(()));
        entry.control = Some(change.clone());
        self.append_log(LogKind::Activity, &entry);
    }

    /// Ends a line of typing after a pause.
    pub(super) fn flush_idle_typing(&mut self) {
        if (self.typing)
            .as_ref()
            .is_some_and(|typing| typing.last.elapsed() >= TYPING_PAUSE)
        {
            self.flush_typing();
        }
    }

    pub(super) fn flush_typing(&mut self) {
        let Some(typing) = self.typing.take() else {
            return;
        };
        let mut entry = self.entry(
            &typing.actor,
            command_for(&typing.input),
            action(&typing.input),
            Ok(()),
        );
        entry.offset = typing.offset;
        entry.human = (typing.actor.kind == ActorKind::Human).then_some(typing.input);
        self.append_log(LogKind::Activity, &entry);
    }

    /// Logs one thing done, after the typing before it.
    pub(super) fn log(
        &mut self,
        actor: &Actor,
        command: String,
        action: String,
        outcome: Result<(), &ProtocolError>,
    ) {
        self.flush_typing();
        let entry = self.entry(actor, command, action, outcome);
        self.append_log(LogKind::Activity, &entry);
    }

    fn entry(
        &self,
        actor: &Actor,
        command: String,
        action: String,
        outcome: Result<(), &ProtocolError>,
    ) -> ActivityEntry {
        let error = outcome.err().map(|error| error.code);
        ActivityEntry {
            time: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            actor: actor.clone(),
            command,
            action,
            duration_ms: 0,
            exit: match error {
                None => 0,
                Some(ErrorCode::PersonHasControl) => 4,
                Some(_) => 1,
            },
            error,
            offset: self.ring.end(),
            request_id: None,
            screenshots: Vec::new(),
            output: None,
            human: None,
            control: None,
        }
    }

    /// Appends a line to one of the logs and tells the clients it's there.
    pub(super) fn append_log(&self, kind: LogKind, entry: &impl Serialize) {
        if let Some(end) = self.journal.append(kind, entry) {
            self.daemon
                .broadcast(&self.record.handle, &EventKind::Logged { log: kind, end });
        }
    }
}

fn input_of(key: Key) -> HumanInput {
    match key {
        Key::Typed(text) => HumanInput::Typed(text),
        Key::Pressed(key) => HumanInput::Pressed { key, times: 1 },
        Key::Raw(count) => HumanInput::Raw(u32::try_from(count).unwrap_or(u32::MAX)),
    }
}

/// The call that writes `input` again.
fn command_for(input: &HumanInput) -> String {
    match input {
        HumanInput::Typed(text) => format!("type {}", quoted(text, 200)),
        HumanInput::Pressed { key, times: 1 } => format!("press {key}"),
        HumanInput::Pressed { key, times } => format!("press {key} --times {times}"),
        HumanInput::Pasted(text) => format!("paste {}", quoted(text, 200)),
        HumanInput::Raw(count) => format!("send {count} raw bytes"),
    }
}

fn action(input: &HumanInput) -> String {
    match input {
        HumanInput::Typed(text) => format!("typed {}", quoted(text, 60)),
        HumanInput::Pressed { key, times: 1 } => format!("pressed {key}"),
        HumanInput::Pressed { key, times } => format!("pressed {key} {times} times"),
        HumanInput::Pasted(text) => format!("pasted {}", quoted(text, 60)),
        HumanInput::Raw(1) => "sent 1 raw byte".into(),
        HumanInput::Raw(count) => format!("sent {count} raw bytes"),
    }
}

/// `text` in quotes and escaped, cut to `max` characters.
pub(super) fn quoted(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return format!("{text:?}");
    }
    let cut: String = text.chars().take(max).collect();
    format!("{cut:?}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs_read_as_agent_terminal_words_them() {
        let pressed = |key: &str, times| HumanInput::Pressed {
            key: key.into(),
            times,
        };
        assert_eq!(
            action(&HumanInput::Typed("pnpm test".into())),
            "typed \"pnpm test\""
        );
        assert_eq!(action(&pressed("enter", 3)), "pressed enter 3 times");
        assert_eq!(command_for(&pressed("ctrl+c", 1)), "press ctrl+c");
        assert_eq!(action(&HumanInput::Raw(14)), "sent 14 raw bytes");
        assert_eq!(quoted(&"x".repeat(5), 3), "\"xxx\"…");
    }
}
