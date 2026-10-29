//! The files a checkout has open: one document per path, read off the main thread once
//! however many views show it, and gone when the last view lets go of it. A document's
//! version moves each time its content is replaced; it's what the editor and the language
//! servers will keep in step by, once a document takes edits.

use std::collections::HashMap;
use std::fs;
use std::future::Future;
use std::io::Read as _;
use std::path::Path;
use std::sync::Arc;

use futures::channel::oneshot;
use gpui_kit::{App, AppContext as _, Context, Entity, SharedString, Task, WeakEntity};

/// Files past this size are noted rather than opened. The editor builds its buffer on the
/// main thread: with the first frame, 4 MB of plain text takes 37 ms (44 at worst) on an M4
/// Max, and 16 MB 155 ms, so this keeps opening a file under about 50 ms.
pub const MOST_BYTES: u64 = 4 << 20;
/// How much of a file is looked at for a NUL byte, which says it's binary, as git looks.
const SNIFF: usize = 8 << 10;

/// What a document holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content {
    Reading,
    /// Its text, any bytes that aren't UTF-8 replaced.
    Text(SharedString),
    Binary,
    TooLarge(u64),
    Failed(SharedString),
}

/// One file of the checkout, as read.
pub struct Document {
    path: Arc<Path>,
    content: Content,
    /// Moves each time `content` is replaced, so a view can tell its copy is out of date.
    version: u64,
    /// Those waiting for it to be read.
    waiting: Vec<oneshot::Sender<()>>,
    _reading: Option<Task<()>>,
}

impl Document {
    fn new(path: Arc<Path>, cx: &mut Context<Self>) -> Self {
        let reading = cx.spawn({
            let path = path.clone();
            async move |this, cx| {
                let content = cx.background_spawn(async move { read(&path) }).await;
                this.update(cx, |this, cx| this.set_content(content, cx))
                    .ok();
            }
        });
        Document {
            path,
            content: Content::Reading,
            version: 0,
            waiting: Vec::new(),
            _reading: Some(reading),
        }
    }

    pub fn path(&self) -> &Arc<Path> {
        &self.path
    }

    pub fn content(&self) -> &Content {
        &self.content
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// Done once the file's read, at once if it is; or once the document's gone.
    pub fn read(&mut self) -> impl Future<Output = ()> + use<> {
        let (read, waiting) = oneshot::channel();
        if self.content == Content::Reading {
            self.waiting.push(read);
        } else {
            let _: Result<(), ()> = read.send(());
        }
        async move {
            let _: Result<(), oneshot::Canceled> = waiting.await;
        }
    }

    fn set_content(&mut self, content: Content, cx: &mut Context<Self>) {
        self.content = content;
        self.version += 1;
        self._reading = None;
        for read in self.waiting.drain(..) {
            let _: Result<(), ()> = read.send(());
        }
        cx.notify();
    }
}

/// The checkout's documents, by absolute path. The views showing one hold it; this only
/// finds it again, so a file opened twice is read once.
#[derive(Default)]
pub struct Documents {
    open: HashMap<Arc<Path>, WeakEntity<Document>>,
}

impl Documents {
    /// The document for the file at `path`, read unless a view has it already.
    pub fn open(&mut self, path: &Path, cx: &mut App) -> Entity<Document> {
        self.open.retain(|_, document| document.upgrade().is_some());
        if let Some(document) = self.open.get(path).and_then(WeakEntity::upgrade) {
            return document;
        }
        let path: Arc<Path> = path.into();
        let document = cx.new(|cx| Document::new(path.clone(), cx));
        self.open.insert(path, document.downgrade());
        document
    }
}

/// Reads the file at `path` as text, unless it's too large or binary.
fn read(path: &Path) -> Content {
    read_at_most(path, MOST_BYTES)
}

/// Reads the file at `path` as text, unless it's over `most` bytes or binary.
pub fn read_at_most(path: &Path, most: u64) -> Content {
    let opened = fs::File::open(path).and_then(|file| Ok((file.metadata()?.len(), file)));
    let (bytes, file) = match opened {
        Ok(opened) => opened,
        Err(error) => return Content::Failed(error.to_string().into()),
    };
    if bytes > most {
        return Content::TooLarge(bytes);
    }
    let mut data = Vec::with_capacity(bytes as usize);
    // Up to the cap and a byte more, in case it grew since it was measured.
    if let Err(error) = file.take(most + 1).read_to_end(&mut data) {
        return Content::Failed(error.to_string().into());
    }
    if data.len() as u64 > most {
        return Content::TooLarge(data.len() as u64);
    }
    if data.iter().take(SNIFF).any(|&byte| byte == 0) {
        return Content::Binary;
    }
    let text = String::from_utf8(data)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned());
    Content::Text(text.into())
}
