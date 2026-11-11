//! A stand-in agent host, for what a chat's operations ask of one: it answers each request line
//! by its method as the host does, keeps the chat it was asked to make, and remembers every
//! request, so a test reads what a chat was made with. It runs no agent.

use std::collections::HashMap;
use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui_kit::TestAppContext;
use koyo_agent::agent_client::AgentClient;
use koyo_agent::protocol::ids::{ThreadId, Time};
use koyo_agent::protocol::methods::{Catalog, CreateParams, Model, UpdateParams};
use koyo_agent::protocol::model::{Effort, Mode, Thread, ThreadSummary, TitleSource};
use koyo_agent::protocol::wire::HOST_VERSION;
use koyo_app_control::ControlError;
use koyo_daemon::DaemonDir;
use serde_json::{Value, json};

use super::{Caller, Rig, op, read_shell, send, taken};
use crate::chat::Connection;
use crate::chat::chosen::Settings;

/// The id the stand-in gives the chat it makes.
pub(super) const MADE: &str = "0193a2f0-6c1e-7b3a-9f21-000000000001";
const AT: &str = "2026-11-11T10:00:00.000Z";

/// What the host was asked, request by request, as the stand-in heard it, and the chats it keeps.
#[derive(Clone, Default)]
pub(super) struct Heard {
    asked: Arc<Mutex<Vec<Value>>>,
    chats: Arc<Mutex<Chats>>,
}

impl Heard {
    /// Has the stand-in keep the chat `summary` lists, set as it says, as the host keeps the
    /// chats it lists.
    pub(super) fn keep(&self, summary: &ThreadSummary) {
        let thread = Thread {
            id: summary.id.clone(),
            cwd: summary.cwd.clone(),
            model: summary.model.clone(),
            effort: summary.effort,
            fast: summary.fast,
            permission: summary.permission,
            plan: summary.plan,
            ..thread_of(blank())
        };
        self.chats.lock().unwrap().insert(thread.id.clone(), thread);
    }

    /// The method of every request, in order.
    pub(super) fn methods(&self) -> Vec<String> {
        let heard = self.asked.lock().unwrap();
        (heard.iter())
            .filter_map(|request| request["method"].as_str().map(str::to_owned))
            .collect()
    }

    /// The params of every request of `method`, in order.
    pub(super) fn of(&self, method: &str) -> Vec<Value> {
        let heard = self.asked.lock().unwrap();
        (heard.iter())
            .filter(|request| request["method"] == method)
            .map(|request| request["params"].clone())
            .collect()
    }
}

/// A model of the stand-in's catalog.
fn model(id: &str, runs: &str, label: &str, efforts: &[Effort], auto: bool) -> Model {
    Model {
        id: id.into(),
        name: label.into(),
        label: label.into(),
        runs: Some(runs.into()),
        efforts: efforts.to_vec(),
        fast: id == "opus",
        auto,
    }
}

/// What Claude Code lists for the owner's account: its recommended row runs Fable 5.1, while
/// their settings name `opus`; Koyo's own default mode is left to Claude's, `auto`.
pub(super) fn claude_catalog() -> Catalog {
    let efforts = [
        Effort::Low,
        Effort::Medium,
        Effort::High,
        Effort::Xhigh,
        Effort::Max,
    ];
    Catalog {
        models: vec![
            model("default", "claude-fable-5-1", "Fable 5.1", &efforts, true),
            model("opus", "claude-opus-5-5", "Opus 5.5", &efforts, true),
            model("haiku", "claude-haiku-5-5", "Haiku 5.5", &[], false),
        ],
        commands: Vec::new(),
        default_mode: Some(Mode::Auto),
        default_model: Some("opus".into()),
    }
}

/// Has the shell's chats talk to a stand-in host offering `claude_catalog`, which hears what they
/// ask; its socket is in the test's own scratch folder.
pub(super) fn stand_in(rig: &Rig, cx: &mut TestAppContext) -> Heard {
    let catalog = claude_catalog();
    let root = rig.worktrees.join(".host");
    std::fs::create_dir_all(&root).unwrap();
    let dir = DaemonDir::new(root, "host");
    let listener = koyo_daemon::bind(&dir.socket()).unwrap();
    let heard = Heard::default();
    let hearing = heard.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            let (hearing, catalog) = (hearing.clone(), catalog.clone());
            std::thread::spawn(move || serve(stream, &hearing, &catalog));
        }
    });
    let client = AgentClient::new(dir, || Command::new("/nonexistent/koyo-agent-host")).unwrap();
    let chats = read_shell(rig, cx, |shell, _| shell.chats().clone());
    chats.update(cx, |chats, cx| {
        chats.use_client(client, cx);
        chats.connect(cx);
    });
    // Connected first, so the host's list of chats, none, comes before any a test gives.
    let deadline = Instant::now() + Duration::from_secs(20);
    while chats.read_with(cx, |chats, _| *chats.connection() != Connection::Connected) {
        assert!(
            Instant::now() < deadline,
            "the stand-in host wasn't reached"
        );
        cx.run_until_parked();
        std::thread::park_timeout(Duration::from_millis(5));
    }
    heard
}

