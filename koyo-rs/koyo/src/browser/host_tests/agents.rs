//! The browser tools on a page of the real engine, as an agent calls them through app control:
//! a snapshot reaches open shadow roots, same-origin frames and frames of another site, which
//! run apart, and acts land on their elements; the page's console and requests are heard; an
//! agent gives a file input files, and answers the file chooser its click opened.

use std::path::Path;
use std::time::{Duration, Instant};

use gpui_kit::TestAppContext;
use koyo_app_control::ControlError;
use serde_json::{Value, json};

use super::{PATIENCE, listen, pause, prepare};
use crate::browser::host::Browsers;
use crate::control::Caller;
use crate::control::tests::{Rig, controlled, op, send, taken, terminal};
use crate::test_support::{draw, repo, select, terminal_record};
use koyo_term_control::Status;

/// The page at `/top`: a button, a frame of its own origin, a web component whose shadow root
/// holds a button and slots a link, and a frame of another site (`localhost` where the page is
/// `127.0.0.1`) with a button and a field, which tell the page through messages.
fn fixture(head: &str) -> String {
    let path = head.split_whitespace().nth(1).unwrap_or("/");
    let body = match path {
        "/top" => {
            "<title>Frames</title><body style='margin:0'>\
            <button onclick=\"document.title='clicked top'\">Top</button>\
            <iframe src='/same' style='width:300px;height:60px;border:5px solid;padding:3px'></iframe>\
            <x-card><a href='#slotted' onclick=\"document.title='clicked slotted';return false\">Slotted</a></x-card>\
            <iframe id='other' style='display:block;width:300px;height:120px;margin-left:40px;border:7px solid'></iframe>\
            <input type='file' aria-label='Upload' onchange=\"document.title = 'chose ' + [...this.files].map((f) => f.name).join()\">\
            <button onclick=\"document.getElementById('hidden').click()\">Pick</button>\
            <input type='file' id='hidden' multiple style='display:none' onchange=\"document.title = 'picked ' + [...this.files].map((f) => f.name).join()\">\
            <script>\
            customElements.define('x-card', class extends HTMLElement { constructor() { super();\
              const root = this.attachShadow({ mode: 'open' });\
              root.innerHTML = '<button>Shadow</button><slot></slot>';\
              root.querySelector('button').onclick = () => { document.title = 'clicked shadow'; }; } });\
            document.getElementById('other').src = 'http://localhost:' + location.port + '/other';\
            addEventListener('message', (event) => { document.title = event.data; });\
            console.log('top-log');\
            </script></body>"
        }
        "/same" => {
            "<body style='margin:0'><button onclick=\"top.document.title='clicked same'\">Same</button></body>"
        }
        "/other" => {
            "<body style='margin:20px'>\
            <button onclick=\"parent.postMessage('clicked other', '*')\">Other</button>\
            <input aria-label='Field' oninput=\"parent.postMessage('typed ' + this.value, '*')\"></body>"
        }
        _ => {
            return "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .into();
        }
    };
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// `agent`'s call of `operation` with `input`, the window drawn meanwhile, as the engine
/// answers in real time: the app's clock kept with it, for what waits on it (a click on the
/// agent's pointer reaching the point).
fn ask(
    rig: &Rig,
    agent: &Caller,
    (operation, input): (&str, Value),
    cx: &mut TestAppContext,
) -> Result<Value, ControlError> {
    let answered = send(agent, None, op(operation, input), cx);
    let deadline = Instant::now() + PATIENCE;
    let step = Duration::from_millis(20);
    loop {
        cx.run_until_parked();
        draw(rig.window, cx);
        if let Some(answer) = taken(&answered) {
            return answer;
        }
        assert!(
            Instant::now() < deadline,
            "{operation} had no answer after {PATIENCE:?}"
        );
        pause(step);
        cx.executor().advance_clock(step);
    }
}

/// Asks until `done` holds for the answer, [`PATIENCE`] at most: the answer it held for.
fn ask_until(
    rig: &Rig,
    agent: &Caller,
    (operation, input): (&str, Value),
    cx: &mut TestAppContext,
    done: impl Fn(&Value) -> bool,
) -> Value {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let answer = ask(rig, agent, (operation, input.clone()), cx).expect(operation);
        if done(&answer) {
            return answer;
        }
        assert!(
            Instant::now() < deadline,
            "{operation} never answered as waited for: {answer}"
        );
        pause(Duration::from_millis(100));
    }
}

