//! What tests run on instead of the Mac (`docs/claude-accounts.md` 7): a sandbox folder in the
//! build's target folder with a home and a store in it, a fake `security` holding the Keychain
//! in memory, a fake HTTP that answers what a test queues and nothing else, and a clock a test
//! moves. A sandbox's [`Env`] has the guard on: nothing it runs writes outside its folder or
//! under the person's real login.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::Env;
use crate::env::{Clock, Vars};
use crate::http::{Http, HttpError, Request, Response};
use crate::keychain::{KeychainError, NOT_FOUND_RC, Output, Security};
#[cfg(test)]
use crate::usage_store::{FetchRecord, Identities, UsageEntry, UsageStore};

/// A folder of a test's own under the build's target folder, removed when it's dropped.
pub struct Sandbox {
    root: PathBuf,
    pub security: Arc<FakeSecurity>,
    pub http: Arc<FakeHttp>,
    pub clock: Arc<ManualClock>,
    pub vars: Vars,
}

impl Sandbox {
    pub fn new() -> Sandbox {
        static COUNT: AtomicU64 = AtomicU64::new(0);
        let root = Path::new(env!("KOYO_TARGET_DIR")).join(format!(
            "accounts-tests/{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(root.join("home")).ok();
        std::fs::create_dir_all(root.join("store")).ok();
        // Resolved, as the guard compares resolved paths.
        let root = root.canonicalize().unwrap_or(root);
        Sandbox {
            root,
            security: Arc::new(FakeSecurity::default()),
            http: Arc::new(FakeHttp::default()),
            clock: Arc::new(ManualClock::new(1_800_000_000.)),
            vars: Vars {
                user: Some("tester".to_owned()),
                ..Vars::default()
            },
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The sandbox's home, the tests' `Path.home()`.
    pub fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    /// The sandbox's Koyo store.
    pub fn store(&self) -> PathBuf {
        self.root.join("store")
    }

    /// An `Env` on this sandbox, as it is now: change `vars` before taking one.
    pub fn env(&self) -> Env {
        Env {
            home: self.home(),
            vars: self.vars.clone(),
            store: self.store(),
            security: self.security.clone(),
            http: self.http.clone(),
            clock: self.clock.clone(),
            sandbox: Some(self.root.clone()),
        }
    }
}

impl Default for Sandbox {
    fn default() -> Self {
        Sandbox::new()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One call made of the fake `security`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    pub args: Vec<String>,
    pub stdin: Option<String>,
}

/// What the fake answers a call with instead of doing it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    /// An exit with this code and nothing printed: 51 or 36 for a locked keychain, say.
    Exit(i32),
    Timeout,
}

#[derive(Default)]
struct Keychain {
    items: BTreeMap<(String, String), String>,
    calls: Vec<Call>,
    /// Answers for the next calls, in order, before any other rule.
    next: VecDeque<Answer>,
    /// Answers for every call naming this service, while set.
    services: BTreeMap<String, Answer>,
    /// The answer every call gets while set: a locked keychain.
    all: Option<Answer>,
}

/// The Keychain in memory, behind the `security` command lines cswap and Claude Code write.
#[derive(Default)]
pub struct FakeSecurity {
    state: Mutex<Keychain>,
}

impl FakeSecurity {
    pub fn set(&self, service: &str, account: &str, value: &str) {
        locked(&self.state)
            .items
            .insert((service.to_owned(), account.to_owned()), value.to_owned());
    }

    pub fn get(&self, service: &str, account: &str) -> Option<String> {
        locked(&self.state)
            .items
            .get(&(service.to_owned(), account.to_owned()))
            .cloned()
    }

    pub fn remove(&self, service: &str, account: &str) {
        locked(&self.state)
            .items
            .remove(&(service.to_owned(), account.to_owned()));
    }

    /// Every item, as `(service, account) → value`.
    pub fn items(&self) -> BTreeMap<(String, String), String> {
        locked(&self.state).items.clone()
    }

    pub fn calls(&self) -> Vec<Call> {
        locked(&self.state).calls.clone()
    }

    pub fn clear_calls(&self) {
        locked(&self.state).calls.clear();
    }

    /// The next call gets `answer`, and is not done.
    pub fn answer_next(&self, answer: Answer) {
        locked(&self.state).next.push_back(answer);
    }

    /// Every call naming `service` gets `answer` until [`FakeSecurity::clear_service`].
    pub fn answer_service(&self, service: &str, answer: Answer) {
        locked(&self.state)
            .services
            .insert(service.to_owned(), answer);
    }

    pub fn clear_service(&self, service: &str) {
        locked(&self.state).services.remove(service);
    }

    /// Every call gets `answer` until [`FakeSecurity::unlock`]: a locked or wedged keychain.
    pub fn answer_all(&self, answer: Answer) {
        locked(&self.state).all = Some(answer);
    }

    pub fn unlock(&self) {
        locked(&self.state).all = None;
    }
}

/// `line` split as `security -i` splits it: words, double quotes with `\` escapes.
fn words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let (mut quoted, mut escaped, mut any) = (false, false, false);
    for c in line.trim_end_matches('\n').chars() {
        if escaped {
            word.push(c);
            escaped = false;
        } else if c == '\\' && quoted {
            escaped = true;
        } else if c == '"' {
            quoted = !quoted;
            any = true;
        } else if c.is_whitespace() && !quoted {
            if any || !word.is_empty() {
                out.push(std::mem::take(&mut word));
                any = false;
            }
        } else {
            word.push(c);
        }
    }
    if any || !word.is_empty() {
        out.push(word);
    }
    out
}

/// The value after `flag` among `args`.
fn after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let at = args.iter().position(|arg| arg == flag)?;
    args.get(at + 1).map(String::as_str)
}

fn unhex(hex: &str) -> Option<String> {
    let bytes: Option<Vec<u8>> = (0..hex.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(hex.get(at..at + 2)?, 16).ok())
        .collect();
    String::from_utf8(bytes?).ok()
}

fn exit(status: i32) -> Output {
    Output {
        status,
        ..Output::default()
    }
}

impl Security for FakeSecurity {
    fn run(&self, args: &[&str], input: Option<&str>) -> Result<Output, KeychainError> {
        let mut state = locked(&self.state);
        state.calls.push(Call {
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            stdin: input.map(str::to_owned),
        });
        let args: Vec<String> = if args == ["-i"] {
            words(input.unwrap_or_default())
        } else {
            args.iter().map(|arg| (*arg).to_owned()).collect()
        };
        let service = after(&args, "-s").unwrap_or_default().to_owned();
        let account = after(&args, "-a").unwrap_or_default().to_owned();
        let answer = state
            .next
            .pop_front()
            .or_else(|| state.services.get(&service).cloned())
            .or_else(|| state.all.clone());
        match answer {
            Some(Answer::Timeout) => {
                return Err(KeychainError(format!(
                    "{} timed out after 5s",
                    args.first().map_or("security", String::as_str)
                )));
            }
            Some(Answer::Exit(status)) => return Ok(exit(status)),
            None => {}
        }
        let key = (service, account);
        Ok(match args.first().map(String::as_str) {
            Some("find-generic-password") => match state.items.get(&key) {
                Some(value) if args.iter().any(|arg| arg == "-w") => Output {
                    status: 0,
                    stdout: format!("{value}\n"),
                    stderr: String::new(),
                },
                Some(_) => exit(0),
                None => exit(NOT_FOUND_RC),
            },
            Some("add-generic-password") => match after(&args, "-X").and_then(unhex) {
                Some(value) => {
                    state.items.insert(key, value);
                    exit(0)
                }
                None => exit(1),
            },
            Some("delete-generic-password") => match state.items.remove(&key) {
                Some(_) => exit(0),
                None => exit(NOT_FOUND_RC),
            },
            _ => exit(1),
        })
    }
}

/// HTTP that answers what a test queued for a URL, in order, and nothing else: a request
/// with nothing queued gets [`HttpError::Network`].
/// The answers queued for the URLs starting with a prefix.
type Queued = Vec<(String, VecDeque<Result<Response, HttpError>>)>;

#[derive(Default)]
pub struct FakeHttp {
    answers: Mutex<Queued>,
    requests: Mutex<Vec<Request>>,
}

impl FakeHttp {
    /// The next request to a URL starting with `prefix` gets `answer`.
    pub fn queue(&self, prefix: &str, answer: Result<Response, HttpError>) {
        let mut answers = locked(&self.answers);
        if let Some((_, queue)) = answers.iter_mut().find(|(at, _)| at == prefix) {
            queue.push_back(answer);
        } else {
            answers.push((prefix.to_owned(), VecDeque::from([answer])));
        }
    }

    /// A JSON answer with `status`.
    pub fn queue_json(&self, prefix: &str, status: u16, body: &serde_json::Value) {
        self.queue(prefix, Ok(json_response(status, body)));
    }

    pub fn requests(&self) -> Vec<Request> {
        locked(&self.requests).clone()
    }

    /// The requests to URLs starting with `prefix`.
    pub fn requests_to(&self, prefix: &str) -> Vec<Request> {
        self.requests()
            .into_iter()
            .filter(|request| request.url.starts_with(prefix))
            .collect()
    }
}

impl Http for FakeHttp {
    fn send(&self, request: &Request) -> Result<Response, HttpError> {
        locked(&self.requests).push(request.clone());
        let mut answers = locked(&self.answers);
        answers
            .iter_mut()
            .filter(|(prefix, _)| request.url.starts_with(prefix.as_str()))
            .max_by_key(|(prefix, _)| prefix.len())
            .and_then(|(_, queue)| queue.pop_front())
            .unwrap_or_else(|| {
                Err(HttpError::Network(format!(
                    "nothing queued for {}",
                    request.url
                )))
            })
    }
}

/// A clock that stands still until a test moves it.
pub struct ManualClock(AtomicU64);

impl ManualClock {
    pub fn new(now: f64) -> ManualClock {
        ManualClock(AtomicU64::new(now.to_bits()))
    }

    pub fn set(&self, now: f64) {
        self.0.store(now.to_bits(), Ordering::SeqCst);
    }

    pub fn advance(&self, seconds: f64) {
        self.set(self.now() + seconds);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> f64 {
        f64::from_bits(self.0.load(Ordering::SeqCst))
    }
}

/// A JSON answer with `status`, as an endpoint sends one.
pub fn json_response(status: u16, body: &serde_json::Value) -> Response {
    Response {
        status,
        headers: vec![("content-type".into(), "application/json".into())],
        body: body.to_string().into_bytes(),
    }
}

/// `store.record({num: record}, identities)`, unfenced: one usage row as this crate's tests of
/// the store and of the switcher seed it.
#[cfg(test)]
pub(crate) fn record_as(
    store: &UsageStore,
    num: &str,
    record: FetchRecord,
    identities: &Identities,
) -> std::collections::BTreeSet<String> {
    store
        .record(
            &BTreeMap::from([(num.to_owned(), record)]),
            identities,
            None,
            None,
        )
        .unwrap()
}

/// `store.entries(identities)[num]`, with no models.
#[cfg(test)]
pub(crate) fn entry_as(store: &UsageStore, num: &str, identities: &Identities) -> UsageEntry {
    store.entries(identities, &[]).remove(num).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_security_line_splits_as_security_splits_it() {
        assert_eq!(
            words("add-generic-password -U -a \"me\" -s \"Claude Code-credentials\" -X 7b7d\n"),
            [
                "add-generic-password",
                "-U",
                "-a",
                "me",
                "-s",
                "Claude Code-credentials",
                "-X",
                "7b7d"
            ]
        );
        assert_eq!(words(r#"-a "a\"b\\c" -s """#), ["-a", r#"a"b\c"#, "-s", ""]);
    }
}
