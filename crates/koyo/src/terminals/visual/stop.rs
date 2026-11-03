//! Stopping a terminal from the app (`docs/terminals.md` 7.1): a second shell opened, then
//! stopped with Stop Terminal as its bar's Stop does; idle, it asks nothing, the daemon closes
//! it, and its tab goes, the first shell shown again.

use koyo_term_control::{Handle, RequestBody, ResponseBody, Status};

use super::control;
use super::drive::{act, wait_until};
use crate::shell::{NewTerminal, StopTerminal};
use crate::visual::{Drawn, Result};

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
    if terminals
        .iter()
        .any(|summary| summary.record.handle == second)
    {
        return Err("the stopped terminal is still in the daemon".into());
    }
    said.push("Stop Terminal on an idle shell closed it in the daemon, and its tab went".into());
    Ok(())
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
