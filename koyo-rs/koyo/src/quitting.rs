//! Quitting: ⌘Q and the app's menu, where macOS looks for it. Terminals outlive the app in their
//! daemon and chats in the agent host, so a person quitting with some still running says whether
//! they keep running or stop, or has said so for good in Settings → General. Unsaved edits are
//! asked about first. Quitting from the Dock or by logging out can't be held up to ask, and leaves
//! them running.
//!
//! A restart into an update (`updates::RestartToUpdate`) is not a quit of what runs: it asks
//! about unsaved edits and pages as a quit does, and then ends the app. Terminals and chats
//! aren't asked about and aren't stopped, whatever Settings → General says a quit does: they
//! live in their daemons, and the Koyo that opens picks them up.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::{
    Action, App, AppContext as _, Context, FontWeight, IntoElement, KeyBinding, Menu, MenuItem,
    ParentElement as _, Render, SharedString, Styled as _, Window, actions, div, px,
};
use koyo_agent::agent_client::AgentClient;
use koyo_agent::protocol::methods::{Empty, ThreadsWatch};
use koyo_agent::protocol::model::ThreadSummary;
use koyo_term::TermClient;
use koyo_term_control::{Handle, RequestBody, ResponseBody};
use serde::{Deserialize, Serialize};

use crate::modal::{self, Button, FOOTER, HEADING_LINE, LINE, PADDING, SMALL_LINE, Tone};
use crate::model::PaneKind;
use crate::navigation::OpenSettings;
use crate::settings::{self, checked_menu};
use crate::terminals::{close_all, daemon_client};
use crate::{theme, updates};

actions!(app, [Quit]);

/// What quitting does with the terminals and chats still running.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OnQuit {
    #[default]
    Ask,
    Keep,
    End,
}

/// Sets what quitting does, from the General settings' menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetOnQuit(pub OnQuit);

impl OnQuit {
    const ALL: [OnQuit; 3] = [OnQuit::Ask, OnQuit::Keep, OnQuit::End];

    pub fn label(self) -> &'static str {
        match self {
            OnQuit::Ask => "Ask every time",
            OnQuit::Keep => "Keep running",
            OnQuit::End => "Stop everything",
        }
    }

    /// A menu of the choices with `self` checked.
    pub fn menu(self) -> gpui_kit::component::native_menu::NativeMenu {
        checked_menu(OnQuit::ALL, self, OnQuit::label, |choice| {
            Box::new(SetOnQuit(choice))
        })
    }
}

pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
    cx.on_action(|_: &Quit, cx| cx.defer(|cx| ask_all_then(deal_with_what_runs, cx)));
    cx.on_action(|_: &updates::RestartToUpdate, cx| {
        cx.defer(|cx| ask_all_then(end_into_update, cx));
    });
    cx.on_action(|SetOnQuit(choice): &SetOnQuit, cx| {
        settings::update(cx, |settings| settings.on_quit = *choice);
    });
}

/// The menu bar: macOS gives the first menu the app's name, and with no window open, no view
/// is left to hear ⌘Q but the menu.
pub fn set_menu(cx: &App) {
    cx.set_menus(vec![Menu::new("Koyo").items([
        MenuItem::action("Settings…", OpenSettings),
        MenuItem::separator(),
        MenuItem::action("Quit Koyo", Quit),
    ]), crate::edit_menu::menu()]);
}

/// What runs on without the app: the daemon's terminals, to end, with the names of those
/// running, and the titles of the host's chats still at work.
#[derive(Default)]
struct Listed {
    handles: Vec<Handle>,
    running: Vec<String>,
    chats: Vec<String>,
}

/// What's next, by what's been chosen and how much runs.
#[derive(Debug, PartialEq)]
enum Next {
    Quit,
    Ask,
    End,
}

fn next(choice: OnQuit, running: usize) -> Next {
    match choice {
        OnQuit::End => Next::End,
        OnQuit::Ask if running > 0 => Next::Ask,
        OnQuit::Ask | OnQuit::Keep => Next::Quit,
    }
}

/// Asks about unsaved edits, and the pages whether the person may leave them, then goes on to
/// `then`: what runs, for a quit; handing the staged release over, for a restart into it.
fn ask_all_then(then: fn(&mut App), cx: &mut App) {
    let Some((window, shell)) = crate::control::main_shell(cx) else {
        return then(cx);
    };
    (window.update(cx, |_, window, cx| {
        shell.update(cx, |shell, cx| shell.ask_all_then(then, window, cx));
    }))
    .ok();
}

/// Hands the staged release to its installer, which opens Koyo again once it's in place, and
/// quits only if the installer started: with none, the app stays open and says the update
/// failed.
fn end_into_update(cx: &mut App) {
    let handed = updates::hand_over_to_reopen(cx);
    cx.spawn(async move |cx| {
        if handed.await {
            cx.update(|cx| cx.quit());
        }
    })
    .detach();
}

