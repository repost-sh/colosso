//! The page an agent means, moving a page, and waiting on it; and the protocol call every
//! operation here makes, within its time limit, its refusals in app control's words
//! (`refusals.rs`). Which pages there are is `listing.rs`'s; a page's lease, `asking.rs`'s.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui_kit::{App, AsyncApp, Global};
use koyo_app_control::ControlError;
use koyo_app_control::ops::browser::{
    BrowserDialogInput, BrowserGoInput, BrowserNavigateInput, BrowserWaitInput, BrowserWaitOutput,
    DEFAULT_TIMEOUT_MS, GoAction, MAX_TIMEOUT_MS, PageOutput, WaitFor, WaitUntil,
};
use serde_json::Value;

use super::super::{Caller, Exec, output};
use super::pages::{self, Found, Page};
use super::{refusals, root, script};
use crate::browser::agent::{self, Actor, AgentError, Answer, Go, Mode, NotOpen, Refused};
use crate::browser::{self, BrowserView};

/// How often a wait looks at the page again.
const POLL: Duration = Duration::from_millis(150);

/// The page each caller acted on last, which an operation naming no pane means.
#[derive(Default)]
struct LastPages(HashMap<String, u64>);

impl Global for LastPages {}

/// The page `pane` names; else the one `caller` acted on last; else the shown tab's; else the
/// only one: the one way every browser tool gets its page. The person's own is never one meant
/// unnamed, and named it goes through the gate (`pages/consent.rs`).
pub(super) fn resolve(
    caller: &Caller,
    pane: Option<u64>,
    cx: &mut App,
) -> Result<Page, ControlError> {
    match pages::find(caller, pane, cx)? {
        Found::Page(found) => Ok(found),
        Found::Persons(persons) => persons.admit(caller, cx),
    }
}

/// Carries out `work` on `page` from a task: its answer, which on the person's page stands only
/// if they still let the caller use it once it's done (`Page::settle`).
pub(super) fn later<F>(page: Page, cx: &mut App, work: F) -> Exec
where
    F: AsyncFnOnce(&Page, &mut AsyncApp) -> Result<Value, ControlError> + 'static,
{
    Exec::Later(cx.spawn(async move |cx: &mut AsyncApp| {
        let answer = work(&page, cx).await;
        cx.update(|cx| page.settle(answer, cx))
    }))
}

/// The pane `caller` acted on last, if it did.
pub(super) fn last(caller: &Caller, cx: &App) -> Option<u64> {
    (cx.try_global::<LastPages>()).and_then(|last| last.0.get(&caller.key()).copied())
}

/// Notes pane `pane`, a page `caller` opened or acted on, as the one its next call without a
/// `pane` means.
pub(crate) fn acted(caller: &Caller, pane: u64, cx: &mut App) {
    cx.default_global::<LastPages>()
        .0
        .insert(caller.key(), pane);
}

/// A refusal of the page's lease, in app control's words.
pub(super) fn refused(refused: Refused) -> ControlError {
    match refused {
        Refused::PersonHasControl {
            mode: Mode::Implicit,
            ..
        } => ControlError::person_using_page(),
        Refused::PersonHasControl { .. } => ControlError::person_has_control(),
        Refused::ControlRequired { holder } => ControlError::control_required(format!(
            "{} holds this page; browser_claim with takeover takes it over.",
            holder.label
        )),
    }
}

/// The longest a protocol command waits for the page, well within the client's own wait for
/// the whole call (`koyo_app_control::CALL_TIMEOUT`, 100 s), so the agent hears why.
pub(super) const CALL_LIMIT: Duration = Duration::from_secs(30);
/// As long for a script whose promise is awaited, which may wait on the page or the network.
pub(super) const PROMISE_LIMIT: Duration = Duration::from_secs(90);

/// Runs `method` on `page`, under its lease for `caller` when `acts`: its result, or
/// `timeout` once it has waited [`CALL_LIMIT`].
pub(super) async fn call(
    page: &Page,
    caller: Option<&Caller>,
    method: &str,
    params: Value,
    cx: &mut AsyncApp,
) -> Result<Value, ControlError> {
    call_within(page, caller, (None, method), params, CALL_LIMIT, cx).await
}

const _: () = {
    assert!(CALL_LIMIT.as_millis() < PROMISE_LIMIT.as_millis());
    assert!(PROMISE_LIMIT.as_millis() < koyo_app_control::CALL_TIMEOUT.as_millis());
};

