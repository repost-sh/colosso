//! cswap's tests for `credentials.py` (0fe22a8), translated: `test_credentials.py`,
//! `test_backup_read_pacing.py`, the Keychain-verdict cases of
//! `test_macos_keychain_contract.py`, and the `test_switcher.py` and
//! `test_api_key_accounts.py` cases that drive this module directly.
//!
//! cswap's conftest swaps the `security` wrapper for an in-memory Keychain and its tests
//! monkeypatch it per call; here the sandbox's fake `security` sits behind [`Scripted`], whose
//! rule may answer a call itself (a locked keychain, one failing item), wait, or reach back
//! into the store mid-call, as cswap's patched functions do.

use std::sync::{Arc, Mutex};
use std::thread::ThreadId;

use crate::credentials::CredentialStore;
use crate::keychain::{KeychainError, NOT_FOUND_RC, Output, Security};
use crate::testing::{self, Answer, Call, FakeSecurity, Sandbox};
use crate::{Env, locked};

mod active;
mod backup;
mod contract;
mod cooldown;
mod global_config;
mod pacing;
mod shared;
mod stash;

type Rule = Arc<dyn Fn(&Call) -> Option<Answer> + Send + Sync>;

/// The sandbox's fake `security`, with a rule a test sets and every call recorded, answered
/// or not.
pub(super) struct Scripted {
    pub fake: Arc<FakeSecurity>,
    rule: Mutex<Option<Rule>>,
    calls: Mutex<Vec<Call>>,
}

impl Scripted {
    /// `rule` sees each call first; `Some` answers it, `None` hands it to the fake.
    pub fn set_rule(&self, rule: impl Fn(&Call) -> Option<Answer> + Send + Sync + 'static) {
        *locked(&self.rule) = Some(Arc::new(rule));
    }

    pub fn clear_rule(&self) {
        *locked(&self.rule) = None;
    }

    /// Every read, write and delete fails as a locked keychain does (exit 51).
    pub fn lock(&self) {
        self.set_rule(|_| Some(Answer::Exit(51)));
    }

    pub fn calls(&self) -> Vec<Call> {
        locked(&self.calls).clone()
    }

    pub fn clear_calls(&self) {
        locked(&self.calls).clear();
    }

    /// The services the reads asked for, in order: cswap's tests' `seen`.
    pub fn read_services(&self) -> Vec<String> {
        self.calls()
            .iter()
            .filter(|call| verb(call) == "find-generic-password")
            .filter_map(|call| after(call, "-s"))
            .collect()
    }

    /// How many reads asked for this account.
    pub fn reads_of(&self, account: &str) -> usize {
        self.calls()
            .iter()
            .filter(|call| {
                verb(call) == "find-generic-password"
                    && after(call, "-a").as_deref() == Some(account)
            })
            .count()
    }
}

impl Security for Scripted {
    fn run(&self, args: &[&str], input: Option<&str>) -> Result<Output, KeychainError> {
        let call = Call {
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            stdin: input.map(str::to_owned),
        };
        locked(&self.calls).push(call.clone());
        // Cloned out, so a rule that calls back into the store doesn't wait on itself.
        let rule = locked(&self.rule).clone();
        match rule.and_then(|rule| rule(&call)) {
            Some(Answer::Exit(status)) => Ok(Output {
                status,
                ..Output::default()
            }),
            Some(Answer::Timeout) => {
                Err(KeychainError(format!("{} timed out after 5s", verb(&call))))
            }
            None => self.fake.run(args, input),
        }
    }
}

/// A call's verb, from its arguments or its `security -i` line.
pub(super) fn verb(call: &Call) -> String {
    words(call).into_iter().next().unwrap_or_default()
}

/// The value after `flag` in a call.
pub(super) fn after(call: &Call, flag: &str) -> Option<String> {
    testing::after(&words(call), flag).map(str::to_owned)
}

