use super::*;

#[test]
fn requests_round_trip_as_frames_of_json() {
    let messages = vec![
        AppRequest::Open {
            pane: 7,
            url: "https://example.com".into(),
            viewport: Viewport {
                width: 800,
                height: 600,
                scale: 2.,
            },
            profile: "project-a".into(),
        },
        AppRequest::Resize {
            pane: 7,
            viewport: Viewport {
                width: 390,
                height: 844,
                scale: 3.,
            },
        },
        AppRequest::Edit {
            pane: 7,
            command: Edit::Paste,
        },
        AppRequest::SiteSettings { pane: 7 },
        AppRequest::SetSiteSetting {
            pane: 7,
            permission: Permission::Camera,
            allowed: false,
        },
        AppRequest::ClearData,
        AppRequest::Shutdown,
    ];
    let mut buf = Vec::new();
    for m in &messages {
        write_frame(&mut buf, m).expect("write");
    }
    let mut reader = buf.as_slice();
    for m in &messages {
        assert_eq!(
            read_frame::<AppRequest>(&mut reader)
                .expect("read")
                .as_ref(),
            Some(m)
        );
    }
}

#[test]
fn the_wire_names_are_stable() {
    let event = HostEvent::Cursor {
        pane: 3,
        cursor: Cursor::ResizeColumn,
    };
    assert_eq!(
        serde_json::to_value(&event).expect("json"),
        serde_json::json!({"type": "cursor", "pane": 3, "cursor": "resizeColumn"})
    );
    let go = AppRequest::Go {
        pane: 1,
        to: Go::Back,
    };
    assert_eq!(
        serde_json::to_value(&go).expect("json"),
        serde_json::json!({"type": "go", "pane": 1, "to": "back"})
    );
    let devtools = AppRequest::OpenDevTools {
        pane: 1,
        devtools: 2,
        viewport: Viewport {
            width: 300,
            height: 200,
            scale: 1.,
        },
        inspect: Some(Point { x: 10., y: 20.5 }),
        dock: DevToolsDock::Undocked,
    };
    assert_eq!(
        serde_json::to_value(&devtools).expect("json"),
        serde_json::json!({
            "type": "openDevTools", "pane": 1, "devtools": 2,
            "viewport": {"width": 300, "height": 200, "scale": 1.0},
            "inspect": {"x": 10.0, "y": 20.5},
            "dock": "undocked"
        })
    );
    assert_eq!(devtools.pane(), Some(1), "it acts on the page");
    let unsaid: AppRequest = serde_json::from_value(serde_json::json!({
        "type": "openDevTools", "pane": 1, "devtools": 2,
        "viewport": {"width": 300, "height": 200, "scale": 1.0}, "inspect": null
    }))
    .expect("an older app's request");
    assert!(
        matches!(
            unsaid,
            AppRequest::OpenDevTools {
                dock: DevToolsDock::Bottom,
                ..
            }
        ),
        "no side said is the bottom"
    );
    assert_eq!(
        serde_json::to_value(HostEvent::Closed { pane: 2 }).expect("json"),
        serde_json::json!({"type": "closed", "pane": 2})
    );
}

#[test]
fn what_devtools_ask_from_their_own_controls_has_stable_wire_names() {
    let asks = [
        (
            DevToolsAsk::Dock {
                side: DevToolsDock::Left,
            },
            serde_json::json!({"op": "dock", "side": "left"}),
        ),
        (
            DevToolsAsk::Dock {
                side: DevToolsDock::Undocked,
            },
            serde_json::json!({"op": "dock", "side": "undocked"}),
        ),
        (DevToolsAsk::Close, serde_json::json!({"op": "close"})),
        (
            DevToolsAsk::DeviceMode { on: true },
            serde_json::json!({"op": "deviceMode", "on": true}),
        ),
    ];
    for (ask, wire) in asks {
        let event = HostEvent::DevTools { pane: 3, ask };
        let json = serde_json::to_value(&event).expect("json");
        assert_eq!(
            json,
            serde_json::json!({"type": "devTools", "pane": 3, "ask": wire})
        );
        assert_eq!(
            serde_json::from_value::<HostEvent>(json).expect("back"),
            event
        );
    }
}

#[test]
fn dialogs_and_answers_have_stable_wire_names() {
    let dialog = HostEvent::Dialog {
        pane: 2,
        dialog: 9,
        origin: "https://example.com".into(),
        question: Dialog::Prompt {
            message: "Name?".into(),
            default_text: "Ada".into(),
        },
    };
    assert_eq!(
        serde_json::to_value(&dialog).expect("json"),
        serde_json::json!({
            "type": "dialog", "pane": 2, "dialog": 9, "origin": "https://example.com",
            "question": {"kind": "prompt", "message": "Name?", "defaultText": "Ada"}
        })
    );
    let answer = AppRequest::Answer {
        pane: 2,
        dialog: 9,
        answer: Answer::Credentials {
            username: "ada".into(),
            password: "secret".into(),
        },
    };
    assert_eq!(
        serde_json::to_value(&answer).expect("json"),
        serde_json::json!({
            "type": "answer", "pane": 2, "dialog": 9,
            "answer": {"kind": "credentials", "username": "ada", "password": "secret"}
        })
    );
    let unknown: Vec<Permission> =
        serde_json::from_value(serde_json::json!(["camera", "somethingNew"])).expect("json");
    assert_eq!(unknown, [Permission::Camera, Permission::Other]);
}

