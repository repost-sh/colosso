//! A page's lease, claimed and let go; and the person asked about one of their own pages,
//! pinned or loose (`pages/consent.rs`): `browser_claim` asks to use it, `browser_duplicate` for
//! a copy in the caller's worktree, each in the window's prompt (`confirm.rs`) naming the caller
//! as it says it is, in Koyo's words (`asker.rs`). A question waiting on the person holds the
//! page only weakly. Turned down, or left without an answer, the page isn't asked about again,
//! for anyone, for 10 minutes.

use gpui_kit::{App, AsyncApp, Context, Task, Window};
use koyo_app_control::ops::browser::{BrowserClaimInput, BrowserDuplicateInput, Lease};
use koyo_app_control::ops::shared::{PaneKind, PaneSpec};
use koyo_app_control::{ControlError, Scope};
use serde_json::Value;

use super::super::super::activity::{ActivityKind, Decision};
use super::super::super::confirm::{self, Question};
use super::super::super::{Control, gate, policy};
use super::super::{
    Caller, Exec, beside, info, main_shell, open, output, try_update_shell, window_closed,
};
use super::asker::{self, REASON, TITLE, clean};
use super::page::{acted, refused};
use super::pages::{self, Found, Persons};
use crate::browser;
use crate::browser::profiles::{self, By};
use crate::history;
use crate::model::Spot;
use crate::shell::Shell;

pub(crate) fn claim(
    caller: &Caller,
    input: &BrowserClaimInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let wish = (input.takeover.unwrap_or(false), input.reason.clone());
    let persons = match pages::find(caller, input.pane, cx)? {
        Found::Page(page) => {
            let (actor, (takeover, reason)) = (caller.actor(), wish);
            (page.view()?.update(cx, |view, cx| {
                view.agent_claims(actor, takeover, reason, cx)
            }))
            .map_err(refused)?;
            acted(caller, page.pane(), cx);
            return Ok(Exec::Ready(lease(page.pane(), true)));
        }
        Found::Persons(persons) => persons,
    };
    if persons.renew(caller, wish.clone(), cx)? {
        return Ok(Exec::Ready(lease(persons.pane, true)));
    }
    persons.may_ask(caller, wish.0, cx)?;
    let question = to_use(caller, &persons, (wish.0, wish.1.as_deref()), cx)?;
    let asking = ask(caller, (persons.pane, &question), "browser_claim", cx)?;
    let caller = caller.clone();
    Ok(Exec::Later(cx.spawn(async move |cx: &mut AsyncApp| {
        let decision = asking.await;
        cx.update(|cx| {
            allowed(&caller, persons.pane, decision, (&question, &[Scope::Browser]), cx)?;
            match pages::find(&caller, Some(persons.pane), cx)? {
                Found::Persons(now) if now.same(&persons) => now.grant(&caller, wish, cx)?,
                _ => return Err(moved(persons.pane)),
            }
            acted(&caller, persons.pane, cx);
            lease(persons.pane, true)
        })
    })))
}

/// `caller` lets the page `named` names go: a worktree's, or the person's, whose consent ends
/// with it. Always allowed.
pub(crate) fn release(
    caller: &Caller,
    named: Option<u64>,
    cx: &mut App,
) -> Result<Value, ControlError> {
    let pane = match pages::find(caller, named, cx)? {
        Found::Page(page) => {
            let actor = caller.actor();
            (page.view()?).update(cx, |view, cx| view.agent_releases(&actor, cx));
            page.pane()
        }
        Found::Persons(persons) => {
            persons.release(caller, cx);
            persons.pane
        }
    };
    lease(pane, false)
}

fn lease(pane: u64, held: bool) -> Result<Value, ControlError> {
    output(&Lease { pane, held })
}

