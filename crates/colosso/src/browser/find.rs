//! Looking at a page: zoomed a step at a time, Chrome's steps, the zoom shown in the toolbar
//! until it's back to the page's own size; and searched for text in a bar over the page's
//! top right, the matches counted, Return going to the next and Shift-Return back.

use colosso_browser_ipc::{AppRequest, ViewAction};
use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::{
    AppContext as _, Context, Div, Entity, InteractiveElement as _, KeyDownEvent,
    ParentElement as _, Styled as _, Subscription, Window, div, px,
};

use super::view::BrowserView;
use crate::shell::{glass, icon_button};
use crate::theme;

/// Chrome's zoom steps, in percent of the page's own size.
const STEPS: [u32; 17] = [
    25, 33, 50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300, 400, 500,
];

/// The page's own size.
pub(super) const ACTUAL_SIZE: u32 = 100;

/// The find bar: what's searched for, and how many matches there are.
pub(super) struct FindBar {
    input: Entity<InputState>,
    /// The text last searched for: an input reports a change when Return goes by too.
    searched: String,
    count: u32,
    active: u32,
    _typed: Subscription,
}

impl BrowserView {
    /// Zooms `steps` steps in, or out when negative.
    pub(super) fn zoom_by(&mut self, steps: i32, cx: &mut Context<Self>) {
        let at = STEPS
            .iter()
            .position(|step| *step >= self.zoom)
            .unwrap_or(7);
        let last = STEPS.len() - 1;
        let to = if steps < 0 {
            at.saturating_sub(steps.unsigned_abs() as usize)
        } else {
            (at + steps.unsigned_abs() as usize).min(last)
        };
        self.set_zoom(STEPS[to], cx);
    }

    pub(super) fn set_zoom(&mut self, percent: u32, cx: &mut Context<Self>) {
        if self.zoom != percent {
            self.zoom = percent;
            self.send_zoom(cx);
            cx.notify();
        }
    }

    /// Tells the host the pane's zoom. Chromium keeps a zoom per site, so it's told again
    /// whenever a page loads.
    pub(super) fn send_zoom(&self, cx: &mut Context<Self>) {
        let level = (f64::from(self.zoom) / 100.).ln() / 1.2_f64.ln();
        let (pane, action) = (self.pane, ViewAction::Zoom { level });
        self.send_to_page(AppRequest::View { pane, action }, cx);
    }

    /// Opens the find bar with the keyboard in it, or, open, selects what it searches for.
    pub(super) fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(find) = &self.find {
            find.input.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(window, cx);
            });
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Find in page"));
        let typed = cx.subscribe_in(&input, window, |this, input, event, _, cx| {
            let text = input.read(cx).value().to_string();
            let searched = this.find.as_mut().map(|find| &mut find.searched);
            let changed = searched.is_some_and(|searched| {
                let changed = *searched != text;
                searched.clone_from(&text);
                changed
            });
            match event {
                InputEvent::Change if changed => this.find(text, true, cx),
                InputEvent::PressEnter { shift, .. } => this.find(text, !*shift, cx),
                InputEvent::Change | InputEvent::Focus | InputEvent::Blur => {}
            }
        });
        input.update(cx, |input, cx| input.focus(window, cx));
        self.find = Some(FindBar {
            input,
            searched: String::new(),
            count: 0,
            active: 0,
            _typed: typed,
        });
        cx.notify();
    }

    fn find(&mut self, text: String, forward: bool, cx: &mut Context<Self>) {
        let action = if text.is_empty() {
            if let Some(find) = &mut self.find {
                (find.count, find.active) = (0, 0);
            }
            cx.notify();
            ViewAction::StopFind { clear: true }
        } else {
            ViewAction::Find { text, forward }
        };
        let pane = self.pane;
        self.send_to_page(AppRequest::View { pane, action }, cx);
    }

    /// The next match, or the one before.
    pub(super) fn find_again(&mut self, forward: bool, cx: &mut Context<Self>) {
        if let Some(find) = &self.find {
            let text = find.input.read(cx).value().to_string();
            self.find(text, forward, cx);
        }
    }

    /// Closes the find bar, the match selected staying selected, and gives the page the
    /// keyboard back.
    pub(super) fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.take().is_some() {
            let (pane, action) = (self.pane, ViewAction::StopFind { clear: false });
            self.send_to_page(AppRequest::View { pane, action }, cx);
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    /// The find bar's matches and the one selected, while it's open.
    #[cfg(any(test, feature = "visual-check"))]
    pub(super) fn find_tally(&self) -> Option<(u32, u32)> {
        self.find.as_ref().map(|find| (find.count, find.active))
    }

    /// Whether the find bar's field has the keys.
    #[cfg(feature = "visual-check")]
    pub(super) fn find_focused(&self, window: &Window, cx: &gpui_kit::App) -> bool {
        use gpui_kit::Focusable as _;
        (self.find.as_ref())
            .is_some_and(|find| find.input.read(cx).focus_handle(cx).is_focused(window))
    }

    /// What the find bar searches for, while it's open.
    #[cfg(test)]
    pub(super) fn find_text(&self, cx: &gpui_kit::App) -> Option<String> {
        self.find
            .as_ref()
            .map(|find| find.input.read(cx).value().to_string())
    }

    /// How finding went.
    pub(super) fn found(&mut self, count: u32, active: u32, cx: &mut Context<Self>) {
        if let Some(find) = &mut self.find {
            (find.count, find.active) = (count, active);
            cx.notify();
        }
    }

    /// The find bar over the page's top right, while it's open.
    pub(super) fn find_bar(&self, cx: &mut Context<Self>) -> Option<Div> {
        let find = self.find.as_ref()?;
        let p = theme::palette(cx);
        let tally = match (find.input.read(cx).value().is_empty(), find.count) {
            (true, _) => String::new(),
            (false, 0) => "No matches".to_owned(),
            (false, count) => format!("{} of {count}", find.active),
        };
        let bar = div()
            .absolute()
            .top(px(8.))
            .right(px(12.))
            .w(px(340.))
            .h(px(36.))
            .pl_2()
            .pr_1()
            .flex()
            .items_center()
            .gap_1()
            .rounded(px(10.))
            .border_1()
            .border_color(p.hairline)
            .bg(p.floating())
            .shadow_md()
            .occlude()
            .key_context("BrowserFind")
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.close_find(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&find.input).appearance(false).small()),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(p.muted)
                    .child(tally),
            )
            .child(
                icon_button("find-previous", IconName::ChevronUp, "Previous match", cx)
                    .xsmall()
                    .on_click(cx.listener(|this, _, _, cx| this.find_again(false, cx))),
            )
            .child(
                icon_button("find-next", IconName::ChevronDown, "Next match", cx)
                    .xsmall()
                    .on_click(cx.listener(|this, _, _, cx| this.find_again(true, cx))),
            )
            .child(
                icon_button("find-close", IconName::X, "Close", cx)
                    .xsmall()
                    .on_click(cx.listener(|this, _, window, cx| this.close_find(window, cx))),
            );
        Some(bar)
    }

    /// The zoom, in the toolbar while the page isn't at its own size: pressed, it goes back.
    pub(super) fn zoom_chip(&self, cx: &mut Context<Self>) -> Option<Button> {
        (self.zoom != ACTUAL_SIZE).then(|| {
            glass("browser-zoom", theme::palette(cx).text, cx)
                .label(format!("{}%", self.zoom))
                .xsmall()
                .tooltip("Actual size")
                .on_click(cx.listener(|this, _, _, cx| this.set_zoom(ACTUAL_SIZE, cx)))
        })
    }
}
