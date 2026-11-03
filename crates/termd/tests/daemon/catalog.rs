//! The operation catalog end to end (`docs/terminals.md` 8.5), as an agent's MCP calls carry
//! it out: open, send with its wait and read, the screen and the stream, waits, logs, a
//! repeated request id, and the person's control as the last word.

use std::path::Path;

use koyo_term_control::catalog::Scope;
use koyo_term_control::exec::{Done, Executor};
use koyo_term_control::{ClientKind, ErrorCode, ProtocolError};
use serde_json::{Value, json};

use super::{StopOnDrop, connect, connect_from, test_dir, wait_until};

fn lines(read: &Value) -> Vec<&str> {
    (read["lines"].as_array().unwrap().iter())
        .filter_map(Value::as_str)
        .collect()
}

#[test]
fn an_agent_drives_a_shell_through_the_catalog() {
    let dir = test_dir("catalog");
    let _stop = StopOnDrop(dir.clone());
    // The agent runs in this checkout, which `current` then means.
    let checkout =
        std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
    let relay = connect_from(&dir, ClientKind::Relay, Some(checkout));
    let agent = Executor::new(
        relay.connection.clone(),
        dir.clone(),
        relay.welcome.actor,
        &Scope::AGENT,
    );
    let call = |name: &str, args: Value| -> Result<Done, ProtocolError> { agent.call(name, args) };
    let home = dir.root().parent().unwrap().join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join(".zshrc"), "PS1='%# '\n").unwrap();

    let opened = call(
        "terminal_open",
        json!({
            "name": "agent-shell",
            "command": ["/bin/zsh", "-i"],
            "cwd": home,
            "env": { "HOME": home, "ZDOTDIR": home },
        }),
    )
    .unwrap();
    assert_eq!(opened.structured["existed"], false);
    assert_eq!(opened.structured["control"]["youHold"], true, "{opened:?}");
    let waited = call("terminal_wait", json!({ "for": "prompt" })).unwrap();
    assert_eq!(waited.structured["met"], true, "{waited:?}");

    // Act, wait and read in one call, the terminal the one last acted on.
    let sent = call(
        "terminal_send",
        json!({
            "text": "echo from-the-catalog",
            "enter": true,
            "then": { "wait": { "for": "prompt" }, "read": {} },
        }),
    )
    .unwrap();
    assert_eq!(sent.structured["submission"], "observed", "{sent:?}");
    assert_eq!(sent.structured["wait"]["met"], true);
    assert!(
        lines(&sent.structured["read"]).contains(&"from-the-catalog"),
        "{sent:?}"
    );

    let stream = call("terminal_read", json!({ "mode": "stream" })).unwrap();
    assert!(
        stream.structured["text"]
            .as_str()
            .unwrap()
            .contains("from-the-catalog")
    );
    let missing = call(
        "terminal_wait",
        json!({ "for": "text", "pattern": "nothing like this", "timeoutMs": 300 }),
    )
    .unwrap();
    assert_eq!(missing.structured["met"], false);

    call(
        "terminal_send",
        json!({ "text": "false", "enter": true, "then": { "wait": { "for": "prompt" } } }),
    )
    .unwrap();
    let commands = call("terminal_log", json!({ "kind": "commands" })).unwrap();
    let entries = commands.structured["entries"].as_array().unwrap();
    assert_eq!(entries[0]["command"], "echo from-the-catalog");
    let failed = call(
        "terminal_log",
        json!({ "kind": "commands", "failed": true }),
    )
    .unwrap();
    let failed = failed.structured["entries"].as_array().unwrap();
    assert_eq!((failed.len(), &failed[0]["command"]), (1, &json!("false")));

    // A request sent again is answered from its receipt, not sent twice.
    let once = json!({ "text": "echo once", "enter": true, "requestId": "send-once" });
    let first = call("terminal_send", once.clone()).unwrap();
    assert_eq!(call("terminal_send", once).unwrap(), first);
    let reused = call(
        "terminal_send",
        json!({ "text": "x", "requestId": "send-once" }),
    );
    assert_eq!(reused.unwrap_err().code, ErrorCode::InvalidArgument);
    call(
        "terminal_wait",
        json!({ "for": "text", "pattern": "^once$" }),
    )
    .unwrap();
    let screen = call("terminal_read", json!({})).unwrap();
    let onces = lines(&screen.structured)
        .iter()
        .filter(|line| **line == "once")
        .count();
    assert_eq!(onces, 1, "{screen:?}");

    let unknown = call("terminal_read", json!({ "bogus": 1 })).unwrap_err();
    assert_eq!(unknown.code, ErrorCode::InvalidArgument);
    let running = call("terminal_close", json!({})).unwrap_err();
    assert_eq!(running.code, ErrorCode::ConfirmationRequired);
    let listed = call("terminal_list", json!({})).unwrap();
    assert_eq!(listed.structured["terminals"].as_array().unwrap().len(), 1);

    let handle = opened.structured["terminal"]["terminal"].as_str().unwrap();
    the_person_takes_it_back(&dir, &agent, handle);
}

