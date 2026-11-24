//! What a row picked in the launcher does, in the window behind it, on the checkout it opened
//! on: a tab opened empty, at a page or a search, a chat asked or a command run in a new one,
//! or a tab shown; or the New Worktree dialog opened on its project with the name typed. What was
//! typed for it the history keeps. Opened on a page (⌘L), a page or a search picked loads in
//! that page instead of a new tab, as an address typed in its own field would. Opened for an
//! unpinned page, the page opens unpinned in the space shown, whatever tab is shown.

use gpui_kit::{AnyWindowHandle, App, Context, WeakEntity, Window};

use super::{Kind, Opens};
use super::rows::Act;
use crate::browser::Address;
use crate::history::{self, Place};
use crate::keyboard::Intent;
use crate::new_worktree;
use crate::shell::Shell;

/// Does `act` in `behind`, the window `shell` shows, for what was typed at `place`, in the page
/// of the pane `on` when the launcher opened on one, a page opening as `opens` says.
pub fn launch(
    shell: &WeakEntity<Shell>,
    behind: AnyWindowHandle,
    act: Act,
    (place, on, opens): (&Place, Option<u64>, Opens),
    cx: &mut App,
) {
    let done = behind.update(cx, |_, window, cx| {
        shell.update(cx, |shell, cx| {
            let act = match on {
                Some(pane) => load_in_page(shell, pane, act, cx),
                None => Some(act),
            };
            if let Some(act) = act {
                act_on(shell, act, (place, opens), window, cx);
            }
        })
    });
    if let Err(error) = done.and_then(|done| done) {
        log::warn!("couldn't open what the launcher picked: {error}");
    }
}

/// Loads what `act` visits or searches for in the page of pane `pane`, as a visit typed in its
/// address: the page takes the keyboard, and the person is in its pane. Gives `act` back when
/// it's another kind of pick, or the page closed meanwhile, to do as ⌘T's launcher would.
fn load_in_page(shell: &mut Shell, pane: u64, act: Act, cx: &mut Context<Shell>) -> Option<Act> {
    let open = shell.workspace.pane(pane);
    let Some(view) = open.and_then(|open| open.view().cloned()) else {
        return Some(act);
    };
    let address = match act {
        Act::Visit(url) => Address::Page(url),
        Act::Search(words) => Address::Search(words),
        other => return Some(other),
    };
    shell.enter_pane(pane, cx);
    view.update(cx, |view, cx| view.go_to(address, cx));
    None
}

/// Does `act` for what was typed at `place`: a page it opens is an unpinned one when `opens`
/// says so, else a tab where the person is (`Shell::open_browser_tab`).
fn act_on(
    shell: &mut Shell,
    act: Act,
    (place, opens): (&Place, Opens),
    window: &mut Window,
    cx: &mut Context<Shell>,
) {
    let opening = (opens.unpinned(), false);
    match act {
        Act::Empty(Kind::Browser) => {
            shell.open_browser_tab(None, opening, None, window, cx);
        }
        Act::Empty(kind) => {
            if let Some(pane) = kind.pane() {
                shell.new_tab(pane, cx);
            }
        }
        Act::StartWorktree(name) => {
            new_worktree::open_named(cx.weak_entity(), place.project.clone(), &name, window, cx);
        }
        Act::Visit(url) => {
            if let Some(pane) = shell.open_browser_tab(Some(url), opening, None, window, cx) {
                shell.mark_typed(pane, cx);
            }
        }
        Act::Search(words) => {
            history::used(history::Kind::Search, &words, place, cx);
            let results = crate::settings::get(cx).search_engine.results(&words);
            shell.open_browser_tab(Some(results), opening, None, window, cx);
        }
        Act::Switch(pane) => {
            shell.focus_pane(pane, Intent::Launcher, window, cx);
        }
        Act::Resume(id) => {
            shell.open_chat(&id, cx);
        }
        Act::Ask(text) => shell.ask_new_chat(text, cx),
        Act::Device(chosen) => {
            if let Some(pane) = shell.device_pane(Some(chosen), true, cx) {
                if let Some(view) = shell.device_view_of(pane) {
                    view.update(cx, |view, cx| view.start_asking(window, cx));
                }
                shell.focus_pane(pane, Intent::Launcher, window, cx);
            }
        }
        Act::Run(line) => {
            history::used(history::Kind::Command, &line, place, cx);
            shell.run_in_new_terminal(line, cx);
        }
    }
}
