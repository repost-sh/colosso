use super::*;

use colosso_term_control::PROTOCOL;

fn output(offset: u64, text: &'static str) -> Stream {
    Stream::Output(Output {
        index: 0,
        offset,
        bytes: text.into(),
    })
}

fn offsets(stream: &Receiver<Stream>) -> Vec<u64> {
    stream
        .try_iter()
        .filter_map(|stream| match stream {
            Stream::Output(output) => Some(output.offset),
            _ => None,
        })
        .collect()
}

#[test]
fn two_views_of_a_terminal_both_hear_it_and_the_last_to_go_detaches() {
    let handle = Handle::generate();
    let mut routes = Routes::default();
    let (first_tx, first) = crossbeam_channel::unbounded();
    routes.claim(0, handle.clone(), first_tx);

    // The second view's snapshot goes out before the daemon answers its attach.
    assert_eq!(routes.expect(&handle), Some(0));
    routes.route(0, output(10, "snapshot"));
    let (second_tx, second) = crossbeam_channel::unbounded();
    routes.claim(0, handle.clone(), second_tx);
    routes.route(0, output(18, "after"));
    assert_eq!(offsets(&first), [10, 18]);
    assert_eq!(offsets(&second), [10, 18]);
    assert_eq!(routes.early_bytes, 0);

    assert!(!routes.release(0), "a view still shows the terminal");
    drop(first);
    routes.route(0, output(23, "still"));
    assert_eq!(offsets(&second), [23]);
    assert!(routes.release(0), "the last view let go");
    routes.route(0, output(28, "late"));
    assert!(offsets(&second).is_empty());
    assert_eq!(routes.early_bytes, 0);
}

#[test]
fn a_failed_attach_keeps_nothing_for_it() {
    let handle = Handle::generate();
    let mut routes = Routes::default();
    let (tx, shown) = crossbeam_channel::unbounded();
    routes.claim(0, handle.clone(), tx);
    let shared = routes.expect(&handle);
    routes.route(0, output(0, "kept"));
    routes.unexpect(shared);
    routes.route(0, output(4, "shown"));
    assert_eq!(offsets(&shown), [0, 4]);
    assert!(routes.early.is_empty());
    assert_eq!(routes.early_bytes, 0);
}

#[test]
fn an_unknown_terminal_expects_nothing() {
    let mut routes = Routes::default();
    assert_eq!(routes.expect(&Handle::generate()), None);
    routes.unexpect(None);
    assert!(routes.expecting.is_empty());
}

#[test]
fn a_client_that_stopped_the_daemon_starts_it_no_more() {
    // No daemon runs in a folder that isn't there, and none is started in it.
    let dir = TermDir::at(
        std::env::current_dir()
            .unwrap()
            .join("target/no-such-daemon"),
    );
    let client = TermClient::new(dir, DaemonCommand::binary("colosso"));
    client.shutdown().unwrap();
    let asked = client.request(RequestBody::ListTerminals {
        worktree: None,
        status: None,
    });
    assert!(matches!(asked, Err(ClientError::Closed)), "{asked:?}");
    assert!(
        !std::env::current_dir()
            .unwrap()
            .join("target/no-such-daemon")
            .exists()
    );
}

#[test]
fn a_daemon_of_another_engine_is_another_builds() {
    let welcome = |engine_commit: &str, snapshot_format| Welcome {
        protocol: PROTOCOL,
        daemon_version: "0.1.0".into(),
        engine_commit: engine_commit.into(),
        snapshot_format,
        client_id: 1,
        actor: colosso_term_control::Actor::system(),
        features: Vec::new(),
    };
    let ours = colosso_term_engine::GHOSTTY_COMMIT;
    assert!(this_build(&welcome(ours, SNAPSHOT_FORMAT)).is_ok());
    let older = this_build(&welcome("0123456789abcdef", SNAPSHOT_FORMAT)).unwrap_err();
    assert!(is_other_build(&older));
    assert!(older.to_string().ends_with("on Ghostty 0123456789ab"));
    assert!(is_other_build(
        &this_build(&welcome(ours, SNAPSHOT_FORMAT + 1)).unwrap_err()
    ));
    assert!(!is_other_build(&ClientError::Closed));
}
