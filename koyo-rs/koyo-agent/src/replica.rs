//! One open thread as the host last said it (`docs/chat.md` 5.1): the rows of its snapshot, with
//! each change and append after it merged in. The host sends rows whole, so a row replaces the
//! one it's a newer version of, and only text grows piece by piece.

use std::cmp::Ordering;

use crate::protocol::events::{Appended, Changed};
use crate::protocol::methods::{Rows, ThreadSnapshot};
use crate::protocol::model::{Item, Run, Thread};
use crate::protocol::requests::Request;

#[derive(Clone, Debug, PartialEq)]
pub struct Replica {
    pub thread: Thread,
    /// The runs held, in the order they were sent; older ones come with history.
    pub runs: Vec<Run>,
    /// The timeline's entries held, in their order.
    pub items: Vec<Item>,
    pub requests: Vec<Request>,
    seq: u64,
}

impl Replica {
    /// The thread as the snapshot of a first open has it.
    pub fn new(snapshot: ThreadSnapshot) -> Replica {
        let seq = snapshot.thread.seq;
        let mut replica = Replica {
            thread: snapshot.thread,
            runs: Vec::new(),
            items: Vec::new(),
            requests: Vec::new(),
            seq,
        };
        replica.merge(snapshot.runs, snapshot.items, snapshot.requests);
        replica
    }

    /// What a reopen passes as its `sinceSeq`: the last change this holds.
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// Takes in the snapshot of a reopen: the rows changed since [`Self::seq`], and every
    /// streaming item whole.
    pub fn catch_up(&mut self, snapshot: ThreadSnapshot) {
        self.seq = self.seq.max(snapshot.thread.seq);
        self.thread = snapshot.thread;
        self.merge(snapshot.runs, snapshot.items, snapshot.requests);
    }

    /// Takes in one step of the thread's actor.
    pub fn apply(&mut self, changed: Changed) {
        self.seq = self.seq.max(changed.seq);
        if let Some(thread) = changed.thread {
            self.thread = thread;
        }
        self.merge(changed.runs, changed.items, changed.requests);
    }

    /// Grows the streaming item it's for; false when this doesn't hold one, as the change that
    /// made it comes first.
    pub fn append(&mut self, appended: &Appended) -> bool {
        // What streams is at the end of the timeline, or close to it.
        let item = (self.items.iter_mut().rev()).find(|item| item.id == appended.item_id);
        match item.and_then(|item| item.body.text_mut()) {
            Some(text) => {
                text.push_str(&appended.text);
                true
            }
            None => false,
        }
    }

    /// Takes in a page of older runs.
    pub fn prepend(&mut self, rows: Rows) {
        self.merge(rows.runs, rows.items, rows.requests);
    }

    fn merge(&mut self, runs: Vec<Run>, items: Vec<Item>, requests: Vec<Request>) {
        merge_rows(&mut self.runs, runs, |a, b| a.id.cmp(&b.id), |run| run.seq);
        merge_rows(&mut self.items, items, |a, b| a.ordinal.cmp(&b.ordinal), |item| item.seq);
        merge_rows(&mut self.requests, requests, |a, b| a.id.cmp(&b.id), |request| request.seq);
    }
}

/// Puts each of `new` in its place in `rows`, in the place of the version it replaces unless
/// that's the newer. At the same `seq` the one that came later wins: a streaming item's text
/// grows without a change number (`docs/chat.md` 4.5.8).
fn merge_rows<T>(
    rows: &mut Vec<T>,
    new: Vec<T>,
    order: impl Fn(&T, &T) -> Ordering,
    seq: impl Fn(&T) -> u64,
) {
    // A page of history goes before everything held, in one move rather than one per row.
    let sorted = new.is_sorted_by(|a, b| order(a, b).is_lt());
    if sorted
        && let (Some(last), Some(first)) = (new.last(), rows.first())
        && order(last, first).is_lt()
    {
        rows.splice(0..0, new);
        return;
    }
    for row in new {
        match rows.binary_search_by(|kept| order(kept, &row)) {
            Ok(at) => {
                if let Some(kept) = rows.get_mut(at)
                    && seq(kept) <= seq(&row)
                {
                    *kept = row;
                }
            }
            Err(at) => rows.insert(at, row),
        }
    }
}
