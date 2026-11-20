use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// The time a host's start has taken, as the app counts it while it waits for the host's
/// hello. In the app's process the host stops it while CEF starts on the main thread
/// ([`StartClock::stopped_during`]): the whole app waits on that start, so giving up on it
/// meanwhile gains nothing, and a slow machine would say the engine failed as it started.
#[derive(Clone, Debug)]
pub struct StartClock(Arc<Mutex<Counted>>);

#[derive(Debug)]
struct Counted {
    began: Instant,
    stopped: Duration,
    since: Option<Instant>,
}

impl StartClock {
    pub(super) fn new() -> Self {
        Self(Arc::new(Mutex::new(Counted {
            began: Instant::now(),
            stopped: Duration::ZERO,
            since: None,
        })))
    }

    /// Runs `run`, CEF's start on the main thread, with the clock stopped.
    pub fn stopped_during<T>(&self, run: impl FnOnce() -> T) -> T {
        self.with(|counted| counted.since = Some(Instant::now()));
        let ran = run();
        self.with(|counted| {
            if let Some(since) = counted.since.take() {
                counted.stopped += since.elapsed();
            }
        });
        ran
    }

    /// What's left of `budget` once the time counted is taken off it: how long the wait's next
    /// read may take. While the clock is stopped, a read takes at least a twentieth of
    /// `budget`, so a wait with little left as it stopped doesn't spin until it starts again.
    pub(super) fn left(&self, budget: Duration) -> Duration {
        self.with(|counted| {
            let stopped = counted.stopped + counted.since.map_or(Duration::ZERO, |s| s.elapsed());
            let left = budget.saturating_sub(counted.began.elapsed().saturating_sub(stopped));
            match counted.since {
                Some(_) if !left.is_zero() => left.max(budget / 20),
                _ => left,
            }
        })
    }

    fn with<T>(&self, change: impl FnOnce(&mut Counted) -> T) -> T {
        change(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}
