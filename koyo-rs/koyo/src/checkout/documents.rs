//! The files a checkout has open: one document per path, read off the main thread once
//! however many views show it, and gone when the last view lets go of it. A file read again
//! goes to the views as the lines that differ, compared off the main thread too; the view
//! edits its own copy, which the document knows is unsaved until it's told to save it, and
//! it doesn't read a file again under unsaved edits.

use std::collections::HashMap;
use std::fs;
use std::io::Read as _;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, SharedString, Task, WeakEntity,
};
use similar::{DiffTag, TextDiff};

use crate::changes::Touched;

/// Files past this size are noted rather than opened. The editor builds its buffer on the
/// main thread: with the first frame, 4 MB of plain text takes 37 ms (44 at worst) on an M4
/// Max, and 16 MB 155 ms, so this keeps opening a file under about 50 ms.
pub const MOST_BYTES: u64 = 4 << 20;
/// How much of a file is looked at for a NUL byte, which says it's binary, as git looks.
const SNIFF: usize = 8 << 10;
/// How long a file read again is compared with the text before, before the comparison is
/// settled for as it stands: a file rewritten wholesale would take far longer, for an
/// answer no better than replacing the lot.
const DIFF_TIMEOUT: Duration = Duration::from_millis(100);

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

/// The edits that turn one text into another: each a byte range of the first, and the text
/// that takes its place.
pub type Edits = Vec<(Range<usize>, String)>;

/// One file of the checkout, as read.
pub struct Document {
    path: Arc<Path>,
    content: Content,
    /// Moves with each text the document takes, read or saved, so a read begun before one
    /// is told from one begun after.
    generation: u64,
    /// Whether a view has edits the file doesn't.
    dirty: bool,
    /// Whether the file is to be read again once the read under way is done.
    again: bool,
    _reading: Option<Task<()>>,
}

/// What a document tells the views showing it.
#[derive(Debug, PartialEq, Eq)]
pub enum DocumentEvent {
    /// The file was read and found changed: as the edits that take the text it held to the
    /// text it holds, when both are text, and else to be shown over.
    Read { edits: Option<Edits> },
    /// The file holds the text the views show.
    Saved,
}

impl EventEmitter<DocumentEvent> for Document {}

impl Document {
    fn new(path: Arc<Path>, cx: &mut Context<Self>) -> Self {
        let mut document = Document {
            path,
            content: Content::Reading,
            generation: 0,
            dirty: false,
            again: false,
            _reading: None,
        };
        document.start_reading(cx);
        document
    }

    /// Reads the file off the main thread, and compares it there with the text held.
    fn start_reading(&mut self, cx: &mut Context<Self>) {
        let path = self.path.clone();
        let generation = self.generation;
        let old = match &self.content {
            Content::Text(text) => Some(text.clone()),
            _ => None,
        };
        self._reading = Some(cx.spawn(async move |this, cx| {
            let (content, edits) = cx
                .background_spawn(async move {
                    let content = read(&path);
                    let edits = match (&old, &content) {
                        (Some(old), Content::Text(new)) if old != new => {
                            Some(line_edits(old, new))
                        }
                        _ => None,
                    };
                    (content, edits)
                })
                .await;
            this.update(cx, |this, cx| this.set_content(generation, content, edits, cx))
                .ok();
        }));
    }

    /// Reads the file again, unless a view has edits the file doesn't; one read under way,
    /// it's read once more when that's done. Read the same, nothing's said.
    pub fn reread(&mut self, cx: &mut Context<Self>) {
        if self.dirty {
            log::info!("{} changed on disk under unsaved edits", self.path.display());
        } else if self._reading.is_some() {
            self.again = true;
        } else {
            self.start_reading(cx);
        }
    }

    /// Whether a view has edits the file doesn't.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Whether the view showing it has edits the file doesn't, as the view tells it.
    pub fn set_dirty(&mut self, dirty: bool, cx: &mut Context<Self>) {
        if self.dirty != dirty {
            self.dirty = dirty;
            cx.notify();
        }
    }

    /// Writes `text`, the view's text, to the file off the main thread. Written, the
    /// document holds it, and a read begun before is read over. Done once it's written, or
    /// with why it couldn't be; dropped, the write may not happen.
    pub fn save(&mut self, text: SharedString, cx: &mut Context<Self>) -> Task<Result<(), SharedString>> {
        let path = self.path.clone();
        let written = cx.background_spawn({
            let text = text.clone();
            async move { fs::write(&path, text.as_bytes()) }
        });
        cx.spawn(async move |this, cx| {
            written
                .await
                .map_err(|error| SharedString::from(error.to_string()))?;
            this.update(cx, |this, cx| {
                this.content = Content::Text(text);
                this.generation += 1;
                cx.emit(DocumentEvent::Saved);
                cx.notify();
            })
            .map_err(|_gone| SharedString::from("the document is gone"))
        })
    }

    pub fn path(&self) -> &Arc<Path> {
        &self.path
    }

    pub fn content(&self) -> &Content {
        &self.content
    }

    /// Takes what a read begun at `generation` found, unless the text moved under it, when
    /// it's read over; and reads it over too when asked to while it was read.
    fn set_content(
        &mut self,
        generation: u64,
        content: Content,
        edits: Option<Edits>,
        cx: &mut Context<Self>,
    ) {
        self._reading = None;
        let stale = generation != self.generation;
        if !stale && content != self.content {
            self.content = content;
            self.generation += 1;
            cx.emit(DocumentEvent::Read { edits });
            cx.notify();
        }
        if (stale || std::mem::take(&mut self.again)) && !self.dirty {
            self.start_reading(cx);
        }
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

    /// The document for the file at `path`, if a view has it.
    pub fn find(&self, path: &Path) -> Option<Entity<Document>> {
        self.open.get(path)?.upgrade()
    }

    /// Reads again the open documents `touched` covers, by their paths under `root`.
    pub fn touched(&mut self, touched: &[Touched], root: &Path, cx: &mut App) {
        self.open.retain(|_, document| document.upgrade().is_some());
        for (path, document) in &self.open {
            let Some(relative) = path.strip_prefix(root).ok().and_then(Path::to_str) else {
                continue;
            };
            if touched.iter().any(|touched| touched.covers(relative))
                && let Some(document) = document.upgrade()
            {
                document.update(cx, Document::reread);
            }
        }
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

/// The edits that turn `old` into `new`, one for each run of lines that differ. Past the
/// time allowed, the lines left are taken as all different.
fn line_edits(old: &str, new: &str) -> Edits {
    let diff = TextDiff::configure()
        .timeout(DIFF_TIMEOUT)
        .diff_lines(old, new);
    // Where each of `old`'s lines starts, and past the last one where the text ends.
    let starts: Vec<usize> = std::iter::once(0)
        .chain(diff.old_slices().iter().scan(0, |at, line| {
            *at += line.len();
            Some(*at)
        }))
        .collect();
    (diff.ops().iter())
        .map(similar::DiffOp::as_tag_tuple)
        .filter(|(tag, ..)| *tag != DiffTag::Equal)
        .map(|(_, old, new)| {
            let text = diff.new_slices()[new].concat();
            (starts[old.start]..starts[old.end], text)
        })
        .collect()
}
