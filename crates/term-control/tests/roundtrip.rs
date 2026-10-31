//! Every message kind survives encoding, framing in arbitrary pieces, and decoding.

use std::collections::{BTreeMap, BTreeSet};

use bytes::Bytes;
use colosso_term_control::{
    Actor, ActorKind, AttachMode, Auth, ClientInfo, ClientKind, Control, ControlChange, Controller,
    Decoder, ErrorCode, ErrorDetails, Event, EventKind, Exit, Handle, Hello, IfExists, Input,
    Lagging, Message, OpenRequest, Outcome, Output, PROTOCOL, Progress, ProtocolError, Request,
    RequestBody, Response, ResponseBody, SNAPSHOT_FORMAT, Selector, Signal, Size, SnapshotChunk,
    Status, TerminalRecord, TerminalSummary, Welcome, WorktreeRef, WorktreeSummary,
};

fn agent() -> Actor {
    Actor {
        kind: ActorKind::Agent,
        id: "claude-code:7f3a".into(),
        label: "Claude Code".into(),
    }
}

fn human() -> Actor {
    Actor {
        kind: ActorKind::Human,
        id: "app".into(),
        label: "you".into(),
    }
}

fn worktree() -> WorktreeRef {
    WorktreeRef {
        path: "/w/feat-x".into(),
        repo_main: "/w/main".into(),
        branch: Some("feat-x".into()),
    }
}

fn record() -> TerminalRecord {
    TerminalRecord {
        handle: Handle("term_01J0000000000000000000000A".into()),
        worktree: Some(worktree()),
        name: "tests".into(),
        title: Some("zsh".into()),
        cwd: "/w/feat-x/crates".into(),
        command: vec![],
        env: BTreeMap::from([("FOO".to_owned(), "bar".to_owned())]),
        size: Size {
            cols: 120,
            rows: 40,
            cell_width_px: 8,
            cell_height_px: 17,
        },
        labels: BTreeSet::from(["agent".to_owned()]),
        created_ms: 1_793_377_725_006,
        created_by: agent(),
        incarnation: 1,
        pid: Some(4242),
        exit: None,
        status: Status::Running,
        status_reason: "command running".into(),
        last_exit: Some(0),
        progress: Some(Progress::Percent(42)),
        control: Control {
            holder: Some(Controller::Agent {
                actor: agent(),
                since_ms: 1_793_377_725_006,
                lease_until_ms: 1_793_377_845_006,
                reason: Some("rerunning tests".into()),
            }),
            observers: vec![human()],
        },
        attached_clients: 2,
        last_output_ms: 1_793_377_725_999,
    }
}

fn handle() -> Handle {
    record().handle
}

/// Every request, one of each shape at least.
fn requests() -> Vec<RequestBody> {
    let open = OpenRequest {
        worktree: Some(Selector::Branch("feat-x".into())),
        name: "dev".into(),
        command: vec!["pnpm".into(), "dev".into()],
        cwd: None,
        env: BTreeMap::new(),
        size: Some(Size::default()),
        labels: BTreeSet::from(["dev-server".to_owned()]),
        if_exists: IfExists::Error,
        respawn: true,
    };
    vec![
        RequestBody::ListWorktrees,
        RequestBody::ListTerminals {
            worktree: Some(Selector::Current),
            status: Some(vec![Status::Idle, Status::NeedsInput]),
        },
        RequestBody::ListTerminals {
            worktree: Some(Selector::Path("/w/main".into())),
            status: None,
        },
        RequestBody::Open(open),
        RequestBody::Attach {
            handle: handle(),
            size: None,
            mode: AttachMode::Observe,
            since_offset: Some(77),
        },
        RequestBody::Detach { handle: handle() },
        RequestBody::Resize {
            handle: handle(),
            size: Size {
                cols: 80,
                rows: 24,
                cell_width_px: 0,
                cell_height_px: 0,
            },
        },
        RequestBody::Rename {
            handle: handle(),
            name: Some("ci".into()),
            title: None,
            labels: Some(BTreeSet::new()),
        },
        RequestBody::Signal {
            handle: handle(),
            signal: Signal::Int,
        },
        RequestBody::Close {
            handle: handle(),
            force: true,
        },
        RequestBody::Paste {
            handle: handle(),
            text: "multi\nline".into(),
        },
        RequestBody::Claim {
            handle: handle(),
            takeover: true,
            reason: Some("why".into()),
        },
        RequestBody::Release { handle: handle() },
        RequestBody::Renew { handle: handle() },
        RequestBody::Shutdown,
    ]
}

