//! What's being written in each chat and isn't sent yet (`docs/chat.md` 5.2): a draft for each
//! thread, and one for the new chat of each folder, there again when the chat next opens, after a
//! restart too. They're kept in a file in the host's folder, where it looks to see which attachments
//! they name, written a moment after the last change and on the way out. A composer takes up its
//! chat's draft to write to and is the only one that does: two couldn't both own the files it
//! attaches, which the draft keeps until they're sent or taken away.

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::{App, Global, Task};
use koyo_agent::client;
use koyo_agent::protocol::ids::ThreadId;
use koyo_agent::protocol::model::Attachment;
use serde::{Deserialize, Serialize};

use super::attachments;
use crate::settings;

const SAVE_DELAY: Duration = Duration::from_millis(300);
const FILE: &str = "chat-drafts.json";

/// What's been written and attached, and where the caret is in it, in bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Draft {
    pub(super) text: String,
    pub(super) cursor: usize,
    pub(super) attachments: Vec<Attachment>,
}

/// The chat a draft is for.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum DraftOf {
    Thread(ThreadId),
    /// A chat with no thread yet, in this folder.
    NewChat(PathBuf),
}

struct Drafts {
    /// None of them empty.
    kept: HashMap<DraftOf, Draft>,
    /// Where they're saved, if the app has a folder to save them in.
    path: Option<PathBuf>,
    /// Where what they attach is kept.
    attachments: Option<PathBuf>,
    /// The save waiting for changes to settle; replacing it cancels it.
    pending: Option<Task<()>>,
    /// The chats whose draft a composer has up.
    held: HashSet<DraftOf>,
}

impl Global for Drafts {}

pub(super) fn init(cx: &mut App) {
    // Tests read and write none of a person's.
    let data = settings::data_dir().filter(|_| !cfg!(test));
    open(data.map(|data| client::host_dir(&data).root().join(FILE)), attachments::folder(), cx);
    // A change still settling when the app quits is written on the way out.
    cx.on_app_quit(|cx| {
        let drafts = cx.global_mut::<Drafts>();
        if drafts.pending.take().is_some()
            && let Some(path) = &drafts.path
        {
            write_file(path, &drafts.kept);
        }
        async {}
    })
    .detach();
}

fn open(path: Option<PathBuf>, attachments: Option<PathBuf>, cx: &mut App) {
    cx.set_global(Drafts {
        kept: path.as_deref().map(read_file).unwrap_or_default(),
        path,
        attachments,
        pending: None,
        held: HashSet::new(),
    });
}

/// What's in the file, or nothing if it can't be read: a draft isn't worth stopping for.
fn read_file(path: &Path) -> HashMap<DraftOf, Draft> {
    let kept = std::fs::read(path).and_then(|json| {
        let listed: Vec<(DraftOf, Draft)> = serde_json::from_slice(&json)?;
        Ok(listed.into_iter().collect())
    });
    kept.unwrap_or_else(|error: io::Error| {
        if error.kind() != io::ErrorKind::NotFound {
            log::warn!("couldn't read {}: {error}", path.display());
        }
        HashMap::new()
    })
}

/// JSON can't have a chat as a key, so it's a list.
fn write_file(path: &Path, kept: &HashMap<DraftOf, Draft>) {
    let listed: Vec<(&DraftOf, &Draft)> = kept.iter().collect();
    match serde_json::to_vec_pretty(&listed) {
        Ok(json) => settings::write(path, &json),
        Err(error) => log::error!("couldn't encode the drafts: {error}"),
    }
}

/// Where what's attached to a message being written is kept.
pub(super) fn attachments_folder(cx: &App) -> Option<PathBuf> {
    cx.global::<Drafts>().attachments.clone()
}

/// What `of`'s draft attaches.
pub(super) fn attachments_of<'a>(of: &DraftOf, cx: &'a App) -> &'a [Attachment] {
    cx.global::<Drafts>().kept.get(of).map_or(&[], |draft| &draft.attachments)
}

/// Takes up `of`'s draft to write to, and says what's in it; `None` if another composer has it.
pub(super) fn take_up(of: &DraftOf, cx: &mut App) -> Option<Draft> {
    let drafts = cx.global_mut::<Drafts>();
    drafts.held.insert(of.clone()).then(|| drafts.kept.get(of).cloned().unwrap_or_default())
}

/// Stops writing to `of`'s draft, which stays as it is.
pub(super) fn put_down(of: &DraftOf, cx: &mut App) {
    cx.global_mut::<Drafts>().held.remove(of);
}

/// Stops writing to `of`'s draft, which is no more, what it attaches having gone elsewhere.
pub(super) fn give_up(of: &DraftOf, cx: &mut App) {
    write(of, Draft::default(), cx);
    put_down(of, cx);
}

/// `of`'s draft is now `draft`, if it's still up.
pub(super) fn write(of: &DraftOf, draft: Draft, cx: &mut App) {
    let drafts = cx.global_mut::<Drafts>();
    if !drafts.held.contains(of) {
        return;
    }
    let changed = if draft.text.is_empty() && draft.attachments.is_empty() {
        drafts.kept.remove(of).is_some()
    } else if drafts.kept.get(of) == Some(&draft) {
        false
    } else {
        drafts.kept.insert(of.clone(), draft);
        true
    };
    if changed {
        save(cx);
    }
}

/// A deleted thread takes its draft with it, and the files only that names.
pub(super) fn thread_removed(id: &ThreadId, cx: &mut App) {
    let of = DraftOf::Thread(id.clone());
    let drafts = cx.global_mut::<Drafts>();
    drafts.held.remove(&of);
    let Some(draft) = drafts.kept.remove(&of) else {
        return;
    };
    attachments::forget(drafts.attachments.clone(), draft.attachments, cx);
    save(cx);
}

fn save(cx: &mut App) {
    let save = cx.spawn(async move |cx| {
        cx.background_executor().timer(SAVE_DELAY).await;
        let now = cx.update(|cx| {
            let drafts = cx.global::<Drafts>();
            Some((drafts.path.clone()?, drafts.kept.clone()))
        });
        if let Some((path, kept)) = now {
            cx.background_executor().spawn(async move { write_file(&path, &kept) }).await;
        }
    });
    cx.global_mut::<Drafts>().pending = Some(save);
}

/// What `of`'s draft holds now.
#[cfg(test)]
pub(super) fn kept(of: &DraftOf, cx: &App) -> Option<Draft> {
    cx.global::<Drafts>().kept.get(of).cloned()
}

/// From now on they're kept at `path`, as if the app had just started.
#[cfg(test)]
pub(super) fn open_at(path: &Path, cx: &mut App) {
    open(Some(path.to_owned()), None, cx);
}

/// From now on what they attach is kept in `folder`.
#[cfg(test)]
pub(super) fn attach_into(folder: &Path, cx: &mut App) {
    cx.global_mut::<Drafts>().attachments = Some(folder.to_owned());
}
