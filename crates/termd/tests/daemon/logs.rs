//! The logs end to end (`docs/terminals.md` 5.8): what the person and an agent did, and the
//! commands the shell ran, read on by offset as the app and the MCP read them.

use koyo_term_control::log::read_since;
use koyo_term_control::{
    ActivityEntry, ActorKind, ClientKind, CommandRecord, EventKind, Handle, HumanInput, LogKind,
    RequestBody, ResponseBody, Status, TermDir,
};
use serde::de::DeserializeOwned;

use super::status::open_zsh;
use super::{StopOnDrop, connect, test_dir, wait_until};

fn entries<T: DeserializeOwned>(dir: &TermDir, handle: &Handle, kind: LogKind) -> Vec<T> {
    let read = read_since(&dir.journal(handle, kind), 0, 1 << 20).unwrap();
    (read.lines.iter())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn the_logs_keep_who_did_what_and_every_command_the_shell_ran() {
    let dir = test_dir("logs");
    let _stop = StopOnDrop(dir.clone());
    let person = connect(&dir, ClientKind::App);
    let agent = connect(&dir, ClientKind::Relay);
    let terminal = open_zsh(&person, &dir);
    let handle = terminal.handle;
    let (index, _) = person.attach(&handle, None);
    let idle = |record: &koyo_term_control::TerminalRecord| {
        record.status == Status::Idle && record.status_reason == "prompt"
    };
    let record = || {
        (person.list().into_iter())
            .find(|summary| summary.record.handle == handle)
            .unwrap()
            .record
    };
    wait_until("the prompt", || idle(&record()));

    // The person types a key at a time, as the app sends keys.
    for key in ["e", "c", "h", "o", " ", "h", "i", "\r"] {
        person.connection.input(index, key.as_bytes()).unwrap();
    }
    person.event_until(&handle, |kind| {
        matches!(
            kind,
            EventKind::Logged {
                log: LogKind::Commands,
                ..
            }
        )
    });
    // An agent claims the terminal and types a command that fails. (A paste wouldn't run
    // it: zsh takes a bracketed paste's newline as text.)
    let (agent_index, _) = agent.attach(&handle, None);
    let claim = RequestBody::Claim {
        handle: handle.clone(),
        takeover: false,
        reason: None,
    };
    agent.connection.request(claim).unwrap();
    agent.connection.input(agent_index, b"false\r").unwrap();
    wait_until("the second command", || {
        entries::<CommandRecord>(&dir, &handle, LogKind::Commands).len() == 2
    });

    let commands: Vec<CommandRecord> = entries(&dir, &handle, LogKind::Commands);
    assert_eq!(commands[0].id, 1);
    assert_eq!(commands[0].command, "echo hi");
    assert_eq!(commands[0].exit_code, Some(0));
    assert_eq!(commands[0].output, "hi");
    assert_eq!(
        commands[0].by.as_ref().map(|by| by.kind),
        Some(ActorKind::Human)
    );
    assert_eq!(commands[1].id, 2);
    assert_eq!(commands[1].command, "false");
    assert_eq!(commands[1].exit_code, Some(1));
    assert_eq!(
        commands[1].by.as_ref().map(|by| by.kind),
        Some(ActorKind::Agent)
    );
    assert!(commands[0].first_offset < commands[0].last_offset);

    // The typing is one line, then the enter key, the claim and the paste.
    wait_until("the enter key's line, after the pause", || {
        let activity: Vec<ActivityEntry> = entries(&dir, &handle, LogKind::Activity);
        activity.iter().any(|entry| entry.action == "pressed enter")
    });
    let activity: Vec<ActivityEntry> = entries(&dir, &handle, LogKind::Activity);
    let actions: Vec<&str> = activity.iter().map(|entry| entry.action.as_str()).collect();
    assert!(actions[0].starts_with("opened /bin/zsh -i"), "{actions:?}");
    let typed = activity
        .iter()
        .find(|entry| entry.action == "typed \"echo hi\"")
        .unwrap();
    assert_eq!(typed.human, Some(HumanInput::Typed("echo hi".into())));
    assert_eq!(typed.command, "type \"echo hi\"");
    let at = |action: &str| actions.iter().position(|a| *a == action).unwrap();
    assert!(at("typed \"echo hi\"") < at("pressed enter"));
    assert!(at("pressed enter") < at("took control"));
    assert!(at("took control") < at("typed \"false\""));
    let typed = &activity[at("typed \"false\"")];
    assert_eq!(typed.actor.kind, ActorKind::Agent);
    assert_eq!(typed.human, None, "an agent's input isn't the person's");

    // A reader that reached the end gets nothing more until something happens.
    let path = dir.journal(&handle, LogKind::Activity);
    let end = read_since(&path, 0, 1 << 20).unwrap().next;
    assert!(read_since(&path, end, 1 << 20).unwrap().lines.is_empty());
}

/// The output reads as text from an offset on, as the activity panel's Output tab and the
/// MCP's stream read page through it.
#[test]
fn the_output_reads_as_text_from_where_the_last_read_ended() {
    let dir = test_dir("stream");
    let _stop = StopOnDrop(dir.clone());
    let person = connect(&dir, ClientKind::App);
    let terminal = open_zsh(&person, &dir);
    let handle = terminal.handle;
    let (index, _) = person.attach(&handle, None);
    let read = |since: Option<u64>| {
        let request = RequestBody::ReadStream {
            handle: handle.clone(),
            since,
            max_bytes: None,
        };
        match person.connection.request(request).unwrap() {
            ResponseBody::Stream { text, next, .. } => (text, next),
            other => panic!("not a stream: {other:?}"),
        }
    };
    wait_until("the prompt", || read(None).0.contains('%'));
    let (_, mark) = read(None);
    person
        .connection
        .input(index, b"printf '\\e[1mbold\\e[0m\\n'\r")
        .unwrap();
    wait_until("the output since the mark", || {
        read(Some(mark)).0.contains("\nbold\n")
    });
    let (text, next) = read(Some(mark));
    assert!(!text.contains('\x1b'), "{text:?}");
    assert_eq!(read(Some(next)).0, "", "nothing new since");
}