/// Every outcome a response carries, answers and errors.
fn responses() -> Vec<Outcome> {
    vec![
        Outcome::Ok(ResponseBody::Worktrees {
            worktrees: vec![WorktreeSummary {
                worktree: worktree(),
                terminals: 3,
                needs_input: 1,
            }],
        }),
        Outcome::Ok(ResponseBody::Terminals {
            terminals: vec![TerminalSummary {
                record: record(),
                preview: "$ ".into(),
            }],
        }),
        Outcome::Ok(ResponseBody::Opened {
            terminal: Box::new(record()),
            existed: false,
        }),
        Outcome::Ok(ResponseBody::Attached {
            terminal: Box::new(record()),
            index: 3,
            offset: 1 << 40,
        }),
        Outcome::Ok(ResponseBody::Control {
            control: Control::default(),
        }),
        Outcome::Ok(ResponseBody::Resized {
            size: Size::default(),
        }),
        Outcome::Ok(ResponseBody::Done),
        Outcome::Err(
            ProtocolError::new(ErrorCode::PersonHasControl, "a person has control of tests")
                .with_details(ErrorDetails::Holder {
                    holder: Controller::Human {
                        actor: human(),
                        since_ms: 5,
                        implicit: true,
                    },
                }),
        ),
        Outcome::Err(
            ProtocolError::new(ErrorCode::NameTaken, "taken")
                .with_details(ErrorDetails::Existing { handle: handle() }),
        ),
        Outcome::Err(
            ProtocolError::new(ErrorCode::AmbiguousTerminal, "which").with_details(
                ErrorDetails::Terminals {
                    handles: vec![handle()],
                },
            ),
        ),
        Outcome::Err(
            ProtocolError::new(ErrorCode::AmbiguousWorktree, "which").with_details(
                ErrorDetails::Worktrees {
                    paths: vec!["/a".into(), "/b".into()],
                },
            ),
        ),
        Outcome::Err(ProtocolError::new(ErrorCode::Timeout, "slow").retryable()),
    ]
}

/// Every event a terminal has.
fn events() -> Vec<EventKind> {
    vec![
        EventKind::Opened {
            terminal: Box::new(record()),
        },
        EventKind::Renamed {
            name: "x".into(),
            title: Some("t".into()),
            labels: BTreeSet::new(),
        },
        EventKind::Closed,
        EventKind::Spawned {
            pid: 9,
            incarnation: 2,
        },
        EventKind::Exited {
            exit: Exit {
                code: Some(1),
                signal: None,
                daemon_restart: false,
            },
        },
        EventKind::Title {
            title: "✳ Claude Code".into(),
        },
        EventKind::Cwd {
            cwd: "/tmp".into(),
            outside_worktree: true,
        },
        EventKind::Status {
            status: Status::NeedsInput,
            reason: "bell".into(),
            progress: Some(Progress::Error(None)),
        },
        EventKind::Control {
            change: ControlChange::Taken {
                by: agent(),
                from: human(),
                reason: None,
            },
            control: Control::default(),
        },
        EventKind::Control {
            change: ControlChange::HumanTook {
                by: human(),
                implicit: true,
            },
            control: Control::default(),
        },
        EventKind::Control {
            change: ControlChange::Expired { was: agent() },
            control: Control::default(),
        },
        EventKind::Size {
            size: Size::default(),
        },
        EventKind::Notification {
            title: "Claude Code".into(),
            body: "needs your input".into(),
        },
        EventKind::Bell,
        EventKind::Refused {
            error: ProtocolError::new(ErrorCode::ControlRequired, "observers can't type"),
        },
    ]
}

