//! The page an agent means, the pages listed, a page's lease, moving a page, and waiting on
//! it; and the protocol call every operation here makes, within its time limit, its refusals
//! in app control's words (`refusals.rs`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui_kit::{App, AsyncApp, Entity, Global};
use koyo_app_control::ControlError;
use koyo_app_control::ops::browser::{
    BrowserClaimInput, BrowserGoInput, BrowserListOutput, BrowserNavigateInput, BrowserWaitInput,
    BrowserWaitOutput, DEFAULT_TIMEOUT_MS, GoAction, Lease, MAX_TIMEOUT_MS, PageInfo, PageOutput,
    WaitFor, WaitUntil,
};
use serde_json::Value;

use super::super::{Caller, Exec, main_shell, output, window_closed};
use super::{refusals, script};
use crate::browser::agent::{self, Actor, Answer, Go, Mode, Refused};
use crate::browser::{self, BrowserView};

/// How often a wait looks at the page again.
const POLL: Duration = Duration::from_millis(150);

/// A browser pane: its id in the window, its view, and its worktree.
#[derive(Clone)]
pub(super) struct Page {
    pub pane: u64,
    pub view: Entity<BrowserView>,
    pub worktree: PathBuf,
}

/// The page each caller acted on last, which an operation naming no pane means.
#[derive(Default)]
struct LastPages(HashMap<String, u64>);

impl Global for LastPages {}

/// A browser pane as the tools list it: a page, or one brought back at launch whose tab wasn't
/// shown yet, listed as it was kept, with no page to act on until it's shown
/// (`docs/browser.md` 6).
enum Listed {
    Page(Page),
    Unshown(PageInfo),
}

impl Listed {
    fn pane(&self) -> u64 {
        match self {
            Listed::Page(page) => page.pane,
            Listed::Unshown(info) => info.pane,
        }
    }

    /// Its page, to act on; an unshown one has none yet.
    fn page(&self) -> Result<Page, ControlError> {
        match self {
            Listed::Page(page) => Ok(page.clone()),
            Listed::Unshown(info) => Err(refusals::unshown(info.pane)),
        }
    }
}

/// Every browser pane in the window, `DevTools` left out, and which is in the tab shown.
fn pages(cx: &App) -> Result<(Vec<Listed>, Option<u64>), ControlError> {
    let (_, shell) = main_shell(cx).ok_or_else(window_closed)?;
    let workspace = &shell.read(cx).workspace;
    let mut pages = Vec::new();
    for project in &workspace.projects {
        for unit in &project.branches {
            for tab in &unit.tabs {
                for pane in tab.panes() {
                    if let Some(view) = pane.view()
                        && !view.read(cx).is_inspector()
                    {
                        pages.push(Listed::Page(Page {
                            pane: pane.id,
                            view: view.clone(),
                            worktree: unit.worktree.path.clone(),
                        }));
                    } else if let Some(address) = pane.unloaded_address() {
                        pages.push(Listed::Unshown(PageInfo {
                            pane: pane.id,
                            url: address.to_owned(),
                            title: pane.title.to_string(),
                            loading: false,
                            worktree: unit.worktree.path.display().to_string(),
                            controller: None,
                            viewport: agent::unshown_viewport(),
                        }));
                    }
                }
            }
        }
    }
    let shown = (workspace.shown_tab())
        .and_then(|at| workspace.tab(at))
        .and_then(|tab| {
            (tab.panes().into_iter())
                .find(|pane| pane.address(cx).is_some())
                .map(|pane| pane.id)
        });
    Ok((pages, shown))
}

/// The page `pane` names; else the one `caller` acted on last; else the shown tab's.
pub(super) fn resolve(
    caller: &Caller,
    pane: Option<u64>,
    cx: &mut App,
) -> Result<Page, ControlError> {
    let (pages, shown) = pages(cx)?;
    let find = |id: u64| pages.iter().find(|page| page.pane() == id);
    if let Some(id) = pane {
        let found = find(id).ok_or_else(|| {
            ControlError::not_found(format!(
                "Pane {id} isn't a browser page; browser_list lists them."
            ))
        });
        return found.and_then(Listed::page);
    }
    let last = (cx.try_global::<LastPages>())
        .and_then(|last| last.0.get(&caller.key()).copied())
        .and_then(find);
    if let Some(page) = last.or_else(|| shown.and_then(find)) {
        return page.page();
    }
    match pages.as_slice() {
        [] => Err(ControlError::not_found(
            "There's no browser page; tab_open with kind browser opens one.",
        )),
        [only] => only.page(),
        many => Err(ControlError::ambiguous(
            "Several browser pages are open; name one with `pane`.",
            &many
                .iter()
                .map(|page| page.pane().to_string())
                .collect::<Vec<_>>(),
        )),
    }
}

/// Notes `page` as the one `caller` acted on last.
pub(super) fn acted(caller: &Caller, page: &Page, cx: &mut App) {
    cx.default_global::<LastPages>()
        .0
        .insert(caller.key(), page.pane);
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
    call_within(page, caller, method, params, CALL_LIMIT, cx).await
}

// Every command ends within the client's own wait, and a page out of sight stays on screen for
// the engine through any one call (`AWAKE_FOR`): its longest command, and a wait for a load.
const _: () = {
    assert!(CALL_LIMIT.as_millis() < PROMISE_LIMIT.as_millis());
    assert!(PROMISE_LIMIT.as_millis() < koyo_app_control::CALL_TIMEOUT.as_millis());
    assert!(PROMISE_LIMIT.as_millis() <= agent::AWAKE_FOR.as_millis());
    assert!(Duration::from_millis(MAX_TIMEOUT_MS).as_millis() <= agent::AWAKE_FOR.as_millis());
};

