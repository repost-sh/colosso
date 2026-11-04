//! The app's client: against a stand-in host for what a real one can't be made to do on cue
//! (another version, an answer held back), and against the compiled host on the fake provider
//! for the whole of a turn, a killed host included.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test fails loudly at the step that broke"
)]

mod support;

use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt as _;
use futures::channel::mpsc::{TryRecvError, UnboundedReceiver};
use futures::executor::block_on;
use koyo_agent::agent_client::{AgentClient, Notice};
use koyo_agent::client::{self, Client, ClientError};
use koyo_agent::protocol::events::Event;
use koyo_agent::protocol::ids::{MessageId, ThreadId};
use koyo_agent::protocol::methods::{
    ClientKind, CreateParams, Delivery, Empty, OpenParams, SendParams, ThreadParams, ThreadsCreate,
    ThreadsOpen, ThreadsRead, ThreadsWatch, TurnsSend,
};
use koyo_agent::protocol::model::{ItemBody, Prompt, RunStatus};
use koyo_agent::protocol::wire::HOST_VERSION;
use koyo_agent::replica::Replica;
use koyo_daemon::DaemonDir;
use serde_json::json;
use support::{Host, scratch, stand_in, stand_in_for};

/// The executable `script/agent-host` builds, which `script/check` runs before these tests.
fn compiled_host() -> PathBuf {
    let host = PathBuf::from(env!("KOYO_CHECKOUT"))
        .join("koyo-js/koyo-agent-host/dist/koyo-agent-host");
    assert!(
        host.is_file(),
        "{} isn't there: run script/agent-host first",
        host.display()
    );
    host
}

/// A client of a real host in `dir`, on the fake provider.
fn real(dir: &DaemonDir) -> Arc<AgentClient> {
    let (program, folder) = (compiled_host(), dir.clone());
    AgentClient::new(dir.clone(), move || {
        let mut command = client::host_command(&program, &folder);
        command.env("KOYO_AGENT_PROVIDER", "fake");
        command
    })
    .unwrap()
}

/// A client that fails loudly should it have to start a host.
fn never_starting(dir: &DaemonDir) -> Arc<AgentClient> {
    AgentClient::new(dir.clone(), || Command::new("/nonexistent/koyo-agent-host")).unwrap()
}

fn read_params() -> ThreadParams {
    ThreadParams {
        thread_id: ThreadId("0193a2f0-6c1e-7b3a-9f21-4d5e6f708192".into()),
    }
}

/// A new chat in `cwd`, as the app makes one, on the host's first provider.
fn create(cwd: &str) -> CreateParams {
    CreateParams {
        cwd: cwd.into(),
        provider: None,
        model: None,
        effort: None,
        fast: false,
        permission: None,
        plan: false,
    }
}

fn send(thread_id: ThreadId, text: &str) -> SendParams {
    SendParams {
        thread_id,
        prompt: Prompt {
            message_id: MessageId::fresh(),
            text: text.into(),
            attachments: Vec::new(),
        },
        delivery: Delivery::Auto,
    }
}

/// Takes in what the host says of a thread until `done` says so, failing after a while.
fn follow(
    inbox: &mut UnboundedReceiver<Notice>,
    replica: &mut Replica,
    done: impl Fn(&Replica) -> bool,
) {
    let limit = Instant::now() + Duration::from_secs(20);
    while !done(replica) {
        assert!(Instant::now() < limit, "gave up waiting: {replica:#?}");
        match inbox.try_recv() {
            Ok(Notice::Event(event)) => match *event {
                Event::ThreadChanged(changed) => replica.apply(*changed),
                Event::ItemAppended(appended) => assert!(replica.append(&appended)),
                _ => {}
            },
            Ok(Notice::Disconnected) => panic!("disconnected while following a turn"),
            Err(TryRecvError::Closed) => panic!("the notices ended"),
            Err(TryRecvError::Empty) => std::thread::park_timeout(Duration::from_millis(1)),
        }
    }
}

fn answer(replica: &Replica) -> Option<String> {
    replica.items.iter().find_map(|item| match &item.body {
        ItemBody::AssistantText(text) => Some(text.text.clone()),
        _ => None,
    })
}

fn settled(replica: &Replica) -> bool {
    replica
        .runs
        .iter()
        .any(|run| run.status == RunStatus::Completed)
}

/// Sends the host in `dir` a signal.
fn signal_host(dir: &DaemonDir, signal: &str) {
    let stream = UnixStream::connect(dir.socket()).unwrap();
    let pid = Client::over(stream, ClientKind::Test).unwrap().hello.pid;
    (Command::new("kill").args([signal, &pid.to_string()]).status()).unwrap();
}

/// Waits for the next `Disconnected`, skipping events.
fn disconnected(inbox: &mut UnboundedReceiver<Notice>) {
    block_on(async {
        while let Some(notice) = inbox.next().await {
            if matches!(notice, Notice::Disconnected) {
                return;
            }
        }
        panic!("the notices ended before the disconnection");
    });
}