/// As [`call`], waiting `limit` at most, in the page's own session or the frame's `session`:
/// the one way every browser tool reaches a page, so a page showing a file its caller may not
/// be shown is refused here (`root.rs`), before the command and again once it's answered if the
/// page moved meanwhile.
pub(super) async fn call_within(
    page: &Page,
    caller: Option<&Caller>,
    (session, method): (Option<&str>, &str),
    params: Value,
    limit: Duration,
    cx: &mut AsyncApp,
) -> Result<Value, ControlError> {
    let shown = shown_to_caller(page, cx).await?;
    let answer = command(page, caller, (session, method), params, limit, cx).await;
    if cx.update(|cx| address(page, cx)).is_ok_and(|now| now != shown) {
        shown_to_caller(page, cx).await?;
    }
    answer
}

/// The address `page` shows, once its caller may be shown it.
async fn shown_to_caller(page: &Page, cx: &mut AsyncApp) -> Result<String, ControlError> {
    let shown = cx.update(|cx| address(page, cx))?;
    root::shown_to(page.caller(), shown.clone(), cx).await?;
    Ok(shown)
}

fn address(page: &Page, cx: &App) -> Result<String, ControlError> {
    Ok(page.view()?.read(cx).url().to_owned())
}

async fn command(
    page: &Page,
    caller: Option<&Caller>,
    (session, method): (Option<&str>, &str),
    params: Value,
    limit: Duration,
    cx: &mut AsyncApp,
) -> Result<Value, ControlError> {
    let actor = caller.map(Caller::actor);
    let asked = cx.update(|cx| {
        page.still(cx)?;
        let view = page.view()?;
        let session = session.map(str::to_owned);
        agent::cdp(&view, actor.as_ref(), session, method, params, limit, cx)
            .map_err(|error| refusals::failed(error, page.pane()))
    });
    match asked?.await {
        Answer::Answered(Ok(value)) => Ok(value),
        Answer::Answered(Err(why)) => Err(refusals::command_failed(method, why)),
        Answer::Stopped => Err(ControlError::browser_unavailable(
            "The browser engine stopped before it answered.",
        )),
        Answer::TimedOut => Err(refusals::timed_out(method, limit, caller.is_some())),
        Answer::Asking => {
            let dialog = cx.update(|cx| (page.view().ok()).and_then(|view| view.read(cx).dialog()));
            Err(refusals::asked_meanwhile(
                method,
                dialog.as_ref(),
                caller.is_some(),
            ))
        }
    }
}

/// The page's address and title now.
pub(super) fn now(page: &Page, cx: &App) -> Result<PageOutput, ControlError> {
    let facts = page.view()?.read(cx).facts(cx);
    Ok(PageOutput {
        pane: page.pane(),
        url: facts.url,
        title: facts.title,
        loaded: None,
        dialog: facts.dialog,
    })
}

/// Answers the question `caller`'s page waits on, as `input` says.
pub(crate) fn dialog(
    caller: &Caller,
    input: &BrowserDialogInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let page = resolve(caller, input.pane, cx)?;
    let (actor, action, text) = (caller.actor(), input.action, input.text.clone());
    (page.view()?.update(cx, |view, cx| view.agent_answers(&actor, action, text, cx)))
        .map_err(|why| refusals::unanswered(why, page.pane()))?;
    acted(caller, page.pane(), cx);
    let answer = now(&page, cx).and_then(|at| output(&at));
    Ok(Exec::Ready(page.settle(answer, cx)))
}

/// How long a call may wait, as asked, within the most it may.
fn timeout(asked: Option<u64>) -> Duration {
    Duration::from_millis(asked.unwrap_or(DEFAULT_TIMEOUT_MS).min(MAX_TIMEOUT_MS))
}

pub(crate) fn navigate(
    caller: &Caller,
    input: &BrowserNavigateInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let url = browser::address_to_url(&input.url, crate::settings::get(cx).search_engine)
        .ok_or_else(|| ControlError::invalid_input("`url` is empty."))?;
    let limit = (input.wait != Some(WaitUntil::None)).then(|| timeout(input.timeout_ms));
    steer(caller, input.pane, limit, cx, |view, actor, cx| {
        view.agent_navigate(actor, url, cx)
    })
}