#[test]
fn a_page_icon_travels_as_its_address_then_its_png() {
    let page: PageState = serde_json::from_value(serde_json::json!({
        "url": "https://example.com/", "title": "", "loading": false,
        "canGoBack": false, "canGoForward": false,
    }))
    .expect("an older page state, without an icon");
    assert_eq!(page.favicon, "");
    let mut buf = Vec::new();
    let icon = HostEvent::Favicon {
        pane: 3,
        url: "https://example.com/favicon.ico".into(),
        png: Bytes(vec![0x89, b'P', b'N', b'G']),
    };
    let json = serde_json::to_value(&icon).expect("json");
    assert_eq!(json["png"], "iVBORw", "the icon's bytes travel as base64, as JSON carries bytes");
    write_frame(&mut buf, &icon).expect("write");
    assert_eq!(
        read_frame::<HostEvent>(&mut buf.as_slice()).expect("read"),
        Some(icon)
    );
}

#[test]
fn a_pages_tone_progress_and_whose_command_have_stable_wire_names() {
    use serde_json::json;
    let tone = HostEvent::Tone {
        pane: 3,
        tone: Tone {
            lightness: vec![1.; Tone::ACROSS * Tone::DOWN],
            canvas: [18, 18, 18],
        },
    };
    let json = serde_json::to_value(&tone).expect("json");
    assert_eq!(json["type"], "tone");
    assert_eq!(json["tone"]["canvas"], json!([18, 18, 18]));
    assert_eq!(json["tone"]["lightness"].as_array().map(Vec::len), Some(96));
    let progress = HostEvent::Progress { pane: 3, percent: 40 };
    assert_eq!(
        serde_json::to_value(&progress).expect("json"),
        json!({"type": "progress", "pane": 3, "percent": 40})
    );
    let cdp = AppRequest::Cdp {
        pane: 3,
        call: 7,
        caller: CdpCaller::Agent,
        session: None,
        method: "Runtime.enable".into(),
        params: serde_json::Value::Null,
    };
    let sent = serde_json::to_value(&cdp).expect("json");
    assert_eq!(sent["caller"], "agent");
    assert_eq!(sent.get("session"), None, "the page's own session goes unnamed");
    let in_frame = json!({"type": "cdpEvent", "pane": 3, "session": "AB12", "method": "Page.frameResized", "params": {}});
    let heard: HostEvent = serde_json::from_value(in_frame).expect("a frame's event");
    assert!(matches!(heard, HostEvent::CdpEvent { session: Some(session), .. } if session == "AB12"));
    for message in [tone, progress] {
        let mut buf = Vec::new();
        write_frame(&mut buf, &message).expect("write");
        assert_eq!(read_frame::<HostEvent>(&mut buf.as_slice()).expect("read"), Some(message));
    }
}

