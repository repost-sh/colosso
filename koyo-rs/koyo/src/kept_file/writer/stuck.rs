//! A disk for tests that sticks on one file's write, as a stalled disk does: the write runs on
//! a thread of its own, as the background executor runs one, and holds until let go. Every
//! write lands in the file, and is noted in the order it landed.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use super::{Shared, Writers};
use crate::blocking::{Blocking, in_test};
use crate::kept_file::write_atomic;

/// Writers whose write of one file is running and stuck, until this is dropped.
pub struct Stuck {
    pub writers: Writers,
    landed: Arc<Mutex<Vec<(PathBuf, String)>>>,
    release: Option<Sender<()>>,
    writing: Option<JoinHandle<()>>,
}

impl Stuck {
    /// Writers writing `json` to `path` on a thread of their own, which sticks there.
    pub fn writing(path: &Path, json: &str) -> Self {
        let (started, begun) = mpsc::channel();
        let (release, held) = mpsc::channel::<()>();
        let held = Mutex::new(held);
        let landed = Arc::new(Mutex::new(Vec::new()));
        let notes = Arc::clone(&landed);
        let stuck = path.to_owned();
        let write = move |path: &Path, json: &[u8], may: Blocking<'_>| {
            if path == stuck {
                let _ = started.send(());
                let _ = held.lock().unwrap_or_else(PoisonError::into_inner).recv();
            }
            write_atomic(path, json, may);
            let json = String::from_utf8_lossy(json).into_owned();
            let mut landed = notes.lock().unwrap_or_else(PoisonError::into_inner);
            landed.push((path.to_owned(), json));
        };
        let writers = Writers(Arc::new(Shared::writing_with(Box::new(write))));
        let (_, drain) = writers.hand(path.to_owned(), json.as_bytes().to_vec());
        assert!(drain, "a drain starts for the first write of a file");
        let background = writers.clone();
        let path = path.to_owned();
        let writing = std::thread::spawn(move || in_test(|may| background.drain(&path, may)));
        let begun = begun.recv_timeout(Duration::from_secs(5));
        assert_eq!(begun, Ok(()), "the stuck write began");
        Stuck {
            writers,
            landed,
            release: Some(release),
            writing: Some(writing),
        }
    }

    /// Hands `json` to `path`'s writer, as a store does, and no drain runs for it: it waits.
    pub fn hand(&self, path: &Path, json: &str) {
        drop(self.writers.hand(path.to_owned(), json.as_bytes().to_vec()));
    }

    /// Each write landed so far, the file and what it holds, in the order they landed.
    pub fn landed(&self) -> Vec<(PathBuf, String)> {
        self.landed.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Lets the stuck write go on, and waits until its file's writes have all landed.
    pub fn release(mut self) -> Vec<(PathBuf, String)> {
        self.end();
        self.landed()
    }

    fn end(&mut self) {
        drop(self.release.take());
        if let Some(writing) = self.writing.take() {
            writing.join().unwrap();
        }
    }
}

impl Drop for Stuck {
    fn drop(&mut self) {
        self.end();
    }
}