pub(crate) fn go(
    caller: &Caller,
    input: &BrowserGoInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let to = match input.action {
        GoAction::Back => Go::Back,
        GoAction::Forward => Go::Forward,
        GoAction::Reload => Go::Reload,
    };
    steer(
        caller,
        input.pane,
        Some(timeout(None)),
        cx,
        |view, actor, cx| view.agent_go(actor, to, cx),
    )
}

/// Moves the page for `caller` with `act`, then waits up to `limit` for it to load: where
/// it is then, and whether it loaded.
fn steer(
    caller: &Caller,
    named: Option<u64>,
    limit: Option<Duration>,
    cx: &mut App,
    act: impl FnOnce(
        &mut BrowserView,
        &Actor,
        &mut gpui_kit::Context<BrowserView>,
    ) -> Result<(), AgentError>,
) -> Result<Exec, ControlError> {
    let page = resolve(caller, named, cx)?;
    let actor = caller.actor();
    (page.view()?.update(cx, |view, cx| act(view, &actor, cx)))
        .map_err(|why| refusals::failed(why, page.pane()))?;
    acted(caller, page.pane(), cx);
    Ok(later(page, cx, async move |page, cx| {
        let loaded = match limit {
            Some(limit) => Some(settle(page, limit, cx).await),
            None => None,
        };
        output(&PageOutput {
            loaded,
            ..cx.update(|cx| now(page, cx))?
        })
    }))
}

/// Waits, up to `limit`, until the page has started and finished loading: whether it did.
async fn settle(page: &Page, limit: Duration, cx: &mut AsyncApp) -> bool {
    let started = Instant::now();
    cx.background_executor().timer(POLL).await;
    while started.elapsed() < limit {
        if !cx.update(|cx| (page.view()).is_ok_and(|view| view.read(cx).loading())) {
            return true;
        }
        cx.background_executor().timer(POLL).await;
    }
    false
}

pub(crate) fn wait(
    caller: &Caller,
    input: &BrowserWaitInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let page = resolve(caller, input.pane, cx)?;
    let value = input.value.clone().unwrap_or_default();
    if input.until != WaitFor::Load && value.is_empty() {
        return Err(ControlError::invalid_input(
            "Waiting for a selector, text or address needs `value`.",
        ));
    }
    let (limit, until) = (timeout(input.timeout_ms), input.until);
    Ok(later(page, cx, async move |page, cx| {
        let started = Instant::now();
        loop {
            cx.update(|cx| {
                page.still(cx)?;
                page.view()?.update(cx, BrowserView::agent_uses);
                Ok::<_, ControlError>(())
            })?;
            let met = met(page, until, &value, cx).await?;
            if met || started.elapsed() >= limit {
                return output(&BrowserWaitOutput {
                    met,
                    elapsed_ms: started.elapsed().as_millis() as u64,
                });
            }
            cx.background_executor().timer(POLL).await;
        }
    }))
}

/// The script that says whether the page meets `until` for `value`; none for an address,
/// which the app knows.
fn condition(until: WaitFor, value: &str) -> Option<String> {
    Some(match until {
        WaitFor::Url => return None,
        WaitFor::Load => "document.readyState === 'complete'".to_owned(),
        WaitFor::Selector => format!("!!{}", script::find(value)),
        WaitFor::Text => format!(
            "(document.body ? document.body.innerText : '').includes({})",
            script::quote(value)
        ),
    })
}

/// Whether the page meets the condition now.
async fn met(
    page: &Page,
    until: WaitFor,
    value: &str,
    cx: &mut AsyncApp,
) -> Result<bool, ControlError> {
    let (url, loading, ready) = cx.update(|cx| {
        let view = page.view()?;
        let view = view.read(cx);
        Ok::<_, ControlError>((
            view.url().to_owned(),
            view.loading(),
            view.ready_for_agents(cx),
        ))
    })?;
    let Some(expression) = condition(until, value) else {
        return Ok(url.contains(value));
    };
    match ready {
        Ok(()) => {}
        Err(AgentError::NotOpen(NotOpen::Starting)) => return Ok(false),
        Err(why) => return Err(refusals::failed(why, page.pane())),
    }
    if until == WaitFor::Load && loading {
        return Ok(false);
    }
    match script::evaluate(page, None, &expression, false, cx).await {
        Ok(found) => Ok(found == Value::Bool(true)),
        Err(error) if !error.retryable => Err(error),
        Err(_) => Ok(false),
    }
}
