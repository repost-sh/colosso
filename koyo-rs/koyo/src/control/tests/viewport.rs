//! `browser_viewport` as an agent calls it: a page shown as a phone and back, listed with its
//! viewport, the arguments no page takes refused, and the answer waiting, 5 seconds at
//! most, for the page to lay out at its new size, which a test's page, with no engine to
//! paint it, never does.

use std::time::Duration;

use koyo_app_control::{ControlError, ErrorCode};
use gpui_kit::TestAppContext;
use serde_json::{Value, json};

use super::{Caller, call, controlled, op, send, taken, terminal};
use crate::test_support::{draw, repo, select};

/// Makes `caller`'s call and runs the clock until it's answered, as a call that waits on
/// the page does.
fn call_waiting(
    caller: &Caller,
    input: Value,
    cx: &mut TestAppContext,
) -> Result<Value, ControlError> {
    let answered = send(caller, None, op("browser_viewport", input), cx);
    for _ in 0..80 {
        cx.run_until_parked();
        if let Some(answer) = taken(&answered) {
            return answer;
        }
        cx.executor().advance_clock(Duration::from_millis(100));
    }
    panic!("browser_viewport never answered");
}

#[gpui_kit::test]
fn an_agent_shows_a_page_as_a_phone_and_back(cx: &mut TestAppContext) {
    let root = repo("control-viewport", &[("README.md", "hi\n")]);
    let rig = controlled("control-viewport", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = terminal("term_1");
    let browser = json!({ "kind": "browser", "url": "https://example.com/" });
    let pane = call(&agent, "tab_open", browser, cx).unwrap()["pane"].clone();
    draw(rig.window, cx);
    let listed = call(&agent, "browser_list", json!({}), cx).unwrap();
    assert_eq!(listed["pages"][0]["viewport"]["mode"], "fill");
    assert!(
        listed["pages"][0]["viewport"]["width"].as_u64().unwrap() > 0,
        "the pane's size"
    );

    let phone = json!({ "pane": pane, "device": "iphone-15" });
    let set = call_waiting(&agent, phone, cx).unwrap();
    let viewport = &set["viewport"];
    assert_eq!(viewport["mode"], "device");
    assert_eq!(viewport["device"], "iphone-15");
    assert_eq!(
        (viewport["width"].clone(), viewport["height"].clone()),
        (json!(393), json!(852))
    );
    assert_eq!(viewport["scale"], 3.0);
    assert_eq!(
        (viewport["mobile"].clone(), viewport["touch"].clone()),
        (json!(true), json!(true))
    );
    assert!(viewport["userAgent"].as_str().unwrap().contains("iPhone"));
    assert_eq!(viewport["orientation"], "portrait");
    assert_eq!(viewport["zoom"], "fit");
    assert_eq!(set["settled"], false, "no engine paints a test's page");
    assert!(set.get("data").is_none(), "no image unless asked for");
    let listed = call(&agent, "browser_list", json!({}), cx).unwrap();
    let listed = &listed["pages"][0];
    assert_eq!(listed["viewport"]["device"], "iphone-15");
    assert_eq!(
        listed["viewport"]["setBy"], listed["controller"],
        "the agent's, which holds the page"
    );

    let turned = json!({ "orientation": "landscape", "zoom": 50 });
    let turned = call_waiting(&agent, turned, cx).unwrap();
    assert_eq!(
        (
            turned["viewport"]["width"].clone(),
            turned["viewport"]["zoom"].clone()
        ),
        (json!(852), json!(50))
    );
    assert_eq!(turned["viewport"]["shownPercent"], 50);

    let free = call_waiting(&agent, json!({ "width": 390 }), cx).unwrap();
    assert_eq!(free["viewport"]["mode"], "responsive");
    assert_eq!(
        (
            free["viewport"]["width"].clone(),
            free["viewport"]["height"].clone()
        ),
        (json!(390), json!(393))
    );
    assert_eq!(free["viewport"]["mobile"], false);

    for wrong in [
        json!({ "device": "iphone-15", "width": 300 }),
        json!({ "fill": true, "device": "pixel-8" }),
        json!({ "zoom": 60 }),
        json!({ "width": 20 }),
        json!({}),
    ] {
        let refused = call_waiting(&agent, wrong.clone(), cx).unwrap_err();
        assert_eq!(
            refused.code,
            ErrorCode::InvalidInput,
            "{wrong}: {}",
            refused.message
        );
    }
    // The image is the page's, as browser_screenshot's: asked of a page no engine shows, it
    // can't be had.
    let shot = call_waiting(&agent, json!({ "width": 400, "screenshot": true }), cx).unwrap_err();
    assert_eq!(shot.code, ErrorCode::BrowserUnavailable, "{}", shot.message);

    let filled = call_waiting(&agent, json!({ "fill": true }), cx).unwrap();
    assert_eq!(filled["viewport"]["mode"], "fill");
    assert!(filled["viewport"].get("setBy").is_none());
}
