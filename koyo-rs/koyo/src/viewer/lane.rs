//! The thread a viewer's file is read and drawn on: one a viewer, which opens the file, says
//! what it is, then draws what it's asked for, one piece at a time, the most wanted first. A
//! newer list of what's wanted replaces the one waiting, so a scroll that moves on doesn't wait
//! for tiles nobody sees any more. Whatever Quartz or resvg holds of the file stays on the
//! thread; only finished pixels leave it. The thread ends once the viewer lets go of it.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, PoisonError};

use futures::channel::mpsc::UnboundedSender;
use gpui_kit::RenderImage;

use super::picture::Probe;
use super::tiles::TileKey;

/// What a viewer asks its thread to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Job {
    /// A whole picture, its longest side at most this many pixels.
    Whole(u32),
    /// A tile of a page.
    Tile(TileKey),
}

/// What a file turned out to be, once it's opened.
#[derive(Clone, Debug, PartialEq)]
pub enum Opened {
    Picture(Probe),
    /// Pages, each one's size in points, upright.
    Pages(Vec<(f32, f32)>),
}

/// What the thread tells its viewer.
pub enum Out {
    Opened(Opened),
    /// The file couldn't be opened, for this reason, said to the person.
    Failed(String),
    Drawn(Job, Arc<RenderImage>),
    /// What was asked couldn't be drawn, which is logged: what's shown stays as it was.
    Missed(Job),
}

/// A file opened on the thread, which draws what it's asked.
pub trait Source {
    fn draw(&mut self, job: Job) -> Option<Arc<RenderImage>>;
}

/// The viewer's hold on its thread: dropped, the thread ends once what it's drawing is done.
pub struct Lane {
    shared: Arc<Shared>,
}

struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
}

#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
    closed: bool,
}

impl Lane {
    /// Starts the thread, named for `name`, which opens the file with `open` and tells `out`
    /// what it found and, from then on, what it drew.
    pub fn start<S: Source>(
        name: &str,
        open: impl FnOnce() -> Result<(S, Opened), String> + Send + 'static,
        out: UnboundedSender<Out>,
    ) -> std::io::Result<Lane> {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue::default()),
            wake: Condvar::new(),
        });
        let thread = Arc::clone(&shared);
        std::thread::Builder::new()
            .name(format!("viewer {name}"))
            .spawn(move || run(&thread, open, &out))?;
        Ok(Lane { shared })
    }

    /// Has the thread draw `jobs`, first to last, in place of whatever it hadn't got to.
    pub fn want(&self, jobs: impl IntoIterator<Item = Job>) {
        let mut queue = self
            .shared
            .queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        queue.jobs.clear();
        queue.jobs.extend(jobs);
        drop(queue);
        self.shared.wake.notify_one();
    }
}

impl Drop for Lane {
    fn drop(&mut self) {
        let mut queue = self
            .shared
            .queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        queue.closed = true;
        queue.jobs.clear();
        drop(queue);
        self.shared.wake.notify_one();
    }
}

fn run<S: Source>(
    shared: &Shared,
    open: impl FnOnce() -> Result<(S, Opened), String>,
    out: &UnboundedSender<Out>,
) {
    let mut source = match open() {
        Ok((source, opened)) => {
            if out.unbounded_send(Out::Opened(opened)).is_err() {
                return;
            }
            source
        }
        Err(why) => {
            out.unbounded_send(Out::Failed(why)).ok();
            return;
        }
    };
    while let Some(job) = next(shared) {
        let drawn = match source.draw(job) {
            Some(image) => Out::Drawn(job, image),
            None => Out::Missed(job),
        };
        if out.unbounded_send(drawn).is_err() {
            return;
        }
    }
}

/// The next job, once there is one; `None` once the viewer's gone.
fn next(shared: &Shared) -> Option<Job> {
    let mut queue = shared.queue.lock().unwrap_or_else(PoisonError::into_inner);
    loop {
        if queue.closed {
            return None;
        }
        if let Some(job) = queue.jobs.pop_front() {
            return Some(job);
        }
        queue = shared
            .wake
            .wait(queue)
            .unwrap_or_else(PoisonError::into_inner);
    }
}
