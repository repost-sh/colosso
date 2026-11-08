//! A page's passkeys, through the host alone: the bridge in every frame, the host's checks,
//! the requests it sends the app, the answers it takes, and what it withdraws.

use koyo_browser_ipc::{AppRequest, Bytes, Created, HostEvent, PasskeyAnswer, PasskeyAsk};
use std::time::Duration;

use crate::support::{Session, VIEWPORT, close, evaluate, launch, open, serve, until};

/// A `none` attestation object for a P-256 key, as a platform passkey's is: the host reads
/// the key out of it for the page.
fn attestation_object() -> Vec<u8> {
    const HEX: &str = "a363666d74646e6f6e656761747453746d74a0686175746844617461589411111111111111111111111111111111111111111111111111111111111111114500000007222222222222222222222222222222220010ababababababababababababababababa501020326200121582001010101010101010101010101010101010101010101010101010101010101012258200202020202020202020202020202020202020202020202020202020202020202";
    (0..HEX.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&HEX[at..at + 2], 16).expect("hex"))
        .collect()
}

/// One page for every frame: on `localhost` it embeds a frame of its own origin and one of
/// another (`sub.localhost`, another site, so another renderer), and keeps what each says.
/// A frame asks with `get` as it loads and tells the page how it ended.
const PAGE: &str = "<title>passkeys</title><script>
    const asked = (where) => navigator.credentials.get({ publicKey: { challenge: new Uint8Array(16) } })
        .then(() => 'resolved', (error) => error.name + ': ' + error.message)
        .then((how) => parent.postMessage({ where, how }, '*'));
    window.frames_said = {};
    addEventListener('message', (event) => { frames_said[event.data.where] = event.data.how; });
    if (location.hostname === 'localhost' && parent === window) {
        for (const host of ['localhost', 'sub.localhost']) {
            const frame = document.createElement('iframe');
            frame.src = location.href.replace('localhost', host) + '?framed';
            document.documentElement.append(frame);
        }
    } else if (location.search === '?framed') {
        asked(location.hostname);
    }
</script>";

