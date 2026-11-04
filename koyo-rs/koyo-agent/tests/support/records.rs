//! The records the tests of what's made of a thread build it from.

use koyo_agent::protocol::ids::{ItemId, MessageId, RequestId, RunId, ThreadId, Time};
use koyo_agent::protocol::methods::ThreadSnapshot;
use koyo_agent::protocol::model::{
    Delivered, Item, ItemBody, ItemStatus, Prompt, Run, RunStatus, Subagent, Text, Thread,
    TitleSource, Tool, ToolFamily, UserMessage,
};
use koyo_agent::protocol::requests::{Request, RequestPayload, RequestStatus};
use serde_json::json;

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
        parent: None,
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

pub fn tool_of(family: ToolFamily, label: &str) -> Tool {
    Tool {
        name: "Tool".into(),
        family,
        label: label.into(),
        input: json!({}),
        output: None,
        exit_code: None,
        patch: None,
        files: Vec::new(),
        server: None,
    }
}

pub fn subagent(description: &str) -> Subagent {
    Subagent {
        description: description.into(),
        agent_type: None,
        summary: None,
        thread_id: None,
        progress: None,
        usage: None,
        started_at: None,
        ended_at: None,
    }
}

pub fn reasoning(said: &str) -> ItemBody {
    ItemBody::Reasoning(Text { text: said.into() })
}

/// `body` at `ordinal` in run `r1`, done.
pub fn done(id: &str, ordinal: u64, body: ItemBody) -> Item {
    Item {
        status: ItemStatus::Completed,
        ..item(id, ordinal, body, ordinal)
    }
}

pub fn request(id: &str, status: RequestStatus) -> Request {
    Request {
        id: RequestId(id.into()),
        thread_id: ThreadId(THREAD.into()),
        run_id: RunId("r1".into()),
        status,
        payload: RequestPayload::Plan {
            markdown: "# Plan".into(),
        },
        answer: None,
        created_at: Time("2026-11-03T10:00:00.000Z".into()),
        resolved_at: None,
        seq: 3,
    }
}
