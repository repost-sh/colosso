//! The accounts crate's switcher as the app runs it. All it does blocks (the Keychain, curl,
//! file locks), so it runs off the main thread, one job at a time and in order, so a switch is
//! never read halfway; what each job found goes into the view. A reading waiting takes in the
//! next one asked, and a chat's limits waiting are replaced by newer ones. The person's jobs
//! (switch, add, remove, hold) go one at a time: while one waits or runs, another is refused.
//!
//! Whether a reading sends a request is the usage store's call (`poll.rs`). After the person's
//! job the accounts are taken again from the store, and on a switch the new active account is
//! read.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{Context, Task};
use koyo_accounts::switcher::{AccountsSnapshot, Fetch, LiveLimits, Switcher};
use koyo_accounts::usage_store::UsageEntry;
use koyo_accounts::{AccountsError, Env};

use super::model::{Slot, View};
use super::{Accounts, environment, reading};

/// What the switcher is asked to do.
#[derive(Debug)]
pub enum Job {
    /// Read the active account's usage, whichever it is now, and/or another's, as the store
    /// allows, and take the accounts again; with neither, only take them.
    Read {
        active: bool,
        other: Option<String>,
    },
    Switch(Slot),
    /// Keep the Mac's login as an account.
    Add,
    Remove(Slot),
    /// Hold the account out of auto-switch, or put it back.
    Hold(Slot, bool),
    /// A chat's limits, for the active account.
    Live(LiveLimits),
}

impl Job {
    /// What the panel says while the person's job waits or runs; `None` for the app's own.
    pub(super) fn doing(&self) -> Option<String> {
        match self {
            Job::Read { .. } | Job::Live(_) => None,
            Job::Switch(slot) => Some(format!("Switching to {}…", slot.email)),
            Job::Add => Some("Adding the Mac's login…".to_owned()),
            Job::Remove(slot) => Some(format!("Removing {}…", slot.email)),
            Job::Hold(slot, true) => Some(format!("Holding {} out of auto-switch…", slot.email)),
            Job::Hold(slot, false) => Some(format!("Putting {} back…", slot.email)),
        }
    }
}

/// What a job found.
struct Done {
    /// The accounts after it, or why they couldn't be read; `None` when it didn't take them.
    accounts: Option<Result<AccountsSnapshot, String>>,
    /// The active account's usage, after a chat's limits.
    active: Option<UsageEntry>,
    /// Why it failed, or what it warns of.
    said: Option<String>,
}

/// What an action said, and the accounts to read after it.
type Acted = (Option<String>, BTreeSet<String>);

/// An action's outcome, its failure said as "Couldn't `what`: why".
fn acted(what: &str, done: Result<Acted, AccountsError>) -> Acted {
    done.unwrap_or_else(|why| (Some(format!("Couldn't {what}: {why}")), BTreeSet::new()))
}

/// Runs `job` on `switcher`: blocking.
fn run(switcher: &Switcher, job: Job) -> Done {
    let (said, read) = match job {
        Job::Read { active, other } => {
            // The active account as the Mac's login has it now: the global config and the
            // sequence, no Keychain.
            let current = active
                .then(|| switcher.current_account_number().ok().flatten())
                .flatten();
            (None, current.into_iter().chain(other).collect())
        }
        Job::Live(limits) => {
            let (active, said) = match switcher.adopt_live_limits(limits) {
                Ok(entry) => (entry, None),
                Err(why) => (None, Some(why.to_string())),
            };
            return Done {
                accounts: None,
                active,
                said,
            };
        }
        Job::Switch(Slot { number, email }) => acted(
            &format!("switch to {email}"),
            (switcher.switch_to(&number))
                .map(|done| (joined(&done.warnings), BTreeSet::from([number]))),
        ),
        Job::Add => acted(
            "add the Mac's login",
            (switcher.add_account())
                .map(|added| (joined(&added.notices), BTreeSet::from([added.account_num]))),
        ),
        Job::Remove(Slot { number, email }) => acted(
            &format!("remove {email}"),
            (switcher.remove_account(&number)).map(|_| (None, BTreeSet::new())),
        ),
        Job::Hold(Slot { number, email }, disabled) => acted(
            &format!("change {email}"),
            (switcher.set_account_disabled(&number, disabled)).map(|_| (None, BTreeSet::new())),
        ),
    };
    let fetch = Fetch::Only {
        only: &read,
        scheduled: false,
    };
    Done {
        accounts: Some(
            switcher
                .accounts_snapshot(fetch)
                .map_err(|why| why.to_string()),
        ),
        active: None,
        said,
    }
}