/// The person typing takes the terminal: the agent is told to leave it alone.
fn the_person_takes_it_back(dir: &koyo_term_control::TermDir, agent: &Executor, handle: &str) {
    let call = |name: &str, args: Value| agent.call(name, args);
    let person = connect(dir, ClientKind::App);
    let handle = koyo_term_control::Handle(handle.to_owned());
    let (index, _) = person.attach(&handle, None);
    person.connection.input(index, b" ").unwrap();
    wait_until("the agent to be refused", || {
        call("terminal_send", json!({ "keys": ["backspace"] }))
            .is_err_and(|error| error.code == ErrorCode::PersonHasControl)
    });
    let control = call("terminal_control", json!({})).unwrap();
    assert_eq!(control.structured["control"]["holder"]["kind"], "human");
    assert!(
        control.structured["activityTail"]
            .as_array()
            .is_some_and(|tail| !tail.is_empty())
    );
}

/// An agent's terminals are its worktree's: one in another checkout lists, names and finds
/// only that checkout's (`docs/terminals.md` M4).
#[test]
fn each_agent_sees_its_own_worktrees_terminals() {
    let dir = test_dir("scope");
    let _stop = StopOnDrop(dir.clone());
    let checkout =
        std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
    // Another checkout, as far as the daemon reads one: a `.git` with a HEAD.
    let other = dir.root().parent().unwrap().join("other");
    std::fs::create_dir_all(other.join(".git")).unwrap();
    std::fs::write(other.join(".git/HEAD"), "ref: refs/heads/elsewhere\n").unwrap();
    let agent = |cwd: &Path| {
        let relay = connect_from(&dir, ClientKind::Relay, Some(cwd.to_path_buf()));
        Executor::new(
            relay.connection,
            dir.clone(),
            relay.welcome.actor,
            &Scope::AGENT,
        )
    };
    let (here, there) = (agent(&checkout), agent(&other));
    let sleeper = |name: &str| json!({ "name": name, "command": ["/bin/sleep", "60"] });
    here.call("terminal_open", sleeper("mine")).unwrap();
    there.call("terminal_open", sleeper("theirs")).unwrap();

    let names = |agent: &Executor| -> Vec<String> {
        let listed = agent.call("terminal_list", json!({})).unwrap();
        (listed.structured["terminals"].as_array().unwrap().iter())
            .map(|terminal| terminal["name"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(names(&here), ["mine"]);
    assert_eq!(names(&there), ["theirs"]);
    let missing = here
        .call("terminal_read", json!({ "terminal": "theirs" }))
        .unwrap_err();
    assert_eq!(missing.code, ErrorCode::TerminalNotFound);
    let worktrees = here.call("worktree_list", json!({})).unwrap();
    assert_eq!(
        worktrees.structured["worktrees"].as_array().unwrap().len(),
        2
    );
}
