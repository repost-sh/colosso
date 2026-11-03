//! Stopping terminals from the app (`docs/terminals.md` 7.1, 9): the one shown, from its bar
//! or the Stop Terminal action; a tab's, by ⌥-clicking its ×; or every terminal of a checkout
//! from its row's menu. A busy program asks first, and so does stopping a checkout's
//! terminals; stopping ends the program and closes the terminal in the daemon, whose word
//! takes its tab away. Deleting a worktree stops its terminals too, once its dialog has named
//! them.

use std::path::Path;
use std::sync::Arc;

use koyo_term::TermClient;

use koyo_term_control::{Handle, RequestBody, Status};
use gpui_kit::{
    AppContext as _, AsyncApp, Context, Focusable as _, PromptLevel, SharedString, Window,
};

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

    /// Stops the terminals of the tab at `at`, asking first when one is busy; whether it has
    /// any, else its × closes it as a plain click does.
    pub fn stop_tab_terminals(
        &mut self,
        at: (usize, usize, usize),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(tab) = self.workspace.tab_mut(at) else {
            return false;
        };
        let ids: Vec<u64> = (tab.panes().into_iter())
            .filter_map(|pane| pane.terminal)
            .collect();
        let terminals: Vec<(Handle, &State)> = (ids.iter())
            .filter_map(|id| self.terminals.handle_of(*id))
            .filter_map(|handle| Some((handle.clone(), self.terminals.states.get(handle)?)))
            .collect();
        if terminals.is_empty() {
            return !ids.is_empty();
        }
        let busy: Vec<String> = (terminals.iter())
            .filter(|(_, state)| busy(state))
            .map(|(_, state)| format!("{}: {}", state.name, doing(state)))
            .collect();
        let handles = terminals.into_iter().map(|(handle, _)| handle).collect();
        if busy.is_empty() {
            self.close_terminals(handles, cx);
        } else {
            let detail = format!("{}. Their programs end.", busy.join(" · "));
            self.ask_to_stop("Stop the tab's terminals?", &detail, handles, window, cx);
        }
        true
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
        let answer = window.prompt(
            PromptLevel::Warning,
            question,
            Some(detail),
            &["Stop", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx: &mut AsyncApp| {
            if answer.await == Ok(0) {
                this.update(cx, |this, cx| this.close_terminals(handles, cx))
                    .ok();
            }
        })
        .detach();
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

/// Closes `handles` in the daemon, waiting for each.
fn close_all(client: &Arc<TermClient>, handles: Vec<Handle>) {
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

/// Whether ending the terminal's program would cut something short.
fn busy(state: &State) -> bool {
    matches!(state.status, Status::Running | Status::NeedsInput)
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
