//! A size pinned on a terminal end to end (`docs/responsive.md` 3.1, 4.2, 6): it sizes the
//! program and the engine over every client's size, goes when unpinned or with the
//! connection that set it, is an agent's resize under its lease, stays within the limits,
//! isn't set on a program that ended, and comes from `terminal_open`'s size and
//! `terminal_resize`, which answers with the screen the program redrew.

use std::path::Path;

use koyo_term_control::catalog::Scope;
use koyo_term_control::exec::Executor;
use koyo_term_control::log::read_since;
use koyo_term_control::{
    ActivityEntry, ActorKind, ClientError, ClientKind, ErrorCode, EventKind, Handle, LogKind,
    Pinned, ProtocolError, RequestBody, ResponseBody, Size, Status, TerminalRecord,
};
use serde_json::{Value, json};

use super::{Client, StopOnDrop, connect, connect_from, test_dir, wait_until};

/// Prints `ready`, then the pty's size, `size:<rows> <cols>`, for each line it reads.
const SIZER: [&str; 3] = [
    "/bin/sh",
    "-c",
    "printf 'ready\\n'; while read line; do printf 'size:%s\\n' \"$(stty size)\"; done",
];

/// Redraws its one line a moment after every `SIGWINCH`, as a full-screen program does.
const REDRAWS: &str = "draw() { sleep 0.2; set -- $(stty size); \
                       printf '\\033[H\\033[2JSize: %sx%s\\n' \"$2\" \"$1\"; }; \
                       trap draw WINCH; draw; while :; do sleep 1 & wait $!; done";

impl Client {
    fn pin(
        &self,
        handle: &Handle,
        cells: Option<(u16, u16)>,
    ) -> Result<(Size, Option<Pinned>), ProtocolError> {
        let request = RequestBody::Pin {
            handle: handle.clone(),
            cells,
        };
        match self.connection.request(request) {
            Ok(ResponseBody::Pinned { size, pinned }) => Ok((size, pinned)),
            Err(ClientError::Protocol(error)) => Err(error),
            other => panic!("{other:?}"),
        }
    }

    fn record(&self, handle: &Handle) -> TerminalRecord {
        (self.list().into_iter())
            .find(|summary| summary.record.handle == *handle)
            .unwrap()
            .record
    }

    /// Types a line into `SIZER` and the size it prints, `<rows> <cols>`.
    fn stty(&self, index: u32) -> String {
        self.connection.input(index, b"\r").unwrap();
        let (mut text, _) = self.output_until(index, "size:");
        let start = text.find("size:").unwrap() + "size:".len();
        while !text[start..].contains('\n') {
            text.push_str(&self.output_until(index, "\n").0);
        }
        text[start..].lines().next().unwrap().trim_end().to_owned()
    }
}

fn cells(size: Size) -> (u16, u16) {
    (size.cols, size.rows)
}

#[test]
fn a_pin_sizes_the_terminal_over_its_clients_and_unpinning_gives_it_back() {
    let dir = test_dir("pin");
    let _stop = StopOnDrop(dir.clone());
    let cwd = dir.root().parent().unwrap().to_path_buf();
    let person = connect(&dir, ClientKind::App);
    let (terminal, _) = person.open("sizer", &cwd, &SIZER, false);
    let handle = terminal.handle;
    let index = person.attach_ready(&handle);
    assert_eq!(person.stty(index), "12 60");

    let (size, pinned) = person.pin(&handle, Some((100, 30))).unwrap();
    assert_eq!(cells(size), (100, 30));
    let pinned = pinned.unwrap();
    assert_eq!(
        (pinned.cols, pinned.rows, pinned.by.kind),
        (100, 30, ActorKind::Human)
    );
    let event = person.event_until(&handle, |kind| matches!(kind, EventKind::Size { .. }));
    assert_eq!(
        event,
        EventKind::Size {
            size,
            pinned: Some(pinned.clone()),
        }
    );
    // The program and the engine both have the pinned size.
    assert_eq!(person.stty(index), "30 100");
    let ResponseBody::Screen(screen) = (person.connection)
        .request(RequestBody::ReadScreen {
            handle: handle.clone(),
        })
        .unwrap()
    else {
        panic!("not a screen");
    };
    assert_eq!((screen.cols, screen.rows), (100, 30));

    // A smaller client attaching doesn't size it while the pin holds.
    let other = connect(&dir, ClientKind::Cli);
    other.attach(&handle, None);
    let record = person.record(&handle);
    assert_eq!(cells(record.size), (100, 30));
    assert_eq!(record.pinned, Some(pinned));

    // Unpinned, the clients' smallest size is its size again.
    let (size, pinned) = person.pin(&handle, None).unwrap();
    assert_eq!((cells(size), pinned), ((60, 12), None));
    assert_eq!(person.stty(index), "12 60");
    assert_eq!(person.record(&handle).pinned, None);
}

