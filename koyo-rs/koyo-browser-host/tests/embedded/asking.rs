//! What a test asks of pane 1's page, as an agent does and as the app does.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use koyo_browser_ipc::client::{Connection, Incoming};
use koyo_browser_ipc::{AppRequest, CdpCaller, HostEvent};
use serde_json::{Value, json};

use crate::inside::wait;
use crate::pump::idle;

/// How long the page may take to come to what [`until`] waits for.
const PATIENCE: Duration = Duration::from_secs(10);

/// What the page answers an agent's protocol command.
pub fn cdp(
    host: &mut Connection,
    incoming: &Receiver<Incoming>,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    static CALLS: AtomicU64 = AtomicU64::new(1);
    let call = CALLS.fetch_add(1, Ordering::Relaxed);
    let method = method.to_owned();
    let request = AppRequest::Cdp { pane: 1, call, caller: CdpCaller::Agent, session: None, method, params };
    host.send(&request).map_err(|error| format!("{request:?}: {error}"))?;
    let (result, error) = wait(incoming, &format!("{request:?}"), |event| match event {
        HostEvent::CdpReply { call: answered, result, error, .. } if *answered == call => {
            Some((result.clone(), error.clone()))
        }
        _ => None,
    })?;
    error.map_or_else(|| Ok(result.unwrap_or_default()), |error| Err(format!("{request:?}: {error}")))
}

/// What `expression` comes to in the page, a promise awaited, run as if the person acted.
pub fn evaluate(host: &mut Connection, incoming: &Receiver<Incoming>, expression: &str) -> Result<Value, String> {
    let params = json!({
        "expression": expression,
        "awaitPromise": true,
        "returnByValue": true,
        "userGesture": true,
    });
    let result = cdp(host, incoming, "Runtime.evaluate", params)?;
    if !result["exceptionDetails"].is_null() {
        return Err(format!("{expression}: {result}"));
    }
    Ok(result["result"]["value"].clone())
}

/// Waits for `expression` to come to `wanted` in the page.
pub fn until(
    host: &mut Connection,
    incoming: &Receiver<Incoming>,
    expression: &str,
    wanted: &Value,
) -> Result<(), String> {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let now = evaluate(host, incoming, expression)?;
        if now == *wanted {
            return Ok(());
        }
        if Instant::now() > deadline {
            return Err(format!("{expression} is {now}, not {wanted}"));
        }
        idle(Duration::from_millis(50));
    }
}

/// Tells the host what the app does of the page's sight, its window one that's seen: whether an
/// agent keeps it on screen for the engine.
pub fn kept(host: &mut Connection, kept: bool) -> Result<(), String> {
    (host.send(&AppRequest::Sight { pane: 1, kept, seen: true })).map_err(|error| format!("sight: {error}"))
}
