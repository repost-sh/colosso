use serde_json::json;

use super::*;
use crate::{Command, ErrorCode, catalog};

/// Why `input` is refused, by `Command::parse` or by the operation's rules.
fn refused(name: &str, input: &serde_json::Value) -> ControlError {
    let checked = match Command::parse(name, input.clone()) {
        Err(error) => Err(error),
        Ok(Command::DesktopClick(input)) => input.target().and(input.count()).map(drop),
        Ok(Command::DesktopMove(input)) => input.target().map(drop),
        Ok(Command::DesktopDrag(input)) => input.from.target().and(input.to.target()).map(drop),
        Ok(Command::DesktopScroll(input)) => input.target().and(input.amount()).map(drop),
        Ok(Command::DesktopType(input)) => input.typing().map(drop),
        Ok(Command::DesktopClose(input)) => input.closing().map(drop),
        Ok(Command::DesktopScreenshot(input)) => input.scale().map(drop),
        Ok(other) => panic!("{} has no rule here", other.name()),
    };
    let Err(error) = checked else {
        panic!("{name} {input} isn't refused");
    };
    error
}

#[test]
fn an_act_lands_on_a_ref_or_a_point_never_both() {
    assert_eq!(
        target(Some("e3"), None, None).unwrap(),
        Target::Element("e3".into())
    );
    assert_eq!(
        target(None, Some(10.), Some(20.5)).unwrap(),
        Target::Point { x: 10., y: 20.5 }
    );
    for (element, x, y) in [
        (None, None, None),
        (Some("e3"), Some(1.), Some(2.)),
        (Some("e3"), Some(1.), None),
        (None, Some(1.), None),
        (None, None, Some(2.)),
    ] {
        let error = target(element, x, y).unwrap_err();
        assert_eq!(
            error.code,
            ErrorCode::InvalidInput,
            "{element:?} {x:?} {y:?}"
        );
    }
    for (name, input) in [
        ("desktop_click", json!({})),
        ("desktop_click", json!({ "ref": "e1", "x": 1, "y": 2 })),
        ("desktop_click", json!({ "ref": "e1", "count": 4 })),
        ("desktop_click", json!({ "ref": "e1", "count": 0 })),
        ("desktop_move", json!({ "x": 1 })),
        ("desktop_drag", json!({ "from": { "ref": "e1" }, "to": {} })),
        ("desktop_scroll", json!({ "ref": "e1" })),
        ("desktop_scroll", json!({ "ref": "e1", "dx": 0, "dy": 0 })),
        (
            "desktop_click",
            json!({ "ref": "e1", "modifiers": ["meta"] }),
        ),
        ("desktop_drag", json!({ "from": { "ref": "e1" } })),
    ] {
        assert_eq!(
            refused(name, &input).code,
            ErrorCode::InvalidInput,
            "{name} {input}"
        );
    }
}

#[test]
fn a_click_reads_its_button_count_and_modifiers() {
    let Command::DesktopClick(click) = Command::parse(
        "desktop_click",
        json!({ "x": 5, "y": 6, "window": 42, "button": "right", "count": 2, "modifiers": ["cmd", "shift", "alt", "ctrl"] }),
    )
    .unwrap() else {
        panic!("desktop_click parsed as another command");
    };
    assert_eq!(click.target().unwrap(), Target::Point { x: 5., y: 6. });
    assert_eq!(click.count().unwrap(), 2);
    assert_eq!(click.window, Some(42));
    assert_eq!(
        click.modifiers.unwrap(),
        [
            Modifier::Cmd,
            Modifier::Shift,
            Modifier::Alt,
            Modifier::Ctrl
        ]
    );
    let Command::DesktopScroll(scroll) =
        Command::parse("desktop_scroll", json!({ "ref": "e2", "dy": -40 })).unwrap()
    else {
        panic!("desktop_scroll parsed as another command");
    };
    assert_eq!(scroll.amount().unwrap(), (0., -40.));
}

