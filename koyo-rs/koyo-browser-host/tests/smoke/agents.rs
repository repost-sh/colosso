//! An agent's protocol commands on a page, beside the page's own `DevTools`.

use koyo_browser_ipc::{AppRequest, CdpCaller, HostEvent, Viewport};
use std::time::{Duration, Instant};

use crate::support::{Session, launch};

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn agents_run_protocol_commands_on_the_page_beside_its_dev_tools() {
    let launch = launch("agent");
    let mut session = Session::start(&launch);
    let viewport = Viewport {
        width: 400,
        height: 300,
        scale: 1.,
    };
    let page = "data:text/html,<title>Agent</title><script>console.log('before-the-agent')</script>\
        <body style='margin:0;background:rgb(255,0,0)'><button>Go</button></body>";
    session
        .host
        .send(&AppRequest::Open {
            pane: 1,
            url: page.into(),
            viewport,
            profile: "smoke-agent".into(),
        })
        .expect("open");
    cdp(
        &mut session,
        1,
        "Runtime.evaluate",
        serde_json::json!({"expression": "document.title", "returnByValue": true}),
    );
    let (result, error) = reply(&mut session, 1);
    assert_eq!(error, None);
    assert_eq!(result.expect("a result")["result"]["value"], "Agent");

    session
        .host
        .send(&AppRequest::OpenDevTools {
            pane: 1,
            devtools: 2,
            viewport,
            inspect: None,
            dock: koyo_browser_ipc::DevToolsDock::Bottom,
        })
        .expect("open devtools");
    session.wait("the inspector to attach to the page", |event| {
        matches!(event, HostEvent::Page { pane: 2, state } if state.title.starts_with("DevTools - "))
    });
    cdp(&mut session, 2, "No.such", serde_json::json!({}));
    cdp(&mut session, 3, "Runtime.enable", serde_json::Value::Null);
    let (mut failed, mut enabled, mut logged) = (None, false, false);
    session.wait(
        "the bad command, Runtime.enable and the earlier log",
        |event| {
            match event {
                HostEvent::CdpReply { call: 2, error, .. } => failed = error.clone(),
                HostEvent::CdpReply {
                    call: 3,
                    error: None,
                    ..
                } => enabled = true,
                HostEvent::CdpEvent { method, params, .. }
                    if method == "Runtime.consoleAPICalled" =>
                {
                    logged |= params["args"][0]["value"] == "before-the-agent";
                }
                _ => {}
            }
            failed.is_some() && enabled && logged
        },
    );
    assert!(failed.expect("an error").contains("wasn't found"));

    cdp(
        &mut session,
        4,
        "Runtime.evaluate",
        serde_json::json!({"expression": "new Promise(() => {})", "awaitPromise": true}),
    );
    session
        .host
        .send(&AppRequest::Close { pane: 1 })
        .expect("close");
    let (_, error) = reply(&mut session, 4);
    assert!(error.is_some());
    session.host.shutdown(Duration::from_secs(10));
    let _ = std::fs::remove_dir_all(launch.profiles.parent().expect("scratch"));
}

/// The page's events in its own session reach the app only for an agent that enabled their
/// domain: not for the app's own commands, even one that enables a domain, nor for what open
/// `DevTools` enable there.
#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_pages_events_reach_the_app_only_for_the_domains_an_agent_enabled() {
    let launch = launch("agent-events");
    let mut session = Session::start(&launch);
    let page = "data:text/html,<title>Ticks</title>\
        <script>setInterval(() => console.log('tick'), 100)</script><body>ticking</body>";
    crate::support::open(&mut session, page, "Ticks");
    let app = CdpCaller::App;
    command(&mut session, 1, app, "Runtime.enable", serde_json::Value::Null);
    let (_, error) = reply(&mut session, 1);
    assert_eq!(error, None);
    session
        .host
        .send(&AppRequest::OpenDevTools {
            pane: 1,
            devtools: 2,
            viewport: crate::support::VIEWPORT,
            inspect: None,
            dock: koyo_browser_ipc::DevToolsDock::Bottom,
        })
        .expect("open devtools");
    let mut heard = Vec::new();
    session.pump(Instant::now() + Duration::from_secs(4), |event| {
        if let HostEvent::CdpEvent { method, .. } = event {
            heard.push(method.clone());
        }
        false
    });
    assert_eq!(heard, Vec::<String>::new(), "events no agent enabled");

    cdp(&mut session, 2, "Runtime.enable", serde_json::Value::Null);
    let mut ticked = false;
    session.wait("a tick, once an agent enabled the page's runtime", |event| {
        if let HostEvent::CdpEvent { method, params, .. } = event {
            heard.push(method.clone());
            ticked |= method == "Runtime.consoleAPICalled" && params["args"][0]["value"] == "tick";
        }
        ticked
    });
    let others: Vec<&String> = heard.iter().filter(|method| !method.starts_with("Runtime.")).collect();
    assert_eq!(others, Vec::<&String>::new(), "only the domain the agent enabled");
    crate::support::close(session, &launch);
}

/// Sends the agent command `call` on pane 1.
pub fn cdp(session: &mut Session, call: u64, method: &str, params: serde_json::Value) {
    command(session, call, CdpCaller::Agent, method, params);
}

/// Sends `caller`'s command `call` on pane 1.
fn command(
    session: &mut Session,
    call: u64,
    caller: CdpCaller,
    method: &str,
    params: serde_json::Value,
) {
    session
        .host
        .send(&AppRequest::Cdp {
            pane: 1,
            call,
            caller,
            session: None,
            method: method.into(),
            params,
        })
        .expect("cdp");
}

/// The host's reply to the agent command `call`: its result, or its error.
pub fn reply(session: &mut Session, call: u64) -> (Option<serde_json::Value>, Option<String>) {
    let mut answer = None;
    session.wait("the agent's answer", |event| {
        if let HostEvent::CdpReply {
            call: answered,
            result,
            error,
            ..
        } = event
            && *answered == call
        {
            answer = Some((result.clone(), error.clone()));
            return true;
        }
        false
    });
    answer.expect("answered")
}
