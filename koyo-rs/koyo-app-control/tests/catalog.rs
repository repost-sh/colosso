//! The catalog as relays publish it: names, scopes, hints and schemas that hold to
//! `docs/app-control.md` 3 to 5, and commands that read the inputs the schemas describe.

use std::collections::BTreeSet;

use koyo_app_control::ops::browser::OpenViewport;
use koyo_app_control::ops::pane::SplitSource;
use koyo_app_control::ops::settings::{BranchPrefix, SettingsSetInput};
use koyo_app_control::ops::tab::CloseInput;
use koyo_app_control::{
    APP_LAUNCH, Command, ErrorCode, REQUEST_ID, REQUEST_ID_PATTERN, Scope, Topic, catalog, info,
    infos, summarize, valid_tool_name,
};
use serde_json::{Value, json};

#[test]
fn names_are_valid_unique_and_clear_of_the_terminals() {
    let mut names = BTreeSet::new();
    for descriptor in catalog() {
        assert!(valid_tool_name(descriptor.name), "{}", descriptor.name);
        assert!(names.insert(descriptor.name), "{} twice", descriptor.name);
    }
    assert!(!names.contains(APP_LAUNCH));
    assert!(valid_tool_name(APP_LAUNCH));
    for terminal in koyo_term_control::catalog::CATALOG {
        assert!(
            !names.contains(terminal.name),
            "{} is a terminal tool",
            terminal.name
        );
        assert_ne!(terminal.name, APP_LAUNCH);
    }
    assert_eq!(names.len(), 70);
}

#[test]
fn every_operation_reads_as_a_tool_should() {
    for descriptor in catalog() {
        let name = descriptor.name;
        let title = descriptor.title;
        assert!(
            title.starts_with(|c: char| c.is_uppercase()),
            "{name}: {title}"
        );
        assert!(!title.ends_with('.'), "{name}: {title}");
        assert!(descriptor.description.ends_with('.'), "{name}");
        assert!(descriptor.short_description().ends_with('.'), "{name}");
        for field in descriptor.subject {
            assert!(
                descriptor.input_schema["properties"].get(*field).is_some(),
                "{name}'s subject {field} isn't an input field"
            );
        }
        assert_eq!(info(name), Some(descriptor.info), "{name}");
    }
    assert_eq!(
        infos(),
        catalog().iter().map(|d| d.info).collect::<Vec<_>>()
    );
}

#[test]
fn every_input_schema_is_an_object_that_refuses_unknown_fields() {
    for descriptor in catalog() {
        let name = descriptor.name;
        let schema = &descriptor.input_schema;
        assert_eq!(schema["type"], "object", "{name}");
        assert_eq!(schema["additionalProperties"], false, "{name}");
        assert!(schema["properties"].is_object(), "{name}");
        assert!(schema.get("title").is_none(), "{name}");
        // The schema says it, and the input type does it.
        let error = Command::parse(name, json!({ "zzUnknown": 1 })).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{name}");
        assert!(
            error.message.contains("unknown field"),
            "{name}: {}",
            error.message
        );
        assert_eq!(descriptor.output_schema["type"], "object", "{name}");
    }
}

#[test]
fn every_input_field_is_described() {
    for descriptor in catalog() {
        let Value::Object(properties) = &descriptor.input_schema["properties"] else {
            panic!("{} has no properties", descriptor.name);
        };
        for (field, schema) in properties {
            assert!(
                schema["description"]
                    .as_str()
                    .is_some_and(|d| !d.is_empty()),
                "{}.{field} has no description",
                descriptor.name
            );
        }
    }
}

/// The agent desktop's tools see and drive other apps of the person's Mac, and say so; every
/// other tool acts on Koyo alone.
#[test]
fn only_the_agent_desktops_tools_reach_beyond_koyo() {
    for descriptor in catalog() {
        assert_eq!(
            descriptor.hints.open_world,
            descriptor.name.starts_with("desktop_"),
            "{}",
            descriptor.name
        );
    }
}