#[test]
fn typing_is_keys_unless_it_replaces_a_named_field() {
    let typing = |input: serde_json::Value| {
        let Command::DesktopType(input) = Command::parse("desktop_type", input).unwrap() else {
            panic!("desktop_type parsed as another command");
        };
        input.typing()
    };
    assert_eq!(
        typing(json!({ "text": "hi\n" })).unwrap(),
        Typing::Keys {
            text: "hi\n".into(),
            into: None
        }
    );
    assert_eq!(
        typing(json!({ "text": "hi", "ref": "e4" })).unwrap(),
        Typing::Keys {
            text: "hi".into(),
            into: Some("e4".into())
        }
    );
    assert_eq!(
        typing(json!({ "text": "hi", "ref": "e4", "replace": true })).unwrap(),
        Typing::SetValue {
            element: "e4".into(),
            text: "hi".into()
        }
    );
    let unnamed = typing(json!({ "text": "hi", "replace": true })).unwrap_err();
    assert_eq!(unnamed.code, ErrorCode::InvalidInput);
    assert!(unnamed.message.contains("`ref`"), "{}", unnamed.message);
    // Long text goes in as a value, not as keys.
    let long = "é".repeat(MAX_TYPED + 1);
    let error = typing(json!({ "text": long })).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert!(error.message.contains("replace"), "{}", error.message);
    assert!(typing(json!({ "text": "é".repeat(MAX_TYPED) })).is_ok());
    assert!(typing(json!({ "text": long, "ref": "e4", "replace": true })).is_ok());
}

#[test]
fn closing_is_a_window_or_an_app() {
    let closing = |input: serde_json::Value| {
        let Command::DesktopClose(input) = Command::parse("desktop_close", input).unwrap() else {
            panic!("desktop_close parsed as another command");
        };
        input.closing()
    };
    assert_eq!(closing(json!({ "window": 7 })).unwrap(), Closing::Window(7));
    assert_eq!(closing(json!({ "pid": 501 })).unwrap(), Closing::App(501));
    for input in [json!({}), json!({ "window": 7, "pid": 501 })] {
        assert_eq!(closing(input).unwrap_err().code, ErrorCode::InvalidInput);
    }
}

#[test]
fn a_screenshot_is_at_scale_1_or_2_and_a_wait_stays_inside_its_bounds() {
    let shot = DesktopScreenshotInput::default();
    assert_eq!(shot.scale().unwrap(), 1);
    for (scale, ok) in [(1, true), (2, true), (0, false), (3, false)] {
        let shot = DesktopScreenshotInput {
            window: None,
            scale: Some(scale),
        };
        assert_eq!(shot.scale().is_ok(), ok, "{scale}");
    }
    let wait = DesktopWaitInput::default();
    assert_eq!((wait.quiet_ms(), wait.timeout_ms()), (500, 10_000));
    let long = DesktopWaitInput {
        quiet_ms: Some(60_000),
        timeout_ms: Some(120_000),
    };
    assert_eq!((long.quiet_ms(), long.timeout_ms()), (10_000, 60_000));
}

#[test]
fn reads_need_read_and_acts_need_desktop() {
    let desktop: Vec<_> = (catalog().into_iter())
        .filter(|descriptor| descriptor.name.starts_with("desktop_"))
        .collect();
    assert_eq!(desktop.len(), 15);
    for descriptor in desktop {
        let reads = [
            "desktop_windows",
            "desktop_screenshot",
            "desktop_snapshot",
            "desktop_wait",
        ]
        .contains(&descriptor.name);
        assert_eq!(descriptor.hints.read_only, reads, "{}", descriptor.name);
        let scope = if reads { Scope::Read } else { Scope::Desktop };
        assert_eq!(descriptor.scope, scope, "{}", descriptor.name);
        assert!(!descriptor.confirm, "{}", descriptor.name);
    }
    assert!(!Scope::DEFAULT_GRANT.contains(&Scope::Desktop));
    assert!(Scope::AGENT.contains(&Scope::Desktop));
    assert_eq!(Scope::Desktop.id(), "desktop");
}

#[test]
fn the_desktop_being_away_is_retryable() {
    let error = ControlError::desktop_unavailable("The agent desktop is starting.");
    assert_eq!(error.code.id(), "desktop_unavailable");
    assert_eq!(
        error.retryable,
        ControlError::browser_unavailable("").retryable
    );
    assert!(error.retryable);
}

#[test]
fn a_copy_answers_what_it_copied() {
    let copied = json!({ "done": true, "copied": "héllo" });
    let text = crate::summarize("desktop_key", &copied).unwrap();
    assert!(text.contains("copying 5 characters"), "{text}");
    let plain = crate::summarize("desktop_key", &json!({ "done": true })).unwrap();
    assert_eq!(plain, "Pressed the keys on the agent desktop");
    assert_eq!(
        serde_json::to_value(DesktopKeyOutput {
            done: true,
            copied: None
        })
        .unwrap(),
        json!({ "done": true })
    );
}