/// The call's words; a `security -i` line split as the fake splits it.
fn words(call: &Call) -> Vec<String> {
    if call.args == ["-i"] {
        testing::words(call.stdin.as_deref().unwrap_or_default())
    } else {
        call.args.clone()
    }
}

/// A sandbox and a store on it whose `security` is [`Scripted`].
pub(super) struct World {
    pub sandbox: Sandbox,
    pub security: Arc<Scripted>,
    pub store: Arc<CredentialStore>,
}

impl World {
    pub fn new() -> World {
        World::with(|_| {})
    }

    /// `setup` sets the sandbox's variables before the store is made.
    pub fn with(setup: impl FnOnce(&mut Sandbox)) -> World {
        let mut sandbox = Sandbox::new();
        setup(&mut sandbox);
        let security = Arc::new(Scripted {
            fake: sandbox.security.clone(),
            rule: Mutex::new(None),
            calls: Mutex::new(Vec::new()),
        });
        let env = Env {
            security: security.clone(),
            ..sandbox.env()
        };
        let store = Arc::new(CredentialStore::new(env));
        World {
            sandbox,
            security,
            store,
        }
    }

    /// The account the active item is keyed by.
    pub fn account(&self) -> String {
        crate::keychain::keychain_account_name(self.sandbox.vars.user.as_deref())
    }

    /// No call so far carried `secret` in its arguments, nor in plain on stdin.
    pub fn assert_no_secret_in_calls(&self, secret: &str) {
        for call in self.security.calls() {
            assert!(
                !call.args.iter().any(|arg| arg.contains(secret)),
                "a secret in a call's arguments"
            );
            assert!(
                !call.stdin.as_deref().unwrap_or_default().contains(secret),
                "a secret in plain on stdin"
            );
        }
    }
}

/// An OAuth credential, as cswap's tests' `_cred(tag)`.
pub(super) fn cred(tag: &str) -> String {
    testing::creds(
        &format!("sk-{tag}"),
        &format!("rt-{tag}"),
        Some(9_999_999_999_000_u64.into()),
    )
}

/// The answer a read of a missing item gets, for rules that pass a call on as "absent".
pub(super) const ABSENT: Answer = Answer::Exit(NOT_FOUND_RC);

/// The log lines this thread's code wrote, as cswap's `caplog`.
pub(super) mod logs {
    use super::{ThreadId, locked};
    use log::{Level, LevelFilter, Log, Metadata, Record};
    use std::sync::{Mutex, Once};

    static RECORDS: Mutex<Vec<(ThreadId, Level, String)>> = Mutex::new(Vec::new());

    struct Capture;

    impl Log for Capture {
        fn enabled(&self, metadata: &Metadata<'_>) -> bool {
            metadata.target().starts_with("koyo_accounts::credentials")
        }

        fn log(&self, record: &Record<'_>) {
            if self.enabled(record.metadata()) {
                locked(&RECORDS).push((
                    std::thread::current().id(),
                    record.level(),
                    record.args().to_string(),
                ));
            }
        }

        fn flush(&self) {}
    }

    /// Starts capturing, with this thread's earlier lines forgotten.
    pub fn capture() {
        static INSTALL: Once = Once::new();
        INSTALL.call_once(|| {
            static CAPTURE: Capture = Capture;
            log::set_logger(&CAPTURE).expect("no other logger in this crate's tests");
            log::set_max_level(LevelFilter::Trace);
        });
        let me = std::thread::current().id();
        locked(&RECORDS).retain(|(thread, _, _)| *thread != me);
    }

    /// This thread's lines since [`capture`].
    pub fn lines() -> Vec<(Level, String)> {
        let me = std::thread::current().id();
        locked(&RECORDS)
            .iter()
            .filter(|(thread, _, _)| *thread == me)
            .map(|(_, level, line)| (*level, line.clone()))
            .collect()
    }

    /// This thread's lines containing `text`.
    pub fn containing(text: &str) -> Vec<(Level, String)> {
        lines()
            .into_iter()
            .filter(|(_, line)| line.contains(text))
            .collect()
    }
}
