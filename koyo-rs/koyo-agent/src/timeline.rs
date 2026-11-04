//! The chat's timeline as rows (`docs/chat.md` 5.3): what a thread's runs, items and requests
//! show, in order. A row names the records it shows by their places in the replica, so it only
//! holds for the replica it was made from, and it draws nothing. Pure, so each rule is a test.

use std::collections::HashSet;
use std::ops::Range;

use crate::protocol::ids::{ItemId, RequestId, RunId};
use crate::protocol::model::{Item, ItemBody, ItemStatus, NoticeLevel, Run, RunStatus};
use crate::protocol::requests::RequestStatus;
use crate::replica::Replica;

/// What a row goes by for as long as it shows: what's kept per row is kept by this.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RowId {
    /// The one item it shows.
    Item(ItemId),
    /// A group of work, by the item it started with.
    Work(ItemId),
    /// Subagents one after the other, by the first.
    Subagents(ItemId),
    Fold(RunId),
    Live(RunId),
    Failure(RunId),
    Stopped(RunId),
    Files(RunId),
    Queued(RunId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: RowId,
    /// The latest change to what it shows: a row of the same id and `seq` has the same height,
    /// bar the text that grows on its own.
    pub seq: u64,
    pub kind: RowKind,
}

/// What a row shows, by places in the replica's `items`, `runs` and `requests`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowKind {
    User { item: usize, run: usize },
    /// Assistant text; `last` when it's the run's final answer.
    Answer { item: usize, run: usize, last: bool },
    /// Tools, thinking and to-dos one after the other, none of them running.
    Work { items: Vec<usize> },
    /// Subagents one after the other, each showing how it's doing whatever its run does.
    Subagents { items: Vec<usize> },
    /// What a live run is working on, `None` before there's anything to say. Its id never changes
    /// while the run works. There's none while the run writes or asks, which show themselves.
    Live { item: Option<usize> },
    /// A settled run's work, which shows while this row is open.
    Fold { run: usize },
    /// `asking` when it's the one a person can answer now, of those waiting.
    Request {
        item: usize,
        request: usize,
        asking: bool,
    },
    Compaction { item: usize },
    /// A note in the timeline, or an item of a kind this build doesn't know.
    Notice { item: usize },
    Failure { run: usize },
    Stopped { run: usize },
    Files { run: usize },
    Queued { run: usize },
}

/// How a settled run's work shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkFold {
    /// Out in the open, as a live run's is.
    None,
    /// Behind a line that says so.
    Closed,
    /// Under that line, as a person opened it.
    Open,
}

/// The rows of `replica`, with each settled run's work as `fold` says.
pub fn rows(replica: &Replica, fold: impl Fn(&RunId) -> WorkFold) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut seen = HashSet::new();
    let mut first = 0;
    let asking = replica.asking().map(|asked| &asked.id);
    for items in replica.items.chunk_by(|a, b| a.run_id == b.run_id) {
        if let Some((index, run)) = items.first().and_then(|item| run_of(replica, item)) {
            seen.insert(index);
            let mut made = RunRows {
                replica,
                rows: &mut rows,
                index,
                run,
                first,
                asking,
                work: None,
            };
            made.add(items, fold(&run.id));
        }
        first += items.len();
    }
    for (index, run) in replica.runs.iter().enumerate() {
        if run.status.is_live() && !seen.contains(&index) {
            rows.push(live_row(run, None));
        }
    }
    for (index, run) in replica.runs.iter().enumerate() {
        if run.status == RunStatus::Queued {
            rows.push(Row {
                id: RowId::Queued(run.id.clone()),
                seq: run.seq,
                kind: RowKind::Queued { run: index },
            });
        }
    }
    rows
}

/// The run `item` is of, and its place.
fn run_of<'a>(replica: &'a Replica, item: &Item) -> Option<(usize, &'a Run)> {
    let index = (replica.runs).binary_search_by(|run| run.id.cmp(&item.run_id)).ok()?;
    Some((index, replica.runs.get(index)?))
}