#[test]
fn a_pin_goes_with_the_connection_that_set_it_and_a_later_one_replaces_it() {
    let dir = test_dir("pin-gone");
    let _stop = StopOnDrop(dir.clone());
    let cwd = dir.root().parent().unwrap().to_path_buf();
    let person = connect(&dir, ClientKind::App);
    let (terminal, _) = person.open("sizer", &cwd, &SIZER, false);
    let handle = terminal.handle;
    person.attach_ready(&handle);
    person.pin(&handle, Some((80, 24))).unwrap();

    // An agent never attaches: it pins from its own connection, which replaces the person's.
    let agent = connect(&dir, ClientKind::Relay);
    let claim = RequestBody::Claim {
        handle: handle.clone(),
        takeover: false,
        reason: None,
    };
    agent.connection.request(claim).unwrap();
    let (size, pinned) = agent.pin(&handle, Some((120, 40))).unwrap();
    assert_eq!(cells(size), (120, 40));
    assert_eq!(pinned.unwrap().by.kind, ActorKind::Agent);

    agent.connection.close();
    drop(agent);
    wait_until("the agent's pin to go with it", || {
        let record = person.record(&handle);
        record.pinned.is_none() && cells(record.size) == (60, 12)
    });
}

#[test]
fn an_agents_pin_needs_its_lease_and_is_logged_and_a_persons_takes_nothing_from_it() {
    let dir = test_dir("pin-lease");
    let _stop = StopOnDrop(dir.clone());
    let cwd = dir.root().parent().unwrap().to_path_buf();
    let person = connect(&dir, ClientKind::App);
    let agent = connect(&dir, ClientKind::Relay);
    let (terminal, _) = person.open("sizer", &cwd, &SIZER, false);
    let handle = terminal.handle;
    person.attach_ready(&handle);

    let refused = agent.pin(&handle, Some((120, 40))).unwrap_err();
    assert_eq!(refused.code, ErrorCode::ControlRequired);
    assert_eq!(person.record(&handle).pinned, None);
    let claim = RequestBody::Claim {
        handle: handle.clone(),
        takeover: false,
        reason: None,
    };
    agent.connection.request(claim).unwrap();
    agent.pin(&handle, Some((120, 40))).unwrap();

    // The person's pin replaces the agent's and leaves the agent holding the terminal.
    let (_, pinned) = person.pin(&handle, Some((80, 24))).unwrap();
    assert_eq!(pinned.unwrap().by.kind, ActorKind::Human);
    let holder = person.record(&handle).control.holder.unwrap();
    assert_eq!(holder.actor().kind, ActorKind::Agent);

    let read = read_since(&dir.journal(&handle, LogKind::Activity), 0, 1 << 20).unwrap();
    let entries: Vec<ActivityEntry> = (read.lines.iter())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let resizes: Vec<(&str, u8)> = (entries.iter())
        .filter(|entry| entry.command.starts_with("resize"))
        .map(|entry| (entry.action.as_str(), entry.exit))
        .collect();
    assert_eq!(
        resizes,
        [("resized to 120x40", 1), ("resized to 120x40", 0)]
    );
    assert!(
        (entries.iter())
            .all(|entry| entry.actor.kind != ActorKind::Human
                || !entry.command.starts_with("resize")),
        "a person's pin isn't logged: {entries:?}"
    );
}

