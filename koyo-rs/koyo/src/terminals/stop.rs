//! Stopping terminals from the app (`docs/terminals.md` 7.1, 9): the one shown, from its bar
//! or the Stop Terminal action; a tab's or a pane's, by closing it; or every terminal of a
//! checkout from its row's menu. A busy program asks first, and so does stopping a checkout's
//! terminals; stopping ends the program and closes the terminal in the daemon, whose word
//! takes its tab away unless closing it already has. Deleting a worktree stops its terminals
//! too, once its dialog has named them.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use koyo_term::{Source, TermClient};

use koyo_term_control::{Handle, RequestBody, Status};
use gpui_kit::{AppContext as _, Context, Focusable as _, PromptLevel, SharedString, Window};

use super::open_request;
use super::states::State;
use crate::shell::Shell;

impl Shell {
    /// Stops the terminal of pane `id`, asking first when its program is busy.
    pub fn stop_terminal(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(handle) = self.terminals.handle_of(id).cloned() else {
            return;
        };
        let Some(state) = self.terminals.states.get(&handle) else {
            return;
        };
        if !busy(state) {
            return self.close_terminals(vec![handle], cx);
        }
        let question = format!("Stop {}?", state.name);
        let detail = format!("Its program is {}; it ends.", doing(state));
        self.ask_to_stop(&question, &detail, vec![handle], window, cx);
    }

    /// Stops the terminal that has the keyboard, else the shown tab's first, as the Stop
    /// Terminal action does.
    pub fn stop_shown_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focused = (self.terminals.views.iter())
            .find(|(_, made)| made.view.focus_handle(cx).contains_focused(window, cx))
            .map(|(id, _)| *id);
        if let Some(id) = focused.or_else(|| Some(self.shown()?.0)) {
            self.stop_terminal(id, window, cx);
        }
    }

    /// Does `then` once the person has agreed to end what's running in the terminals of panes
    /// `ids`, which closing their tab or pane does, or at once when nothing is.
    pub(crate) fn ask_to_end_terminals(
        &mut self,
        ids: &[u64],
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A terminal the daemon has said nothing of yet may be running anything.
        let busy: Vec<(&str, String)> = (ids.iter())
            .filter_map(|id| match self.terminals.sources.get(id)? {
                Source::Existing { handle, name } => match self.terminals.states.get(handle) {
                    Some(state) => busy(state).then(|| (state.name.as_str(), doing(state))),
                    None => Some((name.as_str(), "not known yet".to_owned())),
                },
                Source::Open(_) => None,
            })
            .collect();
        let question = match busy.as_slice() {
            [] => return then(self, window, cx),
            [(name, _)] => format!("Close {name}?"),
            several => format!("Close {} terminals?", several.len()),
        };
        let listed: Vec<String> = (busy.iter())
            .map(|(name, doing)| format!("{name}: {doing}"))
            .collect();
        let detail = format!("{}. Closing ends what's running.", listed.join(" · "));
        confirm(&question, &detail, "Close", then, window, cx);
    }

    /// Closes the terminals of panes `ids` in the daemon as their tab or pane closes, so they
    /// don't come back. The tab, should ⌘⇧T open it again, starts a new shell of the name.
    pub(crate) fn end_terminals(&mut self, ids: &[u64], cx: &mut Context<Self>) {
        let panes: Vec<(PathBuf, u64)> = (self.workspace.terminal_tabs())
            .filter(|(_, id)| ids.contains(id))
            .map(|(worktree, id)| (worktree.to_path_buf(), id))
            .collect();
        let mut handles = Vec::new();
        for (worktree, id) in panes {
            match self.terminals.sources.get(&id) {
                Some(Source::Existing { handle, name }) => {
                    handles.push(handle.clone());
                    let shell = open_request(&worktree, name.clone());
                    (self.terminals.sources).insert(id, Source::Open(Box::new(shell)));
                }
                // Not attached yet, as a shell a daemon restart ended and that wasn't looked
                // at: the daemon still lists it, under its name.
                Some(Source::Open(request)) => handles.extend(
                    (self.terminals.states.of_worktree(&worktree).into_iter())
                        .find(|(_, state)| state.name == request.name)
                        .map(|(handle, _)| handle),
                ),
                None => {}
            }
        }
        self.close_terminals(handles, cx);
    }

    /// Stops every terminal of the checkout at `worktree`, once the person says so.
    pub fn stop_terminals_of(
        &mut self,
        worktree: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let terminals = self.terminals.states.of_worktree(worktree);
        if terminals.is_empty() {
            return;
        }
        let listed: Vec<String> = (terminals.iter())
            .map(|(_, state)| format!("{}: {}", state.name, doing(state)))
            .collect();
        let question = match terminals.len() {
            1 => "Stop the checkout's terminal?".to_owned(),
            count => format!("Stop the checkout's {count} terminals?"),
        };
        let detail = format!("{}. Their programs end.", listed.join(" · "));
        let handles = terminals.into_iter().map(|(handle, _)| handle).collect();
        self.ask_to_stop(&question, &detail, handles, window, cx);
    }

    /// The terminals deleting the checkout at `worktree` stops, as its dialog names them.
    pub fn terminals_listed(&self, worktree: &Path) -> Vec<SharedString> {
        (self.terminals.states.of_worktree(worktree).into_iter())
            .map(|(_, state)| format!("{} ({})", state.name, doing(state)).into())
            .collect()
    }

    /// What closes the checkout's terminals in the daemon, one after the other, to run before
    /// its folder goes, so none of their programs is left in it.
    pub fn closing_terminals_of(&mut self, worktree: &Path) -> impl FnOnce() + Send + use<> {
        let handles: Vec<Handle> = (self.terminals.states.of_worktree(worktree).into_iter())
            .map(|(handle, _)| handle)
            .collect();
        let client = (!handles.is_empty())
            .then(|| self.terminals.client())
            .flatten();
        move || {
            if let Some(client) = client {
                close_all(&client, handles);
            }
        }
    }

    fn ask_to_stop(
        &mut self,
        question: &str,
        detail: &str,
        handles: Vec<Handle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let stop = move |this: &mut Self, _: &mut Window, cx: &mut Context<Self>| {
            this.close_terminals(handles, cx);
        };
        confirm(question, detail, "Stop", stop, window, cx);
    }

    /// Closes `handles` in the daemon; their tabs go when it says they closed.
    fn close_terminals(&mut self, handles: Vec<Handle>, cx: &mut Context<Self>) {
        let Some(client) = self.terminals.client() else {
            return;
        };
        cx.background_spawn(async move { close_all(&client, handles) })
            .detach();
    }
}