fn joined(lines: &[String]) -> Option<String> {
    (!lines.is_empty()).then(|| lines.join(" "))
}

#[derive(Default)]
pub struct Service {
    /// Once the login shell has said where Claude Code keeps its login.
    switcher: Option<Arc<Switcher>>,
    /// Each account's usage as the last snapshot had it, for the cadence's pick of another.
    pub entries: BTreeMap<String, UsageEntry>,
    waiting: VecDeque<Job>,
    /// The job running; dropping it, with Manage Claude accounts off, drops what it found.
    working: Option<Task<()>>,
    heard: Heard,
    _making: Option<Task<()>>,
}

/// The least time between two chats' limits taken into the store: each is a locked, synced
/// write, and a busy chat hears them with every response.
const LIVE_EVERY: f64 = 30.;

/// Chats' limits on their way to the store, one write each [`LIVE_EVERY`] at most.
#[derive(Default)]
struct Heard {
    /// The newest, waiting for the next write.
    latest: Option<LiveLimits>,
    sent_at: Option<f64>,
    /// The wait for the next write.
    flushing: Option<Task<()>>,
}

/// The `Env` tests hand the accounts before `crate::init`, in place of the Mac's login shell's.
#[cfg(test)]
pub struct TestEnv(pub Env);

#[cfg(test)]
impl gpui_kit::Global for TestEnv {}

/// Where the accounts' `Env` comes from.
enum Source {
    /// A test's sandbox.
    #[cfg(test)]
    Test(Env),
    /// The Mac's login shell, with Koyo's store under this folder.
    Mac(std::path::PathBuf),
}

impl Service {
    /// Starts once the login shell has been read (a test's sandbox at once); the first reading
    /// is the cadence's.
    pub fn start(cx: &mut Context<Accounts>) -> Service {
        // Tests that hand nothing run on nothing: never the Mac's login.
        let source = crate::settings::data_dir()
            .filter(|_| !cfg!(test))
            .map(|dir| Source::Mac(dir.join("claude-accounts")));
        #[cfg(test)]
        let source = (cx.try_global::<TestEnv>())
            .map(|TestEnv(env)| Source::Test(env.clone()))
            .or(source);
        let making = source.map(|source| {
            cx.spawn(async move |this, cx| {
                let (env, overridden_by) = cx
                    .background_executor()
                    .spawn(async move {
                        match source {
                            #[cfg(test)]
                            Source::Test(env) => (Ok(env), None),
                            Source::Mac(store) => {
                                let shell = environment::read();
                                (Env::system(store, shell.vars), shell.overridden_by)
                            }
                        }
                    })
                    .await;
                this.update(cx, |accounts, cx| {
                    accounts.view.overridden_by = overridden_by;
                    match env {
                        Ok(env) => accounts.made(env, cx),
                        Err(why) => accounts.view.unreadable = Some(why.to_string()),
                    }
                    cx.notify();
                })
                .ok();
            })
        });
        Service {
            _making: making,
            ..Service::default()
        }
    }

    /// The switcher is on `env`.
    pub fn on(&mut self, env: Env) {
        self.switcher = Some(Arc::new(Switcher::new(env)));
    }

    /// Whether the switcher can be asked anything.
    pub fn on_hand(&self) -> bool {
        self.switcher.is_some()
    }

