//! Stopping a terminal from the app (`docs/terminals.md` 7.1): a second shell opened, then
//! stopped with Stop Terminal as its bar's Stop does; idle, it asks nothing, the daemon closes
//! it, and its tab goes, the first shell shown again. Another then closed with ⌘W: it ends the
//! same way and stays gone when the app next looks for terminals, and ⌘⇧T starts a new shell.

use std::time::Duration;

use koyo_term_control::{Handle, RequestBody, ResponseBody, Status};

use super::control;
use super::drive::{act, press, text, type_line, wait, wait_until};
use crate::modal;
use crate::quitting::QuitDialog;
use crate::shell::{CloseTab, NewTerminal, ReopenClosedTab, Shell, StopTerminal};
use crate::visual::{Drawn, Result, checkout};

pub(super) fn stop(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    let first = shown(drawn).ok_or("the first terminal isn't known yet")?;
    act(drawn, Box::new(NewTerminal))?;
    wait_until(drawn, "a second terminal shown", |drawn| {
        Ok(shown(drawn).is_some_and(|handle| handle != first))
    })?;
    let second = shown(drawn).ok_or("the second terminal isn't known")?;
    control::holds(drawn, "the second shell at its prompt", |state| {
        state.status == Status::Idle && state.reason == "prompt"
    })?;
    act(drawn, Box::new(StopTerminal))?;
    let shell = drawn.shell.clone();
    wait_until(drawn, "the stopped terminal's tab to go", |drawn| {
        Ok(drawn.cx.read(|cx| {
            let shell = shell.read(cx);
            let id = shell.terminals.tab_of(&second);
            !id.is_some_and(|id| shell.workspace.terminal_tabs().any(|(_, tab)| tab == id))
        }))
    })?;
    wait_until(drawn, "the first shell shown again", |drawn| {
        Ok(shown(drawn).as_ref() == Some(&first))
    })?;
    if in_the_daemon(drawn, &second)? {
        return Err("the stopped terminal is still in the daemon".into());
    }
    said.push("Stop Terminal on an idle shell closed it in the daemon, and its tab went".into());
    closed(drawn, &first, said)
}

/// A shell closed with ⌘W ends in the daemon, doesn't come back when the app next looks for
/// its terminals, and ⌘⇧T starts a new shell in its tab, leaving `first` shown again.
fn closed(drawn: &mut Drawn, first: &Handle, said: &mut Vec<String>) -> Result<()> {
    const MARK: &str = "from-the-closed-shell";
    act(drawn, Box::new(NewTerminal))?;
    wait_until(drawn, "a shell to close", |drawn| {
        Ok(shown(drawn).is_some_and(|handle| handle != *first))
    })?;
    let handle = shown(drawn).ok_or("the shell to close isn't known")?;
    control::holds(drawn, "the shell to close at its prompt", |state| {
        state.status == Status::Idle && state.reason == "prompt"
    })?;
    type_line(drawn, &format!("echo {MARK}"))?;
    wait(drawn, "the shell's output", |text| text.lines().any(|line| line == MARK))?;
    control::holds(drawn, "the shell back at its prompt", |state| {
        state.status == Status::Idle && state.reason == "prompt"
    })?;

    act(drawn, Box::new(CloseTab))?;
    wait_until(drawn, "the first shell shown again", |drawn| {
        Ok(shown(drawn).as_ref() == Some(first))
    })?;
    wait_until(drawn, "the closed shell to end in the daemon", |drawn| {
        Ok(!in_the_daemon(drawn, &handle)?)
    })?;

    let shell = drawn.shell.clone();
    let tabs = |drawn: &mut Drawn| {
        drawn.cx.read(|cx| shell.read(cx).workspace.terminal_tabs().count())
    };
    let before = tabs(drawn);
    (drawn.cx).update(|cx| shell.update(cx, Shell::restore_terminals_again));
    // Nothing should change, so it can only be given the time to.
    let _ = drawn.until(Duration::from_secs(2), |drawn| Ok(tabs(drawn) != before));
    if tabs(drawn) != before {
        return Err("the closed terminal's tab came back".into());
    }

    act(drawn, Box::new(ReopenClosedTab))?;
    wait_until(drawn, "a new shell in the reopened tab", |drawn| {
        Ok(shown(drawn).is_some_and(|shown| shown != *first && shown != handle))
    })?;
    control::holds(drawn, "the new shell at its prompt", |state| {
        state.status == Status::Idle && state.reason == "prompt"
    })?;
    if text(drawn)?.contains(MARK) {
        return Err("the reopened tab shows the closed shell's screen".into());
    }
    act(drawn, Box::new(StopTerminal))?;
    wait_until(drawn, "the first shell shown once more", |drawn| {
        Ok(shown(drawn).as_ref() == Some(first))
    })?;
    said.push("⌘W ended a shell in the daemon, it stayed gone, ⌘⇧T started a new one".into());
    Ok(())
}

