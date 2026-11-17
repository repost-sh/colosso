use std::any::Any;
use std::fmt;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

#[derive(Debug)]
pub struct WorkerPanicked(String);

impl WorkerPanicked {
    fn from_payload(payload: &(dyn Any + Send)) -> Self {
        let message = payload
            .downcast_ref::<&str>()
            .map(|text| (*text).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "no message".to_owned());
        Self(message)
    }
}

impl fmt::Display for WorkerPanicked {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "a worker thread panicked: {}", self.0)
    }
}

pub fn map_in_order<Item: Sync, Output: Send>(
    items: &[Item],
    work: impl Fn(&Item) -> Output + Sync,
) -> Result<Vec<Output>, WorkerPanicked> {
    let workers = thread::available_parallelism()
        .map_or(1, NonZeroUsize::get)
        .min(items.len());
    let next = AtomicUsize::new(0);
    let per_worker = thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(index) else {
                            return done;
                        };
                        done.push((index, work(item)));
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .map_err(|payload| WorkerPanicked::from_payload(&*payload))
            })
            .collect::<Result<Vec<_>, _>>()
    })?;
    let mut finished: Vec<(usize, Output)> = per_worker.into_iter().flatten().collect();
    finished.sort_by_key(|(index, _)| *index);
    Ok(finished.into_iter().map(|(_, output)| output).collect())
}
