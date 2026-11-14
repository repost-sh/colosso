//! The agent desktop's tools (`docs/agent-desktop.md` 10): each asks the desktop's host
//! (`agent_desktop::ask`) and answers what it says in app control's words. An act, and
//! closing, are refused while the person uses the desktop's tab.

use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use gpui_kit::{App, AsyncApp};
use koyo_app_control::ops::browser::MouseButton;
use koyo_app_control::ops::desktop::{
    Closing, DesktopClickInput, DesktopCloseInput, DesktopDone, DesktopDragInput,
    DesktopFocusInput, DesktopKeyInput, DesktopKeyOutput, DesktopMoveInput, DesktopOpenInput,
    DesktopOpenOutput, DesktopScreenshotInput, DesktopScreenshotOutput, DesktopScrollInput,
    DesktopSnapshotInput, DesktopSnapshotOutput, DesktopTakeFocusInput, DesktopTypeInput,
    DesktopWaitInput, DesktopWaitOutput, DesktopWindow, DesktopWindowsOutput, Modifier, Target,
    Typing,
};
use koyo_app_control::{ControlError, ErrorCode};
use koyo_desk_wire::Point;
use koyo_desk_wire::keys::flags;
use koyo_desk_wire::link::{Act, Answer, Ask, Button, Close, Refusal, RefusalCode, Spot, Window};
use serde_json::Value;

use super::{Exec, beside, output, update_shell};
use crate::agent_desktop;
use crate::browser::control::{Mode, Refused};
use crate::control::Caller;
use crate::shell::Shell;

/// How long each ask may take before the call fails `timeout`; each well inside the client's
/// `CALL_TIMEOUT`.
const OPEN: Duration = Duration::from_secs(45);
const WINDOWS: Duration = Duration::from_secs(5);
const SCREENSHOT: Duration = Duration::from_secs(15);
const SNAPSHOT: Duration = Duration::from_secs(20);
const ACT: Duration = Duration::from_secs(30);
const CLOSE: Duration = Duration::from_secs(10);
/// Typing takes this, and [`PER_KEY`] for each character, at most [`TYPING_MOST`].
const TYPING: Duration = Duration::from_secs(10);
const PER_KEY: Duration = Duration::from_millis(20);
const TYPING_MOST: Duration = Duration::from_secs(90);
/// A wait's ask has this beyond the wait itself.
const WAIT_SLACK: Duration = Duration::from_secs(5);

/// Asks the host, and answers what `answered` makes of its answer.
fn asking(
    ask: Ask,
    limit: Duration,
    cx: &mut App,
    answered: impl FnOnce(Answer) -> Result<Value, ControlError> + 'static,
) -> Exec {
    let asked = agent_desktop::ask(cx, ask, limit);
    Exec::Later(
        cx.spawn(async move |_: &mut AsyncApp| asked.await.map_err(refused).and_then(answered)),
    )
}

/// Asks the host to act for `caller`, which has the desktop from then on, unless the person
/// holds it.
fn acting(
    caller: &Caller,
    ask: Ask,
    limit: Duration,
    cx: &mut App,
    answered: impl FnOnce(Answer) -> Result<Value, ControlError> + 'static,
) -> Exec {
    match agent_desktop::agent_acts(cx, caller.actor()) {
        Ok(()) => asking(ask, limit, cx, answered),
        Err(Refused::PersonHasControl {
            mode: Mode::Implicit,
            ..
        }) => Exec::Ready(Err(person_using())),
        Err(_) => Exec::Ready(Err(ControlError::new(
            ErrorCode::PersonHasControl,
            "The person has taken control of the agent desktop. Don't retry or wait for it; do something else until they give it back.",
        ))),
    }
}

/// An act's answer: it was done.
fn done(answer: Answer) -> Result<Value, ControlError> {
    match answer {
        Answer::Done => output(&DesktopDone { done: true }),
        other => Err(unexpected(&other)),
    }
}

