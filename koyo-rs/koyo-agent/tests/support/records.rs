//! The records the replica's and the timeline's tests build their threads from.

use koyo_agent::protocol::ids::{ItemId, MessageId, RunId, ThreadId, Time};
use koyo_agent::protocol::methods::ThreadSnapshot;
use koyo_agent::protocol::model::{
    Delivered, Item, ItemBody, ItemStatus, Prompt, Run, RunStatus, Text, Thread, TitleSource,
    UserMessage,
};

pub const THREAD: &str = "t1";
const AT: &str = "2026-11-03T10:00:00.000Z";

pub fn thread(seq: u64) -> Thread {
    Thread {
        id: ThreadId(THREAD.into()),
        cwd: "/work/koyo".into(),
        title: format!("At {seq}"),
        title_source: TitleSource::Seed,
        provider: "fake".into(),
        model: None,
        effort: None,
        fast: false,
        permission: None,
        plan: false,
        session_id: None,
        queue_held: false,
        tab_open: true,
        created_at: Time(AT.into()),
        updated_at: Time(AT.into()),
        read_at: None,
        seq,
    }
}

pub fn run(id: &str, status: RunStatus, seq: u64) -> Run {
    Run {
        id: RunId(id.into()),
        thread_id: ThreadId(THREAD.into()),
        status,
        prompt: Prompt {
            message_id: MessageId(format!("m-{id}")),
            text: "Why?".into(),
            attachments: Vec::new(),
        },
        model: None,
        effort: None,
        permission: None,
        plan: false,
        requested_at: Time(AT.into()),
        started_at: None,
        ended_at: None,
        stop: None,
        failure: None,
        usage: None,
        files: Vec::new(),
        seq,
    }
}

pub fn item(id: &str, ordinal: u64, body: ItemBody, seq: u64) -> Item {
    Item {
        id: ItemId(id.into()),
        thread_id: ThreadId(THREAD.into()),
        run_id: RunId("r1".into()),
        ordinal,
        parent_id: None,
        status: ItemStatus::Running,
        seq,
        body,
    }
}

pub fn answer(text: &str) -> ItemBody {
    ItemBody::AssistantText(Text { text: text.into() })
}

pub fn message(text: &str) -> ItemBody {
    ItemBody::UserMessage(UserMessage {
        message_id: MessageId("m".into()),
        text: text.into(),
        attachments: Vec::new(),
        delivered: Delivered::Turn,
    })
}

pub fn snapshot(seq: u64, runs: Vec<Run>, items: Vec<Item>) -> ThreadSnapshot {
    ThreadSnapshot {
        thread: thread(seq),
        runs,
        items,
        requests: Vec::new(),
    }
}

