//! How a row says what it shows, with no drawing.

#[expect(dead_code, reason = "each test crate builds what it needs of these")]
#[path = "support/records.rs"]
mod records;

use std::collections::BTreeMap;

use koyo_agent::protocol::ids::{MessageId, RunId, Time};
use koyo_agent::protocol::methods::Model;
use koyo_agent::protocol::model::{
    Compaction, CompactionTrigger, Effort, Item, ItemBody, ItemStatus, Run, RunStatus, ToolFamily,
};
use koyo_agent::protocol::requests::{
    Answer, Approval, Decision, PlanDecision, Question, QuestionOption, Questions, Request,
    RequestPayload, RequestStatus, Response,
};
use koyo_agent::words::{
    answer_meta, approval_line, compacted, context_line, duration, live_line, plan_outcome,
    plan_parts, question_lines, tool_line, work_line, work_summary, worked_for,
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
    assert_eq!(said(&[&read, &thought]), "Thought and read a file", "a thought isn't hidden");
    assert_eq!(said(&[&thought, &read, &ran]), "Thought, read a file and ran a command");
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
    assert_eq!(answer_meta(&settled, &[]), "claude-opus · Extra high", "it isn't listed");
    let listed = Model {
        id: "claude-opus".into(),
        name: "Opus 5.5 (new)".into(),
        description: String::new(),
        efforts: Vec::new(),
        fast: false,
        auto: false,
    };
    assert_eq!(answer_meta(&settled, &[listed]), "Opus 5.5 · Extra high");
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
        payload: RequestPayload::Question(Questions {
            questions: Vec::new(),
            live: true,
        }),
        ..request("q1", RequestStatus::Answered)
    };
    let elsewhere = Request {
        run_id: RunId("r2".into()),
        ..asked.clone()
    };

    assert_eq!(worked_for(&done, &[]).as_deref(), Some("50s"));
    assert_eq!(worked_for(&done, &[asked.clone(), elsewhere]).as_deref(), Some("14s"));

    let left_at_a_restart = Request {
        created_at: at(40),
        resolved_at: Some(Time("2026-11-03T10:30:00.000Z".into())),
        ..asked
    };
    assert_eq!(worked_for(&done, &[left_at_a_restart]).as_deref(), Some("40s"));

    let plan = request("p", RequestStatus::Answered);
    let plan = Request {
        created_at: at(40),
        resolved_at: Some(Time("2026-11-03T10:30:00.000Z".into())),
        ..plan
    };
    assert_eq!(worked_for(&done, &[plan]).as_deref(), Some("50s"), "nothing waited on it");
}

fn indentation() -> Question {
    Question {
        id: "Tabs or spaces?".into(),
        header: "Indentation".into(),
        text: "Tabs or spaces?".into(),
        multi_select: false,
        options: vec![QuestionOption {
            label: "Tabs".into(),
            description: None,
            preview: None,
        }],
    }
}

fn answered(answer: Answer) -> Request {
    Request {
        answer: Some(answer),
        ..request("q", RequestStatus::Answered)
    }
}

#[test]
fn a_question_says_what_was_asked_and_then_what_the_person_said() {
    let asked = [indentation()];
    let lines = |request: &Request| question_lines(request, &asked);
    let chose = |choices: &[&str]| {
        let answers: BTreeMap<String, Vec<String>> = BTreeMap::from([(
            "Tabs or spaces?".to_owned(),
            choices.iter().map(|choice| (*choice).to_owned()).collect(),
        )]);
        Answer::Response(Response::Question { answers })
    };

    assert_eq!(lines(&request("q", RequestStatus::Pending)), ["Asked: Tabs or spaces?"]);
    assert_eq!(lines(&answered(chose(&["Tabs"]))), ["Indentation: Tabs"]);
    assert_eq!(lines(&answered(chose(&["Tabs", "Both"]))), ["Indentation: Tabs, Both"]);
    let skipped = Answer::Response(Response::Question { answers: BTreeMap::new() });
    assert_eq!(lines(&answered(skipped)), ["Skipped: Tabs or spaces?"]);
    let by_message = Answer::Message { message_id: MessageId("m".into()) };
    assert_eq!(lines(&answered(by_message)), ["Answered by your message: Tabs or spaces?"]);
    assert_eq!(lines(&request("q", RequestStatus::Expired)), ["No longer asked: Tabs or spaces?"]);
}

#[test]
fn a_plan_is_titled_by_its_first_heading() {
    assert_eq!(plan_parts("# Ship it\n\n1. Build\n"), (Some("Ship it"), "1. Build\n"));
    assert_eq!(plan_parts("\n## Ship it"), (Some("Ship it"), ""));
    assert_eq!(
        plan_parts("First this.\n\n# Ship it\nThen."),
        (Some("Ship it"), "First this.\n\n# Ship it\nThen."),
        "there's more above it to read",
    );
    assert_eq!(plan_parts("#hashtag\n1. Build"), (None, "#hashtag\n1. Build"));
}

#[test]
fn a_plan_says_what_became_of_it() {
    let plan = |decision| Answer::Response(Response::Plan { decision });
    assert_eq!(plan_outcome(&request("p", RequestStatus::Pending)), None);
    assert_eq!(plan_outcome(&answered(plan(PlanDecision::Implement))), Some("Implemented"));
    assert_eq!(plan_outcome(&answered(plan(PlanDecision::Dismiss))), Some("Dismissed"));
    let by_message = Answer::Message { message_id: MessageId("m".into()) };
    assert_eq!(plan_outcome(&answered(by_message)), Some("Refined by your message"));
}

#[test]
fn how_full_a_conversation_is_is_said_in_a_line() {
    assert_eq!(context_line(84_000, 200_000), "42 % · 84k of 200k");
    assert_eq!(context_line(10, 0), "0 % · 10 of 0", "a model that says it holds nothing");
}