/// The person acted in the desktop's tab lately.
fn person_using() -> ControlError {
    ControlError::new(
        ErrorCode::PersonHasControl,
        "The person is using the agent desktop; it comes back on its own a few seconds after they stop. Wait, then look again before acting.",
    )
    .retryable(true)
}

/// A refusal of the host's, in app control's words; its message as it is.
pub(super) fn refused(refusal: Refusal) -> ControlError {
    let Refusal { code, message } = refusal;
    match code {
        RefusalCode::NoDesktop | RefusalCode::NotPermitted => {
            ControlError::desktop_unavailable(message)
        }
        RefusalCode::NotFound => ControlError::not_found(message),
        RefusalCode::AppIsThePersons | RefusalCode::NeedsConsent => {
            ControlError::new(ErrorCode::PersonHasControl, message)
        }
        // Cut short by the person: theirs for now, as when they act before it starts.
        RefusalCode::PersonActing => {
            ControlError::new(ErrorCode::PersonHasControl, message).retryable(true)
        }
        RefusalCode::Invalid => ControlError::invalid_input(message),
        RefusalCode::TimedOut => ControlError::timeout(message),
        RefusalCode::Failed => ControlError::internal(message),
    }
}

/// An answer that isn't the one asked for.
fn unexpected(answer: &Answer) -> ControlError {
    let what = match answer {
        Answer::Done => "done",
        Answer::Copied { .. } => "a copy",
        Answer::Opened { .. } => "an app opened",
        Answer::Windows { .. } => "windows",
        Answer::Screenshot { .. } => "a screenshot",
        Answer::Snapshot { .. } => "a snapshot",
        Answer::Waited { .. } => "a wait",
    };
    ControlError::internal(format!(
        "The agent desktop answered with {what}, which wasn't asked for."
    ))
}

fn window(window: Window) -> DesktopWindow {
    DesktopWindow {
        window: window.id,
        pid: window.pid,
        app: window.app,
        title: window.title,
        x: window.frame.x,
        y: window.frame.y,
        width: window.frame.width,
        height: window.frame.height,
    }
}

fn spot(target: Target) -> Spot {
    match target {
        Target::Element(element) => Spot::Element(element),
        Target::Point { x, y } => Spot::Point(Point { x, y }),
    }
}

/// The modifiers as `CGEventFlags`.
fn modifier_flags(modifiers: &[Modifier]) -> u64 {
    modifiers.iter().fold(0, |held, modifier| {
        held | match modifier {
            Modifier::Cmd => flags::COMMAND,
            Modifier::Shift => flags::SHIFT,
            Modifier::Alt => flags::OPTION,
            Modifier::Ctrl => flags::CONTROL,
        }
    })
}

/// How long typing `text` as keys may take.
fn typing_limit(text: &str) -> Duration {
    let keys = u32::try_from(text.chars().count()).unwrap_or(u32::MAX);
    (TYPING + PER_KEY.saturating_mul(keys)).min(TYPING_MOST)
}

pub(crate) fn open(caller: &Caller, input: DesktopOpenInput, cx: &mut App) -> Exec {
    // The desktop is this agent's from here on, if the person doesn't hold it: an app is
    // opened either way, out of the person's sight.
    let _ = agent_desktop::agent_acts(cx, caller.actor());
    // The desktop's tab, where the caller is and not where the person is (`beside::tab`),
    // shown or not as the person set what agents open to be; the desktop works without it,
    // and with the window closed there's none to open.
    let shows = crate::control::policy::shows(None, cx);
    let _ = update_shell(cx, |shell, window, cx| {
        beside::tab(shell, caller, shows, window, cx, Shell::desktop_pane_at)
    });
    let ask = Ask::Open {
        app: input.app,
        document: input.file,
        args: input.args.unwrap_or_default(),
        theirs: input.persons_own.unwrap_or(false),
        consent: input.consent,
    };
    asking(ask, OPEN, cx, |answer| match answer {
        Answer::Opened {
            app,
            pid,
            windows,
            lent,
        } => output(&DesktopOpenOutput {
            app,
            pid,
            windows: windows.into_iter().map(window).collect(),
            lent,
        }),
        other => Err(unexpected(&other)),
    })
}

