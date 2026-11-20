use super::{hearing, hears};
use serde_json::json;
use std::collections::HashSet;

/// The app hears the events of the domains an agent enabled, until it disables them, and of
/// no other: not those the page's `DevTools` or the host enabled in the same session.
#[test]
fn the_app_hears_the_domains_an_agent_enabled_until_it_disables_them() {
    let mut heard = HashSet::new();
    assert!(!hears(&heard, "Target.attachedToTarget"), "nothing enabled");
    hearing(&mut heard, "Runtime.evaluate", &json!({}));
    assert!(!hears(&heard, "Runtime.consoleAPICalled"), "a command that enables nothing");
    hearing(&mut heard, "Runtime.enable", &serde_json::Value::Null);
    hearing(&mut heard, "Network.enable", &json!({}));
    assert!(hears(&heard, "Runtime.consoleAPICalled"));
    assert!(hears(&heard, "Network.requestWillBeSent"));
    assert!(!hears(&heard, "Page.frameNavigated"), "a domain no agent enabled");
    assert!(!hears(&heard, "RuntimeX.thing"), "only the whole domain's name");
    hearing(&mut heard, "Network.disable", &json!({}));
    assert!(!hears(&heard, "Network.requestWillBeSent"), "disabled again");
    assert!(hears(&heard, "Runtime.consoleAPICalled"));
}

/// An agent's `Target.setAutoAttach` has the app hear the frames attached under the page's
/// session, until it asks for it no longer; asking without saying is neither.
#[test]
fn the_app_hears_the_frames_attached_once_an_agent_asked_for_them() {
    let mut heard = HashSet::new();
    hearing(&mut heard, "Target.setAutoAttach", &json!({ "flatten": true }));
    assert!(!hears(&heard, "Target.attachedToTarget"), "not asked to attach");
    let attach = json!({ "autoAttach": true, "waitForDebuggerOnStart": false, "flatten": true });
    hearing(&mut heard, "Target.setAutoAttach", &attach);
    assert!(hears(&heard, "Target.attachedToTarget"));
    assert!(hears(&heard, "Target.detachedFromTarget"));
    hearing(&mut heard, "Page.setAutoAttach", &attach);
    assert!(!hears(&heard, "Page.frameAttached"), "only the Target domain's");
    hearing(&mut heard, "Target.setAutoAttach", &json!({ "autoAttach": false }));
    assert!(!hears(&heard, "Target.attachedToTarget"), "no longer");
}