    /// Queues `job`, into a reading or a chat's limits already waiting.
    pub fn ask(&mut self, job: Job) {
        match (self.waiting.back_mut(), job) {
            (
                Some(Job::Read { active, other }),
                Job::Read {
                    active: also,
                    other: another,
                },
            ) => {
                *active |= also;
                *other = another.or(other.take());
            }
            (Some(Job::Live(waiting)), Job::Live(limits)) => *waiting = limits,
            (_, job) => self.waiting.push_back(job),
        }
    }
}

impl Accounts {
    /// Runs the next job waiting, unless one runs.
    pub(super) fn work(&mut self, cx: &mut Context<Self>) {
        let Some(service) = self.running.as_mut().map(|running| &mut running.service) else {
            return;
        };
        if service.working.is_some() {
            return;
        }
        let Some(switcher) = service.switcher.clone() else {
            return;
        };
        let Some(job) = service.waiting.pop_front() else {
            return;
        };
        let persons = job.doing().is_some();
        service.working = Some(cx.spawn(async move |this, cx| {
            let done = cx
                .background_executor()
                .spawn(async move { run(&switcher, job) })
                .await;
            this.update(cx, |accounts, cx| {
                let before = accounts.view.clone();
                accounts.take(done, persons);
                if accounts.view != before {
                    cx.notify();
                }
                if let Some(running) = &mut accounts.running {
                    running.service.working = None;
                }
                accounts.work(cx);
            })
            .ok();
        }));
    }

    /// A chat's limits for the active account: into the store at once, or the newest of them
    /// [`LIVE_EVERY`] after the last write. With no active account Koyo keeps, nothing.
    pub(super) fn heard(&mut self, limits: LiveLimits, cx: &mut Context<Self>) {
        if self.view.active().is_none() {
            return;
        }
        let Some(heard) = self
            .running
            .as_mut()
            .map(|running| &mut running.service.heard)
        else {
            return;
        };
        let wait = heard
            .sent_at
            .map_or(0., |at| at + LIVE_EVERY - super::now());
        heard.latest = Some(limits);
        if wait <= 0. {
            self.send_heard(cx);
        } else if heard.flushing.is_none() {
            heard.flushing = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_secs_f64(wait))
                    .await;
                this.update(cx, Accounts::send_heard).ok();
            }));
        }
    }

    /// Writes the newest of the chats' limits heard.
    fn send_heard(&mut self, cx: &mut Context<Self>) {
        let Some(heard) = self
            .running
            .as_mut()
            .map(|running| &mut running.service.heard)
        else {
            return;
        };
        heard.flushing = None;
        if let Some(limits) = heard.latest.take() {
            heard.sent_at = Some(super::now());
            self.ask(Job::Live(limits), cx);
        }
    }

    /// Puts what a job found into the view: what the person's job said replaces what the last
    /// one said; another job's only when it failed.
    fn take(&mut self, done: Done, persons: bool) {
        let now = super::now();
        let view = &mut self.view;
        if persons {
            view.doing = None;
        }
        if persons || done.said.is_some() {
            view.said = done.said;
        }
        match done.accounts {
            Some(Ok(accounts)) => {
                take_accounts(view, &accounts, now);
                if let Some(running) = &mut self.running {
                    running.service.entries = (accounts.accounts.into_iter())
                        .map(|account| (account.slot.number, account.usage))
                        .collect();
                }
            }
            Some(Err(why)) => view.unreadable = Some(why),
            None => {}
        }
        if let Some(entry) = done.active
            && let Some(row) = view.rows.iter_mut().find(|row| row.active)
        {
            reading::take_entry(row, &entry, now);
            if let Some(running) = &mut self.running {
                running.service.entries.insert(row.number.clone(), entry);
            }
        }
    }
}

/// `accounts` into `view` at `now`: a row each, and the Mac's login when Koyo doesn't keep it.
fn take_accounts(view: &mut View, accounts: &AccountsSnapshot, now: f64) {
    view.rows = (accounts.accounts.iter())
        .map(|account| reading::row(account, now))
        .collect();
    view.unmanaged = (accounts.current.is_none())
        .then(|| accounts.live.as_ref().map(|live| live.email.clone().into()))
        .flatten();
    view.unreadable = None;
}