/// Asks `question`, and does `then` if the person answers `action`.
fn confirm(
    question: &str,
    detail: &str,
    action: &str,
    then: impl FnOnce(&mut Shell, &mut Window, &mut Context<Shell>) + 'static,
    window: &mut Window,
    cx: &mut Context<Shell>,
) {
    let answer = window.prompt(
        PromptLevel::Warning,
        question,
        Some(detail),
        &[action, "Cancel"],
        cx,
    );
    cx.spawn_in(window, async move |this, cx| {
        if answer.await == Ok(0) {
            this.update_in(cx, then).ok();
        }
    })
    .detach();
}

/// Closes `handles` in the daemon, waiting for each.
pub(crate) fn close_all(client: &Arc<TermClient>, handles: Vec<Handle>) {
    for handle in handles {
        let close = RequestBody::Close {
            handle: handle.clone(),
            force: true,
        };
        if let Err(error) = client.request(close) {
            log::warn!("couldn't stop {handle}: {error}");
        }
    }
}

/// Whether ending the terminal's program may cut something short: anything but a program
/// that ended or a shell at its prompt. Only a shell's marks say it's at its prompt; without
/// them, as for bash or a program run directly, idle only means quiet.
fn busy(state: &State) -> bool {
    match state.status {
        Status::Exited => false,
        Status::Idle => state.reason != "prompt",
        Status::Running | Status::NeedsInput | Status::Unknown => true,
    }
}

/// What the program is doing, as the question says it.
fn doing(state: &State) -> String {
    let status = super::header::words(state.status);
    if state.reason.is_empty() {
        status.to_string()
    } else {
        format!("{status}, {}", state.reason)
    }
}

#[cfg(test)]
mod tests;
