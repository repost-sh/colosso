//! Whether the page is on screen for the engine: while its pane draws it with nothing over it
//! (`page.rs`, `Shown`), and while an agent uses it, drawn or not, so a page out of sight works
//! for the browser tools as if shown (`docs/app-control.md` 5): its animation frames run,
//! input lands at once and screenshots answer. Hidden, Chromium paints nothing, slows its
//! timers, and holds back what waits on a frame; a page nobody draws or uses is hidden.

use std::time::Duration;

use gpui_kit::{Context, WeakEntity};
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

/// How long a page out of sight stays on screen for the engine after an agent last used it:
/// longer than any one browser tool's call takes (its longest command, a script whose
/// promise is awaited, waits 90 s), so a page never goes out of sight while one is under
/// way; and longer than an agent takes between its commands, so a page it works on doesn't
/// flicker in and out of sight, and what the page goes on with after a command (a submit
/// that navigates at the next frame, a transition) gets its frames.
pub(crate) const AWAKE_FOR: Duration = Duration::from_secs(90);

impl BrowserView {
    /// An agent uses the page: it's on screen for the engine, drawn or not, until no agent
    /// has used it for [`AWAKE_FOR`].
    pub(crate) fn agent_uses(&mut self, cx: &mut Context<Self>) {
        let until = cx.background_executor().now() + AWAKE_FOR;
        if let Some((deadline, _)) = &mut self.awake {
            *deadline = until;
            return;
        }
        let task = cx.spawn(async move |this, cx| {
            if sleep_until(&this, |this| Some(this.awake.as_ref()?.0), cx).await {
                let _ = this.update(cx, |this, cx| {
                    this.awake = None;
                    this.tell_sight(cx);
                });
            }
        });
        self.awake = Some((until, task));
        self.tell_sight(cx);
    }

    /// Whether the page is in sight: its pane draws it, in a window seen ([`crate::sight`]).
    pub(in crate::browser) fn drawn(&self) -> bool {
        self.shown.is_some() && (self.window_visible || crate::sight::unseen_counts())
    }

    /// Whether the page is on screen for the engine: drawn, or used by an agent lately.
    pub(in crate::browser) fn on_screen(&self) -> bool {
        self.drawn() || self.awake.is_some()
    }

    /// Tells the host whether the page is on screen ([`BrowserView::on_screen`]), and, as
    /// the person comes to see it, that it's the page they're on, until another has the
    /// keys. The two differ: a page an agent opened is on screen for the engine before its
    /// pane draws it, and was never told as the one the person is on.
    pub(in crate::browser) fn tell_sight(&mut self, cx: &mut Context<Self>) {
        self.set_visible(self.on_screen(), cx);
        let drawn = self.drawn();
        if drawn {
            self.watch_tone(cx);
        } else {
            self.tone_watch = None;
        }
        if drawn && !std::mem::replace(&mut self.seen, drawn) {
            self.tell_active(cx);
        }
        self.seen = drawn;
    }

    /// Tells the host whether the page is on screen, when that changed; out of sight it
    /// stops painting.
    pub(in crate::browser) fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible.replace(visible) != Some(visible) {
            let pane = self.pane;
            self.send_to_page(AppRequest::Visible { pane, visible }, cx);
        }
    }
}
