//! Claude accounts in the app (`docs/claude-accounts.md`): the accounts crate's switcher
//! (`koyo-rs/koyo-accounts`) does the work, off the main thread (`service.rs`); this keeps what
//! the sidebar's button, its panel and Settings → Accounts show, asks for readings on Orca's
//! cadence (`poll.rs`), takes each chat's rate-limit headers as the active account's live
//! numbers, and does what the person asks: switch, add the Mac's login, remove, hold out.
//!
//! With Settings → Accounts → Manage Claude accounts off, none of it runs: Koyo neither reads
//! nor writes the login and polls nothing.

mod auto;
mod button;
mod environment;
mod model;
pub(crate) use model::{Problem, Row, Slot, View};
mod panel;
mod poll;
mod reading;
mod remove;
pub use remove::ask as ask_remove;
mod service;
pub(crate) use service::Job;
#[cfg(feature = "visual-check")]
pub mod visual;
#[cfg(test)]
pub(crate) use service::TestEnv;
pub(crate) mod words;

use std::time::Duration;

use gpui_kit::{AnyView, App, AppContext as _, Context, Entity, Global, Task, Window};
use koyo_accounts::Env;
use koyo_accounts::env::{Clock as _, SystemClock};
use koyo_accounts::usage_store::due_candidate;
use koyo_agent::protocol::limits::AccountLimits;

use crate::menu::Trigger;
use crate::settings;

/// The time, in seconds since the epoch: the accounts crate's clock.
pub fn now() -> f64 {
    SystemClock.now()
}

pub struct Accounts {
    view: View,
    /// The button, which the panel opens from.
    trigger: Trigger,
    /// What runs while Koyo manages the login.
    running: Option<Running>,
}

struct Running {
    service: service::Service,
    cadence: poll::Cadence,
    _looking: Task<()>,
}

struct TheAccounts {
    accounts: Entity<Accounts>,
    button: Entity<button::AccountsButton>,
}

impl Global for TheAccounts {}

/// Makes the accounts and their button, and follows Manage Claude accounts.
pub fn init(cx: &mut App) {
    let accounts = cx.new(|_| Accounts {
        view: View::default(),
        trigger: Trigger::new("accounts-panel"),
        running: None,
    });
    let button = cx.new(|cx| button::AccountsButton::new(accounts.clone(), cx));
    cx.set_global(TheAccounts {
        accounts: accounts.clone(),
        button,
    });
    let follow = move |cx: &mut App| {
        let manage = settings::get(cx).accounts.manage;
        accounts.update(cx, |accounts, cx| accounts.follow(manage, cx));
    };
    follow(cx);
    settings::observe_app(cx, follow).detach();
}

pub fn accounts(cx: &App) -> Entity<Accounts> {
    cx.global::<TheAccounts>().accounts.clone()
}

/// What the button and the panel show, for agents (`control/exec/accounts.rs`).
pub(crate) fn view(cx: &App) -> &View {
    &cx.global::<TheAccounts>().accounts.read(cx).view
}

/// The sidebar foot's accounts button.
pub fn button(cx: &App) -> Option<AnyView> {
    cx.try_global::<TheAccounts>()
        .map(|the| the.button.clone().into())
}

/// A window of Koyo's came to the front: the login is read if its reading is old.
pub fn window_activated(cx: &mut App) {
    if let Some(the) = cx.try_global::<TheAccounts>() {
        the.accounts.clone().update(cx, Accounts::look);
    }
}

/// A chat's response said how the account's limits stand: the active account's reading, kept
/// current between readings of the usage endpoint.
pub fn limits_heard(limits: &AccountLimits, cx: &mut App) {
    if let Some(the) = cx.try_global::<TheAccounts>() {
        let limits = reading::live_limits(limits);
        (the.accounts.clone()).update(cx, |accounts, cx| accounts.heard(limits, cx));
    }
}

