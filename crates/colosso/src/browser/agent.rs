//! What app control asks of a browser page for an agent: what the page is, who holds it,
//! and the ways an agent moves it; and the Chrome `DevTools` protocol commands it runs on
//! it, under the page's lease when they act.

use colosso_browser_ipc::AppRequest;
pub use colosso_browser_ipc::Go;
use futures::channel::oneshot;
use gpui_kit::{App, Context, Entity};

use super::control::Holder;
pub use super::control::{Actor, Refused};
use super::host::Browsers;
pub use super::host::CdpResult;
use super::view::BrowserView;

/// Why a protocol command didn't run for an agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentError {
    /// The page's lease keeps the agent out.
    Refused(Refused),
    /// The host hasn't opened the page: its pane hasn't been drawn yet.
    NotOpen,
}

/// A page as an agent sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageFacts {
    pub url: String,
    pub title: String,
    pub loading: bool,
    /// Who drives it: `person`, or the agent's name.
    pub controller: Option<String>,
    /// Whether the host has it open, so commands reach it.
    pub open: bool,
}

impl BrowserView {
    /// The page as an agent sees it.
    pub fn facts(&self, cx: &App) -> PageFacts {
        PageFacts {
            url: self.page.url.clone(),
            title: self.title(),
            loading: self.page.loading,
            controller: match &self.control.holder {
                Some(Holder::Human { .. }) => Some("person".to_owned()),
                Some(Holder::Agent { actor, .. }) => Some(actor.label.clone()),
                None => None,
            },
            open: self.is_open(cx),
        }
    }

    /// Loads `url` for `actor`, which must hold the page or find it free.
    pub fn agent_navigate(
        &mut self,
        actor: &Actor,
        url: String,
        cx: &mut Context<Self>,
    ) -> Result<(), Refused> {
        self.agent_claims(actor.clone(), false, None, cx)?;
        self.navigate(url, cx);
        Ok(())
    }

    /// Moves through the page's history or reloads it for `actor`, which must hold the page
    /// or find it free.
    pub fn agent_go(
        &mut self,
        actor: &Actor,
        to: Go,
        cx: &mut Context<Self>,
    ) -> Result<(), Refused> {
        self.agent_claims(actor.clone(), false, None, cx)?;
        let pane = self.pane;
        self.page.loading = true;
        self.send_to_page(AppRequest::Go { pane, to }, cx);
        cx.notify();
        Ok(())
    }
}

/// Runs a protocol command on `view`'s page: under its lease for `actor` when it acts, freely
/// when it only reads (`actor` is `None`). The answer arrives on the returned channel.
pub fn cdp(
    view: &Entity<BrowserView>,
    actor: Option<&Actor>,
    method: &str,
    params: serde_json::Value,
    cx: &mut App,
) -> Result<oneshot::Receiver<CdpResult>, AgentError> {
    if !view.read(cx).is_open(cx) {
        return Err(AgentError::NotOpen);
    }
    if let Some(actor) = actor {
        (view.update(cx, |view, cx| {
            view.agent_claims(actor.clone(), false, None, cx)
        }))
        .map_err(AgentError::Refused)?;
    }
    let pane = view.read(cx).pane;
    let browsers = Browsers::global(cx);
    Ok(browsers.update(cx, |browsers, _| browsers.call(pane, method, params)))
}
