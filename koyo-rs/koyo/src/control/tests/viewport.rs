//! `browser_viewport` as an agent calls it: a page shown as a phone and back, listed with its
//! viewport, the arguments no page takes refused, and the answer waiting, 5 seconds at
//! most, for the page to lay out at its new size, which a test's page, with no engine to
//! paint it, never does. And the viewport a page an agent opens starts at: the person's size
//! for agents' pages, 1440 × 900 by default, unless `tab_open` or `pane_split` asks for
//! another.

use std::time::Duration;

use gpui_kit::TestAppContext;
use koyo_app_control::{ControlError, ErrorCode};
use serde_json::{Value, json};

use super::{Caller, call, controlled, external, op, read_shell, send, taken, terminal};
use crate::control::PageSize;
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
    // Filling its pane as the person's pages do, which the agent asks for.
    let browser = json!({
        "kind": "browser", "url": "https://example.com/", "viewport": { "fill": true }
    });
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

/// The viewport `browser_list` gives pane `pane`.
fn listed_viewport(agent: &Caller, pane: &Value, cx: &mut TestAppContext) -> Value {
    let listed = call(agent, "browser_list", json!({}), cx).unwrap();
    (listed["pages"].as_array().unwrap().iter())
        .find(|page| page["pane"] == *pane)
        .map(|page| page["viewport"].clone())
        .expect("the page is listed")
}

/// A page opened by `agent` with `input`'s fields: its pane.
fn open_page(agent: &Caller, input: &Value, cx: &mut TestAppContext) -> Value {
    let mut open = json!({ "kind": "browser", "url": "https://example.com/" });
    open.as_object_mut()
        .unwrap()
        .extend(input.as_object().unwrap().clone());
    call(agent, "tab_open", open, cx).unwrap()["pane"].clone()
}

fn size_of(viewport: &Value) -> (Value, Value) {
    (viewport["width"].clone(), viewport["height"].clone())
}

#[gpui_kit::test]
fn a_page_an_agent_opens_starts_at_the_persons_size_for_agents_pages(cx: &mut TestAppContext) {
    let root = repo("control-opening", &[("README.md", "hi\n")]);
    let rig = controlled("control-opening", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = terminal("term_1");

    // Out of sight, as agents' tabs open by default, in responsive mode at 1440 × 900.
    let first = open_page(&agent, &json!({}), cx);
    let viewport = listed_viewport(&agent, &first, cx);
    assert_eq!(viewport["mode"], "responsive");
    assert_eq!(size_of(&viewport), (json!(1440), json!(900)));
    assert_eq!(viewport["zoom"], "fit");
    assert_eq!(viewport["mobile"], false);
    assert_eq!(viewport["setBy"], "koyo app (in terminal term_1)");
    // Shown, it keeps it, scaled down to fit the pane.
    let shown = open_page(&agent, &json!({ "select": true }), cx);
    draw(rig.window, cx);
    let viewport = listed_viewport(&agent, &shown, cx);
    assert_eq!(size_of(&viewport), (json!(1440), json!(900)));
    assert!(
        viewport["shownPercent"].as_u64().unwrap() < 100,
        "{viewport}"
    );

    // What the call asks for instead: a size, a side, a preset, or the whole pane.
    let sized = open_page(
        &agent,
        &json!({ "viewport": { "width": 1280, "height": 720 } }),
        cx,
    );
    assert_eq!(
        size_of(&listed_viewport(&agent, &sized, cx)),
        (json!(1280), json!(720))
    );
    let narrow = open_page(&agent, &json!({ "viewport": { "width": 1280 } }), cx);
    assert_eq!(
        size_of(&listed_viewport(&agent, &narrow, cx)),
        (json!(1280), json!(900))
    );
    let phone = open_page(
        &agent,
        &json!({ "viewport": { "device": "iphone-15" } }),
        cx,
    );
    let viewport = listed_viewport(&agent, &phone, cx);
    assert_eq!(
        (viewport["mode"].clone(), viewport["device"].clone()),
        (json!("device"), json!("iphone-15"))
    );
    let split = json!({
        "pane": shown, "edge": "right", "kind": "browser", "url": "https://example.com/",
        "viewport": { "fill": true }
    });
    let filled = call(&agent, "pane_split", split, cx).unwrap()["pane"].clone();
    assert_eq!(listed_viewport(&agent, &filled, cx)["mode"], "fill");
    let split = json!({ "pane": shown, "edge": "bottom", "kind": "browser" });
    let split = call(&agent, "pane_split", split, cx).unwrap()["pane"].clone();
    assert_eq!(
        size_of(&listed_viewport(&agent, &split, cx)),
        (json!(1440), json!(900))
    );
}

#[gpui_kit::test]
fn the_persons_size_for_agents_pages_holds_for_the_next_page_opened(cx: &mut TestAppContext) {
    let root = repo("control-opening-default", &[("README.md", "hi\n")]);
    let rig = controlled("control-opening-default", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = terminal("term_1");
    let first = open_page(&agent, &json!({}), cx);

    // The person's size for the next page an agent opens; pages open already keep theirs. An
    // external client's page starts there too.
    cx.update(|cx| {
        crate::control::set_pages_open_at(
            PageSize::Size {
                width: 1920,
                height: 1080,
            },
            cx,
        );
    });
    let next = open_page(&agent, &json!({}), cx);
    assert_eq!(
        size_of(&listed_viewport(&agent, &next, cx)),
        (json!(1920), json!(1080))
    );
    assert_eq!(
        size_of(&listed_viewport(&agent, &first, cx)),
        (json!(1440), json!(900))
    );
    super::allow("Cursor", &koyo_app_control::Scope::DEFAULT_GRANT, cx);
    let theirs = open_page(&external("Cursor"), &json!({}), cx);
    let viewport = listed_viewport(&agent, &theirs, cx);
    assert_eq!(size_of(&viewport), (json!(1920), json!(1080)));
    assert_eq!(viewport["setBy"], "Cursor");
    cx.update(|cx| crate::control::set_pages_open_at(PageSize::Fill, cx));
    let plain = open_page(&agent, &json!({}), cx);
    assert_eq!(listed_viewport(&agent, &plain, cx)["mode"], "fill");
    let asked = open_page(&agent, &json!({ "viewport": { "width": 1280 } }), cx);
    assert_eq!(
        size_of(&listed_viewport(&agent, &asked, cx)),
        (json!(1280), json!(800))
    );

    // A viewport no page takes is refused before anything opens.
    let tabs = |cx: &TestAppContext| {
        read_shell(&rig, cx, |shell, _| {
            shell.workspace.branch().unwrap().tabs.len()
        })
    };
    let before = tabs(cx);
    for wrong in [
        json!({ "kind": "browser", "viewport": {} }),
        json!({ "kind": "browser", "viewport": { "fill": true, "width": 300 } }),
        json!({ "kind": "browser", "viewport": { "width": 20 } }),
        json!({ "kind": "browser", "viewport": { "device": "pixel-8", "height": 500 } }),
        json!({ "kind": "chat", "viewport": { "fill": true } }),
    ] {
        let refused = call(&agent, "tab_open", wrong.clone(), cx).unwrap_err();
        assert_eq!(
            refused.code,
            ErrorCode::InvalidInput,
            "{wrong}: {}",
            refused.message
        );
    }
    assert_eq!(tabs(cx), before);
}