fn live_row(run: &Run, item: Option<usize>) -> Row {
    Row {
        id: RowId::Live(run.id.clone()),
        // One line, whatever it says.
        seq: 0,
        kind: RowKind::Live { item },
    }
}

/// The rows one run's items make.
struct RunRows<'a> {
    replica: &'a Replica,
    rows: &'a mut Vec<Row>,
    index: usize,
    run: &'a Run,
    /// The place in the replica's items of the first of this run's.
    first: usize,
    /// The request a person can answer now.
    asking: Option<&'a RequestId>,
    /// The work being gathered.
    work: Option<Work>,
}

/// Work one item after the other: the item it starts with, whichever state that's in, the places
/// of what's done of it, and the latest change to those.
struct Work {
    start: ItemId,
    items: Vec<usize>,
    seq: u64,
}

impl RunRows<'_> {
    fn add(&mut self, items: &[Item], fold: WorkFold) {
        let answer = items.iter().rfind(|item| is_text(item)).map(|item| &item.id);
        // Failed and interrupted runs show what they did, as that's what someone reads them for.
        let fold = if self.run.status == RunStatus::Completed { fold } else { WorkFold::None };
        let mut foldable = fold != WorkFold::None;
        for (at, item) in items.iter().enumerate() {
            let place = self.first + at;
            if is_work(item) {
                if foldable {
                    let fold = RowKind::Fold { run: self.index };
                    self.push_run(RowId::Fold(self.run.id.clone()), fold);
                    foldable = false;
                }
                if fold == WorkFold::Closed {
                    continue;
                }
            }
            self.item(place, item, answer == Some(&item.id));
        }
        self.flush();
        self.end(items);
    }

    fn item(&mut self, place: usize, item: &Item, last: bool) {
        let mut seq = item.seq;
        let kind = match &item.body {
            ItemBody::Tool(_) | ItemBody::Todos(_) | ItemBody::Reasoning(_) => {
                self.gather(place, item);
                return;
            }
            ItemBody::Subagent(_) => {
                self.flush();
                self.subagent(place, item);
                return;
            }
            ItemBody::UserMessage(_) => RowKind::User {
                item: place,
                run: self.index,
            },
            ItemBody::AssistantText(_) => RowKind::Answer {
                item: place,
                run: self.index,
                last,
            },
            ItemBody::Request(asked) => {
                let requests = &self.replica.requests;
                let found = requests.binary_search_by(|request| request.id.cmp(&asked.request_id));
                let Some(request) = found.ok() else {
                    return;
                };
                seq = seq.max(requests.get(request).map_or(0, |request| request.seq));
                RowKind::Request {
                    item: place,
                    request,
                    asking: self.asking == Some(&asked.request_id),
                }
            }
            ItemBody::Compaction(_) => RowKind::Compaction { item: place },
            ItemBody::Notice(_) | ItemBody::Unknown { .. } => RowKind::Notice { item: place },
        };
        self.flush();
        self.rows.push(Row {
            id: RowId::Item(item.id.clone()),
            seq,
            kind,
        });
    }

    /// A row for a subagent, or a place in the row of the one before it.
    fn subagent(&mut self, place: usize, item: &Item) {
        if let Some(Row {
            seq,
            kind: RowKind::Subagents { items },
            ..
        }) = self.rows.last_mut()
        {
            items.push(place);
            *seq = (*seq).max(item.seq);
        } else {
            self.rows.push(Row {
                id: RowId::Subagents(item.id.clone()),
                seq: item.seq,
                kind: RowKind::Subagents { items: vec![place] },
            });
        }
    }

    /// Adds an item to the work under way, which what's running stays out of while the run's
    /// live: it's what the live row shows.
    fn gather(&mut self, place: usize, item: &Item) {
        let work = self.work.get_or_insert_with(|| Work {
            start: item.id.clone(),
            items: Vec::new(),
            seq: 0,
        });
        if !(self.run.status.is_live() && item.status == ItemStatus::Running) {
            work.items.push(place);
            work.seq = work.seq.max(item.seq);
        }
    }

    fn flush(&mut self) {
        if let Some(work) = self.work.take()
            && !work.items.is_empty()
        {
            self.rows.push(Row {
                id: RowId::Work(work.start),
                seq: work.seq,
                kind: RowKind::Work { items: work.items },
            });
        }
    }

    fn push_run(&mut self, id: RowId, kind: RowKind) {
        self.rows.push(Row {
            id,
            seq: self.run.seq,
            kind,
        });
    }

    /// What comes after a run's items: what it's doing, or how it ended.
    fn end(&mut self, items: &[Item]) {
        let (id, index) = (&self.run.id, self.index);
        if self.run.status.is_live() {
            self.live(items);
            return;
        }
        if !self.run.files.is_empty() {
            self.push_run(RowId::Files(id.clone()), RowKind::Files { run: index });
        }
        match self.run.status {
            RunStatus::Failed => self.push_run(RowId::Failure(id.clone()), RowKind::Failure { run: index }),
            RunStatus::Interrupted => {
                self.push_run(RowId::Stopped(id.clone()), RowKind::Stopped { run: index });
            }
            _ => {}
        }
    }

    /// The live row, unless what the run is doing shows for itself: its answer, or its request.
    fn live(&mut self, items: &[Item]) {
        let asking = (self.replica.requests.iter())
            .any(|request| request.run_id == self.run.id && request.status == RequestStatus::Pending);
        if asking {
            return;
        }
        // A subagent shows for itself, however long it takes.
        let running = (items.iter().enumerate().rev())
            .filter(|(_, item)| !matches!(item.body, ItemBody::Subagent(_)))
            .find(|(_, item)| item.status == ItemStatus::Running);
        let item = match running {
            Some((_, item)) if is_text(item) => return,
            Some((at, _)) => Some(self.first + at),
            None => None,
        };
        self.rows.push(live_row(self.run, item));
    }
}