impl Accounts {
    /// Starts or stops with Manage Claude accounts, and auto-switch with its own setting.
    fn follow(&mut self, manage: bool, cx: &mut Context<Self>) {
        self.follow_auto(cx);
        match (manage, self.running.is_some()) {
            (true, false) => {
                self.running = Some(Running {
                    service: service::Service::start(cx),
                    cadence: poll::Cadence::default(),
                    _looking: cx.spawn(async move |this, cx| {
                        // At least once a minute, so an open panel's minute comes round.
                        while let Ok(Some(wait)) = this.update(cx, |accounts, cx| {
                            let open = accounts.trigger.is_open(cx);
                            (accounts.running.as_ref()).map(|running| {
                                let next = (running.service.on_hand())
                                    .then(|| running.cadence.next_look(now(), open));
                                next.map_or(poll::OTHERS_EVERY, |next| next.min(poll::OTHERS_EVERY))
                            })
                        }) {
                            cx.background_executor()
                                .timer(Duration::from_secs_f64(wait))
                                .await;
                            if this.update(cx, Accounts::look).is_err() {
                                return;
                            }
                        }
                    }),
                });
            }
            (false, true) => {
                self.running = None;
                self.view = View::default();
                self.trigger.close(cx);
            }
            _ => return,
        }
        cx.notify();
    }

    /// The accounts can be read now, once the login shell has said where Claude Code keeps
    /// the login: the cadence's first look.
    fn made(&mut self, env: Env, cx: &mut Context<Self>) {
        if let Some(running) = &mut self.running {
            running.service.on(env);
            self.look(cx);
            self.follow_auto(cx);
        }
    }

    /// Hands the switcher `job`, while Koyo manages the accounts and once it can be asked;
    /// the person's job only while no other of theirs waits or runs. A switch reads the new
    /// active account, as the cadence would.
    pub(crate) fn ask(&mut self, job: Job, cx: &mut Context<Self>) {
        let Some(running) = &mut self.running else {
            return;
        };
        if !running.service.on_hand() {
            return;
        }
        if let Some(doing) = job.doing() {
            if self.view.doing.is_some() {
                return;
            }
            self.view.doing = Some(doing);
            cx.notify();
        }
        if matches!(job, Job::Switch(_)) {
            running.cadence.active_asked(now());
        }
        running.service.ask(job);
        self.work(cx);
    }

    /// Reads the accounts the cadence says are due: the active one, and while the panel is
    /// open another Koyo keeps the login of, the store's due candidate (`due_candidate`: not
    /// in backoff, held or parked, the least recently tried).
    fn look(&mut self, cx: &mut Context<Self>) {
        let focused = cx.active_window().is_some();
        let open = self.trigger.is_open(cx);
        let measured = self.view.active().and_then(|row| row.measured_at);
        let Some(running) = (self.running.as_mut()).filter(|running| running.service.on_hand())
        else {
            return;
        };
        let now = now();
        let due = running.cadence.due(now, focused, measured, open);
        let other = due.other.then(|| {
            let others = (self.view.rows.iter())
                .filter(|row| !row.active && row.switchable)
                .map(|row| row.number.as_str());
            let others: Vec<&str> = others.collect();
            due_candidate(&others, &running.service.entries, now)
        });
        let other = other.flatten();
        if due.active || other.is_some() {
            let active = due.active;
            self.ask(Job::Read { active, other }, cx);
        }
    }

    /// Opens the panel over the button, or closes it; open, another account is read if one's
    /// due.
    fn toggle_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.trigger.is_open(cx) {
            self.trigger.close(cx);
            return;
        }
        let (me, view) = (cx.entity(), self.view.clone());
        self.trigger.toggle_panel(
            panel::extent(&view, window),
            |cx| cx.new(|cx| panel::AccountsPanel::new(&me, view, cx)),
            window,
            cx,
        );
        if self.trigger.is_open(cx) {
            self.look(cx);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
