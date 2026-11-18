//! A file the app keeps as JSON in its data folder, as the chat drafts, the history and the
//! browser tabs are: read as it starts, or nothing if it can't be; written a moment after a
//! change, with whatever changed meanwhile, so changes as steady as typing can't put the write
//! off for good; and written once more on the way out if a write is still due. Compact, as the
//! history runs to thousands of entries. Each file has one writer ([`Writers`]), so writes land
//! in the order they're handed over, off the main thread, and the app quitting waits on them.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::channel::oneshot;
use gpui_kit::App;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::blocking::{self, Blocking};

mod writer;

pub use writer::Writers;

/// What's in the JSON file at `path`; `None` when there's none, or it can't be read, which is
/// said.
pub fn read<T: DeserializeOwned>(path: &Path, _: Blocking<'_>) -> Option<T> {
    let read = fs::read(path).and_then(|json| Ok(serde_json::from_slice(&json)?));
    read.inspect_err(|error: &io::Error| {
        if error.kind() != io::ErrorKind::NotFound {
            log::warn!("couldn't read {}: {error}", path.display());
        }
    })
    .ok()
}

/// `value` as the JSON to write, or `None` if it can't be, which is said.
pub fn encode<T: Serialize + ?Sized>(value: &T) -> Option<Vec<u8>> {
    serde_json::to_vec(value)
        .inspect_err(|error| log::error!("couldn't encode a file to keep: {error}"))
        .ok()
}

/// Where a kept file is, if the app has a folder to keep it in, and whether a write is due.
#[derive(Debug, Default)]
pub struct KeptFile {
    path: Option<PathBuf>,
    due: bool,
}

impl KeptFile {
    pub fn new(path: Option<PathBuf>) -> Self {
        KeptFile { path, due: false }
    }

    /// Notes a change: whether a write is to start now, which it isn't when one is due already,
    /// as the change goes with it, or when there's no file to write.
    #[must_use]
    pub fn changed(&mut self) -> bool {
        if self.path.is_none() || self.due {
            return false;
        }
        self.due = true;
        true
    }

    /// Where to write, if a write is due, which then no longer is.
    pub fn take_due(&mut self) -> Option<PathBuf> {
        std::mem::take(&mut self.due)
            .then(|| self.path.clone())
            .flatten()
    }
}

/// Hands, `delay` from now, what `take` gives then to its file's writer: where, and the JSON,
/// if the write is still due; one handed on the way out meanwhile isn't.
pub fn write_later(
    cx: &mut App,
    delay: Duration,
    take: impl FnOnce(&mut App) -> Option<(PathBuf, Vec<u8>)> + 'static,
) {
    cx.spawn(async move |cx| {
        cx.background_executor().timer(delay).await;
        cx.update(|cx| write_due(take(cx), cx));
    })
    .detach();
}

/// Writes, as the app quits, what `take` gives then, if a write is due, with everything else
/// still waiting to be written.
pub fn write_on_quit(
    cx: &mut App,
    mut take: impl FnMut(&mut App) -> Option<(PathBuf, Vec<u8>)> + 'static,
) {
    blocking::on_quit(cx, move |may, cx| {
        write_due(take(cx), cx);
        Writers::of(cx).settle(may);
    });
}

/// Hands `json` to the writer of the file at `path`; what it returns is ready once the file
/// holds it, or what was handed after it.
pub fn write(path: PathBuf, json: Vec<u8>, cx: &mut App) -> oneshot::Receiver<()> {
    Writers::of(cx).write(path, json, cx)
}

/// Hands `due`, where and the JSON, to its file's writer, if there's a write due.
pub fn write_due(due: Option<(PathBuf, Vec<u8>)>, cx: &mut App) {
    if let Some((path, json)) = due {
        drop(write(path, json, cx));
    }
}

/// Writes `json` to `path` through a temporary file beside it, so a crash mid-write never
/// leaves half a file behind, making its folder first if it has none; a failure is logged.
pub fn write_atomic(path: &Path, json: &[u8], _: Blocking<'_>) {
    let temporary = path.with_extension("json.tmp");
    let written = path
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| fs::write(&temporary, json))
        .and_then(|()| fs::rename(&temporary, path));
    if let Err(error) = written {
        log::error!("couldn't save {}: {error}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gpui_kit::{App, Global, TestAppContext};

    use super::{KeptFile, encode, write_later};
    use crate::blocking::in_test;
    use crate::test_support::scratch;

    fn read<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> Option<T> {
        in_test(|may| super::read(path, may))
    }

    const DELAY: Duration = Duration::from_millis(100);

    /// A store as the app keeps them: words, kept in a file.
    struct Words {
        words: Vec<String>,
        file: KeptFile,
    }

    impl Global for Words {}

    fn say(word: &str, cx: &mut App) {
        let words = cx.global_mut::<Words>();
        words.words.push(word.to_owned());
        if words.file.changed() {
            write_later(cx, DELAY, |cx| {
                let words = cx.global_mut::<Words>();
                Some((words.file.take_due()?, encode(&words.words)?))
            });
        }
    }

    #[gpui_kit::test]
    fn changes_are_written_together_a_moment_after_the_first(cx: &mut TestAppContext) {
        let path = scratch("kept-file-later").join("words.json");
        cx.update(|cx| {
            cx.set_global(Words {
                words: Vec::new(),
                file: KeptFile::new(Some(path.clone())),
            });
            say("one", cx);
        });
        cx.executor().advance_clock(DELAY / 2);
        cx.update(|cx| say("two", cx));
        cx.run_until_parked();
        assert_eq!(read::<Vec<String>>(&path), None, "not yet");
        cx.executor().advance_clock(DELAY / 2);
        cx.run_until_parked();
        assert_eq!(read(&path), Some(vec!["one".to_owned(), "two".to_owned()]));

        cx.update(|cx| {
            say("three", cx);
            let words = cx.global_mut::<Words>();
            let due = words.file.take_due().expect("a write is due");
            let json = encode(&words.words).unwrap();
            super::write_due(Some((due, json)), cx);
        });
        cx.run_until_parked();
        std::fs::remove_file(&path).unwrap();
        cx.executor().advance_clock(DELAY);
        cx.run_until_parked();
        assert!(!path.exists(), "the late write was no longer due");
    }

    #[gpui_kit::test(iterations = 20)]
    fn writes_handed_in_a_row_land_in_the_order_handed(cx: &mut TestAppContext) {
        let path = scratch("kept-file-in-order").join("words.json");
        cx.update(|cx| {
            drop(super::write(path.clone(), encode(&["one"]).unwrap(), cx));
            drop(super::write(path.clone(), encode(&["two"]).unwrap(), cx));
        });
        cx.run_until_parked();
        assert_eq!(read(&path), Some(vec!["two".to_owned()]));
    }

    #[test]
    fn a_file_with_no_folder_is_never_due_and_a_bad_one_reads_as_none() {
        let mut nowhere = KeptFile::new(None);
        assert!(!nowhere.changed());
        assert_eq!(nowhere.take_due(), None);
        let path = scratch("kept-file-bad").join("bad.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(read::<Vec<String>>(&path), None);
        assert_eq!(read::<Vec<String>>(&path.with_file_name("none.json")), None);
    }
}