pub(crate) fn windows(cx: &mut App) -> Exec {
    asking(Ask::Windows, WINDOWS, cx, |answer| match answer {
        Answer::Windows { windows } => output(&DesktopWindowsOutput {
            windows: windows.into_iter().map(window).collect(),
        }),
        other => Err(unexpected(&other)),
    })
}

pub(crate) fn screenshot(
    input: &DesktopScreenshotInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let ask = Ask::Screenshot {
        window: input.window,
        scale: input.scale()?,
    };
    Ok(asking(ask, SCREENSHOT, cx, |answer| match answer {
        Answer::Screenshot {
            png,
            origin,
            size,
            scale,
        } => output(&DesktopScreenshotOutput {
            mime_type: "image/png".to_owned(),
            data: STANDARD.encode(&png),
            x: origin.x,
            y: origin.y,
            width: size.width,
            height: size.height,
            scale,
        }),
        other => Err(unexpected(&other)),
    }))
}

pub(crate) fn snapshot(input: &DesktopSnapshotInput, cx: &mut App) -> Exec {
    let ask = Ask::Snapshot {
        window: input.window,
        interactive: input.interactive.unwrap_or(false),
        menus: input.menus.unwrap_or(false),
    };
    asking(ask, SNAPSHOT, cx, |answer| match answer {
        Answer::Snapshot {
            tree,
            elements,
            truncated,
        } => output(&DesktopSnapshotOutput {
            tree,
            elements,
            truncated,
        }),
        other => Err(unexpected(&other)),
    })
}

pub(crate) fn click(
    caller: &Caller,
    input: &DesktopClickInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let act = Act::Click {
        spot: spot(input.target()?),
        window: input.window,
        button: match input.button.unwrap_or(MouseButton::Left) {
            MouseButton::Left => Button::Left,
            MouseButton::Middle => Button::Middle,
            MouseButton::Right => Button::Right,
        },
        count: input.count()?,
        flags: modifier_flags(input.modifiers.as_deref().unwrap_or_default()),
    };
    Ok(acting(caller, Ask::Act(act), ACT, cx, done))
}

pub(crate) fn move_to(
    caller: &Caller,
    input: &DesktopMoveInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let act = Act::Move {
        spot: spot(input.target()?),
        window: input.window,
    };
    Ok(acting(caller, Ask::Act(act), ACT, cx, done))
}

pub(crate) fn drag(
    caller: &Caller,
    input: &DesktopDragInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let act = Act::Drag {
        from: spot(input.from.target()?),
        to: spot(input.to.target()?),
        window: input.window,
    };
    Ok(acting(caller, Ask::Act(act), ACT, cx, done))
}

pub(crate) fn scroll(
    caller: &Caller,
    input: &DesktopScrollInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let (dx, dy) = input.amount()?;
    let act = Act::Scroll {
        spot: spot(input.target()?),
        window: input.window,
        dx,
        dy,
    };
    Ok(acting(caller, Ask::Act(act), ACT, cx, done))
}

pub(crate) fn type_text(
    caller: &Caller,
    input: &DesktopTypeInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let (act, limit) = match input.typing()? {
        Typing::Keys { text, into } => {
            let limit = typing_limit(&text);
            (Act::Type { text, into }, limit)
        }
        Typing::SetValue { element, text } => (Act::SetValue { element, text }, ACT),
    };
    Ok(acting(caller, Ask::Act(act), limit, cx, done))
}

pub(crate) fn key(caller: &Caller, input: DesktopKeyInput, cx: &mut App) -> Exec {
    let act = Act::Key {
        combo: input.keys,
        window: input.window,
    };
    acting(caller, Ask::Act(act), ACT, cx, |answer| {
        let copied = match answer {
            Answer::Done => None,
            Answer::Copied { text } => Some(text),
            other => return Err(unexpected(&other)),
        };
        output(&DesktopKeyOutput { done: true, copied })
    })
}

