//! macOS's `FSEvents`, driven directly rather than through `notify`: one stream for every
//! checkout open (`notify` makes one for each watcher), handing over its reports on a queue
//! that runs ahead of the work in the app, and without `notify`'s cost for each report.
//!
//! The platform drops reports a process doesn't take quickly enough, and then says only
//! that it lost track of what changed. Measured on 50,000 files written into an ignored
//! folder and then removed: handed over as they come, on a queue that runs ahead, the writes
//! lose none; gathered for 50 ms first, or on a queue at the default priority, they lose
//! some, and the removal loses some however they're handed over. So reports are handed over
//! as they come, and the worker gathers them instead (see `next_batch`).
//!
//! The bindings are `objc2-core-services`, which `notify` already builds on, so nothing new
//! is compiled.

use std::ffi::{CStr, c_char, c_void};
use std::path::PathBuf;
use std::ptr::{self, NonNull};
use std::sync::LazyLock;

use dispatch2::{
    DispatchQoS, DispatchQueue, DispatchQueueAttr, DispatchRetained, GlobalQueueIdentifier,
};
use objc2_core_foundation::{CFArray, CFRetained, CFString};
use objc2_core_services as fs;

use super::{Batch, report};
use crate::places::text;

/// Reports that mean the platform lost track of what changed under the path: the folder
/// must be read again, or its reports were dropped on the way.
const LOST: u32 = fs::kFSEventStreamEventFlagMustScanSubDirs
    | fs::kFSEventStreamEventFlagUserDropped
    | fs::kFSEventStreamEventFlagKernelDropped;

/// Reports about the stream rather than the disk.
const ABOUT_THE_STREAM: u32 =
    fs::kFSEventStreamEventFlagHistoryDone | fs::kFSEventStreamEventFlagEventIdsWrapped;

/// Where every stream's reports are handed over, one batch at a time and in order, so two
/// streams overlapping while one takes over from the other never report at once. It runs
/// at the priority of what the user is waiting on: behind the app's own work, it falls
/// behind a burst, and the platform drops what it can't hand over.
static QUEUE: LazyLock<DispatchRetained<DispatchQueue>> = LazyLock::new(|| {
    let ahead = DispatchQueue::global_queue(GlobalQueueIdentifier::QualityOfService(
        DispatchQoS::UserInteractive,
    ));
    DispatchQueue::new_with_target(
        "colosso.project.fsevents",
        DispatchQueueAttr::SERIAL,
        Some(&ahead),
    )
});

/// The platform's watcher: one stream over every folder watched.
pub(crate) struct Platform {
    stream: Stream,
    folders: Vec<PathBuf>,
}

impl Platform {
    pub(crate) fn start(folders: Vec<PathBuf>) -> Result<Platform, String> {
        Ok(Platform {
            stream: Stream::start(&folders)?,
            folders,
        })
    }

    pub(crate) fn folders(&self) -> &[PathBuf] {
        &self.folders
    }

    /// Watches `folders` from now on. A stream's folders are fixed, so a new one starts
    /// before the old one stops: nothing written in between is missed, though some of it may
    /// be reported twice, which reads a folder again for nothing.
    pub(crate) fn cover(&mut self, folders: Vec<PathBuf>) -> Result<(), String> {
        self.stream = Stream::start(&folders)?;
        self.folders = folders;
        Ok(())
    }
}

/// A started stream, stopped when dropped.
struct Stream(fs::FSEventStreamRef);

// SAFETY: a stream is only started and stopped, under the hub's lock on whichever thread
// holds it; FSEvents hands its reports over on its own queue.
unsafe impl Send for Stream {}

impl Stream {
    fn start(folders: &[PathBuf]) -> Result<Stream, String> {
        let paths: Vec<CFRetained<CFString>> = (folders.iter())
            .map(|folder| CFString::from_str(&text(folder)))
            .collect();
        let paths = CFArray::from_retained_objects(&paths);
        // Every file's own report, rather than the folder's it's in, handed over at once.
        let flags = fs::kFSEventStreamCreateFlagFileEvents | fs::kFSEventStreamCreateFlagNoDefer;
        // SAFETY: the callback matches what FSEvents calls; no context is passed, which it
        // allows; the paths are an array of strings, which the stream copies.
        let stream = unsafe {
            fs::FSEventStreamCreate(
                None,
                Some(callback),
                ptr::null_mut(),
                paths.as_opaque(),
                fs::kFSEventStreamEventIdSinceNow,
                0.0,
                flags,
            )
        };
        if stream.is_null() {
            return Err("macOS couldn't make a file system event stream".to_owned());
        }
        // SAFETY: the stream was just made, and the queue lives as long as the process.
        let started = unsafe {
            fs::FSEventStreamSetDispatchQueue(stream, Some(&QUEUE));
            fs::FSEventStreamStart(stream)
        };
        if !started {
            // SAFETY: made and scheduled, but not started: undone in the order it was done.
            unsafe {
                fs::FSEventStreamInvalidate(stream);
                fs::FSEventStreamRelease(stream);
            }
            return Err("macOS couldn't start watching the checkout".to_owned());
        }
        Ok(Stream(stream))
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        // SAFETY: the stream was started; what it gathered is handed over before it stops,
        // and it's unscheduled before it's released.
        unsafe {
            fs::FSEventStreamFlushSync(self.0);
            fs::FSEventStreamStop(self.0);
            fs::FSEventStreamInvalidate(self.0);
            fs::FSEventStreamRelease(self.0);
        }
    }
}

/// Takes one batch of reports from `FSEvents`, on its queue, copying it off to be sorted:
/// while this runs, the platform holds the reports after it, and drops them past a few.
unsafe extern "C-unwind" fn callback(
    _stream: fs::ConstFSEventStreamRef,
    _context: *mut c_void,
    count: usize,
    paths: NonNull<c_void>,
    flags: NonNull<fs::FSEventStreamEventFlags>,
    _ids: NonNull<fs::FSEventStreamEventId>,
) {
    let paths = paths.as_ptr().cast::<*const c_char>();
    let mut batch = Batch::default();
    for at in 0..count {
        // SAFETY: without `UseCFTypes`, FSEvents hands over `count` paths as C strings, and
        // as many flags, valid for the call.
        let (path, flags) = unsafe { (CStr::from_ptr(*paths.add(at)), *flags.as_ptr().add(at)) };
        let lost = flags & LOST != 0;
        if flags & ABOUT_THE_STREAM != 0 && !lost {
            continue;
        }
        batch.push(&path.to_string_lossy(), lost);
    }
    report(batch);
}