#[test]
fn extensions_have_stable_wire_names() {
    use serde_json::json;
    let act = AppRequest::ExtensionAct {
        pane: 3,
        extension: "abc".into(),
        at: [100., 640.5],
        corner: Corner::Left,
    };
    assert_eq!(
        serde_json::to_value(act).unwrap(),
        json!({ "type": "extensionAct", "pane": 3, "extension": "abc", "at": [100.0, 640.5],
                "corner": "left" })
    );
    let plain: AppRequest =
        serde_json::from_value(json!({ "type": "extensionAct", "pane": 3, "extension": "abc", "at": [1.0, 2.0] }))
            .unwrap();
    assert!(matches!(plain, AppRequest::ExtensionAct { corner: Corner::Right, .. }));
    let change = |change| {
        serde_json::to_value(AppRequest::ChangeExtension {
            profile: "p".into(),
            extension: "abc".into(),
            change,
        })
        .unwrap()
    };
    assert_eq!(
        change(ExtensionChange::Pinned { on: true }),
        json!({ "type": "changeExtension", "profile": "p", "extension": "abc",
                "change": { "change": "pinned", "on": true } })
    );
    assert_eq!(change(ExtensionChange::Remove)["change"], json!({ "change": "remove" }));
    assert_eq!(
        change(ExtensionChange::Enabled { on: false })["change"],
        json!({ "change": "enabled", "on": false })
    );
    assert_eq!(
        serde_json::to_value(AppRequest::Active { pane: 3 }).unwrap(),
        json!({ "type": "active", "pane": 3 })
    );

    let listed: HostEvent = serde_json::from_value(json!({
        "type": "extensions", "profile": "p",
        "extensions": [{ "id": "abc", "name": "Block", "version": "1.2", "enabled": true,
                         "removable": true, "pinned": false, "source": "store" }],
    }))
    .unwrap();
    let HostEvent::Extensions { profile, extensions } = listed else {
        panic!("not a list of extensions");
    };
    assert_eq!(profile, "p");
    assert_eq!(extensions[0].source, ExtensionSource::Store);
    assert!(extensions[0].icon_path.is_empty() && extensions[0].options_url.is_empty());

    let set = HostEvent::ExtensionAction {
        profile: "p".into(),
        extension: "abc".into(),
        pane: None,
        action: ExtensionAction {
            badge_text: "12".into(),
            badge_color: "#FF0000FF".into(),
            enabled: true,
            ..ExtensionAction::default()
        },
    };
    let wire = serde_json::to_value(&set).unwrap();
    assert_eq!(wire["type"], "extensionAction");
    assert_eq!(wire["pane"], json!(null));
    assert_eq!(wire["action"]["badgeText"], "12");
    assert_eq!(wire["action"]["badgeColor"], "#FF0000FF");
    assert_eq!(serde_json::from_value::<HostEvent>(wire).unwrap(), set);

    let opened = HostEvent::TabOpened {
        pane: (1 << 40) + 1,
        opener: 3,
        url: "chrome-extension://abc/options.html".into(),
        active: true,
    };
    assert_eq!(
        serde_json::to_value(opened).unwrap(),
        json!({ "type": "tabOpened", "pane": 1_099_511_627_777_u64, "opener": 3,
                "url": "chrome-extension://abc/options.html", "active": true })
    );
    assert_eq!(
        serde_json::to_value(HostEvent::TabActivated { pane: 3 }).unwrap(),
        json!({ "type": "tabActivated", "pane": 3 })
    );
    assert_eq!(
        serde_json::to_value(HostEvent::TabClosed { pane: 3 }).unwrap(),
        json!({ "type": "tabClosed", "pane": 3 })
    );
}

#[test]
fn downloads_and_printing_have_stable_wire_names() {
    use serde_json::json;
    let asked = HostEvent::DownloadAsked {
        pane: 4,
        download: 9,
        name: "report.pdf".into(),
        url: "https://example.com/report".into(),
    };
    assert_eq!(
        serde_json::to_value(&asked).unwrap(),
        json!({ "type": "downloadAsked", "pane": 4, "download": 9, "name": "report.pdf",
                "url": "https://example.com/report" })
    );
    let going = HostEvent::Download {
        pane: 4,
        download: Download {
            id: 9,
            url: "https://example.com/report".into(),
            path: "/Users/me/Downloads/report.pdf".into(),
            received: 512,
            total: None,
            state: DownloadState::Failed { reason: "The network went away.".into() },
        },
    };
    let wire = serde_json::to_value(&going).unwrap();
    assert_eq!(
        wire,
        json!({ "type": "download", "pane": 4, "download": {
            "id": 9, "url": "https://example.com/report", "path": "/Users/me/Downloads/report.pdf",
            "received": 512, "total": null,
            "state": { "kind": "failed", "reason": "The network went away." } } })
    );
    assert_eq!(serde_json::from_value::<HostEvent>(wire).unwrap(), going);
    assert_eq!(
        serde_json::to_value(HostEvent::PdfSaved { pane: 4, path: "/tmp/a.pdf".into(), saved: true })
            .unwrap(),
        json!({ "type": "pdfSaved", "pane": 4, "path": "/tmp/a.pdf", "saved": true })
    );

    let save = AppRequest::Download {
        pane: 4,
        download: 9,
        act: DownloadAct::SaveTo { path: "/Users/me/Downloads/report (1).pdf".into() },
    };
    assert_eq!(
        serde_json::to_value(&save).unwrap(),
        json!({ "type": "download", "pane": 4, "download": 9,
                "act": { "type": "saveTo", "path": "/Users/me/Downloads/report (1).pdf" } })
    );
    assert_eq!(save.pane(), Some(4));
    let print = AppRequest::Print { pane: 4, print: Print::Pdf { path: "/tmp/a.pdf".into() } };
    assert_eq!(
        serde_json::to_value(&print).unwrap(),
        json!({ "type": "print", "pane": 4, "print": { "type": "pdf", "path": "/tmp/a.pdf" } })
    );
    assert_eq!(
        serde_json::to_value(AppRequest::Print { pane: 4, print: Print::Preview }).unwrap(),
        json!({ "type": "print", "pane": 4, "print": { "type": "preview" } })
    );
    assert_eq!(print.pane(), Some(4));
}