/// Asks the person for a copy of their page in `caller`'s worktree; allowed, opens it there as
/// a page `caller` opens, at the address the page had when they were asked: its place.
pub(crate) fn duplicate(
    caller: &Caller,
    input: &BrowserDuplicateInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let pane = input.pane;
    let persons = match pages::find(caller, Some(pane), cx)? {
        Found::Persons(persons) => persons,
        Found::Page(_) => {
            return Err(ControlError::invalid_input(format!(
                "Pane {pane} is a worktree's page, not the person's: tab_open with kind browser and its url opens another."
            )));
        }
    };
    pages::may_ask(caller, pane, cx)?;
    if !pages::still_open(caller, cx) {
        return Err(ControlError::invalid_input(
            "This call has no connection of Koyo's to tie the person's answer to; call through Koyo's MCP server or control socket.",
        ));
    }
    if !gate::grant(caller, cx)?.contains(&Scope::Navigate) {
        return Err(ControlError::scope_denied(Scope::Navigate));
    }
    let (_, shell) = main_shell(cx).ok_or_else(window_closed)?;
    let at = asker::worktree(caller, shell.read(cx), cx).ok_or_else(no_worktree)?;
    let question = to_copy(caller, &persons, at, input.reason.as_deref(), cx)?;
    let asking = ask(caller, (pane, &question), "browser_duplicate", cx)?;
    let caller = caller.clone();
    Ok(Exec::Later(cx.spawn(async move |cx: &mut AsyncApp| {
        let decision = asking.await;
        cx.update(|cx| {
            let scopes = [Scope::Browser, Scope::Navigate];
            allowed(&caller, pane, decision, (&question, &scopes), cx)?;
            if !pages::still_open(&caller, cx) {
                return Err(pages::gone());
            }
            let unchanged = match pages::find(&caller, Some(pane), cx)? {
                Found::Persons(now) => now.same(&persons) && now.url == persons.url,
                Found::Page(_) => false,
            };
            if !unchanged {
                return Err(ControlError::browser_unavailable(format!(
                    "Pane {pane}'s page closed, moved or went to another address while the person was asked, so nothing was copied; ask again."
                ))
                .retryable(false));
            }
            try_update_shell(cx, |shell, window, cx| {
                let at = asker::worktree(&caller, shell, cx).ok_or_else(no_worktree)?;
                let copy = copy(shell, &caller, at, persons.url.clone(), window, cx)?;
                open::agents_page_made(shell, copy, cx);
                open::open_agents_page(shell, Some(&caller), copy, window, cx);
                output(&info::place(shell, copy)?)
            })
        })
    })))
}

/// Opens a page of `caller`'s at `url` on the checkout at `at`, as a page it opens does: in the
/// mode the person set for agents' pages, right after its own tab when that's there, else after
/// the tab the checkout shows, out of sight unless the person has the window switch to what
/// agents open; its profile the checkout's project's (`Shell::browser_view`). Its pane.
fn copy(
    shell: &mut Shell,
    caller: &Caller,
    at: (usize, usize),
    url: String,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> Result<u64, ControlError> {
    let spec = PaneSpec {
        kind: PaneKind::Browser,
        url: None,
        file: None,
        line: None,
        column: None,
        terminal: None,
        name: None,
        command: None,
        viewport: None,
    };
    let mode = open::page_mode(caller, &spec, window, cx)?;
    let shown = policy::shows(None, cx);
    let own = (beside::own_tab(shell, caller))
        .filter(|&(project, branch, _)| (project, branch) == at);
    let opened = open::on_checkout(
        shell,
        at,
        (shown, false),
        window,
        cx,
        |shell, _, window, cx| {
            if let Some((project, branch, tab)) = own
                && let Some(unit) = shell.workspace.unit_mut(project, branch)
            {
                unit.active_tab = tab;
            }
            shell.open_browser_tab(Some(url), true, mode, window, cx)
        },
    );
    opened.ok_or_else(|| ControlError::internal("The copy of the page didn't open."))
}

/// The profile the person's page in pane `pane` is of, wherever it is.
fn page_profile<'a>(shell: &Shell, pane: u64, cx: &'a App) -> Option<&'a str> {
    let workspace = &shell.workspace;
    let by = workspace.pane(pane)?.by();
    let project = match workspace.spot_of(pane)? {
        Spot::Checkout(project, ..) => workspace.projects.get(project)?.path.clone(),
        Spot::Pinned | Spot::Loose(_) => {
            let (_, project) = workspace.find_apart(|found| found.id == pane)?;
            project.to_owned()
        }
    };
    Some(browser::profile(&project, by, cx))
}

fn no_worktree() -> ControlError {
    ControlError::not_found(
        "browser_duplicate copies a page into your own worktree, and you run in no checkout of a project in Koyo (KOYO_WORKTREE, or a working folder in one). tab_open with kind browser and the page's url opens it in a worktree you name.",
    )
}

fn moved(pane: u64) -> ControlError {
    ControlError::not_found(format!(
        "Pane {pane} closed or moved while the person was asked; browser_list lists the pages."
    ))
}

/// Asks the person `question` about pane `pane`'s page for `caller`'s call of `operation`,
/// logged as a confirmation is: what they answered. Nobody else is asked about the page
/// meanwhile.
fn ask(
    caller: &Caller,
    (pane, question): (u64, &Question),
    operation: &'static str,
    cx: &mut App,
) -> Result<Task<Decision>, ControlError> {
    let key = caller.key();
    let asking = confirm::ask(caller, question, cx)?;
    pages::asking(pane, true, cx);
    let message = question.message.clone();
    Ok(cx.spawn(async move |cx: &mut AsyncApp| {
        let decision = asking.await;
        cx.update(|cx| {
            pages::asking(pane, false, cx);
            let kind = ActivityKind::Confirmation {
                operation: operation.to_owned(),
                question: message,
                decision,
            };
            Control::get_mut(cx).activity.record(key, kind);
            decision
        })
    }))
}

