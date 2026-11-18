//! The browser panes the tools see, every worktree's and the person's own, pinned and loose
//! (`docs/arc-sidebar.md` 11), and the one a call means: the pane it names; else, never one of
//! the person's, the page the caller acted on last, the shown tab's, or the only one.
//! `browser_list` lists them all; of the person's, who holds one is said only as the caller
//! (`you`), another agent, or the person, and what it asks only to a caller they let use it.

use std::path::PathBuf;

use gpui_kit::{App, Entity};
use koyo_app_control::ControlError;
use koyo_app_control::ops::browser::{
    BrowserListOutput, Consent, PageInfo, PageOwner, PagePlace,
};
use serde_json::Value;

use super::super::super::{Caller, main_shell, output, window_closed};
use super::super::page;
use super::super::refusals;
use super::{Admission, Page, Persons, grants};
use crate::browser::{BrowserView, agent, profiles};
use crate::model::{Spot, Tab};

/// Whose a browser pane is.
#[derive(Clone)]
enum Owner {
    /// A worktree's, at its checkout.
    Worktree(PathBuf),
    /// The person's own: pinned, or loose in a space.
    Persons(Spot),
    /// The person's own in a worktree, at its checkout: one they opened, signed in as them,
    /// while agents' pages are kept apart (`docs/browser.md` 15.7).
    PersonsWorktree(PathBuf, Spot),
}

/// A browser pane as the tools see it: its page, or none to act on yet for a tab brought back
/// at launch and not shown since, or a pinned page closed, listed as it was kept
/// (`docs/browser.md` 6).
#[derive(Clone)]
struct Listed {
    pane: u64,
    view: Option<Entity<BrowserView>>,
    /// The address and title of one with no page.
    kept: (String, String),
    owner: Owner,
    /// The address a pinned page is pinned at.
    home: Option<String>,
}

/// What a call names or means: a worktree's page, or one of the person's, which only the gate
/// opens (`consent.rs`).
pub(in super::super) enum Found {
    Page(Page),
    Persons(Persons),
}

impl Listed {
    /// What `caller`'s call finds in it: a worktree's page, refused while it has none; or the
    /// person's.
    fn found(&self, caller: &Caller, cx: &App) -> Result<Found, ControlError> {
        match (&self.owner, &self.view) {
            (Owner::Worktree(_), Some(view)) => {
                let admitted = Admission::Checkout(caller.clone());
                Ok(Found::Page(Page::new(self.pane, view, admitted)))
            }
            (Owner::Worktree(_), None) => Err(refusals::unshown(self.pane)),
            (Owner::Persons(spot) | Owner::PersonsWorktree(_, spot), view) => {
                Ok(Found::Persons(self.persons(*spot, view.as_ref(), cx)))
            }
        }
    }

    fn persons(&self, spot: Spot, view: Option<&Entity<BrowserView>>, cx: &App) -> Persons {
        let shown = view.map(|view| {
            let facts = view.read(cx).facts(cx);
            (facts.url, facts.title)
        });
        let shown = shown.unwrap_or_else(|| self.kept.clone());
        Persons::new((self.pane, spot), shown, self.home.clone(), view)
    }

    /// As `browser_list` lists it for `caller`.
    fn info(&self, caller: &Caller, cx: &App) -> PageInfo {
        let (place, worktree, spot) = match &self.owner {
            Owner::Worktree(path) => (PagePlace::Worktree, Some(path.display().to_string()), None),
            Owner::PersonsWorktree(path, spot) => (
                PagePlace::Worktree,
                Some(path.display().to_string()),
                Some(*spot),
            ),
            Owner::Persons(spot @ Spot::Pinned) => (PagePlace::Pinned, None, Some(*spot)),
            Owner::Persons(spot) => (PagePlace::Loose, None, Some(*spot)),
        };
        let granted = spot.is_some_and(|spot| {
            self.persons(spot, self.view.as_ref(), cx).granted(caller, cx)
        });
        let whose = PageOwner {
            worktree,
            place,
            persons: spot.is_some(),
            consent: if granted { Consent::Granted } else { Consent::None },
        };
        let Some(view) = &self.view else {
            return PageInfo {
                pane: self.pane,
                url: self.kept.0.clone(),
                title: self.kept.1.clone(),
                loading: false,
                open: false,
                on_screen: false,
                whose,
                controller: None,
                viewport: agent::unshown_viewport(),
                dialog: None,
            };
        };
        let facts = view.read(cx).facts(cx);
        let (controller, dialog) = match spot {
            None => (facts.controller, facts.dialog),
            Some(_) => (
                holder(self.pane, view, caller, cx),
                facts.dialog.filter(|_| granted),
            ),
        };
        PageInfo {
            pane: self.pane,
            url: facts.url,
            title: facts.title,
            loading: facts.loading,
            open: facts.open,
            on_screen: facts.on_screen,
            whose,
            controller,
            viewport: facts.viewport,
            dialog,
        }
    }
}

/// Who holds the person's page `view`, in pane `pane`, as `caller` is told: `you` while it
/// holds it under its own consent, on its connection; `agent` for any other client, another
/// connection of the same name too; or `person`.
fn holder(pane: u64, view: &Entity<BrowserView>, caller: &Caller, cx: &App) -> Option<String> {
    let shown = view.read(cx);
    let said = match shown.agent_holding() {
        Some(_) if grants::covered_mine(caller, pane, view, cx) => "you",
        Some(_) => "agent",
        None if shown.person_holding() => "person",
        None => return None,
    };
    Some(said.to_owned())
}