/// The chats the stand-in keeps, by id: one it didn't make is made when it's first asked of.
type Chats = HashMap<ThreadId, Thread>;

/// Answers one connection's requests until it ends.
fn serve(stream: UnixStream, heard: &Heard, catalog: &Catalog) {
    let mut writer = stream.try_clone().unwrap();
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { return };
        let request: Value = serde_json::from_str(&line).unwrap();
        heard.asked.lock().unwrap().push(request.clone());
        let result = answer(&request, catalog, &heard.chats);
        let reply = json!({ "id": request["id"], "result": result });
        if writeln!(writer, "{reply}").is_err() {
            return;
        }
    }
}

fn answer(request: &Value, catalog: &Catalog, made: &Mutex<Chats>) -> Value {
    let params = request["params"].clone();
    let mut made = made.lock().unwrap();
    match request["method"].as_str().unwrap_or_default() {
        "hello" => json!({ "version": HOST_VERSION, "pid": 4242, "busy": false }),
        "threads.watch" => json!([]),
        "providers.status" => json!([{
            "provider": "claude", "state": "ready", "executable": null, "version": null,
            "account": null, "message": null,
        }]),
        "catalog" => serde_json::to_value(catalog).unwrap(),
        "threads.create" => {
            let thread = thread_of(serde_json::from_value(params).unwrap());
            let thread = made.entry(thread.id.clone()).insert_entry(thread);
            serde_json::to_value(thread.get()).unwrap()
        }
        "threads.update" => {
            let change: UpdateParams = serde_json::from_value(params).unwrap();
            let thread = kept(&mut made, change.thread_id);
            if let Some(model) = change.model {
                thread.model = model;
            }
            if let Some(effort) = change.effort {
                thread.effort = effort;
            }
            if let Some(permission) = change.permission {
                thread.permission = permission;
            }
            thread.fast = change.fast.unwrap_or(thread.fast);
            thread.plan = change.plan.unwrap_or(thread.plan);
            if let Some(tab_open) = change.tab_open {
                thread.tab_open = tab_open;
            }
            serde_json::to_value(&*thread).unwrap()
        }
        "turns.send" => json!({ "runId": "run-1", "delivered": "started" }),
        "threads.open" => {
            let id = ThreadId(params["threadId"].as_str().unwrap().into());
            let thread = kept(&mut made, id);
            json!({ "thread": thread, "runs": [], "items": [], "requests": [] })
        }
        _ => json!({}),
    }
}

/// The chat `id`, made now if the stand-in hasn't one: a test gave it to the app alone.
fn kept(made: &mut Chats, id: ThreadId) -> &mut Thread {
    made.entry(id.clone()).or_insert_with(|| Thread {
        id,
        ..thread_of(blank())
    })
}

/// A chat in `/` that chose nothing.
fn blank() -> CreateParams {
    Settings::default().to_create(Path::new("/"))
}

fn thread_of(params: CreateParams) -> Thread {
    Thread {
        id: ThreadId(MADE.into()),
        cwd: params.cwd,
        title: "A new chat".into(),
        title_source: TitleSource::Seed,
        provider: "claude".into(),
        model: params.model,
        effort: params.effort,
        fast: params.fast,
        permission: params.permission,
        plan: params.plan,
        session_id: None,
        resume_at: None,
        files_settled_at: None,
        queue_held: false,
        tab_open: false,
        parent: None,
        background_tasks: 0,
        created_at: Time(AT.into()),
        updated_at: Time(AT.into()),
        read_at: None,
        seq: 1,
    }
}

/// Makes `caller`'s call of `operation` with `input`, and waits for its answer, which may take
/// the stand-in host's, on threads of its own: the app's loop runs until it's given.
pub(super) fn call_through_host(
    caller: &Caller,
    operation: &str,
    input: Value,
    cx: &mut TestAppContext,
) -> Result<Value, ControlError> {
    let answered = send(caller, None, op(operation, input), cx);
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        cx.run_until_parked();
        if let Some(answer) = taken(&answered) {
            return answer;
        }
        assert!(Instant::now() < deadline, "{operation} wasn't answered");
        std::thread::park_timeout(Duration::from_millis(5));
    }
}
