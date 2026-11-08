//! A page's passkeys, in the real engine: without the entitlement every request is turned
//! down at once, so a site moves on to its other ways of signing in; with it, what the
//! system's sheet answers reaches the page as a `PublicKeyCredential`. A test stands in for
//! the sheet, which needs the entitlement to come up.

use std::path::PathBuf;

use gpui_kit::{Entity, TestAppContext, VisualTestContext};
use koyo_browser_ipc::{Bytes, Created, Got, PasskeyAnswer, PasskeyAsk};

use super::{finish, open_with, serve, wait_for};
use crate::browser::host::TestLaunch;
use crate::browser::passkeys::TestSheet;
use crate::browser::view::BrowserView;

/// Runs `script`, an async function body, in a page on this machine (a secure context by
/// name), and hands back the title it ends with: `done ` and what it returns, as JSON.
fn page(name: &str) -> Option<String> {
    let script = match name {
        "refused" => REFUSED,
        "answered" => ANSWERED,
        _ => return None,
    };
    Some(format!(
        "<title>waiting</title><script>(async () => {{ {script} }})().then(\
         (out) => {{ document.title = 'done ' + JSON.stringify(out); }},\
         (error) => {{ document.title = 'failed ' + error; }});</script>"
    ))
}

/// What a page tries without the entitlement, timing each request.
const REFUSED: &str = "
    const challenge = new Uint8Array(32);
    const timed = async (promise) => {
        const start = performance.now();
        try { await promise; return 'resolved'; }
        catch (error) { return error.name + ' ' + Math.round(performance.now() - start); }
    };
    const user = { id: new Uint8Array([1]), name: 'ada', displayName: 'Ada' };
    const out = {};
    out.platform = await PublicKeyCredential.isUserVerifyingPlatformAuthenticatorAvailable();
    out.conditional = await PublicKeyCredential.isConditionalMediationAvailable();
    out.hybrid = (await PublicKeyCredential.getClientCapabilities()).hybridTransport;
    out.create = await timed(navigator.credentials.create({ publicKey: {
        rp: { name: 'Test' }, user, challenge,
        pubKeyCredParams: [{ type: 'public-key', alg: -7 }] } }));
    out.get = await timed(navigator.credentials.get({ publicKey: { challenge } }));
    out.otherSite = await timed(navigator.credentials.get({
        publicKey: { challenge, rpId: 'example.com' } }));
    out.noChallenge = await timed(navigator.credentials.get({ publicKey: {} }));
    const aborting = new AbortController();
    const autofill = timed(navigator.credentials.get({
        mediation: 'conditional', publicKey: { challenge }, signal: aborting.signal }));
    setTimeout(() => aborting.abort(), 50);
    out.autofill = (await autofill).split(' ')[0];
    out.bridged = !navigator.credentials.get.toString().includes('[native code]');
    return out;
";

/// A page making a passkey and signing in with it, reading back what a site reads.
const ANSWERED: &str = "
    const made = await navigator.credentials.create({ publicKey: {
        rp: { name: 'Test' },
        user: { id: new Uint8Array([7, 7]), name: 'ada', displayName: 'Ada' },
        challenge: new Uint8Array([1, 2, 3]),
        pubKeyCredParams: [{ type: 'public-key', alg: -7 }],
        extensions: { credProps: true } } });
    const used = await navigator.credentials.get({ publicKey: {
        challenge: new Uint8Array([4, 5, 6]), allowCredentials: [{ type: 'public-key', id: made.rawId }] } });
    const bytes = (buffer) => Array.from(new Uint8Array(buffer)).join(',');
    const aborting = new AbortController();
    const waited = navigator.credentials.get({ publicKey: { challenge: new Uint8Array([9]) },
        signal: aborting.signal }).catch((error) => error.name);
    setTimeout(() => aborting.abort(), 50);
    return {
        credential: made instanceof PublicKeyCredential,
        attestation: made.response instanceof AuthenticatorAttestationResponse,
        id: made.id,
        rawId: bytes(made.rawId),
        type: made.type,
        attachment: made.authenticatorAttachment,
        algorithm: made.response.getPublicKeyAlgorithm(),
        publicKey: bytes(made.response.getPublicKey()),
        authenticatorData: bytes(made.response.getAuthenticatorData()),
        transports: made.response.getTransports(),
        discoverable: made.getClientExtensionResults().credProps.rk,
        json: made.toJSON().response.attestationObject.length,
        assertion: used.response instanceof AuthenticatorAssertionResponse,
        signature: bytes(used.response.signature),
        userHandle: bytes(used.response.userHandle),
        aborted: await waited,
    };
";

/// A pane opening `url`, on a host told the app can ask for passkeys, whose sheet `sheet`
/// stands in for.
fn open_with_sheet<'a>(
    test: &str,
    url: &str,
    sheet: fn(&str, &PasskeyAsk) -> Option<PasskeyAnswer>,
    cx: &'a mut TestAppContext,
) -> (Entity<BrowserView>, &'a mut VisualTestContext, PathBuf) {
    let setup = move |cx: &mut gpui_kit::App| {
        cx.global_mut::<TestLaunch>().0.passkeys = true;
        cx.set_global(TestSheet(sheet));
    };
    open_with(test, url, setup, cx)
}

fn finished(cx: &mut VisualTestContext, view: &Entity<BrowserView>) -> serde_json::Value {
    wait_for(cx, view, "the page's script to finish", |view| {
        !view.page.title.starts_with("waiting") && !view.page.title.is_empty()
    });
    let title = view.read_with(cx, |view, _| view.page.title.clone());
    let out = title
        .strip_prefix("done ")
        .unwrap_or_else(|| panic!("the page's script failed: {title}"));
    serde_json::from_str(out).expect("the page's results")
}

