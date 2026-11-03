//! Control end to end (`docs/terminals.md` 5.6, D11): agents write under leases they claim,
//! another agent takes over only by asking to, a person's typing takes the terminal from any
//! agent, and only their "Give back" opens it to agents again.

use koyo_term_control::{
    ActorKind, ClientError, ClientKind, Control, ControlChange, Controller, ErrorCode,
    ErrorDetails, EventKind, Handle, ProtocolError, RequestBody, ResponseBody,
};

use super::{Client, ECHOER, StopOnDrop, connect, test_dir, wait_until};

impl Client {
    fn claim(&self, handle: &Handle, takeover: bool) -> Result<Control, ProtocolError> {
        let request = RequestBody::Claim {
            handle: handle.clone(),
            takeover,
            reason: takeover.then(|| "rerunning tests".to_owned()),
        };
        match self.connection.request(request) {
            Ok(ResponseBody::Control { control }) => Ok(control),
            Err(ClientError::Protocol(error)) => Err(error),
            other => panic!("{other:?}"),
        }
    }

    fn paste(&self, handle: &Handle, text: &str) -> Result<(), ProtocolError> {
        let request = RequestBody::Paste {
            handle: handle.clone(),
            text: text.into(),
        };
        match self.connection.request(request) {
            Ok(ResponseBody::Done) => Ok(()),
            Err(ClientError::Protocol(error)) => Err(error),
            other => panic!("{other:?}"),
        }
    }

    fn holder(&self, handle: &Handle) -> Option<Controller> {
        (self.list().into_iter())
            .find(|summary| summary.record.handle == *handle)
            .unwrap()
            .record
            .control
            .holder
    }
}

fn holder_kind(error: &ProtocolError) -> ActorKind {
    match error.details.as_deref() {
        Some(ErrorDetails::Holder { holder }) => holder.actor().kind,
        other => panic!("{error:?} names no holder: {other:?}"),
    }
}

#[test]
fn agents_hold_by_lease_and_a_person_takes_over_until_giving_back() {
    let dir = test_dir("control");
    let _stop = StopOnDrop(dir.clone());
    let cwd = dir.root().parent().unwrap().to_path_buf();
    let person = connect(&dir, ClientKind::App);
    let codex = connect(&dir, ClientKind::Relay);
    let claude = connect(&dir, ClientKind::Relay);
    let (terminal, _) = person.open("shared", &cwd, &ECHOER, false);
    let handle = terminal.handle;
    let index = person.attach_ready(&handle);
    codex.attach(&handle, None);
    claude.attach(&handle, None);

    // An agent writes nothing before it claims; then its writes go through.
    let early = codex.paste(&handle, "early\n").unwrap_err();
    assert_eq!(early.code, ErrorCode::ControlRequired);
    codex.claim(&handle, false).unwrap();
    codex.paste(&handle, "from codex\n").unwrap();
    person.output_until(index, "got:from codex");

    // Another agent displaces it only by asking to, and the first is told who did.
    let plain = claude.claim(&handle, false).unwrap_err();
    assert_eq!(plain.code, ErrorCode::ControlRequired);
    assert_eq!(holder_kind(&plain), ActorKind::Agent);
    claude.claim(&handle, true).unwrap();
    let taken = codex.event_until(&handle, |kind| {
        matches!(
            kind,
            EventKind::Control {
                change: ControlChange::Taken { .. },
                ..
            }
        )
    });
    let EventKind::Control {
        change: ControlChange::Taken { reason, .. },
        ..
    } = taken
    else {
        panic!("{taken:?}")
    };
    assert_eq!(reason.as_deref(), Some("rerunning tests"));
    let lost = codex.paste(&handle, "still me\n").unwrap_err();
    assert_eq!(lost.code, ErrorCode::ControlTakenBy);
    // The person's view reporting focus takes nothing; the line discipline echoes the report,
    // and the agent's ^U drops it from the line.
    person.connection.input(index, b"\x1b[I").unwrap();
    person.output_until(index, "^[[I");
    claude.paste(&handle, "\u{15}from claude\n").unwrap();
    person.output_until(index, "got:from claude");
    assert!(matches!(
        person.holder(&handle),
        Some(Controller::Agent { .. })
    ));

    // The person types: the input goes in, the terminal is theirs, and agents stay out,
    // taking over or not.
    person.connection.input(index, b"mine\r").unwrap();
    person.output_until(index, "got:mine");
    claude.event_until(&handle, |kind| {
        matches!(
            kind,
            EventKind::Control {
                change: ControlChange::HumanTook { implicit: true, .. },
                ..
            }
        )
    });
    assert!(matches!(
        person.holder(&handle),
        Some(Controller::Human { implicit: true, .. })
    ));
    for refused in [
        claude.paste(&handle, "mine again\n"),
        claude.claim(&handle, true).map(drop),
    ] {
        let error = refused.unwrap_err();
        assert_eq!(error.code, ErrorCode::PersonHasControl);
        assert_eq!(holder_kind(&error), ActorKind::Human);
    }

    // Give back opens it to agents again.
    let given = person.connection.request(RequestBody::Release {
        handle: handle.clone(),
    });
    assert!(matches!(given, Ok(ResponseBody::Control { control }) if control.holder.is_none()));
    codex.claim(&handle, false).unwrap();
    codex.paste(&handle, "back\n").unwrap();
    person.output_until(index, "got:back");

    // An agent's lease ends with its connection.
    codex.connection.close();
    person.event_until(&handle, |kind| {
        matches!(
            kind,
            EventKind::Control {
                change: ControlChange::Expired { .. },
                ..
            }
        )
    });
    wait_until("the lease to go", || person.holder(&handle).is_none());
}

/// Typing in a terminal no agent holds takes nothing, so an agent may still claim it; a
/// person's "Take control" holds it whoever held it.
#[test]
fn typing_in_a_free_terminal_takes_nothing_but_take_control_does() {
    let dir = test_dir("control-free");
    let _stop = StopOnDrop(dir.clone());
    let cwd = dir.root().parent().unwrap().to_path_buf();
    let person = connect(&dir, ClientKind::App);
    let agent = connect(&dir, ClientKind::Relay);
    let (terminal, _) = person.open("free", &cwd, &ECHOER, false);
    let handle = terminal.handle;
    let index = person.attach_ready(&handle);
    person.connection.input(index, b"hello\r").unwrap();
    person.output_until(index, "got:hello");
    assert_eq!(person.holder(&handle), None);

    agent.claim(&handle, false).unwrap();
    let control = person.claim(&handle, false).unwrap();
    assert!(matches!(
        control.holder,
        Some(Controller::Human {
            implicit: false,
            ..
        })
    ));
    assert_eq!(
        agent.claim(&handle, true).unwrap_err().code,
        ErrorCode::PersonHasControl
    );
}
