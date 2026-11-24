//! Whether the page is on screen for the engine: while its pane draws it with nothing over it
//! (`page.rs`, `Shown`), and while an agent's command on it runs and a few seconds after,
//! drawn or not, so a page out of sight works for the browser tools as if shown
//! (`docs/app-control.md` 5): its animation frames run, input lands at once and screenshots
//! answer. Hidden, Chromium paints nothing, slows its
//! timers, and holds back what waits on a frame; a page nobody draws or uses is hidden.

use std::time::{Duration, Instant};

use gpui_kit::{App, Context, Task, WeakEntity};
use koyo_browser_ipc::AppRequest;

use super::BrowserView;
use crate::browser::control::sleep_until;

/// A page drawn in a window, kept by the page element for as long as it's drawn: once it
/// isn't, the page is out of sight, and the engine stops painting it, unless it's drawn
/// elsewhere since or an agent is using it.
pub struct Shown;

impl Shown {
    pub fn new(page: WeakEntity<BrowserView>, cx: &mut Context<Self>) -> Self {
        let me = cx.entity_id();
        cx.on_release(move |_: &mut Self, cx| {
            let _ = page.update(cx, |page, cx| {
                if page
                    .shown
                    .as_ref()
                    .is_some_and(|shown| shown.entity_id() == me)
                {
                    page.shown = None;
                    page.tell_sight(cx);
                }
            });
        })
        .detach();
        Shown
    }
}

/// How long a page out of sight stays on screen for the engine after an agent's command on it
/// ended, or after an agent used it otherwise (claimed it, opened it): long enough for what
/// the page goes on with after a command to get its frames (a submit that navigates at the
/// next frame, a transition), short enough that a page an agent left doesn't run on as if
/// shown. Each command wakes the page again before it runs.
pub(crate) const AWAKE_AFTER: Duration = Duration::from_secs(5);

/// How long after an agent last used a page out of sight what the page opens is the agent's,
/// and opens behind ([`BrowserView::shows_what_it_opens`]): longer than an agent takes
/// between its commands, so a page that opens a tab a while after the agent's click (a
/// sign-in's) doesn't bring it in front of the person.
const AGENTS_FOR: Duration = Duration::from_secs(90);

/// What the app tells the host of a page's sight (`AppRequest::Sight`): whether an agent keeps
/// it on screen for the engine, a pane drawing it or not, and whether the window its pane
/// draws it in counts as seen. Where a pane draws it is the host's to know.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Told {
    pub(crate) kept: bool,
    pub(crate) seen: bool,
}

/// An agent's use of a page, which keeps it on screen for the engine though no pane draws it.
#[derive(Default)]
pub(crate) struct Awake {
    /// When the page rests, while it's awake, and the task that rests it then.
    until: Option<(Instant, Task<()>)>,
    /// The agents' commands on the page under way.
    commands: usize,
    /// When an agent last used the page.
    used: Option<Instant>,
}

impl BrowserView {
    /// An agent uses the page: it's on screen for the engine, drawn or not, for
    /// [`AWAKE_AFTER`] at least.
    pub(crate) fn agent_uses(&mut self, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        self.awake.used = Some(now);
        self.wake_until(now + AWAKE_AFTER, cx);
    }

    /// An agent's command on the page starts, which waits `limit` at most: the page is on
    /// screen for the engine until [`AWAKE_AFTER`] after the last command under way ends
    /// ([`Self::agent_command_ends`]), or after it could have, if it's never heard to.
    pub(crate) fn agent_command_starts(&mut self, limit: Duration, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        self.awake.commands += 1;
        self.awake.used = Some(now);
        self.wake_until(now + limit + AWAKE_AFTER, cx);
    }

    /// An agent's command on the page ended, answered or not.
    pub(crate) fn agent_command_ends(&mut self, cx: &mut Context<Self>) {
        self.awake.commands = self.awake.commands.saturating_sub(1);
        if self.awake.commands > 0 || self.awake.until.is_none() {
            return;
        }
        self.awake.until = None;
        let now = cx.background_executor().now();
        self.wake_until(now + AWAKE_AFTER, cx);
    }

    /// Keeps the page on screen for the engine until `at` at least, and rests it then.
    fn wake_until(&mut self, at: Instant, cx: &mut Context<Self>) {
        if let Some((until, _)) = &mut self.awake.until {
            *until = (*until).max(at);
            return;
        }
        let task = cx.spawn(async move |this, cx| {
            if sleep_until(&this, |this| Some(this.awake.until.as_ref()?.0), cx).await {
                let _ = this.update(cx, |this, cx| {
                    this.awake.until = None;
                    this.awake.commands = 0;
                    this.tell_sight(cx);
                });
            }
        });
        self.awake.until = Some((at, task));
        self.tell_sight(cx);
    }

    /// Whether the page is in sight: its pane draws it, in a window seen ([`crate::sight`]).
    pub(in crate::browser) fn drawn(&self) -> bool {
        self.shown.is_some() && (self.window_visible || crate::sight::unseen_counts())
    }

    /// Whether the page is on screen for the engine: drawn, or used by an agent just now.
    pub(in crate::browser) fn on_screen(&self) -> bool {
        self.drawn() || self.awake.until.is_some()
    }

    /// Whether a tab the page opens in front is shown: it is, unless an agent used the page
    /// out of the person's sight in the last [`AGENTS_FOR`], when it's what an agent opened,
    /// shown only if the person set what agents open to show (`docs/app-control.md`).
    pub(in crate::browser) fn shows_what_it_opens(&self, cx: &App) -> bool {
        let now = cx.background_executor().now();
        let agents = (self.awake.used).is_some_and(|used| now.saturating_duration_since(used) < AGENTS_FOR);
        self.drawn() || !agents || crate::control::policy(cx).show_what_agents_open
    }

    /// Tells the host what only the app knows of the page's sight ([`Told`]), and, as the
    /// person comes to see it, that it's the page they're on, until another has the keys. The
    /// host knows itself whether a pane draws the page, and with this whether it's on screen
    /// for the engine ([`BrowserView::on_screen`]): a page an agent opened is on screen for the
    /// engine before its pane draws it, and was never told as the one the person is on.
    pub(in crate::browser) fn tell_sight(&mut self, cx: &mut Context<Self>) {
        self.set_sight(self.sight(), cx);
        let drawn = self.drawn();
        if drawn && !std::mem::replace(&mut self.seen, drawn) {
            self.tell_active(cx);
        }
        self.seen = drawn;
    }

    /// What the host is to be told of the page's sight now: whether an agent keeps it, and
    /// whether its window counts as seen.
    pub(in crate::browser) fn sight(&self) -> Told {
        let seen = self.window_visible || crate::sight::unseen_counts();
        Told { kept: self.awake.until.is_some(), seen }
    }

    /// Tells the host `told`, when it changed.
    pub(in crate::browser) fn set_sight(&mut self, told: Told, cx: &mut Context<Self>) {
        if self.told.replace(told) != Some(told) {
            let (pane, Told { kept, seen }) = (self.pane, told);
            self.send_to_page(AppRequest::Sight { pane, kept, seen }, cx);
        }
    }
}

#[cfg(test)]
mod tests;
