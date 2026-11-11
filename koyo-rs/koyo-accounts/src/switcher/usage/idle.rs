//! Port of claude-swap's `switcher.py` (0fe22a8), one account's usage fetch and the pool that
//! runs a pass's fetches: `_static_usage_sentinel`, `_fetch_account_usage`, `_read_only_fetch`
//! and `_run_usage_fetches`.
//!
//! cswap fetches an idle account with `refresh_via=consume_backup_grant`: an expired token is
//! refreshed through the gate first, a 401 refreshed and retried. Koyo never refreshes to read
//! usage (`docs/claude-accounts.md` 4), so an idle backup is read as cswap reads a credential it
//! must never refresh, its live-session branch: an expired copy is known refused without
//! asking (the `token expired` sentinel; the fetch sends nothing for one), and
//! `_read_only_fetch` asks once, a 401 being `token expired` too with the refused access token
//! stamped on the row so no pass asks again until the credential changed (the gate's refresh,
//! a re-add). Every other error keeps its kind, a 429 above all. The auto-switch engine
//! freshens a candidate through the gate before it activates it.
//!
//! The pool keeps cswap's bound (`ThreadPoolExecutor`'s default, `min(32, cpus + 4)`) and its
//! stagger between request starts, each `stagger × index` after the pass started; a pass with
//! one fetch runs it on the caller's thread. Left out: the session-profile reads and adoption
//! of `_fetch_account_usage` (`_adopt_session_credential`, the live-session checks), with
//! session mode, and the `api key` sentinel, with API-key accounts.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crate::fsutil;
use crate::locked;
use crate::oauth::{
    NO_ACCESS_TOKEN_ERROR, TOKEN_EXPIRED_ERROR, UNAUTHORIZED_ERROR, USAGE_KEYCHAIN_UNAVAILABLE,
    USAGE_NO_CREDENTIALS, USAGE_TOKEN_EXPIRED, access_token_fingerprint, extract_access_token,
    try_fetch_usage_for_account,
};
use crate::switcher::Switcher;
use crate::switcher::info::AccountInfo;
use crate::usage_store::{FetchRecord, UsageEntry};

/// `_FETCH_STAGGER_S`: the delay between successive request starts of one pass, so N accounts
/// never reach the endpoint in the same instant (request hygiene, cswap #85). None in this
/// crate's tests, whose HTTP is a fake.
const FETCH_STAGGER: Duration = if cfg!(test) {
    Duration::ZERO
} else {
    Duration::from_millis(250)
};

/// cswap's pool bound, `ThreadPoolExecutor()`'s default.
fn pool_bound() -> usize {
    let cpus = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    (cpus + 4).min(32)
}

#[cfg(test)]
pub(crate) type FetchSeam = Box<dyn FnMut(&[&AccountInfo]) -> BTreeMap<String, FetchRecord>>;

#[cfg(test)]
thread_local! {
    /// The test seam for `run_usage_fetches`, as cswap's tests patch `_run_usage_fetches`.
    pub(crate) static RUN_FETCHES: std::cell::RefCell<Option<FetchSeam>> =
        const { std::cell::RefCell::new(None) };
}

impl Switcher {
    /// `_static_usage_sentinel`: a sentinel known without any network call, else `None`.
    /// Derived again on every pass, never stored, so it can't outlive its condition.
    pub(crate) fn static_usage_sentinel(&self, info: &AccountInfo) -> Option<&'static str> {
        if extract_access_token(&info.creds).is_none_or(|token| token.is_empty()) {
            let unavailable = match info.active {
                Some(verdict) => verdict.unavailable,
                // This slot's own read verdict, through the display cache (a hit is the read
                // the accounts info just made; after a failure the cooldown makes it free):
                // "no credentials" would send the person to re-add a slot that has one.
                None => {
                    self.store
                        .read_account_credentials_ex(info.num(), info.email(), true)
                        .1
                }
            };
            if unavailable {
                return Some(USAGE_KEYCHAIN_UNAVAILABLE);
            }
            return Some(USAGE_NO_CREDENTIALS);
        }
        // An expired token isn't a static state: the fetch path says so itself.
        None
    }

    /// `_fetch_account_usage`: one account's fetch. `rejected_fp` is the row's refused-token
    /// stamp, if any.
    pub(crate) fn fetch_account_usage(
        &self,
        info: &AccountInfo,
        rejected_fp: Option<&str>,
    ) -> FetchRecord {
        match info.active {
            Some(verdict) => self.fetch_active_usage(info, verdict),
            None => self.read_only_fetch(info.num(), &info.creds, rejected_fp),
        }
    }

    /// `_read_only_fetch`: a fetch with a credential Koyo must not refresh. An expired token
    /// or a 401 is `token expired`, the refused token stamped; a token already refused is not
    /// asked again.
    pub(crate) fn read_only_fetch(
        &self,
        account_num: &str,
        creds: &str,
        rejected_fp: Option<&str>,
    ) -> FetchRecord {
        let stamp = access_token_fingerprint(creds);
        if stamp.is_some() && stamp.as_deref() == rejected_fp {
            return FetchRecord::sentinel(USAGE_TOKEN_EXPIRED);
        }
        let outcome = try_fetch_usage_for_account(&self.env, account_num, creds);
        match outcome.error.as_deref() {
            Some(NO_ACCESS_TOKEN_ERROR) => FetchRecord::sentinel(USAGE_NO_CREDENTIALS),
            Some(TOKEN_EXPIRED_ERROR) => FetchRecord::sentinel(USAGE_TOKEN_EXPIRED),
            Some(UNAUTHORIZED_ERROR) => FetchRecord {
                rejected_fp: stamp,
                ..FetchRecord::sentinel(USAGE_TOKEN_EXPIRED)
            },
            _ => FetchRecord::from(outcome),
        }
    }

    /// `_run_usage_fetches`: the given accounts fetched in parallel, the starts staggered so
    /// they never reach the endpoint in the same instant. `entries` is the pre-fetch snapshot,
    /// for each row's refused-token stamp.
    pub(crate) fn run_usage_fetches(
        &self,
        infos: &[&AccountInfo],
        entries: &BTreeMap<String, UsageEntry>,
    ) -> BTreeMap<String, FetchRecord> {
        #[cfg(test)]
        if let Some(records) =
            RUN_FETCHES.with_borrow_mut(|seam| seam.as_mut().map(|run| run(infos)))
        {
            return records;
        }
        let fetch = |info: &AccountInfo| {
            let rejected = entries
                .get(info.num())
                .and_then(|entry| entry.rejected_fingerprint.as_deref());
            (
                info.num().to_owned(),
                self.fetch_account_usage(info, rejected),
            )
        };
        if let [info] = infos {
            return BTreeMap::from([fetch(info)]);
        }
        let start = Instant::now();
        let next = AtomicUsize::new(0);
        let records = Mutex::new(BTreeMap::new());
        std::thread::scope(|scope| {
            for _ in 0..pool_bound().min(infos.len()) {
                scope.spawn(|| {
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(info) = infos.get(index) else {
                            break;
                        };
                        let due =
                            FETCH_STAGGER.saturating_mul(u32::try_from(index).unwrap_or(u32::MAX));
                        let wait = due.saturating_sub(start.elapsed());
                        if !wait.is_zero() {
                            fsutil::nap(wait);
                        }
                        let (num, record) = fetch(info);
                        locked(&records).insert(num, record);
                    }
                });
            }
        });
        records
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
