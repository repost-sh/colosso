//! The accounts crate as the app runs it: everything it does blocks (the Keychain, curl), so it
//! runs off the main thread, one reading at a time, and what it found goes into the view.
//!
//! Until the crate's switcher keeps several accounts, this reads the Mac's login alone, as
//! Claude Code reads it: its item in the Keychain, and the `oauthAccount` of its global config.
//! Its usage comes from Claude's usage endpoint with its access token, never refreshed to read
//! it (`docs/claude-accounts.md` 4). A failure backs off as the usage store's does, in memory,
//! keeping the last reading.

use std::sync::Arc;

use gpui_kit::{Context, Task};
use koyo_accounts::Env;
use koyo_accounts::credentials::CredentialStore;
use koyo_accounts::oauth::{self, RATE_LIMITED_ERROR, UsageOutcome};
use koyo_accounts::usage_store::failure_backoff_s;
use serde_json::Value;

use super::model::{Problem, Row, View};
use super::{Accounts, environment, reading};

/// A failed reading's backoff.
#[derive(Default)]
struct Backoff {
    failures: i64,
    until: f64,
}

pub struct Service {
    /// The Mac's, once the login shell has said where Claude Code keeps its login: one for the
    /// process, as its Keychain state is learned from real calls.
    store: Option<Arc<CredentialStore>>,
    reading: bool,
    backoff: Backoff,
    _making: Option<Task<()>>,
}

/// The `Env` tests hand the accounts before `crate::init`, in place of the Mac's.
#[cfg(test)]
pub struct TestEnv(pub Env);

#[cfg(test)]
impl gpui_kit::Global for TestEnv {}

/// The Mac's login as Claude Code has it, and what the usage endpoint said of it: blocking.
fn current_login(store: &CredentialStore) -> Option<(Row, Option<UsageOutcome>)> {
    let account = store
        .read_global_config()
        .and_then(|mut config| config.remove("oauthAccount"));
    let text = |key: &str| {
        (account.as_ref())
            .and_then(|account| account.get(key))
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    let active = store.read_active_credentials();
    let credential = active.value.filter(|value| !value.is_empty());
    let email =
        text("emailAddress").filter(|_| credential.is_some() || active.keychain_unavailable)?;
    let row = Row {
        email: email.into(),
        organisation: text("organizationName").map(Into::into),
        organisation_uuid: text("organizationUuid").unwrap_or_default(),
        active: true,
        bars: Vec::new(),
        measured_at: None,
        problem: active
            .keychain_unavailable
            .then_some(Problem::KeychainUnavailable),
    };
    let outcome = credential
        .map(|credential| oauth::try_fetch_usage_for_account(store.env(), "active", &credential));
    Some((row, outcome))
}

impl Service {
    /// Starts once the login shell has been read; the first reading is the cadence's.
    pub fn start(cx: &mut Context<Accounts>) -> Service {
        #[cfg(test)]
        if let Some(TestEnv(env)) = cx.try_global::<TestEnv>() {
            return Service::on(env.clone());
        }
        // Tests that hand nothing run on nothing: never the Mac's login.
        let store = crate::settings::data_dir()
            .filter(|_| !cfg!(test))
            .map(|dir| dir.join("claude-accounts"));
        let making = store.map(|store| {
            cx.spawn(async move |this, cx| {
                let (env, overridden_by) = cx
                    .background_executor()
                    .spawn(async move {
                        let shell = environment::read();
                        (Env::system(store, shell.vars), shell.overridden_by)
                    })
                    .await;
                this.update(cx, |accounts, cx| {
                    accounts.view.overridden_by = overridden_by;
                    match env {
                        Ok(env) => accounts.made(env),
                        Err(why) => accounts.view.said = Some(why.to_string()),
                    }
                    cx.notify();
                })
                .ok();
            })
        });
        Service {
            _making: making,
            ..Service::empty()
        }
    }

    fn empty() -> Service {
        Service {
            store: None,
            reading: false,
            backoff: Backoff::default(),
            _making: None,
        }
    }

    /// A service on `env`.
    pub fn on(env: Env) -> Service {
        Service {
            store: Some(Arc::new(CredentialStore::new(env))),
            ..Service::empty()
        }
    }

    /// Whether the login can be read yet.
    pub fn ready(&self) -> bool {
        self.store.is_some()
    }

    /// Reads the Mac's login and its usage, unless a reading runs or the last failure's backoff
    /// lasts.
    pub fn read(&mut self, cx: &mut Context<Accounts>) {
        let Some(store) = self.store.clone() else {
            return;
        };
        if self.reading || store.env().now() < self.backoff.until {
            return;
        }
        self.reading = true;
        cx.spawn(async move |this, cx| {
            let reader = store.clone();
            let found = cx
                .background_executor()
                .spawn(async move { current_login(&reader) })
                .await;
            this.update(cx, |accounts, cx| {
                if let Some(running) = &mut accounts.running {
                    running.service.reading = false;
                    let now = store.env().now();
                    running.service.take(found, now, &mut accounts.view);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Puts what was found at `now` into `view`: a failed reading keeps the last one's bars.
    fn take(&mut self, found: Option<(Row, Option<UsageOutcome>)>, now: f64, view: &mut View) {
        let old = std::mem::take(&mut view.rows);
        let Some((mut row, outcome)) = found else {
            return;
        };
        if let Some(kept) = old.into_iter().find(|old| old.same_account(&row)) {
            (row.bars, row.measured_at) = (kept.bars, kept.measured_at);
        }
        match outcome {
            Some(UsageOutcome {
                usage: Some(usage), ..
            }) => {
                self.backoff = Backoff::default();
                row.bars = reading::bars(&usage, now);
                row.measured_at = Some(now);
            }
            Some(UsageOutcome {
                error: Some(kind),
                retry_after_s,
                ..
            }) => {
                self.backoff.failures += 1;
                let rate_limited = kind == RATE_LIMITED_ERROR;
                self.backoff.until =
                    now + failure_backoff_s(self.backoff.failures, retry_after_s, rate_limited);
                row.problem = Some(reading::problem(&kind, self.backoff.until));
            }
            _ => {}
        }
        view.rows = vec![row];
    }
}
