//! How a row says what it shows, with no drawing.

#[expect(dead_code, reason = "each test crate builds what it needs of these")]
#[path = "support/records.rs"]
mod records;

use koyo_agent::protocol::ids::{RunId, Time};
use koyo_agent::protocol::model::{
    Compaction, CompactionTrigger, Effort, Item, ItemBody, ItemStatus, Run, RunStatus, ToolFamily,
};
use koyo_agent::protocol::requests::{
    Answer, Approval, Decision, Request, RequestStatus, Response,
};
use koyo_agent::words::{
    answer_meta, approval_line, compacted, duration, live_line, tool_line, work_line, work_summary,
    worked_for,
};
use records::{done, reasoning, request, run, subagent, tool_of};
use serde_json::json;

#[test]
fn work_done_together_is_said_in_a_sentence() {
    let of = |family| done("a", 1, ItemBody::Tool(tool_of(family, "a")));
    let (read, ran, web) = (of(ToolFamily::Read), of(ToolFamily::Command), of(ToolFamily::Web));
    let said = |items: &[&Item]| work_summary(items.iter().copied());
    assert_eq!(said(&[&read]), "Read a file");
    assert_eq!(said(&[&read, &read, &read, &ran, &ran]), "Read 3 files and ran 2 commands");
    assert_eq!(said(&[&ran, &read, &web, &web]), "Ran a command, read a file and searched the web");
    let agent = done("b", 1, ItemBody::Subagent(subagent("Check")));
    assert_eq!(said(&[&agent, &agent]), "Ran 2 agents");
    let thought = done("c", 1, reasoning("Hm"));
    assert_eq!(said(&[&thought, &thought]), "Thought");
    let refused = Item {
        status: ItemStatus::Declined,
        ..of(ToolFamily::Edit)
    };
    assert_eq!(said(&[&read, &refused]), "Read a file · 1 declined", "it changed nothing");
    assert_eq!(said(&[&refused, &refused]), "2 declined");
}

#[test]
fn a_tools_line_says_what_it_did_with_paths_from_the_checkout() {
    let command = tool_of(ToolFamily::Command, "cargo test");
    assert_eq!(tool_line(&command, ItemStatus::Running, "/work"), "Running cargo test");
    assert_eq!(tool_line(&command, ItemStatus::Completed, "/work"), "Ran cargo test");
    assert_eq!(tool_line(&command, ItemStatus::Declined, "/work"), "cargo test");
    let read = tool_of(ToolFamily::Read, "/work/koyo/src/main.rs");
    assert_eq!(tool_line(&read, ItemStatus::Completed, "/work/koyo/"), "Read src/main.rs");
    let search = tool_of(ToolFamily::Search, "fn main in /work/koyo/src");
    assert_eq!(
        tool_line(&search, ItemStatus::Completed, "/work/koyo"),
        "Searched for fn main in src"
    );
}

#[test]
fn a_line_of_work_says_the_latest_thought_and_the_live_row_only_that_it_thinks() {
    let thinking = done("a", 1, reasoning("First\nThen this\n\n"));
    assert_eq!(work_line(&thinking, "/work"), "Then this");
    assert_eq!(live_line(Some(&thinking), "/work"), "Thinking…");
    assert_eq!(live_line(None, "/work"), "Working…");
    let agent = done("b", 1, ItemBody::Subagent(subagent("Check the callers")));
    assert_eq!(work_line(&agent, "/work"), "Check the callers");
    assert_eq!(live_line(Some(&agent), "/work"), "Check the callers");
    let running = Item {
        status: ItemStatus::Running,
        ..done("c", 1, ItemBody::Tool(tool_of(ToolFamily::Command, "ls")))
    };
    assert_eq!(live_line(Some(&running), "/work"), "Running ls");
}

#[test]
fn times_and_tokens_are_said_as_a_person_would() {
    assert_eq!(duration(41), "41s");
    assert_eq!(duration(63), "1m 3s");
    assert_eq!(duration(7500), "2h 5m");
    let compaction = |before, after| Compaction {
        trigger: CompactionTrigger::Auto,
        before,
        after,
    };
    assert_eq!(
        compacted(&compaction(Some(182_400), Some(999))),
        "Context compacted · 182k → 999 tokens"
    );
    assert_eq!(compacted(&compaction(None, Some(999))), "Context compacted");
    let mut settled = run("r1", RunStatus::Completed, 5);
    settled.model = Some("claude-opus".into());
    settled.effort = Some(Effort::Xhigh);
    assert_eq!(answer_meta(&settled), "claude-opus · Extra high");
}

#[test]
fn a_settled_approval_says_what_was_decided_of_what() {
    let approval = |label: &str| Approval {
        tool: "Fetch".into(),
        family: ToolFamily::Other,
        title: "Do it".into(),
        label: label.into(),
        detail: String::new(),
        change: None,
        input: json!(null),
        reason: None,
        allow_session: None,
    };
    let line = |decision: Option<(Decision, Option<&str>)>, approval: &Approval| {
        let answer = decision.map(|(decision, message)| {
            Answer::Response(Response::Approval {
                decision,
                message: message.map(str::to_owned),
            })
        });
        let settled = Request {
            answer,
            ..request("q", RequestStatus::Answered)
        };
        approval_line(&settled, approval, "/work/")
    };
    let command = approval("cargo test");
    let edit = approval("/work/src/a.rs");
    assert_eq!(line(Some((Decision::Allow, None)), &command), "Allowed cargo test");
    assert_eq!(
        line(Some((Decision::AllowSession, None)), &edit),
        "Allowed src/a.rs for this chat",
    );
    assert_eq!(line(Some((Decision::Deny, None)), &command), "Denied cargo test");
    assert_eq!(
        line(Some((Decision::Deny, Some("Not that one"))), &command),
        "Denied cargo test: Not that one",
    );
    assert_eq!(line(Some((Decision::Cancel, None)), &command), "Stopped at cargo test");
    assert_eq!(line(None, &command), "No longer asked: cargo test");
}

#[test]
fn a_run_worked_for_as_long_as_it_took_less_what_it_waited_on_a_person_for() {
    let at = |seconds: u32| Time(format!("2026-11-03T10:00:{seconds:02}.000Z"));
    let done = Run {
        started_at: Some(at(0)),
        ended_at: Some(at(50)),
        ..run("r1", RunStatus::Completed, 5)
    };
    let asked = Request {
        created_at: at(10),
        resolved_at: Some(at(46)),
        ..request("q1", RequestStatus::Answered)
    };
    let elsewhere = Request {
        run_id: RunId("r2".into()),
        ..asked.clone()
    };

    assert_eq!(worked_for(&done, &[]).as_deref(), Some("50s"));
    assert_eq!(worked_for(&done, &[asked, elsewhere]).as_deref(), Some("14s"));
}
