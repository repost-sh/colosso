//! Quitting: ⌘Q and the app's menu, where macOS looks for it. Terminals outlive the app in
//! their daemon, so a person quitting with some running says whether they end or stay, or has
//! said so for good in Settings → General. Unsaved edits are asked about first. Quitting from
//! the Dock or by logging out can't be held up to ask, and leaves terminals running.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::{
    Action, App, AppContext as _, Context, FontWeight, IntoElement, KeyBinding, Menu, MenuItem,
    ParentElement as _, Render, SharedString, Styled as _, Window, actions, div, px,
};
use koyo_term::TermClient;
use koyo_term_control::{Handle, RequestBody, ResponseBody};
use serde::{Deserialize, Serialize};

use crate::files::{self, Then};
use crate::modal::{self, Button, FOOTER, HEADING_LINE, LINE, PADDING, SMALL_LINE, Tone};
use crate::settings::{self, checked_menu};
use crate::terminals::{close_all, daemon_client};
use crate::theme;

actions!(app, [Quit]);

/// What quitting does with the terminals still running.
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
            OnQuit::Keep => "Keep terminals running",
            OnQuit::End => "End everything",
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
    // After the key's dispatch, which holds the window that `quit` looks in.
    cx.on_action(|_: &Quit, cx| cx.defer(quit));
    cx.on_action(|SetOnQuit(choice): &SetOnQuit, cx| {
        settings::update(cx, |settings| settings.on_quit = *choice);
    });
}

/// The menu bar: macOS gives the first menu the app's name, and with no window open, no view
/// is left to hear ⌘Q but the menu.
pub fn set_menu(cx: &App) {
    cx.set_menus(vec![
        Menu::new("Koyo").items([MenuItem::action("Quit Koyo", Quit)]),
    ]);
}

/// What the daemon runs and keeps: the terminals to end, and the names of those running.
#[derive(Default)]
struct Listed {
    handles: Vec<Handle>,
    running: Vec<String>,
}

/// What's next, by what's been chosen and how many terminals run.
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

/// Asks about unsaved edits, then goes on to the terminals.
fn quit(cx: &mut App) {
    let Some((window, shell)) = crate::control::main_shell(cx) else {
        return deal_with_terminals(cx);
    };
    let unsaved = shell.read(cx).unsaved(cx);
    if unsaved.is_empty() {
        return deal_with_terminals(cx);
    }
    (window.update(cx, |_, window, cx| {
        files::ask(
            unsaved,
            Then::Close(Box::new(deal_with_terminals)),
            window,
            cx,
        );
    }))
    .ok();
}

/// Acts on the choice made, looking first for what runs in the daemon unless it can't matter.
fn deal_with_terminals(cx: &mut App) {
    if settings::get(cx).on_quit == OnQuit::Keep {
        return cx.quit();
    }
    let shown = crate::control::main_shell(cx);
    // The app's own client where there is one: its views then don't start the daemon again.
    let client = shown
        .and_then(|(_, shell)| shell.update(cx, |shell, _| shell.terminals.client()))
        .or_else(daemon_client);
    let Some(client) = client else {
        return cx.quit();
    };
    cx.spawn(async move |cx| {
        let listing = Arc::clone(&client);
        let listed = cx.background_spawn(async move { listed(&listing) }).await;
        cx.update(
            |cx| match next(settings::get(cx).on_quit, listed.running.len()) {
                Next::Quit => cx.quit(),
                Next::End => end_then_quit(client, listed.handles, cx),
                Next::Ask => ask(client, listed, cx),
            },
        );
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

/// Closes `handles` rather than only stopping the daemon, whose shells would run again at the
/// next launch, as after a restart; then stops the daemon and quits. A close is answered
/// before its program has gone, and stopping the daemon then would keep the terminal, so this
/// waits for the list to empty, for as long as the daemon gives a program to end.
pub(crate) fn end_then_quit(client: Arc<TermClient>, handles: Vec<Handle>, cx: &mut App) {
    let executor = cx.background_executor().clone();
    let ending = cx.background_spawn(async move {
        close_all(&client, handles);
        let given_up = Instant::now() + ENDING;
        while !listed(&client).handles.is_empty() && Instant::now() < given_up {
            executor.timer(Duration::from_millis(50)).await;
        }
        if let Err(error) = client.shutdown() {
            log::warn!("couldn't stop the terminal daemon: {error}");
        }
    });
    cx.spawn(async move |cx| {
        ending.await;
        cx.update(|cx| cx.quit());
    })
    .detach();
}

/// Asks over the window, opening it again if it was closed.
fn ask(client: Arc<TermClient>, listed: Listed, cx: &mut App) {
    if crate::control::main_shell(cx).is_none()
        && let Err(error) = crate::open_main_window(cx)
    {
        log::warn!("couldn't open the window to ask about quitting: {error:#}");
    }
    let Some((window, _)) = crate::control::main_shell(cx) else {
        return cx.quit();
    };
    let Listed { handles, running } = listed;
    let dialog = QuitDialog {
        running,
        remember: false,
        end: Some(Box::new(move |cx| end_then_quit(client, handles, cx))),
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

/// Ends everything and quits.
type End = Box<dyn FnOnce(&mut App)>;

pub(crate) struct QuitDialog {
    running: Vec<String>,
    remember: bool,
    /// Taken once it's chosen.
    end: Option<End>,
}

impl QuitDialog {
    fn what_runs(&self) -> SharedString {
        let names = self.running.join(", ");
        match self.running.len() {
            1 => format!("A terminal is still running: {names}."),
            count => format!("{count} terminals are still running: {names}."),
        }
        .into()
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

    fn end(&mut self, cx: &mut Context<Self>) {
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
                    .child("Keep running to carry on next launch, or end everything for good."),
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
                        Button::new("quit-end", "End Everything", Tone::Danger)
                            .busy(ending)
                            .on_click(cx.listener(|this, _, _, cx| this.end(cx))),
                    ),
            )
    }
}

#[cfg(test)]
mod tests;
