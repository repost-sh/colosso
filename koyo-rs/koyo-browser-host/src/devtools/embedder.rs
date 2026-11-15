//! The frontend as its embedder (`docs/browser.md` 13.2): its address, which turns its own
//! dock menu, × and device toolbar toggle on, and the shim (`embedder.js`) that passes what
//! those ask to the app, where Chromium's own embedder does nothing with it.
//!
//! The side the frontend opens at doesn't go in its address: Chromium loads a frontend only
//! at an address its own check leaves as it is, which drops every parameter it doesn't know
//! (`ERR_INVALID_URL` otherwise). At a valid address Chromium attaches its own embedder to the
//! page (`DevToolsUIBindings`, `DevToolsAPI` in the page), which is what the shim takes from.
//! The host puts the shim in over the frontend page's own `DevTools` session once the page has
//! loaded, with a binding the shim tells the host through (`Runtime.bindingCalled`), and the
//! side, which the shim sets through the frontend's own setting.
use koyo_browser_ipc::{DevToolsAsk, DevToolsDock};

/// The frontend's own page: the shim goes in it and nowhere else.
pub(crate) const FRONTEND: &str = "devtools://devtools/bundled/devtools_app.html";

/// The function the host adds to the frontend's page for the shim to tell it through.
const BINDING: &str = "koyoDevToolsAsk";

/// The shim, a function of the side and the binding's name.
const SHIM: &str = include_str!("embedder.js");

/// The frontend's address, for the bridge on `port` at the path `token`. All it says besides
/// its page is what Chromium's check of a frontend's address keeps as it is, in the order
/// given: the bridge (`ws`), passed on as given, and `can_dock`, a flag it keeps as `true`,
/// which is what turns the dock menu, the × and the device toolbar toggle on.
pub(super) fn address(port: u16, token: &str) -> String {
    format!("{FRONTEND}?ws=127.0.0.1:{port}/{token}&can_dock=true")
}

/// What puts the shim in the frontend's page at `side`, each a command for the page's own
/// session: the page's runtime on (for the binding's calls to be heard), the binding, which
/// outlives the page's loads, and the shim, which a load clears. The side goes in under the
/// frontend's own name for it, which is the wire's.
pub(crate) fn install(side: DevToolsDock) -> [(&'static str, serde_json::Value); 3] {
    let call = format!(
        "({})({}, {})",
        SHIM.trim_end().trim_end_matches(';'),
        serde_json::json!(side),
        serde_json::json!(BINDING),
    );
    [
        ("Runtime.enable", serde_json::Value::Null),
        ("Runtime.addBinding", serde_json::json!({"name": BINDING})),
        (
            "Runtime.evaluate",
            serde_json::json!({"expression": call, "returnByValue": true}),
        ),
    ]
}

/// What a message of the frontend page's own session asks for the shim: the binding's call,
/// in the page's root session, with one of the asks. `None` for any other message.
pub(crate) fn asked(fields: &super::Fields) -> Option<DevToolsAsk> {
    if fields.contains_key("sessionId")
        || fields.get("method").and_then(serde_json::Value::as_str) != Some("Runtime.bindingCalled")
    {
        return None;
    }
    let params = fields.get("params")?;
    if params.get("name").and_then(serde_json::Value::as_str) != Some(BINDING) {
        return None;
    }
    serde_json::from_str(params.get("payload")?.as_str()?).ok()
}

#[cfg(test)]
mod tests;
