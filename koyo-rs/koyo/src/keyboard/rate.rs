//! How often, in a second, a window's first responder is put back and its refusals and heals
//! are logged. A page asking again each time it's refused, or a key ping-ponging with one, would
//! have the main thread and the log do nothing else: past the second's allowance the first
//! responder is left as it is, and the lines are counted and summed up once the second is up.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use super::lines;
use super::state::WindowKey;

/// How many times a second a window's first responder is put back, and as many lines of its
/// refusals and heals.
pub const MOST: u32 = 20;
pub const PER: Duration = Duration::from_secs(1);

/// Runs a task on the main thread once `Duration` has passed, on its next turn for none.
pub type Later = Box<dyn Fn(Duration, Box<dyn FnOnce()>)>;

/// What a second has had so far.
struct Second {
    since: Instant,
    count: u32,
}

/// Each window's second.
type Seconds = RefCell<HashMap<WindowKey, Second>>;

thread_local! {
    static PUT_BACK: Seconds = RefCell::default();
    static TOLD: Seconds = RefCell::default();
    /// The lines left out for each window since its summing up was asked for.
    static LEFT_OUT: RefCell<HashMap<WindowKey, u32>> = RefCell::default();
}

/// Counts one more for `window` in `seconds`; how many this second has had with it.
fn count(seconds: &'static std::thread::LocalKey<Seconds>, window: WindowKey) -> u32 {
    seconds.with(|seconds| {
        let mut seconds = seconds.borrow_mut();
        let now = Instant::now();
        let second = seconds.entry(window).or_insert(Second { since: now, count: 0 });
        if now.duration_since(second.since) >= PER {
            *second = Second { since: now, count: 0 };
        }
        second.count += 1;
        second.count
    })
}

/// Whether `window`'s first responder may be put back once more this second.
pub fn may_put_back(window: WindowKey) -> bool {
    let count = count(&PUT_BACK, window);
    if count == MOST + 1 {
        lines::write(
            log::Level::Error,
            &format!(
                "keyboard: the first responder of a window was put back {MOST} times in a \
                 second; left as it is until the second is up"
            ),
        );
    }
    count <= MOST
}

/// Writes the refusal or heal `line` makes for `window` at warning, if the second has room for
/// it; else counts it, the count written once the second is up, through `later`.
pub fn tell(window: WindowKey, line: impl FnOnce() -> String, later: &Later) {
    if count(&TOLD, window) <= MOST {
        lines::write(log::Level::Warn, &line());
        return;
    }
    let first = LEFT_OUT.with(|left| {
        let mut left = left.borrow_mut();
        let count = left.entry(window).or_insert(0);
        *count += 1;
        *count == 1
    });
    if first {
        later(PER, Box::new(move || sum_up(window)));
    }
}

/// Writes how many lines `window` had left out, and starts counting anew.
fn sum_up(window: WindowKey) {
    let left = LEFT_OUT.with(|left| left.borrow_mut().remove(&window)).unwrap_or(0);
    if left > 0 {
        lines::write(log::Level::Warn, &lines::left_out(left));
    }
}
