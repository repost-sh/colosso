//! Reading many folders at once. Readers take folders off a queue on a few threads; the
//! calling thread takes the folders read so far, files them together, and says which folders
//! to read next, so what's under a folder is known only once the folder itself is.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, PoisonError, mpsc};
use std::thread;

use crate::lock;
use crate::read::{Job, Read, read};

/// The folders waiting to be read, and whether reading is over.
struct Queue {
    jobs: VecDeque<Job>,
    finished: bool,
}

struct Shared<'a> {
    root: &'a Path,
    queue: Mutex<Queue>,
    ready: Condvar,
    stopped: &'a AtomicBool,
}

/// Reads `jobs` under `root` on up to `threads` threads, passing the folders read to `found`
/// as many at a time as are waiting, which returns the folders to read next. Returns once
/// nothing is left to read, or `stopped` is set.
pub(crate) fn scan(
    root: &Path,
    threads: usize,
    jobs: Vec<Job>,
    stopped: &AtomicBool,
    mut found: impl FnMut(Vec<Read>) -> Vec<Job>,
) {
    let mut outstanding = jobs.len();
    if outstanding == 0 {
        return;
    }
    let shared = Shared {
        root,
        queue: Mutex::new(Queue {
            jobs: jobs.into(),
            finished: false,
        }),
        ready: Condvar::new(),
        stopped,
    };
    let (sender, read_folders) = mpsc::channel();
    thread::scope(|scope| {
        for _ in 0..threads.max(1) {
            let sender = sender.clone();
            let shared = &shared;
            scope.spawn(move || reader(shared, &sender));
        }
        drop(sender);
        while outstanding > 0 && !stopped.load(Ordering::Relaxed) {
            let Ok(folder) = read_folders.recv() else {
                break;
            };
            let mut folders = vec![folder];
            folders.extend(read_folders.try_iter());
            outstanding -= folders.len();
            let next = found(folders);
            if !next.is_empty() {
                outstanding += next.len();
                shared.queue().jobs.extend(next);
                shared.ready.notify_all();
            }
        }
        shared.queue().finished = true;
        shared.ready.notify_all();
    });
}

impl Shared<'_> {
    fn queue(&self) -> std::sync::MutexGuard<'_, Queue> {
        lock(&self.queue)
    }

    /// The next folder to read, waiting for one; `None` once reading is over.
    fn next(&self) -> Option<Job> {
        let mut queue = self.queue();
        loop {
            if queue.finished || self.stopped.load(Ordering::Relaxed) {
                return None;
            }
            if let Some(job) = queue.jobs.pop_front() {
                return Some(job);
            }
            queue = self
                .ready
                .wait(queue)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

fn reader(shared: &Shared<'_>, sender: &mpsc::Sender<Read>) {
    while let Some(job) = shared.next() {
        if sender.send(read(shared.root, job)).is_err() {
            return;
        }
    }
}