#[test]
fn a_turn_runs_end_to_end_and_a_killed_host_comes_back_with_what_it_kept() {
    let dir = scratch();
    let client = real(&dir);
    let mut inbox = client.subscribe();

    let thread = block_on(client.ordered::<ThreadsCreate>(create("/work/koyo"))).unwrap();
    assert_eq!(thread.provider, "fake");
    assert!(client::has_store(&dir));
    let id = thread.id;
    let open = OpenParams {
        thread_id: id.clone(),
        since_seq: None,
    };
    let mut replica = Replica::new(block_on(client.ordered::<ThreadsOpen>(open)).unwrap());
    block_on(client.ordered::<TurnsSend>(send(id.clone(), "Is it on?"))).unwrap();
    follow(&mut inbox, &mut replica, settled);
    assert_eq!(answer(&replica).as_deref(), Some("You said: Is it on?"));

    // Killed, its lock goes and its socket stays, which the next host takes over.
    signal_host(&dir, "-9");
    disconnected(&mut inbox);

    // A request connects again, which starts a host, on the same chats.
    let listed = client.request::<ThreadsWatch>(&Empty {}).unwrap();
    assert!(listed.iter().any(|summary| summary.id == id));
    let reopen = OpenParams {
        thread_id: id.clone(),
        since_seq: Some(replica.seq()),
    };
    replica.catch_up(block_on(client.ordered::<ThreadsOpen>(reopen)).unwrap());
    block_on(client.ordered::<TurnsSend>(send(id, "And now?"))).unwrap();
    let said = |replica: &Replica| {
        replica.items.iter().any(|item| {
            matches!(&item.body, ItemBody::AssistantText(text) if text.text == "You said: And now?")
        })
    };
    follow(&mut inbox, &mut replica, said);

    client.replace_host().unwrap();
}

/// Blocks every signal on this thread, as on the app's background threads, which are GCD's.
fn block_signals() {
    // SAFETY: the set is initialised by `sigfillset` before it's read.
    unsafe {
        let mut all = std::mem::zeroed::<libc::sigset_t>();
        libc::sigfillset(&raw mut all);
        libc::pthread_sigmask(libc::SIG_SETMASK, &raw const all, std::ptr::null_mut());
    }
}

#[test]
fn a_host_started_from_a_thread_with_signals_blocked_still_stops_on_a_signal() {
    let dir = scratch();
    let client = real(&dir);
    block_signals();
    client.request::<ThreadsWatch>(&Empty {}).unwrap();
    signal_host(&dir, "-TERM");
    let limit = Instant::now() + Duration::from_secs(5);
    while UnixStream::connect(dir.socket()).is_ok() {
        assert!(Instant::now() < limit, "the host didn't stop on SIGTERM");
        std::thread::park_timeout(Duration::from_millis(20));
    }
}

#[test]
fn what_is_asked_in_order_reaches_the_host_in_order_though_nobody_awaits_it() {
    let dir = scratch();
    let host = stand_in(&dir, |mut host| {
        host.greet("app", HOST_VERSION);
        for expected in 0..8 {
            let read = host.request();
            assert_eq!(read["method"], "threads.read");
            assert_eq!(read["params"]["threadId"], format!("t{expected}"));
            host.send(&json!({ "id": read["id"], "result": {} }));
        }
    });
    let client = never_starting(&dir);
    let answers: Vec<_> = (0..8)
        .map(|index| {
            client.ordered::<ThreadsRead>(ThreadParams {
                thread_id: ThreadId(format!("t{index}")),
            })
        })
        .collect();
    host.join().unwrap();
    for answer in answers {
        block_on(answer).unwrap();
    }
}

#[test]
fn a_host_of_another_build_that_is_idle_is_replaced_without_asking() {
    let dir = scratch();
    let old = stand_in_for(
        &dir,
        vec![
            Box::new(|mut host: Host| host.greet("app", HOST_VERSION + 1)),
            Box::new(|mut host: Host| {
                host.greet("app", HOST_VERSION + 1);
                host.answer_shutdown();
            }),
        ],
    );
    let client = real(&dir);
    let listed = client.request::<ThreadsWatch>(&Empty {}).unwrap();
    assert!(listed.is_empty());
    old.join().unwrap();
    client.replace_host().unwrap();
}

#[test]
fn a_host_of_another_build_that_is_busy_waits_for_the_person() {
    let dir = scratch();
    let host = stand_in_for(
        &dir,
        vec![
            Box::new(|mut host: Host| host.greet_as("app", HOST_VERSION + 1, true)),
            Box::new(|mut host: Host| {
                host.greet("app", HOST_VERSION + 1);
                host.answer_shutdown();
            }),
        ],
    );
    let client = never_starting(&dir);
    let refused = client.request::<ThreadsRead>(&read_params());
    assert!(matches!(refused, Err(ClientError::OtherBuild)), "{refused:?}");

    // The person chose to restart the chats: it's stopped, and nothing listens.
    client.replace_host().unwrap();
    host.join().unwrap();
    assert!(UnixStream::connect(dir.socket()).is_err());
}

#[test]
fn word_of_a_connection_ending_comes_before_anything_from_the_next() {
    let dir = scratch();
    let host = stand_in_for(
        &dir,
        vec![
            Box::new(|mut host: Host| host.greet("app", HOST_VERSION)),
            Box::new(|mut host: Host| {
                host.greet("app", HOST_VERSION);
                let notice = json!({ "level": "info", "message": "the second connection" });
                host.send(&json!({ "event": "host.notice", "params": notice }));
                let read = host.request();
                host.send(&json!({ "id": read["id"], "result": {} }));
            }),
        ],
    );
    let client = never_starting(&dir);
    let mut inbox = client.subscribe();
    // The first connection hangs up as soon as it has answered the hello.
    let first = client.request::<ThreadsRead>(&read_params());
    assert!(
        matches!(first, Err(ClientError::Closed | ClientError::Io(_))),
        "{first:?}"
    );
    client.request::<ThreadsRead>(&read_params()).unwrap();
    host.join().unwrap();

    block_on(async {
        assert!(matches!(inbox.next().await, Some(Notice::Disconnected)));
        match inbox.next().await {
            Some(Notice::Event(event)) => {
                assert!(matches!(*event, Event::HostNotice(_)), "{event:?}");
            }
            other => panic!("expected the second connection's event: {other:?}"),
        }
    });
}