/// What stopping everything ends, where it runs: the daemon's terminals, and the agent host
/// through the app's own client when there is one, which then starts it no more.
pub(crate) struct Running {
    pub(crate) terminals: Option<Arc<TermClient>>,
    pub(crate) handles: Vec<Handle>,
    /// The app's data folder, which the agent host serves chats from.
    pub(crate) data: Option<PathBuf>,
    pub(crate) chats: Option<Arc<AgentClient>>,
}

/// Acts on the choice made, looking first for what runs on unless it can't matter.
fn deal_with_what_runs(cx: &mut App) {
    if settings::get(cx).on_quit == OnQuit::Keep {
        return cx.quit();
    }
    let shown = crate::control::main_shell(cx);
    let terminals = shown
        .clone()
        .and_then(|(_, shell)| shell.update(cx, |shell, _| shell.terminals.client()))
        .or_else(daemon_client);
    let chats = shown.and_then(|(_, shell)| shell.read(cx).chats().read(cx).client());
    let data = settings::data_dir().filter(|_| !cfg!(test));
    cx.spawn(async move |cx| {
        let (listing, asking) = (terminals.clone(), data.clone());
        let listed = cx
            .background_spawn(async move {
                let mut listed = listing.map(|client| listed(&client)).unwrap_or_default();
                listed.chats = asking.map(|data| at_work(&data)).unwrap_or_default();
                listed
            })
            .await;
        cx.update(|cx| {
            let running = Running {
                terminals,
                handles: listed.handles.clone(),
                data,
                chats,
            };
            let count = listed.running.len() + listed.chats.len();
            match next(settings::get(cx).on_quit, count) {
                Next::Quit => cx.quit(),
                Next::End => stop_then_quit(running, cx),
                Next::Ask => ask(running, listed, cx),
            }
        });
    })
    .detach();
}

fn listed(client: &TermClient) -> Listed {
    let list = RequestBody::ListTerminals {
        worktree: None,
        status: None,
    };
    let records = match client.request_if_running(list) {
        Ok(Some(ResponseBody::Terminals { terminals })) => terminals,
        Ok(_) => return Listed::default(),
        Err(error) => {
            log::warn!("couldn't list the running terminals before quitting: {error}");
            return Listed::default();
        }
    };
    let mut found = Listed::default();
    for summary in records {
        if summary.record.exit.is_none() {
            found.running.push(summary.record.name);
        }
        found.handles.push(summary.record.handle);
    }
    found.running.sort();
    found
}

/// The titles of the host's chats still at work, asked of a host that runs in the app's data
/// folder `data`, starting none.
fn at_work(data: &Path) -> Vec<String> {
    let dir = koyo_agent::client::host_dir(data);
    let summaries = match koyo_agent::client::request_if_running::<ThreadsWatch>(&dir, &Empty {}) {
        Ok(summaries) => summaries.unwrap_or_default(),
        Err(error) => {
            log::warn!("couldn't list the chats at work before quitting: {error}");
            return Vec::new();
        }
    };
    let mut titles: Vec<String> = (summaries.iter())
        .filter(|summary| works(summary))
        .map(|summary| match summary.title.trim() {
            "" => PaneKind::Chat.new_tab_title().to_owned(),
            title => title.to_owned(),
        })
        .collect();
    titles.sort();
    titles
}

/// Whether a chat is at work, or waits for the person, all of which stopping the host would end.
fn works(summary: &ThreadSummary) -> bool {
    summary.work().any() || summary.needs.is_some()
}

/// Closes the terminals rather than only stopping their daemon, whose shells would run again at
/// the next launch, as after a restart, then stops the daemon; stops the agent host, which stops
/// each chat's turn and ends what it runs in the background; then quits. A close is answered
/// before its program has gone, and stopping the daemon then would keep the terminal, so this
/// waits for the list to empty, for as long as the daemon gives a program to end.
pub(crate) fn stop_then_quit(running: Running, cx: &mut App) {
    let Running {
        terminals,
        handles,
        data,
        chats,
    } = running;
    let executor = cx.background_executor().clone();
    let ending = cx.background_spawn(async move {
        let Some(client) = terminals else {
            return;
        };
        close_all(&client, handles);
        let given_up = Instant::now() + ENDING;
        while !listed(&client).handles.is_empty() && Instant::now() < given_up {
            executor.timer(Duration::from_millis(50)).await;
        }
        if let Err(error) = client.shutdown() {
            log::warn!("couldn't stop the terminal daemon: {error}");
        }
    });
    let stopping = cx.background_spawn(async move {
        let stopped = match (chats, data) {
            (Some(client), _) => client.stop_for_good(),
            (None, Some(data)) => {
                koyo_agent::client::stop_host(&koyo_agent::client::host_dir(&data))
            }
            (None, None) => Ok(()),
        };
        if let Err(error) = stopped {
            log::warn!("couldn't stop the agent host: {error}");
        }
    });
    cx.spawn(async move |cx| {
        ending.await;
        stopping.await;
        cx.update(|cx| cx.quit());
    })
    .detach();
}