/// The page's address on `localhost`, a secure context by name.
fn on_localhost(address: &str) -> String {
    address.replace("127.0.0.1", "localhost")
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn without_passkeys_a_frame_is_turned_down_and_another_origins_refused_for_its_origin() {
    // How a page's own requests are answered is the app's engine test's
    // (`koyo/src/browser/host_tests/passkeys.rs`); here, frames, other credentials.
    let launch = launch("passkeys-off");
    let mut session = Session::start(&launch);
    open(&mut session, &on_localhost(&serve(PAGE)), "passkeys");
    let passwords = evaluate(&mut session, "typeof navigator.credentials.store");
    assert_eq!(
        passwords, "function",
        "other credentials are Chromium's still"
    );
    let frames = until(
        &mut session,
        "JSON.stringify(frames_said)",
        "both frames to say how they were answered",
        Duration::from_secs(20),
        |said| said.as_object().is_some_and(|said| said.len() == 2),
    );
    let own = frames["localhost"].as_str().unwrap_or_default();
    assert!(
        own.starts_with("NotAllowedError: Passkeys aren't available"),
        "{own}"
    );
    let other = frames["sub.localhost"].as_str().unwrap_or_default();
    assert!(
        other.starts_with("NotAllowedError: The 'publickey-credentials-get' feature"),
        "{other}"
    );
    close(session, &launch);
}

/// Runs `expression` in pane 1's page, as call `call`, and waits for both its reply and the
/// passkey request it makes, which can come in either order: the request's number and ask.
fn asking(session: &mut Session, call: u64, expression: &str) -> (u64, PasskeyAsk) {
    let params = serde_json::json!({"expression": expression, "returnByValue": true});
    crate::agents::cdp(session, call, "Runtime.evaluate", params);
    let (mut replied, mut got) = (false, None);
    session.wait("the script's reply and its passkey request", |event, _| {
        match event {
            HostEvent::CdpReply { call: c, error, .. } if *c == call => {
                assert_eq!(*error, None);
                replied = true;
            }
            HostEvent::Passkey {
                pane: 1,
                request,
                ask,
                ..
            } => got = Some((*request, (**ask).clone())),
            _ => {}
        }
        replied && got.is_some()
    });
    got.expect("asked")
}

/// Opens [`PAGE`] as pane 1 and waits for it to load and its same-origin frame to ask, the
/// other frame being refused: the frame's request.
fn open_and_ask(session: &mut Session) -> u64 {
    session
        .host
        .send(&AppRequest::Open {
            pane: 1,
            url: on_localhost(&serve(PAGE)),
            viewport: VIEWPORT,
            profile: "smoke".into(),
        })
        .expect("open");
    let (mut loaded, mut framed) = (false, None);
    session.wait("the page to load and its frame to ask", |event, _| {
        match event {
            HostEvent::Page { pane: 1, state } => {
                loaded |= state.title == "passkeys" && !state.loading;
            }
            HostEvent::Passkey {
                pane: 1,
                request,
                origin,
                ..
            } => {
                assert!(
                    origin.starts_with("http://localhost:"),
                    "asked from {origin}"
                );
                assert!(framed.replace(*request).is_none(), "one frame asks");
            }
            _ => {}
        }
        loaded && framed.is_some()
    });
    framed.expect("the frame asked")
}

/// The page makes a passkey, which the app's answer gives it.
fn answer_a_creation(session: &mut Session) {
    let (request, ask) = asking(
        session,
        1001,
        "window.made = navigator.credentials.create({ publicKey: {
            rp: { name: 'Smoke', id: 'localhost' }, challenge: new Uint8Array([1, 2]),
            user: { id: new Uint8Array([3]), name: 'ada', displayName: 'Ada' },
            pubKeyCredParams: [{ type: 'public-key', alg: -7 }] } })
            .then((c) => ({ id: c.id, alg: c.response.getPublicKeyAlgorithm(),
                            key: c.response.getPublicKey().byteLength, json: c.toJSON() }),
                  (error) => error.name);
        1",
    );
    let PasskeyAsk::Create(creation) = ask else {
        panic!("a creation");
    };
    assert_eq!(creation.rp.id, "localhost");
    assert_eq!(creation.challenge.0, [1, 2]);
    session
        .host
        .send(&AppRequest::Passkey {
            pane: 1,
            request,
            answer: PasskeyAnswer::Created(Created {
                id: Bytes(vec![1, 2, 3]),
                client_data_json: Bytes(b"{}".to_vec()),
                attestation_object: Bytes(attestation_object()),
                transports: vec!["internal".into()],
                attachment: Some("platform".into()),
                discoverable: None,
            }),
        })
        .expect("answer");
    let made = evaluate(session, "window.made");
    assert_eq!(made["id"], "AQID");
    assert_eq!(made["alg"], -7);
    assert_eq!(
        made["key"], 91,
        "the P-256 key as DER, read out by the host"
    );
    let object = made["json"]["response"]["attestationObject"].as_str();
    assert_eq!(object.map(str::len), Some(238));
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn with_passkeys_the_app_is_asked_and_its_answer_or_a_withdrawal_follows() {
    let mut launch = launch("passkeys-on");
    launch.passkeys = true;
    let mut session = Session::start(&launch);
    let framed = open_and_ask(&mut session);
    answer_a_creation(&mut session);

    // An aborted request is withdrawn; so is the frame's, as the page goes.
    let (aborting, _) = asking(
        &mut session,
        1002,
        "window.stop_it = new AbortController();
         window.aborted = navigator.credentials.get({ publicKey: { challenge: new Uint8Array(1) },
             signal: stop_it.signal }).catch((error) => error.name);
         1",
    );
    // Aborted: withdrawn from the app, which can hear of it before the script's reply.
    let params = serde_json::json!({"expression": "stop_it.abort()"});
    crate::agents::cdp(&mut session, 1003, "Runtime.evaluate", params);
    let (mut replied, mut withdrawn) = (false, Vec::new());
    session.wait("the abort and its request withdrawn", |event, _| {
        match event {
            HostEvent::CdpReply { call: 1003, .. } => replied = true,
            HostEvent::PasskeyWithdrawn { request, .. } => withdrawn.push(*request),
            _ => {}
        }
        replied && withdrawn.contains(&aborting)
    });
    assert_eq!(evaluate(&mut session, "window.aborted"), "AbortError");
    session
        .host
        .send(&AppRequest::Navigate {
            pane: 1,
            url: "data:text/html,<title>away</title>".into(),
        })
        .expect("navigate");
    session.wait(
        "the framed request withdrawn as the page goes",
        |event, _| {
            if let HostEvent::PasskeyWithdrawn { request, .. } = event {
                withdrawn.push(*request);
            }
            withdrawn.contains(&framed)
        },
    );
    close(session, &launch);
}