#[test]
fn changes_take_a_request_id_and_reads_dont() {
    for descriptor in catalog() {
        let property = &descriptor.input_schema["properties"][REQUEST_ID];
        if descriptor.hints.read_only {
            assert!(
                property.is_null(),
                "{} reads, yet takes a requestId",
                descriptor.name
            );
            assert_eq!(descriptor.scope, Scope::Read, "{}", descriptor.name);
        } else {
            assert_eq!(
                property["pattern"], REQUEST_ID_PATTERN,
                "{}",
                descriptor.name
            );
            assert_ne!(descriptor.scope, Scope::Read, "{}", descriptor.name);
        }
    }
}

#[test]
fn confirmed_operations_are_destructive_and_need_the_scope() {
    let mut confirmed = Vec::new();
    for descriptor in catalog() {
        assert_eq!(
            descriptor.confirm,
            descriptor.scope == Scope::Destructive,
            "{}",
            descriptor.name
        );
        if descriptor.confirm {
            assert!(descriptor.hints.destructive, "{}", descriptor.name);
            confirmed.push(descriptor.name);
        }
        for scope in descriptor.also {
            assert_eq!(*scope, Scope::Destructive, "{}", descriptor.name);
        }
    }
    assert_eq!(confirmed, ["worktree_delete"]);
}

#[test]
fn closing_with_discard_needs_destructive_and_is_confirmed() {
    for name in ["tab_close", "pane_close"] {
        let keep = Command::parse(name, json!({ "pane": 4 })).unwrap();
        assert_eq!(keep.scopes(), [Scope::Navigate]);
        assert!(!keep.confirmed());
        let discard = Command::parse(name, json!({ "pane": 4, "discard": true })).unwrap();
        assert_eq!(discard.scopes(), [Scope::Navigate, Scope::Destructive]);
        assert!(discard.confirmed());
        assert!(discard.info().may_confirm());
    }
    let delete = Command::parse("worktree_delete", json!({ "worktree": "feat" })).unwrap();
    assert_eq!(delete.scopes(), [Scope::Destructive]);
    assert!(delete.confirmed());
    let Command::TabClose(close) = Command::parse("tab_close", json!({ "pane": 1 })).unwrap()
    else {
        panic!("tab_close parsed as another command");
    };
    assert_eq!(
        close,
        CloseInput {
            pane: 1,
            discard: None
        }
    );
}

#[test]
fn a_command_goes_to_its_parts_and_back() {
    let command = Command::parse(
        "pane_split",
        json!({ "pane": 3, "edge": "right", "kind": "browser", "url": "http://localhost" }),
    )
    .unwrap();
    assert_eq!(command.name(), "pane_split");
    let (name, input) = command.to_parts();
    assert_eq!(name, "pane_split");
    assert_eq!(
        input,
        json!({ "pane": 3, "edge": "right", "kind": "browser", "url": "http://localhost" })
    );
    assert_eq!(Command::parse(name, input).unwrap(), command);
    let moved =
        Command::parse("pane_split", json!({ "pane": 3, "edge": "top", "move": 9 })).unwrap();
    assert_eq!(
        moved.to_parts().1,
        json!({ "pane": 3, "edge": "top", "move": 9 })
    );
    assert_eq!(
        Command::parse("app_status", Value::Null)
            .unwrap()
            .to_parts()
            .1,
        json!({})
    );
    let unknown = Command::parse("terminal_send", json!({})).unwrap_err();
    assert!(
        unknown.message.contains("no operation"),
        "{}",
        unknown.message
    );
    let wait = Command::parse("browser_wait", json!({ "for": "text", "value": "Ready" }));
    assert!(wait.is_ok(), "{wait:?}");
}

#[test]
fn settings_tell_a_reset_from_a_field_left_out() {
    let parse = |input: Value| -> SettingsSetInput {
        let Command::SettingsSet(input) = Command::parse("settings_set", input).unwrap() else {
            panic!("settings_set parsed as another command");
        };
        input
    };
    assert_eq!(parse(json!({})).worktrees_dir, None);
    assert_eq!(
        parse(json!({ "worktreesDir": null })).worktrees_dir,
        Some(None)
    );
    assert_eq!(
        parse(json!({ "worktreesDir": "/w" })).worktrees_dir,
        Some(Some("/w".into()))
    );
    assert_eq!(
        parse(json!({ "mostLanguageServers": null })).most_language_servers,
        Some(None)
    );
    let prefixes = [
        (json!("githubUser"), BranchPrefix::GithubUser),
        (json!("none"), BranchPrefix::None),
        (json!({ "custom": "fs" }), BranchPrefix::Custom("fs".into())),
    ];
    for (wire, prefix) in prefixes {
        let input = parse(json!({ "branchPrefix": wire.clone() }));
        assert_eq!(input.branch_prefix, Some(prefix));
        assert_eq!(
            serde_json::to_value(&input).unwrap(),
            json!({ "branchPrefix": wire })
        );
    }
    let reset = parse(json!({ "worktreesDir": null }));
    assert_eq!(
        serde_json::to_value(&reset).unwrap(),
        json!({ "worktreesDir": null })
    );
}

