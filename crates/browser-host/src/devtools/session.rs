//! The frontend's session on the page, from finding the page's target to detaching from
//! it: the host's own steps and lookups told apart from the frontend's by their message ids.
use colosso_browser_ipc::{PaneId, Point};
use std::collections::HashSet;
use std::sync::atomic::{AtomicI64, Ordering};

/// Where the host's own message ids start; the frontend counts up from one.
const HOST_IDS: i64 = 1 << 30;

static NEXT_ID: AtomicI64 = AtomicI64::new(HOST_IDS);

/// What the page keeps while its `DevTools` are open.
pub(crate) struct Inspected {
    /// The `DevTools` pane.
    pub(crate) devtools: PaneId,
    /// The frontend's session on the page.
    pub(crate) session: Session,
}

impl Inspected {
    /// For `DevTools` pane `devtools`.
    pub(crate) fn new(devtools: PaneId) -> Self {
        Self {
            devtools,
            session: Session::default(),
        }
    }
}

/// Revealing an element in the inspector: the frontend learns of it as a
/// request from the backend, which the host makes once it has looked the
/// element up through the frontend's own DOM session.
#[derive(Default)]
pub(crate) struct Reveal {
    /// The frontend has enabled the DOM domain, so nodes can be looked up.
    dom_enabled: bool,
    /// An element to reveal once that is possible.
    inspect: Option<Point>,
    /// The id of the host's own node lookup that is under way.
    lookup: Option<i64>,
}

impl Reveal {
    /// Asks for the element at `at` to be revealed, now or once the
    /// frontend can show it.
    pub(crate) fn inspect(&mut self, at: Point) {
        self.inspect = Some(at);
    }
}

/// The frontend's session on the page, from finding the page's target to
/// detaching from it.
#[derive(Default)]
pub(crate) struct Session {
    stage: Stage,
    /// What the frontend sent before its session was open.
    held: Vec<String>,
    /// Sessions of the frames and workers attached under the frontend's.
    children: HashSet<String>,
    /// The element the inspector is to reveal.
    reveal: Reveal,
}

#[derive(Default)]
enum Stage {
    /// Nothing asked yet.
    #[default]
    Idle,
    /// The host's request for the page's own target is under way.
    Finding(i64),
    /// The host's request to attach to it is under way.
    Attaching(i64),
    /// The frontend's session.
    Open(String),
    /// No session of its own could be made: the frontend shares the root
    /// session, as a last resort.
    Shared,
}

/// What becomes of a message of the frontend's own session.
enum Own {
    /// The frontend's: it goes to the frontend as it came.
    Forward,
    /// The answer to a node lookup: the frontend gets the backend's
    /// request to reveal the node instead.
    Instead(String),
    /// Another answer to the host, which needs nothing more.
    Kept,
}

/// Messages a page message turned into.
#[derive(Default)]
pub(crate) struct Routed {
    pub(crate) to_frontend: Vec<String>,
    pub(crate) to_page: Vec<String>,
}

/// A message id of the host's own, apart from any frontend's.
pub(crate) fn host_id() -> i64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// A protocol message's fields.
pub(crate) type Fields = serde_json::Map<String, serde_json::Value>;

/// `text` as a protocol message's fields, when it is a JSON object.
pub(crate) fn object(text: &str) -> Option<Fields> {
    match serde_json::from_str(text) {
        Ok(serde_json::Value::Object(fields)) => Some(fields),
        _ => None,
    }
}

fn str_at<'a>(value: Option<&'a serde_json::Value>, path: &[&str]) -> Option<&'a str> {
    path.iter()
        .try_fold(value?, |value, key| value.get(key))?
        .as_str()
}

impl Session {
    /// The first request, on the root session: which target the page is.
    pub(crate) fn begin(&mut self) -> String {
        let id = host_id();
        self.stage = Stage::Finding(id);
        serde_json::json!({"id": id, "method": "Target.getTargetInfo"}).to_string()
    }

    /// The request that ends the frontend's session, on the root session.
    pub(crate) fn end(&self) -> Option<String> {
        let Stage::Open(session) = &self.stage else {
            return None;
        };
        Some(
            serde_json::json!({
                "id": host_id(),
                "method": "Target.detachFromTarget",
                "params": {"sessionId": session},
            })
            .to_string(),
        )
    }

    /// Reveals the element at `at` once the frontend can show it; the
    /// lookup to send when it can now.
    pub(crate) fn inspect(&mut self, at: Point) -> Option<String> {
        self.reveal.inspect(at);
        let lookup = node_lookup(&mut self.reveal)?;
        Some(self.addressed(lookup))
    }

    /// `message` in the frontend's session, unless it names one already.
    fn addressed(&self, message: String) -> String {
        let Stage::Open(session) = &self.stage else {
            return message;
        };
        let Some(mut fields) = object(&message) else {
            return message;
        };
        if fields.contains_key("sessionId") {
            return message;
        }
        fields.insert("sessionId".into(), session.clone().into());
        serde_json::Value::Object(fields).to_string()
    }