/// Every browser pane in the window, `DevTools` left out: every worktree's, then the pinned,
/// then each space's loose pages; and which is in the worktree's tab shown.
fn pages(cx: &App) -> Result<(Vec<Listed>, Option<u64>), ControlError> {
    let (_, shell) = main_shell(cx).ok_or_else(window_closed)?;
    let workspace = &shell.read(cx).workspace;
    let separate = profiles::get(cx).separate();
    let mut pages = Vec::new();
    for (project_at, project) in workspace.projects.iter().enumerate() {
        for (branch, unit) in project.branches.iter().enumerate() {
            let path = &unit.worktree.path;
            for (at, tab) in unit.tabs.iter().enumerate() {
                let spot = Spot::Checkout(project_at, branch, at);
                let theirs = separate.then(|| Owner::PersonsWorktree(path.clone(), spot));
                let owners = (&Owner::Worktree(path.clone()), theirs.as_ref());
                add_checkouts(&mut pages, tab, owners, cx);
            }
        }
    }
    for pinned in workspace.pinned() {
        let owner = Owner::Persons(Spot::Pinned);
        add(&mut pages, &pinned.tab, (&owner, Some(&pinned.home)), cx);
    }
    for (space, loose, _) in workspace.loose_by_space() {
        let owner = Owner::Persons(Spot::Loose(space));
        for page in loose {
            add(&mut pages, &page.tab, (&owner, None), cx);
        }
    }
    let shown = (workspace.shown_tab())
        .and_then(|at| workspace.tab(at))
        .and_then(|tab| {
            (tab.panes().into_iter())
                .find(|pane| pane.address().is_some())
                .map(|pane| pane.id)
        });
    Ok((pages, shown))
}

/// Lists the pages of a checkout's `tab`: an agent's the worktree's, and one the person opened
/// `theirs` when that's given, as it is while agents' pages are kept apart; else the
/// worktree's too.
fn add_checkouts(
    pages: &mut Vec<Listed>,
    tab: &Tab,
    (worktree, theirs): (&Owner, Option<&Owner>),
    cx: &App,
) {
    let from = pages.len();
    add(pages, tab, (worktree, None), cx);
    let Some(theirs) = theirs else {
        return;
    };
    let persons: Vec<u64> = (tab.panes().into_iter())
        .filter(|pane| !pane.by_agent)
        .map(|pane| pane.id)
        .collect();
    for listed in &mut pages[from..] {
        if persons.contains(&listed.pane) {
            listed.owner = theirs.clone();
        }
    }
}

/// Lists the pages of `tab`, `owner`'s, pinned at `home` when it's a pinned page.
fn add(pages: &mut Vec<Listed>, tab: &Tab, (owner, home): (&Owner, Option<&String>), cx: &App) {
    for pane in tab.panes() {
        let view = pane.view().filter(|view| !view.read(cx).is_inspector());
        let kept = pane.unloaded_address();
        if view.is_none() && kept.is_none() {
            continue;
        }
        pages.push(Listed {
            pane: pane.id,
            view: view.cloned(),
            kept: (kept.unwrap_or_default().to_owned(), pane.title.to_string()),
            owner: owner.clone(),
            home: home.cloned(),
        });
    }
}

/// The page `pane` names; else, of the worktrees' only, the one `caller` acted on last, else
/// the shown tab's, else the only one. A person's page is never one a call means unnamed.
pub(in super::super) fn find(
    caller: &Caller,
    pane: Option<u64>,
    cx: &App,
) -> Result<Found, ControlError> {
    let (pages, shown) = pages(cx)?;
    if let Some(id) = pane {
        let named = pages.iter().find(|listed| listed.pane == id);
        let named = named.ok_or_else(|| {
            ControlError::not_found(format!(
                "Pane {id} isn't a browser page; browser_list lists them."
            ))
        })?;
        return named.found(caller, cx);
    }
    let worktrees: Vec<&Listed> = (pages.iter())
        .filter(|listed| matches!(listed.owner, Owner::Worktree(_)))
        .collect();
    let find = |id: u64| worktrees.iter().find(|listed| listed.pane == id).copied();
    let last = page::last(caller, cx).and_then(find);
    if let Some(listed) = last.or_else(|| shown.and_then(find)) {
        return listed.found(caller, cx);
    }
    match worktrees.as_slice() {
        [] => Err(ControlError::not_found(
            "There's no browser page of a worktree; tab_open with kind browser opens one. The person's own pages are only used by name (`pane`), with their consent.",
        )),
        [only] => only.found(caller, cx),
        many => Err(ControlError::ambiguous(
            "Several browser pages are open; name one with `pane`.",
            &many
                .iter()
                .map(|listed| listed.pane.to_string())
                .collect::<Vec<_>>(),
        )),
    }
}

pub(crate) fn list(caller: &Caller, cx: &mut App) -> Result<Value, ControlError> {
    let (pages, _) = pages(cx)?;
    let pages = pages.iter().map(|listed| listed.info(caller, cx)).collect();
    output(&BrowserListOutput { pages })
}
