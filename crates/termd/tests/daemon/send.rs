//! An agent writing and reading without attaching (`docs/terminals.md` 8.5): text and named
//! keys encoded for the terminal, the screen read back, and the person's control still the
//! last word.

use koyo_term_control::{
    ClientError, ClientKind, ErrorCode, Handle, RequestBody, ResponseBody, SendRequest, Status,
};

use super::status::open_zsh;
use super::{StopOnDrop, connect, test_dir, wait_until};

fn send(handle: &Handle) -> SendRequest {
    SendRequest {
        handle: handle.clone(),
        text: None,
        paste: None,
        keys: Vec::new(),
        enter: false,
        interrupt: false,
    }
}

fn refusal(result: Result<ResponseBody, ClientError>) -> Option<ErrorCode> {
    match result {
        Err(ClientError::Protocol(error)) => Some(error.code),
        _ => None,
    }
}

#[test]
fn an_agent_sends_text_and_keys_and_reads_the_screen_without_attaching() {
    let dir = test_dir("send");
    let _stop = StopOnDrop(dir.clone());
    let person = connect(&dir, ClientKind::App);
    let agent = connect(&dir, ClientKind::Relay);
    let handle = open_zsh(&person, &dir).handle;
    wait_until("the prompt", || {
        agent.screen(&handle)
            .lines
            .iter()
            .any(|line| line.starts_with('%'))
    });
    let first = agent.screen(&handle);
    assert_eq!((first.cols, first.rows), (60, 12));
    assert!(first.cursor.is_some_and(|cursor| cursor.visible));

    let unclaimed = SendRequest {
        text: Some("echo nope".into()),
        ..send(&handle)
    };
    let refused = refusal(agent.connection.request(RequestBody::Send(unclaimed)));
    assert_eq!(
        refused,
        Some(ErrorCode::ControlRequired),
        "an agent claims first"
    );
    let claim = RequestBody::Claim {
        handle: handle.clone(),
        takeover: false,
        reason: None,
    };
    agent.connection.request(claim).unwrap();
    let typed = SendRequest {
        text: Some("echo sent-by-agent".into()),
        enter: true,
        ..send(&handle)
    };
    let sent = agent.connection.request(RequestBody::Send(typed)).unwrap();
    assert_eq!(sent, ResponseBody::Sent { bytes: 19 });
    wait_until("the agent's output on the screen", || {
        agent.screen(&handle)
            .lines
            .iter()
            .any(|line| line == "sent-by-agent")
    });
    let unknown = SendRequest {
        keys: vec!["hyper+x".into()],
        ..send(&handle)
    };
    let refused = refusal(agent.connection.request(RequestBody::Send(unknown)));
    assert_eq!(refused, Some(ErrorCode::InvalidArgument));

    // Ctrl-C by name stops what runs. The program says it started, so the interrupt reaches
    // it and not the shell still starting it; `sh` and its `sleep` die of it together.
    let sleeping = SendRequest {
        text: Some("sh -c 'echo asleep; sleep 30'".into()),
        enter: true,
        ..send(&handle)
    };
    agent
        .connection
        .request(RequestBody::Send(sleeping))
        .unwrap();
    wait_until("the program to run", || {
        let screen = agent.screen(&handle);
        screen.status == Status::Running && screen.lines.iter().any(|line| line == "asleep")
    });
    let interrupt = SendRequest {
        interrupt: true,
        ..send(&handle)
    };
    agent
        .connection
        .request(RequestBody::Send(interrupt))
        .unwrap();
    wait_until("the prompt after Ctrl-C", || {
        let record = (person.list().into_iter())
            .find(|summary| summary.record.handle == handle)
            .unwrap()
            .record;
        record.status == Status::Idle && record.last_exit == Some(130)
    });

    // The person typing takes the terminal; the agent's sends are refused from then on.
    let (index, _) = person.attach(&handle, None);
    person.connection.input(index, b" ").unwrap();
    wait_until("the agent to be refused", || {
        let again = SendRequest {
            keys: vec!["up".into()],
            ..send(&handle)
        };
        refusal(agent.connection.request(RequestBody::Send(again)))
            == Some(ErrorCode::PersonHasControl)
    });
    person.connection.request(RequestBody::Shutdown).unwrap();
    wait_until("the socket to go", || !dir.socket().exists());
}