fn every_message() -> Vec<Message> {
    let (requests, responses, events) = (requests(), responses(), events());

    let mut messages = vec![
        Message::Hello(Hello {
            protocol: PROTOCOL,
            client: ClientInfo {
                kind: ClientKind::Relay,
                name: "claude-code".into(),
                version: "2.1".into(),
                pid: 77,
                session: Some("s1".into()),
            },
            auth: Auth::Token(serde_bytes::ByteBuf::from(vec![1, 2, 3])),
            capabilities: vec!["snapshot_v1".into()],
            cwd: Some("/w/feat-x".into()),
        }),
        Message::Hello(Hello {
            protocol: PROTOCOL,
            client: ClientInfo {
                kind: ClientKind::App,
                name: "colosso".into(),
                version: "0.1".into(),
                pid: 1,
                session: None,
            },
            auth: Auth::None,
            capabilities: vec![],
            cwd: None,
        }),
        Message::Welcome(Welcome {
            protocol: PROTOCOL,
            daemon_version: "0.1.0".into(),
            engine_commit: "83fb956".into(),
            snapshot_format: SNAPSHOT_FORMAT,
            client_id: 12,
            actor: agent(),
            features: vec!["log_v1".into()],
        }),
        Message::Output(Output {
            index: 1,
            offset: u64::MAX,
            bytes: Bytes::from_static(b"\x1b[31mred\x1b[0m"),
        }),
        Message::Input(Input {
            index: 2,
            bytes: Bytes::from_static(b"\x03"),
        }),
        Message::Snapshot(SnapshotChunk {
            index: 3,
            offset: 10,
            format: 1,
            chunk: 1,
            of: 2,
            bytes: Bytes::from(vec![0u8; 70_000]),
        }),
        Message::Lagging(Lagging { index: 4 }),
    ];
    messages.extend(requests.into_iter().enumerate().map(|(req, body)| {
        Message::Request(Request {
            req: req as u32,
            body,
        })
    }));
    messages.extend(responses.into_iter().enumerate().map(|(req, outcome)| {
        Message::Response(Response {
            req: req as u32,
            outcome,
        })
    }));
    messages.extend(events.into_iter().map(|kind| {
        Message::Event(Event {
            handle: handle(),
            kind,
        })
    }));
    messages
}

#[test]
fn every_message_round_trips_through_a_stream_fed_in_pieces() {
    let messages = every_message();
    let mut stream = Vec::new();
    for message in &messages {
        message.encode(&mut stream).unwrap();
    }
    for piece in [1, 7, 4096, stream.len()] {
        let mut decoder = Decoder::new();
        let mut got = Vec::new();
        for chunk in stream.chunks(piece) {
            decoder.feed(chunk);
            while let Some(frame) = decoder.next_frame().unwrap() {
                got.push(Message::decode(frame).unwrap());
            }
        }
        assert_eq!(got.len(), messages.len(), "piece size {piece}");
        for (back, want) in got.iter().zip(&messages) {
            assert_eq!(back, want);
        }
        assert_eq!(decoder.pending(), 0);
    }
}

#[test]
fn hot_frames_cost_their_header_and_nothing_more() {
    let mut out = Vec::new();
    Message::Output(Output {
        index: 0,
        offset: 0,
        bytes: Bytes::from(vec![0u8; 65_536]),
    })
    .encode(&mut out)
    .unwrap();
    assert_eq!(out.len(), 4 + Output::HEADER + 65_536);
    let mut out = Vec::new();
    Message::Input(Input {
        index: 0,
        bytes: Bytes::from_static(b"a"),
    })
    .encode(&mut out)
    .unwrap();
    assert_eq!(out.len(), 4 + Input::HEADER + 1);
}

#[test]
fn a_control_frame_with_bad_cbor_is_an_error_not_a_panic() {
    let mut decoder = Decoder::new();
    decoder.feed(&[0x03, 3, 0, 0, 0xff, 0xff, 0xff]);
    let frame = decoder.next_frame().unwrap().unwrap();
    assert!(Message::decode(frame).is_err());
}
