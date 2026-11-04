//! One open chat: the rows the host last said, what arrived before the snapshot they start from,
//! and a markdown view per text a row has drawn, which a token delta grows on its own.

use std::collections::HashMap;

use gpui_kit::component::text::TextViewState;
use gpui_kit::{AppContext as _, Context, Entity, SharedString};
use koyo_agent::client::ClientError;
use koyo_agent::protocol::events::{Appended, Changed};
use koyo_agent::protocol::ids::{ItemId, ThreadId};
use koyo_agent::protocol::methods::{Rows, ThreadSnapshot};
use koyo_agent::protocol::model::Item;
use koyo_agent::protocol::wire::ErrorCode;
use koyo_agent::replica::Replica;

/// What the host said of an open thread.
#[derive(Debug)]
pub enum ThreadEvent {
    Changed(Box<Changed>),
    Appended(Appended),
}

pub struct ChatThread {
    id: ThreadId,
    /// The rows, once a snapshot came.
    replica: Option<Replica>,
    /// An open was asked for and isn't answered yet.
    opening: bool,
    /// What came meanwhile. The host wrote it after the snapshot (`docs/chat.md` 4.3.4); only
    /// the roads to this entity differ.
    early: Vec<ThreadEvent>,
    /// Why it can't be shown.
    failed: Option<SharedString>,
    /// Each text a row has drawn.
    texts: HashMap<ItemId, Entity<TextViewState>>,
}

impl ChatThread {
    pub fn new(id: ThreadId) -> Self {
        ChatThread {
            id,
            replica: None,
            opening: false,
            early: Vec::new(),
            failed: None,
            texts: HashMap::new(),
        }
    }

    pub fn id(&self) -> &ThreadId {
        &self.id
    }

    pub fn replica(&self) -> Option<&Replica> {
        self.replica.as_ref()
    }

    pub fn failed(&self) -> Option<&SharedString> {
        self.failed.as_ref()
    }

    /// An open is about to be asked for: what the old connection left is no use, and this is
    /// what to pass as its `sinceSeq`.
    pub fn opening(&mut self) -> Option<u64> {
        self.opening = true;
        self.early.clear();
        self.replica.as_ref().map(Replica::seq)
    }

    /// Takes in the answer to an open.
    pub fn opened(
        &mut self,
        opened: Result<ThreadSnapshot, ClientError>,
        cx: &mut Context<Self>,
    ) {
        self.opening = false;
        let early = std::mem::take(&mut self.early);
        match opened {
            Ok(snapshot) => {
                set_texts(&self.texts, &snapshot.items, cx);
                match &mut self.replica {
                    Some(replica) => replica.catch_up(snapshot),
                    None => self.replica = Some(Replica::new(snapshot)),
                }
                self.failed = None;
                self.take_in(early, cx);
            }
            // The connection went: it opens again when the connection's back.
            Err(ClientError::Closed | ClientError::Io(_)) => {}
            Err(ClientError::Host(error)) if error.code == ErrorCode::ThreadNotFound => {
                self.failed = Some("This chat is gone.".into());
            }
            Err(error) => {
                log::warn!("opening chat {}: {error}", self.id);
                self.failed = Some(error.to_string().into());
            }
        }
        cx.notify();
    }

    /// Applies what the host said since, the entity notified once if the rows changed: an
    /// append only notifies its text.
    pub fn take_in(&mut self, events: Vec<ThreadEvent>, cx: &mut Context<Self>) {
        if self.opening {
            self.early.extend(events);
            return;
        }
        // An open that failed left nothing for these to go on; the next one brings them.
        let Some(replica) = &mut self.replica else {
            return;
        };
        let mut structure = false;
        for event in together(events) {
            match event {
                ThreadEvent::Changed(changed) => {
                    set_texts(&self.texts, &changed.items, cx);
                    replica.apply(*changed);
                    structure = true;
                }
                ThreadEvent::Appended(appended) => {
                    if !replica.append(&appended) {
                        log::debug!("an append to an item {} doesn't hold", self.id);
                    } else if let Some(text) = self.texts.get(&appended.item_id) {
                        text.update(cx, |text, cx| text.push_str(&appended.text, cx));
                    }
                }
            }
        }
        if structure {
            cx.notify();
        }
    }

    /// Takes in a page of older runs.
    pub fn prepend(&mut self, rows: Rows, cx: &mut Context<Self>) {
        if let Some(replica) = &mut self.replica {
            replica.prepend(rows);
            cx.notify();
        }
    }

    /// The markdown view of the text item `id`, if a row has drawn it.
    pub fn drawn_text(&self, id: &ItemId) -> Option<&Entity<TextViewState>> {
        self.texts.get(id)
    }

    /// The markdown view of the text item `id` holds, `text` when it's made now.
    pub fn text(
        &mut self,
        id: &ItemId,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Entity<TextViewState> {
        self.texts
            .entry(id.clone())
            .or_insert_with(|| cx.new(|cx| TextViewState::markdown(text, cx)))
            .clone()
    }
}

/// The texts already drawn of `items` become what the host says they are, whole.
fn set_texts(
    texts: &HashMap<ItemId, Entity<TextViewState>>,
    items: &[Item],
    cx: &mut Context<ChatThread>,
) {
    for item in items {
        if let (Some(new), Some(text)) = (item.body.text(), texts.get(&item.id)) {
            text.update(cx, |text, cx| text.set_text(new, cx));
        }
    }
}

/// `events` with appends in a row to one item made one, as each append starts a parse.
fn together(events: Vec<ThreadEvent>) -> Vec<ThreadEvent> {
    let mut joined: Vec<ThreadEvent> = Vec::with_capacity(events.len());
    for event in events {
        match (joined.last_mut(), event) {
            (Some(ThreadEvent::Appended(last)), ThreadEvent::Appended(next))
                if last.item_id == next.item_id =>
            {
                last.text.push_str(&next.text);
            }
            (_, event) => joined.push(event),
        }
    }
    joined
}