/// The ref the snapshot `tree` marks the element of the line that starts `line` with.
fn ref_of(tree: &str, line: &str) -> String {
    let found = (tree.lines()).find(|said| said.trim_start().starts_with(line));
    let found = found.unwrap_or_else(|| panic!("no {line:?} in\n{tree}"));
    let (_, after) = found.split_once("[ref=").expect("a ref");
    after.trim_end_matches(']').to_owned()
}

/// Stops the host, which must quit when asked, and drops the test's scratch.
fn stop(cx: &mut TestAppContext, scratch: std::path::PathBuf) {
    let browsers = cx.update(Browsers::global);
    if let Some(connection) = browsers.update(cx, |browsers, _| browsers.stop()) {
        connection.shutdown(Duration::from_secs(5));
    }
    let _ = std::fs::remove_dir_all(scratch);
}

/// The agent the test's calls are, on the page it opened.
struct OnPage<'a> {
    rig: &'a Rig,
    agent: Caller,
    pane: Value,
}

impl OnPage<'_> {
    /// `operation` on the page, with `more` of its input.
    fn ask(
        &self,
        operation: &str,
        more: Value,
        cx: &mut TestAppContext,
    ) -> Result<Value, ControlError> {
        ask(self.rig, &self.agent, (operation, self.input(more)), cx)
    }

    /// As [`ask_until`], on the page.
    fn until(
        &self,
        operation: &str,
        more: Value,
        cx: &mut TestAppContext,
        done: impl Fn(&Value) -> bool,
    ) -> Value {
        ask_until(
            self.rig,
            &self.agent,
            (operation, self.input(more)),
            cx,
            done,
        )
    }

    fn input(&self, more: Value) -> Value {
        let mut input = json!({ "pane": self.pane });
        if let (Some(input), Value::Object(more)) = (input.as_object_mut(), more) {
            input.extend(more);
        }
        input
    }

    /// Waits until the page's title is `want`.
    fn titled(&self, want: &str, cx: &mut TestAppContext) {
        self.until(
            "browser_eval",
            json!({ "expression": "document.title" }),
            cx,
            |answer| answer["value"] == want,
        );
    }
}

/// The snapshot has every element, the frame of another site's under its line.
fn snapshot_reaches_every_element(page: &OnPage<'_>, cx: &mut TestAppContext) {
    let snapshot = page.until("browser_snapshot", json!({}), cx, |answer| {
        answer["tree"]
            .as_str()
            .is_some_and(|tree| tree.contains("\"Other\""))
    });
    let tree = snapshot["tree"].as_str().expect("a tree").to_owned();
    for (line, reference) in [
        ("- button \"Top\"", "e1"),
        ("- button \"Same\"", "e2"),
        ("- button \"Shadow\"", "e3"),
        ("- link \"Slotted\"", "e4"),
        ("- button \"Other\"", "f1e1"),
        ("- textbox \"Field\"", "f1e2"),
        ("- button \"Upload\"", "e5"),
        ("- button \"Pick\"", "e6"),
    ] {
        assert_eq!(ref_of(&tree, line), reference, "{line}");
    }
    let frame_line = (tree.lines())
        .position(|line| line.ends_with("[frame=f1]"))
        .expect("the other site's frame");
    let other_line = (tree.lines())
        .position(|line| line.contains("\"Other\""))
        .expect("its button");
    assert_eq!(
        other_line,
        frame_line + 1,
        "the frame's tree is under its line"
    );
}

/// Clicks and typing land on the elements of shadow roots and of every frame.
fn acts_land_in_every_frame(page: &OnPage<'_>, cx: &mut TestAppContext) {
    for (reference, clicked) in [
        ("e3", "clicked shadow"),
        ("e2", "clicked same"),
        ("e4", "clicked slotted"),
        ("f1e1", "clicked other"),
    ] {
        page.ask("browser_click", json!({ "ref": reference }), cx)
            .expect("the click lands");
        page.titled(clicked, cx);
    }
    page.ask("browser_type", json!({ "ref": "f1e2", "text": "hi" }), cx)
        .expect("typed");
    page.titled("typed hi", cx);
}