pub(crate) fn focus(caller: &Caller, input: &DesktopFocusInput, cx: &mut App) -> Exec {
    let act = Act::Raise {
        window: input.window,
    };
    acting(caller, Ask::Act(act), ACT, cx, done)
}

/// The agent desktop onto the person's screen, on their word (`docs/agent-desktop.md` 5).
pub(crate) fn take_focus(caller: &Caller, input: DesktopTakeFocusInput, cx: &mut App) -> Exec {
    let ask = Ask::TakeFocus {
        window: input.window,
        consent: input.consent,
    };
    acting(caller, ask, ACT, cx, done)
}

/// The person's own desktop back on their screen: never held up, whoever drives the tab.
pub(crate) fn return_focus(cx: &mut App) -> Exec {
    asking(Ask::ReturnFocus, ACT, cx, done)
}

pub(crate) fn wait(input: &DesktopWaitInput, cx: &mut App) -> Exec {
    let timeout_ms = input.timeout_ms();
    let ask = Ask::Wait {
        quiet_ms: input.quiet_ms(),
        timeout_ms,
    };
    let limit = Duration::from_millis(timeout_ms) + WAIT_SLACK;
    asking(ask, limit, cx, |answer| match answer {
        Answer::Waited { still, millis } => output(&DesktopWaitOutput { still, millis }),
        other => Err(unexpected(&other)),
    })
}

pub(crate) fn close(
    caller: &Caller,
    input: &DesktopCloseInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let ask = match input.closing()? {
        Closing::Window(window) => Ask::Close(Close::Window(window)),
        Closing::App(pid) => Ask::Close(Close::App(pid)),
    };
    Ok(acting(caller, ask, CLOSE, cx, done))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hosts_refusals_read_as_app_control_codes() {
        for (code, expected) in [
            (RefusalCode::NoDesktop, ErrorCode::DesktopUnavailable),
            (RefusalCode::NotPermitted, ErrorCode::DesktopUnavailable),
            (RefusalCode::NotFound, ErrorCode::NotFound),
            (RefusalCode::AppIsThePersons, ErrorCode::PersonHasControl),
            (RefusalCode::NeedsConsent, ErrorCode::PersonHasControl),
            (RefusalCode::PersonActing, ErrorCode::PersonHasControl),
            (RefusalCode::Invalid, ErrorCode::InvalidInput),
            (RefusalCode::TimedOut, ErrorCode::Timeout),
            (RefusalCode::Failed, ErrorCode::Internal),
        ] {
            let error = refused(Refusal::new(code, "As the host said it."));
            assert_eq!(error.code, expected, "{code:?}");
            assert_eq!(error.message, "As the host said it.", "{code:?}");
        }
        // The person's app stays theirs; their use of the tab passes.
        let theirs = refused(Refusal::new(RefusalCode::AppIsThePersons, "Theirs."));
        assert!(!theirs.retryable);
        assert!(person_using().retryable);
        assert_eq!(person_using().code, ErrorCode::PersonHasControl);
    }

    #[test]
    fn modifiers_and_typing_time_read_as_the_host_takes_them() {
        assert_eq!(modifier_flags(&[]), 0);
        assert_eq!(
            modifier_flags(&[Modifier::Shift, Modifier::Cmd]),
            flags::SHIFT | flags::COMMAND
        );
        assert_eq!(
            modifier_flags(&[Modifier::Alt, Modifier::Ctrl]),
            flags::OPTION | flags::CONTROL
        );
        assert_eq!(typing_limit(""), Duration::from_secs(10));
        assert_eq!(typing_limit(&"a".repeat(500)), Duration::from_secs(20));
        assert_eq!(typing_limit(&"a".repeat(4000)), Duration::from_secs(90));
    }

    #[test]
    fn an_unexpected_answer_is_internal() {
        let error = unexpected(&Answer::Waited {
            still: true,
            millis: 3,
        });
        assert_eq!(error.code, ErrorCode::Internal);
    }
}