/// Asks over the window, opening it again first if it was closed.
fn ask(running: Running, listed: Listed, cx: &mut App) {
    if crate::control::main_shell(cx).is_some() {
        return ask_in_window(running, listed, cx);
    }
    crate::reopen_main_window(cx, move |opened, cx| {
        if let Err(error) = opened {
            log::warn!("couldn't open the window to ask about quitting: {error:#}");
        }
        ask_in_window(running, listed, cx);
    });
}

/// Asks over the window, or quits with no window to ask in.
fn ask_in_window(running: Running, listed: Listed, cx: &mut App) {
    let Some((window, _)) = crate::control::main_shell(cx) else {
        return cx.quit();
    };
    let dialog = QuitDialog {
        running: listed.running,
        chats: listed.chats,
        remember: false,
        end: Some(Box::new(move |cx| stop_then_quit(running, cx))),
    };
    (window.update(cx, |_, window, cx| {
        modal::open(WIDTH, HEIGHT, window, cx, |_, _| dialog);
    }))
    .ok();
}

/// How long a program gets to end: a little over the daemon's grace (`CLOSE_GRACE`).
const ENDING: Duration = Duration::from_secs(6);
const WIDTH: f32 = 480.;
const GAP: f32 = 8.;
/// Its heading, what runs on up to two lines, the choice to remember and the footer.
const HEIGHT: f32 = 2. * PADDING
    + HEADING_LINE
    + GAP
    + 2. * LINE
    + GAP
    + SMALL_LINE
    + 2. * GAP
    + LINE
    + 2. * GAP
    + FOOTER;

/// Stops everything and quits.
type End = Box<dyn FnOnce(&mut App)>;

pub(crate) struct QuitDialog {
    /// The terminals still running, by name.
    running: Vec<String>,
    /// The chats still at work, by title.
    chats: Vec<String>,
    remember: bool,
    /// Taken once it's chosen.
    end: Option<End>,
}

impl QuitDialog {
    fn what_runs(&self) -> SharedString {
        let names = [self.running.as_slice(), self.chats.as_slice()]
            .concat()
            .join(", ");
        let (terminals, chats) = (self.running.len(), self.chats.len());
        let said = match (terminals, chats) {
            (1, 0) => format!("A terminal is still running: {names}"),
            (count, 0) => format!("{count} terminals are still running: {names}"),
            (0, 1) => format!("A chat is still running: {names}"),
            (0, count) => format!("{count} chats are still running: {names}"),
            _ => format!(
                "{} and {} are still running: {names}",
                counted(terminals, "terminal"),
                counted(chats, "chat")
            ),
        };
        if said.ends_with(['.', '!', '?', '…']) {
            said.into()
        } else {
            format!("{said}.").into()
        }
    }

    /// Takes the choice in for good if asked to.
    fn chose(&self, choice: OnQuit, cx: &mut App) {
        if self.remember {
            settings::update(cx, |settings| settings.on_quit = choice);
        }
    }

    fn keep(&mut self, cx: &mut Context<Self>) {
        self.chose(OnQuit::Keep, cx);
        cx.quit();
    }

    /// Stops everything and quits, as its Stop Everything button does.
    pub(crate) fn end(&mut self, cx: &mut Context<Self>) {
        let Some(end) = self.end.take() else {
            return;
        };
        self.chose(OnQuit::End, cx);
        cx.notify();
        end(cx);
    }
}

impl Render for QuitDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let ending = self.end.is_none();
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(modal::heading("Quit Koyo?"))
            .child(
                div()
                    .h(px(2. * LINE))
                    .overflow_hidden()
                    .child(self.what_runs()),
            )
            .child(
                modal::small_line()
                    .text_color(p.muted)
                    .child("They carry on while Koyo is closed, unless you stop everything."),
            )
            .child(
                div()
                    .mt(px(GAP))
                    .h(px(LINE))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Checkbox::new("quit-remember")
                            .label("Don't ask again")
                            .checked(self.remember)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.remember = *checked;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::NORMAL)
                            .text_color(p.muted)
                            .child("Settings → General changes it."),
                    ),
            )
            .child(div().flex_1())
            .child(
                modal::footer(None, p)
                    .child(modal::cancel("quit-cancel").disabled(ending))
                    .child(
                        Button::new("quit-keep", "Keep Running", Tone::Primary)
                            .disabled(ending)
                            .on_click(cx.listener(|this, _, _, cx| this.keep(cx))),
                    )
                    .child(
                        Button::new("quit-end", "Stop Everything", Tone::Danger)
                            .busy(ending)
                            .on_click(cx.listener(|this, _, _, cx| this.end(cx))),
                    ),
            )
    }
}

/// `count` of `what`, as "1 chat" or "2 chats".
fn counted(count: usize, what: &str) -> String {
    if count == 1 {
        format!("1 {what}")
    } else {
        format!("{count} {what}s")
    }
}

#[cfg(test)]
mod tests;