/// Files set on an input, and given to the chooser the agent's click opened.
fn files_reach_the_page(page: &OnPage<'_>, root: &Path, cx: &mut TestAppContext) {
    page.ask(
        "browser_upload",
        json!({ "ref": "e5", "paths": ["shot.txt"] }),
        cx,
    )
    .expect("set on the input");
    page.titled("chose shot.txt", cx);
    page.ask("browser_click", json!({ "ref": "e6" }), cx)
        .expect("the click lands");
    let listed = ask_until(
        page.rig,
        &page.agent,
        ("browser_list", json!({})),
        cx,
        |list| list["pages"][0]["dialog"]["kind"] == "fileChooser",
    );
    assert_eq!(
        listed["pages"][0]["dialog"]["message"],
        "The page asks for files."
    );
    let chosen = json!({ "paths": [root.join("shot.txt"), "README.md"] });
    let answered = page
        .ask("browser_upload", chosen, cx)
        .expect("the chooser answered");
    assert_eq!(answered.get("dialog"), None, "nothing asked any more");
    page.titled("picked shot.txt,README.md", cx);
}

/// The console has what the page logged before the first call; the requests, what it made after.
fn the_page_is_heard(page: &OnPage<'_>, port: u16, cx: &mut TestAppContext) {
    let console = page
        .ask("browser_console", json!({}), cx)
        .expect("the console");
    let logged = (console["messages"].as_array().into_iter().flatten())
        .any(|message| message["text"] == "top-log");
    assert!(
        logged,
        "the log before the first call, as the runtime replays it: {console}"
    );

    let none = page
        .ask("browser_network", json!({}), cx)
        .expect("the requests");
    assert_eq!(none["requests"], json!([]), "recorded from the first call");
    let fetch = json!({ "expression": "fetch('/missing').then((answer) => answer.status)" });
    assert_eq!(
        page.ask("browser_eval", fetch, cx).expect("fetched")["value"],
        json!(404)
    );
    let requests = page.until("browser_network", json!({}), cx, |answer| {
        (answer["requests"].as_array().into_iter().flatten())
            .any(|request| request["finished"] == json!(true))
    });
    let request = &requests["requests"][0];
    assert_eq!(
        (
            &request["method"],
            &request["url"],
            &request["status"],
            &request["resourceType"]
        ),
        (
            &json!("GET"),
            &json!(format!("http://127.0.0.1:{port}/missing")),
            &json!(404),
            &json!("Fetch")
        )
    );
}

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn agents_reach_shadow_roots_and_frames_of_every_site_and_hear_the_page(cx: &mut TestAppContext) {
    let scratch = prepare("agent-frames", cx);
    let port = listen(fixture);
    let root = repo(
        "agent-frames",
        &[("README.md", "hi\n"), ("shot.txt", "pixels")],
    );
    let rig = controlled("agent-frames", &[&root], cx);
    select(&rig.shell, &root, cx);
    let at = root.to_string_lossy().into_owned();
    let record = terminal_record("term_1", &at, Status::Idle, "prompt");
    rig.shell.update(cx, |shell, _| shell.terminals.heard_of(&record));
    let agent = Caller {
        worktree: Some(root.clone()),
        ..terminal("term_1")
    };
    let opening =
        json!({ "kind": "browser", "url": format!("http://127.0.0.1:{port}/top"), "select": true });
    let pane =
        ask(&rig, &agent, ("tab_open", opening), cx).expect("the page opens")["pane"].clone();
    let on_page = OnPage {
        rig: &rig,
        agent,
        pane,
    };
    snapshot_reaches_every_element(&on_page, cx);
    acts_land_in_every_frame(&on_page, cx);
    files_reach_the_page(&on_page, &root, cx);
    the_page_is_heard(&on_page, port, cx);
    stop(cx, scratch);
}