#[test]
fn an_agent_cant_set_what_chats_run_with() {
    // A chat could choose its next session's program or mode, or its own tools.
    for setting in ["claudePath", "chatMode", "koyoTools", "externalControl"] {
        let refused = Command::parse("settings_set", json!({ setting: null })).unwrap_err();
        assert!(refused.message.contains("unknown field"), "{setting}");
    }
}

#[test]
fn summaries_read_the_outputs_of_their_operation() {
    let page = json!({ "pane": 3, "url": "http://localhost:3000", "title": "Home" });
    let text = summarize("browser_navigate", &page).unwrap();
    assert!(text.contains("Home") && text.contains("pane 3"), "{text}");
    assert_eq!(
        summarize("file_save", &json!({ "saved": ["/w/a.rs", "/w/b.rs"] })).unwrap(),
        "Saved 2 files"
    );
    assert_eq!(summarize("file_save", &json!({ "nope": 1 })), None);
    assert_eq!(summarize("terminal_list", &json!({})), None);
}

#[test]
fn ids_are_the_wire_names() {
    for code in ErrorCode::ALL {
        assert_eq!(serde_json::to_value(code).unwrap(), code.id());
    }
    for scope in Scope::ALL {
        assert_eq!(serde_json::to_value(scope).unwrap(), scope.id());
        assert_eq!(Scope::from_id(scope.id()), Some(scope));
    }
    for topic in Topic::ALL {
        assert_eq!(serde_json::to_value(topic).unwrap(), topic.id());
        for operation in topic.operations() {
            assert_eq!(
                info(operation).map(|i| i.scope),
                Some(Topic::SCOPE),
                "{operation}"
            );
        }
    }
    assert_eq!(
        Scope::DEFAULT_GRANT,
        [Scope::Read, Scope::Navigate, Scope::Manage, Scope::Browser]
    );
    assert_eq!(
        Scope::AGENT,
        [
            Scope::Read,
            Scope::Navigate,
            Scope::Manage,
            Scope::Browser,
            Scope::Desktop,
            Scope::Settings
        ]
    );
}

#[test]
fn browser_viewport_takes_the_presets_by_name_a_zoom_and_a_request_id() {
    let descriptor = catalog()
        .into_iter()
        .find(|descriptor| descriptor.name == "browser_viewport")
        .expect("browser_viewport is in the catalog");
    assert_eq!(descriptor.scope, Scope::Browser);
    assert!(descriptor.hints.idempotent && !descriptor.hints.read_only);
    let properties = &descriptor.input_schema["properties"];
    assert_eq!(properties[REQUEST_ID]["pattern"], REQUEST_ID_PATTERN);
    // The presets are listed, under the schema's definitions.
    let schema = serde_json::to_string(&descriptor.input_schema).unwrap();
    for name in [
        "responsive",
        "iphone-se",
        "iphone-15",
        "pixel-8",
        "ipad-pro-11",
    ] {
        assert!(
            schema.contains(&format!("\"{name}\"")),
            "{name} in {schema}"
        );
    }
    for field in [
        "width",
        "height",
        "zoom",
        "fill",
        "screenshot",
        "orientation",
    ] {
        assert!(properties.get(field).is_some(), "{field}");
    }
    let output = serde_json::to_string(&descriptor.output_schema).unwrap();
    assert!(output.contains("settled") && output.contains("shownPercent"));
    for input in [
        json!({ "device": "iphone-15", "orientation": "landscape", "zoom": "fit" }),
        json!({ "width": 390, "zoom": 75, "screenshot": true }),
        json!({ "fill": true }),
    ] {
        let parsed = Command::parse("browser_viewport", input.clone());
        assert!(parsed.is_ok(), "{input}: {parsed:?}");
    }
    for input in [
        json!({ "device": "nokia-3310" }),
        json!({ "zoom": "half" }),
        json!({ "width": -1 }),
    ] {
        let error = Command::parse("browser_viewport", input.clone()).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{input}");
    }
    let listed = serde_json::to_string(
        &catalog()
            .into_iter()
            .find(|descriptor| descriptor.name == "browser_list")
            .unwrap()
            .output_schema,
    )
    .unwrap();
    assert!(
        listed.contains("viewport"),
        "browser_list's pages carry their viewport"
    );
}