#[test]
fn pins_outside_the_limits_or_on_a_program_that_ended_are_refused() {
    let dir = test_dir("pin-limits");
    let _stop = StopOnDrop(dir.clone());
    let cwd = dir.root().parent().unwrap().to_path_buf();
    let person = connect(&dir, ClientKind::App);
    let (terminal, _) = person.open("sizer", &cwd, &SIZER, false);
    let handle = terminal.handle;
    person.attach_ready(&handle);
    for (cols, rows) in [(1, 24), (80, 1), (1001, 24), (80, 1001), (0, 0)] {
        let error = person.pin(&handle, Some((cols, rows))).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument, "{cols}x{rows}");
    }
    for (cols, rows) in [(2, 1000), (1000, 2), (2, 2)] {
        let (size, _) = person.pin(&handle, Some((cols, rows))).unwrap();
        assert_eq!(cells(size), (cols, rows));
    }

    let (ended, _) = person.open("ended", &cwd, &["/bin/sh", "-c", "exit 3"], false);
    wait_until("the program to end", || {
        person.record(&ended.handle).status == Status::Exited
    });
    let error = person.pin(&ended.handle, Some((80, 24))).unwrap_err();
    assert_eq!(error.code, ErrorCode::Exited);
    // Unpinning one is harmless.
    assert_eq!(person.pin(&ended.handle, None).unwrap().1, None);
}

#[test]
fn an_agent_sizes_a_terminal_by_opening_it_and_resizing_it_and_reads_the_redraw() {
    let dir = test_dir("pin-mcp");
    let _stop = StopOnDrop(dir.clone());
    let checkout =
        std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
    let relay = connect_from(&dir, ClientKind::Relay, Some(checkout));
    let agent = Executor::new(
        relay.connection.clone(),
        dir.clone(),
        relay.welcome.actor,
        &Scope::AGENT,
    );
    let call = |name: &str, args: Value| agent.call(name, args).unwrap().structured;
    let cwd = dir.root().parent().unwrap().to_path_buf();
    let opened = call(
        "terminal_open",
        json!({
            "name": "tui",
            "command": ["/bin/sh", "-c", REDRAWS],
            "cwd": cwd,
            "size": { "cols": 50, "rows": 8 },
        }),
    );
    assert_eq!(opened["terminal"]["pinned"]["cols"], 50, "{opened}");
    assert_eq!(opened["terminal"]["pinned"]["by"]["kind"], "agent");
    let handle = Handle(opened["terminal"]["terminal"].as_str().unwrap().to_owned());
    let waited = call(
        "terminal_wait",
        json!({ "for": "text", "pattern": "Size: 50x8" }),
    );
    assert_eq!(waited["met"], true, "{waited}");

    // The person's view attaching doesn't size away what the agent opened it at.
    let person = connect(&dir, ClientKind::App);
    person.attach(&handle, None);
    let record = person.record(&handle);
    assert_eq!(cells(record.size), (50, 8));
    let listed = call("terminal_list", json!({}));
    assert_eq!(listed["terminals"][0]["pinned"]["rows"], 8, "{listed}");

    // A resize answers with the screen the program redrew after it.
    let resized = call("terminal_resize", json!({ "cols": 60, "rows": 10 }));
    assert_eq!(resized["size"], json!({ "cols": 60, "rows": 10 }));
    assert_eq!(resized["pinned"]["cols"], 60);
    assert_eq!(resized["wait"]["met"], true, "{resized}");
    assert_eq!(resized["wait"]["condition"], "stable");
    assert_eq!(resized["read"]["lines"][0], "Size: 60x10", "{resized}");
    assert_eq!(resized["read"]["header"]["cols"], 60, "{resized}");

    // `then: {}` answers at once, without waiting or reading.
    let quick = call(
        "terminal_resize",
        json!({ "cols": 70, "rows": 9, "then": {} }),
    );
    assert_eq!(quick["size"], json!({ "cols": 70, "rows": 9 }));
    assert!(
        quick.get("wait").is_none() && quick.get("read").is_none(),
        "{quick}"
    );

    // Filling again, the person's view sizes it.
    let filled = call("terminal_resize", json!({ "fill": true }));
    assert_eq!(filled["pinned"], Value::Null, "{filled}");
    assert_eq!(filled["size"], json!({ "cols": 60, "rows": 12 }));
    assert_eq!(filled["read"]["lines"][0], "Size: 60x12", "{filled}");
    let refused = agent
        .call("terminal_resize", json!({ "cols": 1, "rows": 24 }))
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::InvalidArgument);
}

