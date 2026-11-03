//! The threads a finder's searches run on, made once with the finder, and the matcher each
//! thread matches with.

use std::cell::RefCell;

use nucleo_matcher::{Config, Matcher};
use rayon::prelude::*;
use rayon::{ThreadPool, ThreadPoolBuilder};

/// A finder's threads. With none, as when one was asked for or the system wouldn't start
/// them, everything runs on the caller's thread. A thread that panics panics the caller.
pub(super) struct Pool(Option<ThreadPool>);

impl Pool {
    pub(super) fn new(threads: usize) -> Pool {
        if threads <= 1 {
            return Pool(None);
        }
        let built = ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|nth| format!("finder-{nth}"))
            .build();
        match built {
            Ok(pool) => Pool(Some(pool)),
            Err(error) => {
                log::warn!("Go to File searches on one thread, as threads wouldn't start: {error}");
                Pool(None)
            }
        }
    }

    /// How many threads a search runs on.
    pub(super) fn threads(&self) -> usize {
        self.0.as_ref().map_or(1, ThreadPool::current_num_threads)
    }

    /// What `work` makes of each of `items`, in their order, the items shared among the
    /// threads as each is free.
    pub(super) fn each<T: Send, R: Send>(
        &self,
        items: Vec<T>,
        work: impl Fn(T) -> R + Sync + Send,
    ) -> Vec<R> {
        match &self.0 {
            Some(pool) if items.len() > 1 => {
                pool.install(|| items.into_par_iter().map(work).collect())
            }
            _ => items.into_iter().map(work).collect(),
        }
    }
}

/// A matcher, with what it reads paths into and says where they matched in, which a thread
/// keeps for every search it takes part in.
pub(super) struct Matching {
    pub matcher: Matcher,
    pub chars: Vec<char>,
    pub indices: Vec<u32>,
}

thread_local! {
    static MATCHING: RefCell<Matching> = RefCell::new(Matching {
        matcher: Matcher::new(Config::DEFAULT.match_paths()),
        chars: Vec::new(),
        indices: Vec::new(),
    });
}

/// `work` with this thread's matcher, which it mustn't ask for again meanwhile.
pub(super) fn matching<R>(work: impl FnOnce(&mut Matching) -> R) -> R {
    MATCHING.with_borrow_mut(work)
}