#[test]
fn activity_list_is_a_read_with_a_limit() {
    let descriptor = catalog()
        .into_iter()
        .find(|descriptor| descriptor.name == "activity_list")
        .expect("activity_list is in the catalog");
    assert_eq!(descriptor.scope, Scope::Read);
    assert_eq!(descriptor.hints, koyo_app_control::Hints::READ);
    assert!(!descriptor.confirm && descriptor.also.is_empty());
    let properties = &descriptor.input_schema["properties"];
    assert!(properties.get("limit").is_some());
    assert!(
        properties.get(REQUEST_ID).is_none(),
        "a read takes no requestId"
    );
    let output = serde_json::to_string(&descriptor.output_schema).unwrap();
    for field in [
        "entries",
        "onlyYours",
        "decision",
        "timed_out",
        "confirmation",
    ] {
        assert!(output.contains(field), "{field} in {output}");
    }
    for input in [json!({}), json!({ "limit": 10 }), Value::Null] {
        let parsed = Command::parse("activity_list", input.clone());
        assert!(parsed.is_ok(), "{input}: {parsed:?}");
    }
    for input in [
        json!({ "limit": -1 }),
        json!({ "limit": "ten" }),
        json!({ "client": "x" }),
    ] {
        let error = Command::parse("activity_list", input.clone()).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{input}");
    }
}

#[test]
fn a_browser_page_an_agent_opens_takes_a_viewport() {
    for name in ["tab_open", "pane_split"] {
        let descriptor = catalog()
            .into_iter()
            .find(|descriptor| descriptor.name == name)
            .unwrap();
        assert!(
            descriptor.input_schema["properties"]
                .get("viewport")
                .is_some(),
            "{name}"
        );
        // Agents learn the default from the description.
        assert!(descriptor.description.contains("1440 × 900"), "{name}");
    }
    let Command::TabOpen(sized) = Command::parse(
        "tab_open",
        json!({ "kind": "browser", "url": "http://localhost", "viewport": { "width": 1280, "height": 720 } }),
    )
    .unwrap() else {
        panic!("tab_open parsed as another command");
    };
    assert_eq!(
        sized.spec().viewport,
        Some(OpenViewport {
            width: Some(1280),
            height: Some(720),
            ..OpenViewport::default()
        })
    );
    // Left out, the app applies the person's default.
    let Command::TabOpen(plain) = Command::parse("tab_open", json!({ "kind": "browser" })).unwrap()
    else {
        panic!("tab_open parsed as another command");
    };
    assert_eq!(plain.spec().viewport, None);
    let Command::PaneSplit(split) = Command::parse(
        "pane_split",
        json!({ "pane": 3, "edge": "right", "kind": "browser", "viewport": { "fill": true } }),
    )
    .unwrap() else {
        panic!("pane_split parsed as another command");
    };
    let SplitSource::New(spec) = split.source().unwrap() else {
        panic!("a new pane");
    };
    assert_eq!(spec.viewport.and_then(|viewport| viewport.fill), Some(true));
    // A pane moved keeps its own page.
    let Command::PaneSplit(moved) = Command::parse(
        "pane_split",
        json!({ "pane": 3, "edge": "top", "move": 9, "viewport": { "fill": true } }),
    )
    .unwrap() else {
        panic!("pane_split parsed as another command");
    };
    assert_eq!(moved.source().unwrap_err().code, ErrorCode::InvalidInput);
    for viewport in [
        json!({ "zoom": "fit" }),
        json!({ "device": "nokia-3310" }),
        json!(true),
    ] {
        let error = Command::parse(
            "tab_open",
            json!({ "kind": "browser", "viewport": viewport }),
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{viewport}");
    }
}