fn is_text(item: &Item) -> bool {
    matches!(item.body, ItemBody::AssistantText(_))
}

/// Whether an item is the work a settled run folds. What was said stays, however much came after
/// it, and so do asks, subagents, compactions and what went wrong.
fn is_work(item: &Item) -> bool {
    match &item.body {
        ItemBody::Reasoning(_) | ItemBody::Tool(_) | ItemBody::Todos(_) => true,
        ItemBody::Notice(notice) => notice.level == NoticeLevel::Info,
        ItemBody::UserMessage(_)
        | ItemBody::AssistantText(_)
        | ItemBody::Request(_)
        | ItemBody::Subagent(_)
        | ItemBody::Compaction(_)
        | ItemBody::Unknown { .. } => false,
    }
}

/// What to tell a list holding the rows `old` for it to hold `new`: which of its rows to replace
/// with how many. Rows above and below it keep their places and heights, however their places in
/// the replica moved.
pub fn changed(old: &[Row], new: &[Row]) -> Option<(Range<usize>, usize)> {
    // A row follows its records, an answer whether it's still the last, and a request whether
    // it's the one asked.
    let told = |row: &Row| match row.kind {
        RowKind::Answer { last, .. } => last,
        RowKind::Request { asking, .. } => asking,
        _ => false,
    };
    let same = |a: &Row, b: &Row| a.id == b.id && a.seq == b.seq && told(a) == told(b);
    let before = (old.iter().zip(new)).take_while(|(a, b)| same(a, b)).count();
    if before == old.len() && before == new.len() {
        return None;
    }
    let after = (old[before..].iter().rev().zip(new[before..].iter().rev()))
        .take_while(|(a, b)| same(a, b))
        .count();
    Some((before..old.len() - after, new.len() - before - after))
}
