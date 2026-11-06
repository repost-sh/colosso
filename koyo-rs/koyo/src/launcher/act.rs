//! What a row picked in the launcher does, in the window behind it, on the checkout it opened
//! on: a tab opened empty, at a page or a search, a chat asked or a command run in a new one,
//! or a tab shown. What was typed for it the history keeps.

use gpui_kit::{AnyWindowHandle, App, Context, WeakEntity, Window};

use super::Kind;
use super::rows::Act;
use crate::browser::search_url;
use crate::history::{self, Place};
use crate::shell::Shell;

/// Does `act` in `behind`, the window `shell` shows, for what was typed at `place`.
pub fn launch(
    shell: &WeakEntity<Shell>,
    behind: AnyWindowHandle,
    act: Act,
    place: &Place,
    cx: &mut App,
) {
    let done = behind.update(cx, |_, window, cx| {
        shell.update(cx, |shell, cx| act_on(shell, act, place, window, cx))
    });
    if let Err(error) = done.and_then(|done| done) {
        log::warn!("couldn't open what the launcher picked: {error}");
    }
}

fn act_on(
    shell: &mut Shell,
    act: Act,
    place: &Place,
    window: &mut Window,
    cx: &mut Context<Shell>,
) {
    match act {
        Act::Empty(Kind::Browser) => shell.open_browser(None, window, cx),
        Act::Empty(kind) => shell.new_tab(kind.pane(), cx),
        Act::Visit(url) => {
            if let Some(pane) = shell.open_browser_tab(Some(url), false, window, cx) {
                shell.mark_typed(pane, cx);
            }
        }
        Act::Search(words) => {
            history::used(history::Kind::Search, &words, place, cx);
            shell.open_browser(Some(search_url(&words)), window, cx);
        }
        Act::Switch(pane) => {
            shell.focus_pane(pane, window, cx);
        }
        Act::Resume(id) => {
            shell.open_chat(&id, cx);
        }
        Act::Ask(text) => shell.ask_new_chat(text, cx),
        Act::Run(line) => {
            history::used(history::Kind::Command, &line, place, cx);
            shell.run_in_new_terminal(line, cx);
        }
    }
}