    /// A message from the frontend: what goes to the page for it, now or,
    /// while the session is being made, once it is open.
    pub(crate) fn frontend_said(&mut self, text: &str) -> Vec<String> {
        if matches!(
            self.stage,
            Stage::Idle | Stage::Finding(_) | Stage::Attaching(_)
        ) {
            self.held.push(text.to_owned());
            return Vec::new();
        }
        let fields = object(text);
        let own = fields
            .as_ref()
            .is_some_and(|f| !f.contains_key("sessionId"));
        let mut to_page = vec![self.addressed(text.to_owned())];
        let method = fields
            .as_ref()
            .and_then(|f| f.get("method"))
            .and_then(serde_json::Value::as_str);
        if own && method == Some("DOM.enable") {
            self.reveal.dom_enabled = true;
        }
        if let Some(lookup) = node_lookup(&mut self.reveal) {
            to_page.push(self.addressed(lookup));
        }
        to_page
    }

    /// The session is open, or given up on: what the frontend sent
    /// meanwhile goes now.
    fn release(&mut self) -> Vec<String> {
        std::mem::take(&mut self.held)
            .iter()
            .flat_map(|text| self.frontend_said(text))
            .collect()
    }

    /// A message from the page: the host's own steps and answers are
    /// taken, the frontend's session and those under it reach the
    /// frontend, as if its session were the page's own, and everything
    /// else is another client's.
    pub(crate) fn page_said(&mut self, text: &str, mut fields: Fields) -> Routed {
        let mut routed = Routed::default();
        let id = fields.get("id").and_then(serde_json::Value::as_i64);
        let session = fields
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        match (&self.stage, session) {
            (Stage::Finding(want), None) if id == Some(*want) => {
                if let Some(target) = str_at(fields.get("result"), &["targetInfo", "targetId"]) {
                    let id = host_id();
                    routed.to_page.push(
                        serde_json::json!({
                            "id": id,
                            "method": "Target.attachToTarget",
                            "params": {"targetId": target, "flatten": true},
                        })
                        .to_string(),
                    );
                    self.stage = Stage::Attaching(id);
                } else {
                    self.stage = Stage::Shared;
                    routed.to_page = self.release();
                }
            }
            (Stage::Attaching(want), None) if id == Some(*want) => {
                self.stage = match str_at(fields.get("result"), &["sessionId"]) {
                    Some(session) => Stage::Open(session.to_owned()),
                    None => Stage::Shared,
                };
                routed.to_page = self.release();
            }
            (Stage::Shared, None) => match self.own_message(&fields, id) {
                Own::Forward => routed.to_frontend.push(text.to_owned()),
                Own::Instead(message) => routed.to_frontend.push(message),
                Own::Kept => {}
            },
            (Stage::Open(own), Some(session)) if *own == session => {
                self.track_children(&fields);
                match self.own_message(&fields, id) {
                    Own::Forward => {
                        // The session's name goes last; cut it off rather
                        // than write out a message that may be large.
                        let tail = format!(",\"sessionId\":\"{session}\"}}");
                        let message = if let Some(head) = text.strip_suffix(tail.as_str()) {
                            format!("{head}}}")
                        } else {
                            fields.remove("sessionId");
                            serde_json::Value::Object(fields).to_string()
                        };
                        routed.to_frontend.push(message);
                    }
                    Own::Instead(message) => routed.to_frontend.push(message),
                    Own::Kept => {}
                }
            }
            (Stage::Open(_), Some(session)) if self.children.contains(&session) => {
                self.track_children(&fields);
                routed.to_frontend.push(text.to_owned());
            }
            _ => {}
        }
        routed
    }

    /// Follows targets attaching and detaching under the frontend's session.
    fn track_children(&mut self, fields: &serde_json::Map<String, serde_json::Value>) {
        let method = fields.get("method").and_then(serde_json::Value::as_str);
        let child = str_at(fields.get("params"), &["sessionId"]).map(str::to_owned);
        match (method, child) {
            (Some("Target.attachedToTarget"), Some(child)) => {
                self.children.insert(child);
            }
            (Some("Target.detachedFromTarget"), Some(child)) => {
                self.children.remove(&child);
            }
            _ => {}
        }
    }

    /// Sorts a message of the frontend's own session, with message id `id`.
    fn own_message(
        &mut self,
        fields: &serde_json::Map<String, serde_json::Value>,
        id: Option<i64>,
    ) -> Own {
        let id = match id {
            Some(id) if id >= HOST_IDS => id,
            _ => return Own::Forward,
        };
        if self.reveal.lookup != Some(id) {
            return Own::Kept;
        }
        self.reveal.lookup = None;
        let node = fields
            .get("result")
            .and_then(|r| r.get("backendNodeId"))
            .and_then(serde_json::Value::as_i64);
        match node {
            Some(node) => Own::Instead(
                serde_json::json!({
                    "method": "Overlay.inspectNodeRequested",
                    "params": {"backendNodeId": node},
                })
                .to_string(),
            ),
            None => Own::Kept,
        }
    }
}

/// The host's request for the node to reveal, once the frontend can
/// show it.
fn node_lookup(reveal: &mut Reveal) -> Option<String> {
    if !reveal.dom_enabled || reveal.lookup.is_some() {
        return None;
    }
    let at = reveal.inspect.take()?;
    let id = host_id();
    reveal.lookup = Some(id);
    Some(
        serde_json::json!({
            "id": id,
            "method": "DOM.getNodeForLocation",
            "params": {
                "x": at.x.round(),
                "y": at.y.round(),
                "includeUserAgentShadowDOM": false,
            },
        })
        .to_string(),
    )
}

#[cfg(test)]
mod tests;