/// As [`call`], waiting `limit` at most.
pub(super) async fn call_within(
    page: &Page,
    caller: Option<&Caller>,
    method: &str,
    params: Value,
    limit: Duration,
    cx: &mut AsyncApp,
) -> Result<Value, ControlError> {
    let actor = caller.map(Caller::actor);
    let asked = cx.update(|cx| agent::cdp(&page.view, actor.as_ref(), method, params, limit, cx));
    match asked
        .map_err(|error| refusals::failed(error, page.pane))?
        .await
    {
        Answer::Answered(Ok(value)) => Ok(value),
        Answer::Answered(Err(why)) => Err(ControlError::browser_unavailable(why)),
        Answer::Stopped => Err(ControlError::browser_unavailable(
            "The browser engine stopped before it answered.",
        )),
        Answer::TimedOut => Err(refusals::timed_out(method, limit)),
    }
}

/// The page's address and title now.
pub(super) fn now(page: &Page, cx: &App) -> PageOutput {
    let facts = page.view.read(cx).facts(cx);
    PageOutput {
        pane: page.pane,
        url: facts.url,
        title: facts.title,
    }
}

pub(crate) fn list(cx: &mut App) -> Result<Value, ControlError> {
    let (pages, _) = pages(cx)?;
    let pages = (pages.iter())
        .map(|listed| match listed {
            Listed::Page(page) => {
                let facts = page.view.read(cx).facts(cx);
                PageInfo {
                    pane: page.pane,
                    url: facts.url,
                    title: facts.title,
                    loading: facts.loading,
                    worktree: page.worktree.display().to_string(),
                    controller: facts.controller,
                    viewport: facts.viewport,
                }
            }
            Listed::Unshown(info) => info.clone(),
        })
        .collect();
    output(&BrowserListOutput { pages })
}

pub(crate) fn claim(
    caller: &Caller,
    input: &BrowserClaimInput,
    cx: &mut App,
) -> Result<Value, ControlError> {
    let page = resolve(caller, input.pane, cx)?;
    let (actor, takeover, reason) = (
        caller.actor(),
        input.takeover.unwrap_or(false),
        input.reason.clone(),
    );
    (page.view.update(cx, |view, cx| {
        view.agent_claims(actor, takeover, reason, cx)
    }))
    .map_err(refused)?;
    acted(caller, &page, cx);
    output(&Lease {
        pane: page.pane,
        held: true,
    })
}

pub(crate) fn release(
    caller: &Caller,
    named: Option<u64>,
    cx: &mut App,
) -> Result<Value, ControlError> {
    let page = resolve(caller, named, cx)?;
    let actor = caller.actor();
    page.view
        .update(cx, |view, cx| view.agent_releases(&actor, cx));
    output(&Lease {
        pane: page.pane,
        held: false,
    })
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
    let url = browser::address_to_url(&input.url)
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
/// it is then.
fn steer(
    caller: &Caller,
    named: Option<u64>,
    limit: Option<Duration>,
    cx: &mut App,
    act: impl FnOnce(
        &mut BrowserView,
        &Actor,
        &mut gpui_kit::Context<BrowserView>,
    ) -> Result<(), Refused>,
) -> Result<Exec, ControlError> {
    let page = resolve(caller, named, cx)?;
    let actor = caller.actor();
    (page.view.update(cx, |view, cx| act(view, &actor, cx))).map_err(refused)?;
    acted(caller, &page, cx);
    Ok(Exec::Later(cx.spawn(async move |cx: &mut AsyncApp| {
        if let Some(limit) = limit {
            settle(&page, limit, cx).await;
        }
        output(&cx.update(|cx| now(&page, cx)))
    })))
}

/// Waits, up to `limit`, until the page has started and finished loading.
async fn settle(page: &Page, limit: Duration, cx: &mut AsyncApp) {
    let started = Instant::now();
    // The host says the page is loading a moment after it's asked to load.
    cx.background_executor().timer(POLL).await;
    while started.elapsed() < limit {
        if !cx.update(|cx| page.view.read(cx).facts(cx).loading) {
            return;
        }
        cx.background_executor().timer(POLL).await;
    }
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
    Ok(Exec::Later(cx.spawn(async move |cx: &mut AsyncApp| {
        let started = Instant::now();
        loop {
            // What it waits for may need the page's frames, as an animated page's next step
            // does: it's on screen for the engine while the agent waits on it.
            cx.update(|cx| page.view.update(cx, BrowserView::agent_uses));
            let met = met(&page, until, &value, cx).await?;
            if met || started.elapsed() >= limit {
                return output(&BrowserWaitOutput {
                    met,
                    elapsed_ms: started.elapsed().as_millis() as u64,
                });
            }
            cx.background_executor().timer(POLL).await;
        }
    })))
}

/// Whether the page meets the condition now.
async fn met(
    page: &Page,
    until: WaitFor,
    value: &str,
    cx: &mut AsyncApp,
) -> Result<bool, ControlError> {
    let facts = cx.update(|cx| page.view.read(cx).facts(cx));
    let expression = match until {
        WaitFor::Url => return Ok(facts.url.contains(value)),
        WaitFor::Load if facts.loading || !facts.open => return Ok(false),
        WaitFor::Load => "document.readyState === 'complete'".to_owned(),
        WaitFor::Selector => format!("!!document.querySelector({})", script::quote(value)),
        WaitFor::Text => format!(
            "(document.body ? document.body.innerText : '').includes({})",
            script::quote(value)
        ),
    };
    if !facts.open {
        return Ok(false);
    }
    let found = script::evaluate(page, None, &expression, false, cx).await;
    // A page between documents can't answer; it isn't there yet.
    Ok(found.is_ok_and(|value| value == Value::Bool(true)))
}