#[test]
fn an_open_pins_its_size_only_with_the_hold_and_says_so_when_refused() {
    let dir = test_dir("pin-open");
    let _stop = StopOnDrop(dir.clone());
    let checkout =
        std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
    let agent = |relay: &Client| {
        Executor::new(
            relay.connection.clone(),
            dir.clone(),
            relay.welcome.actor.clone(),
            &Scope::AGENT,
        )
    };
    let relays = [
        connect_from(&dir, ClientKind::Relay, Some(checkout.clone())),
        connect_from(&dir, ClientKind::Relay, Some(checkout)),
    ];
    let (first, second) = (agent(&relays[0]), agent(&relays[1]));
    let cwd = dir.root().parent().unwrap().to_path_buf();
    let open = |agent: &Executor, name: &str, extra: Value| {
        let mut args = json!({
            "name": name,
            "command": SIZER,
            "cwd": cwd,
            "size": { "cols": 50, "rows": 8 },
        });
        args.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        agent.call("terminal_open", args).unwrap()
    };

    // Without the claim, the size is the one it starts at, and nothing was refused.
    let unclaimed = open(&first, "unclaimed", json!({ "claim": false }));
    let structured = &unclaimed.structured;
    assert!(
        !unclaimed.summary.contains("not pinned"),
        "{}",
        unclaimed.summary
    );
    assert!(
        structured.get("pinned").is_none() && structured.get("pinRefused").is_none(),
        "{structured}"
    );
    assert_eq!(structured["terminal"]["pinned"], Value::Null);
    let handle = Handle(
        structured["terminal"]["terminal"]
            .as_str()
            .unwrap()
            .to_owned(),
    );
    let record = relays[0].record(&handle);
    assert_eq!((cells(record.size), record.pinned), ((50, 8), None));

    // Claimed, it's pinned, as the answer says.
    let held = open(&first, "held", json!({}));
    assert_eq!(held.structured["pinned"]["cols"], 50, "{}", held.structured);
    assert_eq!(held.structured["pinned"]["by"]["kind"], "agent");

    // Another agent's hold refuses a second agent's size, in the answer as in its summary.
    let refused = open(
        &second,
        "held",
        json!({ "size": { "cols": 70, "rows": 9 } }),
    );
    let structured = &refused.structured;
    assert_eq!(structured["existed"], true);
    assert_eq!(structured["pinned"], Value::Null, "{structured}");
    assert_eq!(
        structured["pinRefused"]["code"], "control_required",
        "{structured}"
    );
    assert!(
        (structured["pinRefused"]["message"].as_str()).is_some_and(|message| !message.is_empty()),
        "{structured}"
    );
    assert!(
        refused.summary.contains("not pinned"),
        "{}",
        refused.summary
    );
    assert_eq!(
        structured["terminal"]["pinned"]["cols"], 50,
        "the first pin stands"
    );
}