/// Goes on when the person allowed it and `caller` may still make the call, as `scopes` say;
/// turned down, or no answer in time, `caller` isn't let ask about pane `pane`'s page again for
/// a while, nor anyone else for a minute (`pages/asks.rs`).
fn allowed(
    caller: &Caller,
    pane: u64,
    decision: Decision,
    (question, scopes): (&Question, &[Scope]),
    cx: &mut App,
) -> Result<(), ControlError> {
    let declined = |how: &str, cx: &mut App| {
        pages::declined(caller, pane, cx);
        Err(ControlError::user_declined(format!(
            "The person {how}: {}. Koyo doesn't ask them about pane {pane} for you again for 10 minutes.",
            question.message
        )))
    };
    match decision {
        Decision::Allowed => {
            let granted = gate::grant(caller, cx)?;
            match scopes.iter().find(|scope| !granted.contains(scope)) {
                Some(missing) => Err(ControlError::scope_denied(*missing)),
                None => Ok(()),
            }
        }
        Decision::Denied => declined("cancelled it", cx),
        Decision::TimedOut => declined("didn't answer in time", cx),
    }
}

/// What the person is asked before `caller` may use their page.
fn to_use(
    caller: &Caller,
    persons: &Persons,
    (takeover, reason): (bool, Option<&str>),
    cx: &App,
) -> Result<Question, ControlError> {
    let (_, shell) = main_shell(cx).ok_or_else(window_closed)?;
    let asker = asker::named(caller, shell.read(cx), cx);
    let (title, site) = shown_as(persons);
    let what = if takeover {
        "It can read the page, run script in it, click, type and navigate it, signed in as you, until it lets go, you take the page back, or 10 minutes pass without it acting. If another agent uses the page now, it takes the page from that agent."
    } else {
        "It can read the page, run script in it, click, type and navigate it, signed in as you, until it lets go, you take the page back, or 10 minutes pass without it acting."
    };
    Ok(Question {
        message: format!("{asker} wants to use your page “{title}” ({site})"),
        detail: Some(said(what, reason)),
        action: "Allow",
    })
}

/// What the person is asked before a copy of their page opens in `caller`'s worktree at `at`:
/// whether the copy, an agent's page, is signed in as the page is, its profile and the page's
/// being the same; with the agents' own sign-ins, agents' pages kept apart; or with the
/// worktree's own.
fn to_copy(
    caller: &Caller,
    persons: &Persons,
    at: (usize, usize),
    reason: Option<&str>,
    cx: &App,
) -> Result<Question, ControlError> {
    let (_, shell) = main_shell(cx).ok_or_else(window_closed)?;
    let shell = shell.read(cx);
    let asker = asker::named(caller, shell, cx);
    let (title, site) = shown_as(persons);
    let worktree = asker::label(shell, at);
    let copy_profile = browser::profile(&shell.workspace.projects[at.0].path, By::Agent, cx);
    let signed_in = if copy_profile == profiles::AGENTS {
        "with the agents' own sign-ins, not yours"
    } else if page_profile(shell, persons.pane, cx) == Some(copy_profile) {
        "signed in as you, as this page is"
    } else {
        "with that worktree's own sign-ins"
    };
    Ok(Question {
        message: format!(
            "{asker} asks for a copy of your page “{title}” ({site}) in its worktree {worktree}"
        ),
        detail: Some(said(
            &format!(
                "Only its address is copied: a new tab of that worktree opens at it, {signed_in}. Your page stays as it is."
            ),
            reason,
        )),
        action: "Allow",
    })
}

/// The page's title, its host when it has none, and its host, its address when it has none:
/// as a prompt may show them.
fn shown_as(persons: &Persons) -> (String, String) {
    let site = history::host(&persons.url).unwrap_or_else(|| persons.url.clone());
    let (title, site) = (clean(&persons.title, TITLE), clean(&site, TITLE));
    let title = if title.is_empty() { site.clone() } else { title };
    (title, site)
}

/// `detail`, and the caller's reason after it as its own words, when it gave one.
fn said(detail: &str, reason: Option<&str>) -> String {
    match reason.map(|reason| clean(reason, REASON)) {
        Some(reason) if !reason.is_empty() => {
            format!("{detail} It says why, in its own words: “{reason}”")
        }
        _ => detail.to_owned(),
    }
}
