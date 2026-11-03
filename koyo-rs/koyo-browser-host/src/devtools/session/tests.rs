use super::*;

impl Session {
    fn said(&mut self, text: &str) -> Routed {
        self.page_said(text, object(text).expect("an object"))
    }
}

fn json(text: &str) -> serde_json::Value {
    serde_json::from_str(text).expect("json")
}

/// A session through finding and attaching to the page, with what the
/// frontend sent meanwhile.
fn open(session: &mut Session, early: &[&str]) -> Vec<String> {
    let find = json(&session.begin());
    assert_eq!(find["method"], "Target.getTargetInfo");
    for text in early {
        assert!(
            session.frontend_said(text).is_empty(),
            "held until the session is open"
        );
    }
    let routed = session.said(
        &json!({"id": find["id"], "result": {"targetInfo": {"targetId": "PAGE"}}}).to_string(),
    );
    assert!(routed.to_frontend.is_empty());
    let attach = json(&routed.to_page[0]);
    assert_eq!(attach["method"], "Target.attachToTarget");
    assert_eq!(
        attach["params"],
        json!({"targetId": "PAGE", "flatten": true})
    );
    let routed =
        session.said(&json!({"id": attach["id"], "result": {"sessionId": "OWN"}}).to_string());
    assert!(routed.to_frontend.is_empty());
    routed.to_page
}

use serde_json::json;

#[test]
fn each_frontend_talks_in_a_session_of_its_own_and_ends_it() {
    let mut session = Session::default();
    assert!(session.end().is_none(), "nothing to end before it opens");
    let sent = open(&mut session, &[r#"{"id":1,"method":"Runtime.enable"}"#]);
    assert_eq!(sent.len(), 1, "what the frontend sent early goes once open");
    assert_eq!(
        json(&sent[0]),
        json!({"id": 1, "method": "Runtime.enable", "sessionId": "OWN"})
    );

    // The frontend's session answers it as if it were the page's own.
    let answer = session.said(r#"{"id":1,"result":{},"sessionId":"OWN"}"#);
    assert_eq!(answer.to_frontend, [r#"{"id":1,"result":{}}"#]);
    let event = session
        .said(r#"{"sessionId":"OWN","method":"Runtime.consoleAPICalled","params":{"type":"log"}}"#);
    assert_eq!(
        json(&event.to_frontend[0]),
        json!({"method": "Runtime.consoleAPICalled", "params": {"type": "log"}})
    );
    // The root session and other clients' sessions are not the frontend's.
    assert!(
        session
            .said(r#"{"id":1,"result":{}}"#)
            .to_frontend
            .is_empty()
    );
    assert!(
        session
            .said(r#"{"id":1,"result":{},"sessionId":"MCP"}"#)
            .to_frontend
            .is_empty()
    );

    // Frames attached under the frontend's session reach it as they are.
    session.said(
        r#"{"sessionId":"OWN","method":"Target.attachedToTarget","params":{"sessionId":"FRAME"}}"#,
    );
    let child = r#"{"sessionId":"FRAME","method":"Runtime.executionContextCreated","params":{}}"#;
    assert_eq!(session.said(child).to_frontend, [child]);
    let to_frame = r#"{"id":9,"method":"Runtime.enable","sessionId":"FRAME"}"#;
    assert_eq!(session.frontend_said(to_frame), [to_frame]);
    session.said(
        r#"{"sessionId":"OWN","method":"Target.detachedFromTarget","params":{"sessionId":"FRAME"}}"#,
    );
    assert!(session.said(child).to_frontend.is_empty());

    let end = json(&session.end().expect("an open session ends"));
    assert_eq!(end["method"], "Target.detachFromTarget");
    assert_eq!(end["params"], json!({"sessionId": "OWN"}));
    assert!(end["id"].as_i64().expect("an id") >= HOST_IDS);
}

#[test]
fn without_a_session_of_its_own_the_frontend_shares_the_root_one() {
    let mut session = Session::default();
    let find = json(&session.begin());
    assert!(
        session
            .frontend_said(r#"{"id":1,"method":"Page.enable"}"#)
            .is_empty()
    );
    let routed = session
        .said(&json!({"id": find["id"], "error": {"code": -32000, "message": "no"}}).to_string());
    assert_eq!(routed.to_page, [r#"{"id":1,"method":"Page.enable"}"#]);
    assert_eq!(
        session.said(r#"{"id":1,"result":{}}"#).to_frontend,
        [r#"{"id":1,"result":{}}"#]
    );
    assert!(session.end().is_none());
}

#[test]
fn revealing_an_element_waits_for_the_dom_and_asks_in_the_frontends_session() {
    let mut session = Session::default();
    open(&mut session, &[]);
    assert!(
        session.inspect(Point { x: 10., y: 20.4 }).is_none(),
        "nothing to look up before the DOM domain is on"
    );
    let sent = session.frontend_said(r#"{"id":2,"method":"DOM.enable"}"#);
    assert_eq!(sent.len(), 2);
    let lookup = json(&sent[1]);
    assert_eq!(lookup["method"], "DOM.getNodeForLocation");
    assert_eq!(lookup["params"]["x"], 10.);
    assert_eq!(lookup["params"]["y"], 20.);
    assert_eq!(lookup["sessionId"], "OWN");
    let id = lookup["id"].as_i64().expect("an id");
    assert!(id >= HOST_IDS);
    let found = session.said(
        &json!({"id": id, "result": {"backendNodeId": 42, "nodeId": 3}, "sessionId": "OWN"})
            .to_string(),
    );
    assert_eq!(
        json(&found.to_frontend[0]),
        json!({"method": "Overlay.inspectNodeRequested", "params": {"backendNodeId": 42}})
    );
    // The host's other answers stay with the host.
    let other = json!({"id": id + 1_000, "result": {}, "sessionId": "OWN"}).to_string();
    assert!(session.said(&other).to_frontend.is_empty());
    // Once the DOM is on, a reveal is asked for at once.
    let again = json(&session.inspect(Point { x: 1., y: 2. }).expect("a lookup"));
    assert_eq!(again["sessionId"], "OWN");
}
