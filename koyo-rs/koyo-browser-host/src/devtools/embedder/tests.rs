use super::*;

/// What Chromium's check of a frontend's address (`SanitizeFrontendURL`, which it loads a
/// frontend only at an address it leaves as it is) keeps of what Koyo puts in it: the bridge
/// as given, and `can_dock` as `true`, which its own docked tools are loaded with.
const KEPT: [(&str, Option<&str>); 2] = [("ws", None), ("can_dock", Some("true"))];

#[test]
fn the_frontend_is_loaded_able_to_dock_with_nothing_in_its_address_chromium_drops() {
    let address = address(4242, "0a1b2c");
    assert_eq!(
        address,
        "devtools://devtools/bundled/devtools_app.html?ws=127.0.0.1:4242/0a1b2c&can_dock=true"
    );
    let query = address
        .strip_prefix(FRONTEND)
        .and_then(|rest| rest.strip_prefix('?'))
        .expect("the frontend's page, then its query");
    for parameter in query.split('&') {
        let (key, value) = parameter.split_once('=').expect("a key and a value");
        let kept = KEPT.iter().find(|(name, _)| *name == key);
        assert!(
            kept.is_some(),
            "{key} would be dropped, and the page refused"
        );
        if let Some((_, Some(only))) = kept {
            assert_eq!(value, *only, "{key} would be changed, and the page refused");
        }
        let plain = |c: char| c.is_ascii_alphanumeric() || ".:/_-".contains(c);
        assert!(
            value.chars().all(plain),
            "{value} as it is, nothing to escape"
        );
    }
}

#[test]
fn the_shim_goes_in_the_frontends_page_with_the_side_and_a_binding_to_tell_through() {
    let commands = install(DevToolsDock::Left);
    assert_eq!(
        commands.each_ref().map(|(method, _)| *method),
        ["Runtime.enable", "Runtime.addBinding", "Runtime.evaluate"],
        "the binding before the shim that calls it"
    );
    assert_eq!(commands[1].1["name"], BINDING);
    let call = commands[2].1["expression"].as_str().expect("the shim");
    assert!(call.starts_with("(// The DevTools frontend's embedder shim"));
    for (side, name) in [
        (DevToolsDock::Bottom, "bottom"),
        (DevToolsDock::Left, "left"),
        (DevToolsDock::Right, "right"),
        (DevToolsDock::Undocked, "undocked"),
    ] {
        let commands = install(side);
        let call = commands[2].1["expression"].as_str().expect("the shim");
        assert!(
            call.ends_with(&format!(r#"}}))("{name}", "{BINDING}")"#)),
            "{call}"
        );
        assert!(SHIM.contains(&format!("'{name}'")), "{name}");
    }
}

#[test]
fn the_shim_is_heard_only_through_its_binding_in_the_pages_own_session() {
    let called = |fields: serde_json::Value| {
        let serde_json::Value::Object(fields) = fields else {
            panic!("an object");
        };
        asked(&fields)
    };
    let close = r#"{"op":"close"}"#;
    assert_eq!(
        called(serde_json::json!({
            "method": "Runtime.bindingCalled",
            "params": {"name": BINDING, "payload": close, "executionContextId": 1},
        })),
        Some(DevToolsAsk::Close)
    );
    for other in [
        serde_json::json!({"method": "Runtime.bindingCalled", "params": {"name": "other", "payload": close}}),
        serde_json::json!({"method": "Runtime.bindingCalled", "params": {"name": BINDING, "payload": close}, "sessionId": "S"}),
        serde_json::json!({"method": "Runtime.consoleAPICalled", "params": {"name": BINDING, "payload": close}}),
        serde_json::json!({"method": "Runtime.bindingCalled", "params": {"name": BINDING, "payload": "{}"}}),
        serde_json::json!({"id": 3, "result": {}}),
    ] {
        assert_eq!(called(other.clone()), None, "{other}");
    }
}

#[test]
fn the_shim_names_the_asks_the_app_reads() {
    for op in ["op: 'dock'", "op: 'close'", "op: 'deviceMode'"] {
        assert!(SHIM.contains(op), "{op}");
    }
    assert!(
        SHIM.contains(FRONTEND),
        "it goes in the frontend's page only"
    );
    assert!(
        SHIM.contains("((side, binding) =>"),
        "a function of the side and the binding"
    );
    assert!(
        SHIM.trim_end().ends_with("})"),
        "an expression, to be called"
    );
}