/// Milliseconds a request took to fail with `name`, from the page's `name ms`.
fn refused_within(out: &serde_json::Value, key: &str, name: &str) -> u64 {
    let said = out[key].as_str().unwrap_or_default();
    let (got, ms) = said.split_once(' ').unwrap_or((said, ""));
    assert_eq!(got, name, "{key}: {said}");
    ms.parse().unwrap_or(u64::MAX)
}

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn without_the_entitlement_a_pages_passkey_requests_are_turned_down_at_once(
    cx: &mut TestAppContext,
) {
    let port = serve(page);
    let (view, cx, scratch) = super::open(
        "passkeys-refused",
        &format!("http://localhost:{port}/refused"),
        cx,
    );
    let out = finished(cx, &view);
    eprintln!("the page saw {out}");
    assert_eq!(out["bridged"], true, "the bridge answers for the engine");
    assert_eq!(out["platform"], false);
    assert_eq!(out["conditional"], false);
    assert_eq!(out["hybrid"], false);
    for key in ["create", "get"] {
        let ms = refused_within(&out, key, "NotAllowedError");
        assert!(ms < 500, "{key} was turned down in {ms} ms");
    }
    refused_within(&out, "otherSite", "SecurityError");
    refused_within(&out, "noChallenge", "TypeError");
    assert_eq!(
        out["autofill"], "AbortError",
        "an autofill request waits until aborted"
    );
    finish(cx, scratch);
}

/// A `none` attestation object for a P-256 key, x all ones and y all twos, as a platform
/// passkey's is: the host reads the key out of it for the page.
fn attestation_object() -> Vec<u8> {
    const HEX: &str = "a363666d74646e6f6e656761747453746d74a0686175746844617461589411111111111111111111111111111111111111111111111111111111111111114500000007222222222222222222222222222222220010ababababababababababababababababa501020326200121582001010101010101010101010101010101010101010101010101010101010101012258200202020202020202020202020202020202020202020202020202020202020202";
    (0..HEX.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&HEX[at..at + 2], 16).expect("hex"))
        .collect()
}

/// The sheet's stand-in: a new passkey for a creation, a sign-in for a sign-in, and no answer
/// at all for the challenge `[9]`, which the page aborts.
fn answer(origin: &str, ask: &PasskeyAsk) -> Option<PasskeyAnswer> {
    assert!(
        origin.starts_with("http://localhost:"),
        "asked from {origin}"
    );
    assert_eq!(ask.rp_id(), "localhost");
    match ask {
        PasskeyAsk::Create(creation) => {
            assert_eq!(creation.challenge.0, [1, 2, 3]);
            assert_eq!(creation.user.id.0, [7, 7]);
            assert!(creation.extensions.cred_props);
            Some(PasskeyAnswer::Created(Created {
                id: Bytes(vec![0xfb, 0xff, 0x10]),
                client_data_json: Bytes(b"{}".to_vec()),
                attestation_object: Bytes(attestation_object()),
                transports: vec!["hybrid".into(), "internal".into()],
                attachment: Some("platform".into()),
                discoverable: Some(true),
            }))
        }
        PasskeyAsk::Get(get) if get.challenge.0 == [9] => None,
        PasskeyAsk::Get(get) => {
            assert_eq!(get.allow_credentials[0].id.0, [0xfb, 0xff, 0x10]);
            Some(PasskeyAnswer::Got(Got {
                id: Bytes(vec![0xfb, 0xff, 0x10]),
                client_data_json: Bytes(b"{}".to_vec()),
                authenticator_data: Bytes(vec![5]),
                signature: Bytes(vec![6, 6]),
                user_handle: Some(Bytes(vec![7, 7])),
                attachment: Some("platform".into()),
            }))
        }
    }
}

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn what_the_sheet_answers_reaches_the_page_as_a_public_key_credential(cx: &mut TestAppContext) {
    let port = serve(page);
    let url = format!("http://localhost:{port}/answered");
    let (view, cx, scratch) = open_with_sheet("passkeys-answered", &url, answer, cx);
    let out = finished(cx, &view);
    eprintln!("the page saw {out}");
    assert_eq!(out["credential"], true);
    assert_eq!(out["attestation"], true);
    assert_eq!(out["id"], "-_8Q");
    assert_eq!(out["rawId"], "251,255,16");
    assert_eq!(out["type"], "public-key");
    assert_eq!(out["attachment"], "platform");
    assert_eq!(out["algorithm"], -7);
    // The P-256 key as DER, read out of the attestation object by the host; and the
    // authenticator data around it, 148 bytes.
    let key = out["publicKey"].as_str().unwrap_or_default();
    assert!(
        key.starts_with("48,89,48,19,6,7,42,134,72,206,61,2,1"),
        "{key}"
    );
    assert_eq!(key.split(',').count(), 91);
    let data = out["authenticatorData"].as_str().unwrap_or_default();
    assert_eq!(data.split(',').count(), 148, "{data}");
    assert_eq!(out["transports"], serde_json::json!(["hybrid", "internal"]));
    assert_eq!(out["discoverable"], true);
    // The attestation object in base64url: 178 bytes.
    assert_eq!(out["json"], 238);
    assert_eq!(out["assertion"], true);
    assert_eq!(out["signature"], "6,6");
    assert_eq!(out["userHandle"], "7,7");
    assert_eq!(out["aborted"], "AbortError");
    // The aborted request was withdrawn from the app, not just from the page.
    wait_for(cx, &view, "the withdrawn request to go", |view| {
        !view.passkey.waiting()
    });
    finish(cx, scratch);
}
