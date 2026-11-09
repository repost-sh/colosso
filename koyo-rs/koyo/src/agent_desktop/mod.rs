//! Agent desktops (`docs/agent-desktop.md`): the app's side. This part is the setup and the
//! session: what the root helper says of the Mac, the holder that keeps the agent account's
//! session, Settings → Agent desktop's actions, and the rule that a quit of the app ends the
//! session first (6.1).

mod holder;
mod setup;

use gpui_kit::{App, AppContext as _, AsyncApp, Global};
use koyo_desk_holder::helper;
use koyo_desk_wire::helper::{HelperAnswer, HelperAsk, HelperStatus};
use koyo_desk_wire::holder::{HolderAsk, HolderStatus};

/// What's under way, so the page shows one action at a time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Busy {
    #[default]
    Idle,
    SettingUp,
    Removing,
    Starting,
    Ending,
}

/// How the last action went: said on the page until the next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub message: String,
    pub steps: Vec<String>,
}

impl Outcome {
    pub fn done(message: impl Into<String>) -> Outcome {
        Outcome {
            ok: true,
            message: message.into(),
            steps: Vec::new(),
        }
    }

    pub fn failed(message: impl Into<String>) -> Outcome {
        Outcome {
            ok: false,
            message: message.into(),
            ..Outcome::done("")
        }
    }

    /// What the helper answered to an action: `done` said when it's done.
    pub fn from_helper(answer: std::io::Result<HelperAnswer>, done: &str) -> Outcome {
        match answer {
            Ok(HelperAnswer::Done { steps }) => Outcome {
                steps,
                ..Outcome::done(done)
            },
            Ok(HelperAnswer::Failed { message, steps }) => Outcome {
                steps,
                ..Outcome::failed(message)
            },
            Ok(other) => Outcome::failed(format!("the helper answered {other:?}")),
            Err(error) => Outcome::failed(error.to_string()),
        }
    }
}

#[derive(Default)]
pub struct Desk {
    /// The helper's word on the Mac: `None` until asked, `Err` when it can't be reached (not set
    /// up, or set up and broken).
    pub helper: Option<Result<HelperStatus, String>>,
    /// The holder's, when one runs.
    pub holder: Option<HolderStatus>,
    pub busy: Busy,
    pub last: Option<Outcome>,
}

impl Global for Desk {}

pub fn get(cx: &App) -> &Desk {
    cx.global::<Desk>()
}

/// Changes the state; whatever shows it observes the global.
fn change(cx: &mut App, change: impl FnOnce(&mut Desk)) {
    change(cx.global_mut::<Desk>());
}

pub fn init(cx: &mut App) {
    cx.set_global(Desk::default());
    // A quit ends the agent session before the app goes, so a restart or a logout never finds
    // it logged in (6.1). Blocking here is the point: loginwindow waits for the app.
    cx.on_app_quit(|_| async {
        if let Some(status) = holder::ask_running(&HolderAsk::End) {
            log::info!("agent desktop: the session ended on quit ({status:?})");
        }
    })
    .detach();
}

/// Asks the helper and the holder how things stand, off the main thread.
pub fn refresh(cx: &mut App) {
    let asked = cx.background_spawn(async {
        let helper = match helper::ask(&HelperAsk::Status) {
            Ok(HelperAnswer::Status(status)) => Ok(status),
            Ok(other) => Err(format!("the helper answered {other:?}")),
            Err(error) => Err(error.to_string()),
        };
        (helper, holder::ask_running(&HolderAsk::Status))
    });
    cx.spawn(async move |cx: &mut AsyncApp| {
        let (helper, holder) = asked.await;
        cx.update(|cx| {
            change(cx, |desk| {
                desk.helper = Some(helper);
                desk.holder = holder;
            });
        });
    })
    .detach();
}

/// Runs `work` off the main thread as `busy`, then says how it went and asks again.
fn act(busy: Busy, work: impl FnOnce() -> Outcome + Send + 'static, cx: &mut App) {
    if get(cx).busy != Busy::Idle {
        return;
    }
    change(cx, |desk| desk.busy = busy);
    let done = cx.background_spawn(async move { work() });
    cx.spawn(async move |cx: &mut AsyncApp| {
        let outcome = done.await;
        if !outcome.ok {
            log::warn!("agent desktop: {}", outcome.message);
        }
        cx.update(|cx| {
            change(cx, |desk| {
                desk.busy = Busy::Idle;
                desk.last = Some(outcome);
            });
            refresh(cx);
        });
    })
    .detach();
}

/// Settings → Agent desktop → Set up…: the helper's `setup`, run as root through the system's
/// administrator dialog (5.1).
pub fn set_up(cx: &mut App) {
    act(Busy::SettingUp, setup::run, cx);
}

/// Logs the agent account in (the holder) and waits for it to hold the session.
pub fn start_session(cx: &mut App) {
    let program = std::env::current_exe().ok();
    act(Busy::Starting, move || holder::start(program), cx);
}

pub fn end_session(cx: &mut App) {
    act(
        Busy::Ending,
        || {
            Outcome::done(match holder::ask_running(&HolderAsk::End) {
                Some(_) => "The agent session ended.",
                None => "No agent session was held.",
            })
        },
        cx,
    );
}

/// Settings → Agent desktop → Remove…, with the administrator's name and password the user
/// record needs (5.5). The session ends first; the helper does the rest.
pub fn remove(admin: String, password: String, cx: &mut App) {
    act(
        Busy::Removing,
        move || {
            holder::ask_running(&HolderAsk::End);
            Outcome::from_helper(
                helper::ask(&HelperAsk::Remove { admin, password }),
                "Removed: the account, its home, Screen Sharing's access, the firewall rule and \
                 the helper.",
            )
        },
        cx,
    );
}
