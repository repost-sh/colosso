//! The host's protocol fixtures (`koyo-js/koyo-agent-host/test/fixtures/protocol`), each read
//! as the app reads it and written again to the same JSON, so the two sides can't drift apart.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test fails loudly at the step that broke"
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use koyo_agent::protocol::events::Event;
use koyo_agent::protocol::methods::{self, METHODS, Method, ThreadSnapshot};
use koyo_agent::protocol::model::ItemBody;
use koyo_agent::protocol::requests::{Answer, RequestPayload};
use koyo_agent::protocol::wire::{self, ErrorCode, HOST_VERSION, Inbound, MAX_LINE_BYTES};
use serde_json::{Value, json};

fn fixtures() -> PathBuf {
    Path::new(env!("KOYO_CHECKOUT")).join("koyo-js/koyo-agent-host/test/fixtures/protocol")
}

fn json_at(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The names of the fixtures in `folder`, without `.json`.
fn names(folder: &str) -> BTreeSet<String> {
    let entries = std::fs::read_dir(fixtures().join(folder)).unwrap();
    (entries.map(|entry| entry.unwrap().path()))
        .filter_map(|path| {
            Some(
                path.file_name()?
                    .to_str()?
                    .strip_suffix(".json")?
                    .to_owned(),
            )
        })
        .collect()
}

/// The result of the reply `line`, which answers the request `id`.
fn result_of(line: &Value, id: u64) -> Box<serde_json::value::RawValue> {
    match wire::read(&line.to_string()).unwrap() {
        Inbound::Reply {
            id: Some(answered),
            outcome: Ok(result),
        } if answered == id => result,
        other => panic!("not the result of {id}: {other:?}"),
    }
}

/// `M`'s fixture: its request written as the app writes it, its reply read as the app reads
/// it, and the result written again, each the same JSON as the fixture's.
fn method<M: Method>(tested: &mut BTreeSet<String>) {
    let fixture = json_at(&fixtures().join(format!("methods/{}.json", M::NAME)));
    let request = &fixture["request"];
    let id = request["id"].as_u64().unwrap();
    let params: M::Params = serde_json::from_str(&request["params"].to_string())
        .unwrap_or_else(|e| panic!("{}'s params: {e}", M::NAME));
    let line = wire::request_line::<M>(id, &params).unwrap();
    assert!(line.ends_with('\n') && !line.trim_end().contains('\n'));
    assert_eq!(
        serde_json::from_str::<Value>(&line).unwrap(),
        *request,
        "{}",
        M::NAME
    );

    let raw = result_of(&fixture["response"], id);
    let result: M::Result =
        serde_json::from_str(raw.get()).unwrap_or_else(|e| panic!("{}'s result: {e}", M::NAME));
    let written = serde_json::to_value(result).unwrap();
    assert_eq!(written, fixture["response"]["result"], "{}", M::NAME);
    tested.insert(M::NAME.to_owned());
}

#[test]
fn every_method_writes_its_request_and_reads_its_result_as_the_fixture_has_them() {
    let mut tested = BTreeSet::new();
    let tested = &mut tested;
    method::<methods::Hello>(tested);
    method::<methods::Shutdown>(tested);
    method::<methods::ConfigSet>(tested);
    method::<methods::ProvidersStatus>(tested);
    method::<methods::GetCatalog>(tested);
    method::<methods::ThreadsWatch>(tested);
    method::<methods::ThreadsCreate>(tested);
    method::<methods::ThreadsOpen>(tested);
    method::<methods::ThreadsHistory>(tested);
    method::<methods::ThreadsClose>(tested);
    method::<methods::ThreadsUpdate>(tested);
    method::<methods::ThreadsRead>(tested);
    method::<methods::ThreadsDelete>(tested);
    method::<methods::ThreadsFilesBack>(tested);
    method::<methods::ThreadsRewind>(tested);
    method::<methods::TurnsSend>(tested);
    method::<methods::TurnsStop>(tested);
    method::<methods::TurnsCompact>(tested);
    method::<methods::QueueRemove>(tested);
    method::<methods::QueueEdit>(tested);
    method::<methods::QueueSendNow>(tested);
    method::<methods::QueueResume>(tested);
    method::<methods::RequestsRespond>(tested);
    // The host keeps a fixture for each of its methods; the app has them all, and tests each.
    let mine: BTreeSet<String> = METHODS.iter().map(|&name| name.to_owned()).collect();
    assert_eq!(names("methods"), mine);
    assert_eq!(*tested, mine);
}

#[test]
fn every_event_reads_and_writes_as_the_fixture_has_it() {
    for name in names("events") {
        let fixture = json_at(&fixtures().join(format!("events/{name}.json")));
        let Inbound::Event(event) = wire::read(&fixture.to_string()).unwrap() else {
            panic!("{name} isn't an event");
        };
        assert!(!matches!(event, Event::Unknown { .. }), "{name} is unknown");
        assert_eq!(serde_json::to_value(event).unwrap(), fixture, "{name}");
    }
}

#[test]
fn an_error_reads_with_its_code_and_writes_as_the_fixture_has_it() {
    let fixture = json_at(&fixtures().join("error.json"));
    let Inbound::Reply {
        id: Some(7),
        outcome: Err(error),
    } = wire::read(&fixture.to_string()).unwrap()
    else {
        panic!("not the refusal of request 7");
    };
    assert_eq!(error.code, ErrorCode::ThreadNotFound);
    assert_eq!(serde_json::to_value(error).unwrap(), fixture["error"]);
}

/// What a newer host may add without this build knowing it: kept whole, written as it came.
#[test]
fn kinds_from_a_newer_host_read_as_unknown_and_write_as_they_came() {
    let fixture = json_at(&fixtures().join("methods/threads.open.json"));
    let mut snapshot = fixture["response"]["result"].clone();
    snapshot["items"][0]["kind"] = json!("hologram");
    snapshot["items"][0]["payload"] = json!({ "beams": 3 });
    snapshot["requests"][0]["payload"] = json!({ "kind": "telepathy", "depth": 2 });
    snapshot["requests"][0]["answer"] = json!({ "kind": "nod", "firmly": true });
    let read: ThreadSnapshot = serde_json::from_str(&snapshot.to_string()).unwrap();
    assert!(matches!(&read.items[0].body, ItemBody::Unknown { kind, .. } if kind == "hologram"));
    assert!(matches!(
        read.requests[0].payload,
        RequestPayload::Unknown(_)
    ));
    assert_eq!(read.requests[0].payload.kind(), None);
    assert!(matches!(read.requests[0].answer, Some(Answer::Unknown(_))));
    assert_eq!(serde_json::to_value(read).unwrap(), snapshot);

    let event = json!({ "event": "thread.glowed", "params": { "threadId": "t", "lumens": 9 } });
    let Inbound::Event(read) = wire::read(&event.to_string()).unwrap() else {
        panic!("not an event");
    };
    assert!(matches!(&read, Event::Unknown { event, .. } if event == "thread.glowed"));
    assert_eq!(serde_json::to_value(read).unwrap(), event);

    let refusal = json!({ "id": 3, "error": { "code": "too_shiny", "message": "No." } });
    let Inbound::Reply {
        outcome: Err(error),
        ..
    } = wire::read(&refusal.to_string()).unwrap()
    else {
        panic!("not a refusal");
    };
    assert_eq!(error.code, ErrorCode::Unknown("too_shiny".to_owned()));
}

#[test]
fn a_line_that_is_neither_a_reply_nor_an_event_is_refused() {
    for line in [
        r#"{"id": 1}"#,
        r#"{"result": {}}"#,
        r#"{"id": 1, "result": {}, "error": {"code": "internal", "message": ""}}"#,
        r#"{"event": "host.notice"}"#,
        r#"{"id": 1, "result": {}, "event": "host.notice", "params": {}}"#,
        "not json",
    ] {
        assert!(wire::read(line).is_err(), "{line}");
    }
}

/// The host's constants, read from its source, which says what they are.
#[test]
fn the_version_and_the_line_limit_are_the_hosts() {
    let path =
        Path::new(env!("KOYO_CHECKOUT")).join("koyo-js/koyo-agent-host/src/protocol/wire.ts");
    let source = std::fs::read_to_string(path).unwrap();
    let constant = |name: &str| -> u64 {
        let prefix = format!("export const {name} = ");
        let line = (source.lines().find_map(|line| line.strip_prefix(&prefix)))
            .unwrap_or_else(|| panic!("wire.ts has no {name}"));
        let product = line.strip_suffix(';').unwrap();
        product
            .split('*')
            .map(|factor| factor.trim().parse::<u64>().unwrap())
            .product()
    };
    assert_eq!(constant("HOST_VERSION"), u64::from(HOST_VERSION));
    assert_eq!(
        constant("MAX_LINE_BYTES"),
        u64::try_from(MAX_LINE_BYTES).unwrap()
    );
}