/// "End Everything" with a shell slow to end, which ignores the hangup: its program is gone,
/// nothing is kept to run again at the next launch, and the daemon stopped.
pub(super) fn end_everything(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    type_line(drawn, "trap '' HUP; while :; do sleep 1; done")?;
    control::holds(drawn, "the slow shell's loop", |state| state.status == Status::Running)?;
    asked_on_quitting(drawn, said)?;
    let shell = drawn.shell.clone();
    let client = (drawn.cx)
        .update(|cx| shell.update(cx, |shell, _| shell.terminal_client()))
        .ok_or("the app has no terminal client")?;
    let list = RequestBody::ListTerminals {
        worktree: None,
        status: None,
    };
    let listed = client.request(list).map_err(|error| format!("listing: {error}"))?;
    let ResponseBody::Terminals { terminals } = listed else {
        return Err("the daemon answered the list with something else".into());
    };
    let handles = terminals.into_iter().map(|summary| summary.record.handle).collect();
    let dir = client.dir().clone();
    (drawn.cx).update(|cx| crate::quitting::end_then_quit(client, handles, cx));
    let socket = dir.socket();
    drawn.until(Duration::from_secs(15), |_| Ok(!socket.exists()))?;
    let kept = dir.remembered();
    if !kept.is_empty() {
        return Err(format!("the daemon kept {} terminals to run again", kept.len()).into());
    }
    said.push("End Everything ended a shell ignoring the hangup, kept nothing, stopped".into());
    Ok(())
}

/// ⌘Q with a terminal running and no choice kept opens the dialog; cancelling it closes it.
fn asked_on_quitting(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    press(drawn, "cmd-q")?;
    wait_until(drawn, "the quit dialog", |drawn| {
        Ok(drawn.cx.read(modal::current::<QuitDialog>).is_some())
    })?;
    let window = (drawn.cx.read(modal::window)).ok_or("the dialog has no window")?;
    (drawn.cx)
        .update_window(window, |_, window, cx| window.draw(cx).clear(cx))
        .map_err(|error| error.to_string())?;
    let image = (drawn.cx.capture_screenshot(window)).map_err(|error| error.to_string())?;
    let path = checkout()?.join(super::OUT).join("quit-dialog.png");
    image.save(&path).map_err(|error| error.to_string())?;
    said.push(format!("{}", path.display()));
    drawn.cx.update(modal::close);
    wait_until(drawn, "the quit dialog to close", |drawn| {
        Ok(drawn.cx.read(modal::current::<QuitDialog>).is_none())
    })
}

/// Whether the daemon lists terminal `handle`.
fn in_the_daemon(drawn: &mut Drawn, handle: &Handle) -> Result<bool> {
    let shell = drawn.shell.clone();
    let client = drawn
        .cx
        .update(|cx| shell.update(cx, |shell, _| shell.terminal_client()));
    let list = RequestBody::ListTerminals {
        worktree: None,
        status: None,
    };
    let listed = (client
        .ok_or("the app has no terminal client")?
        .request(list))
    .map_err(|error| format!("listing: {error}"))?;
    let ResponseBody::Terminals { terminals } = listed else {
        return Err("the daemon answered the list with something else".into());
    };
    Ok((terminals.iter()).any(|summary| summary.record.handle == *handle))
}

/// The terminal shown, once the daemon has named it.
fn shown(drawn: &mut Drawn) -> Option<Handle> {
    let shell = drawn.shell.clone();
    drawn.cx.read(|cx| {
        let shell = shell.read(cx);
        let id = shell.shown()?.0;
        shell.terminals.handle_of(id).cloned()
    })
}
