//! The update pill in the window's top bar, beside the downloads button (`docs/updates.md` 1):
//! there while a newer Koyo is offered, comes in or waits staged, and after an install failed;
//! gone while there's nothing to do. One click takes what it offers. It draws only what
//! `updates::shown` says, and owns no state but whether it's there. A view of its own, so a
//! download's progress redraws it and not the shell, which hears only that it came or went.

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::{
    AnyElement, AnyView, App, Context, Div, Entity, EventEmitter, FontWeight, Hsla,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, div, px,
};

use crate::ring::ring;
use crate::theme;
use crate::updates::{self, Shown};

/// How tall the pill is, as the bar's other buttons are.
const HEIGHT: f32 = 22.;

pub struct UpdateButton {
    /// Whether it's there: the update has something to offer or is under way.
    shown: bool,
    /// None for a build that doesn't update itself, whose pill never comes.
    _moved: Option<Subscription>,
}

/// The pill came, with an update to offer, or went, with nothing left to do.
pub struct CameOrWent;

impl EventEmitter<CameOrWent> for UpdateButton {}

impl UpdateButton {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let moved = updates::observed(cx).map(|updates| {
            cx.observe(&updates, |this: &mut Self, _, cx| {
                let shown = there(cx);
                if shown != this.shown {
                    this.shown = shown;
                    cx.emit(CameOrWent);
                }
                cx.notify();
            })
        });
        UpdateButton {
            shown: there(cx),
            _moved: moved,
        }
    }

    /// `button` as the top bar holds it: nothing while there's nothing to offer, else the view.
    pub fn in_bar(button: &Entity<Self>, cx: &App) -> Option<AnyElement> {
        (button.read(cx).shown).then(|| AnyView::from(button.clone()).into_any_element())
    }
}

/// Whether the update has anything for the top bar.
fn there(cx: &App) -> bool {
    !matches!(updates::shown(cx), Shown::Quiet(_))
}

/// The pill's shape, filled with `fill` under words in `text`, with all there is to say under
/// the pointer.
fn pill(fill: Hsla, text: Hsla, said: SharedString) -> Stateful<Div> {
    div()
        .id("update-pill")
        .debug_selector(|| "update-pill".to_owned())
        .h(px(HEIGHT))
        .px(px(9.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(5.))
        .rounded_full()
        .bg(fill)
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(text)
        .whitespace_nowrap()
        .tooltip(move |window, cx| Tooltip::new(said.clone()).build(window, cx))
}

impl Render for UpdateButton {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        match updates::shown(cx) {
            Shown::Quiet(_) => div().into_any_element(),
            Shown::Offer { said, act } => {
                let color = if said.failed { p.danger } else { p.accent };
                pill(color, gpui_kit::white(), said.whole())
                    .cursor_pointer()
                    .hover(move |pill| pill.bg(color.opacity(0.85)))
                    .on_click(|_, _, cx| updates::act(cx))
                    .child(act)
                    .into_any_element()
            }
            Shown::Working { said, done } => {
                let percent = done.map_or(0, |done| (done * 100.).round() as u8);
                pill(p.hover, p.text, said.whole())
                    .child(ring(percent).size(px(14.)).text_color(p.accent))
                    .child("Updating…")
                    .into_any_element()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{
        AnyWindowHandle, Bounds, Modifiers, Pixels, TestAppContext, VisualTestContext, px,
    };

    use crate::settings::{self, Tabs};
    use crate::test_support::{draw, shell_window};
    use crate::theme;
    use crate::updates::testing::{Koyo, koyo, until_it_says};
    use crate::updates::{self, Shown};

    fn pill(window: AnyWindowHandle, cx: &mut TestAppContext) -> Option<Bounds<Pixels>> {
        draw(window, cx);
        VisualTestContext::from_window(window, cx).debug_bounds("update-pill")
    }

    #[gpui_kit::test]
    fn the_pill_comes_with_an_offer_on_either_top_row_and_a_click_takes_it(cx: &mut TestAppContext) {
        cx.update(crate::test_support::init);
        let koyo = koyo("update-pill", true, cx);
        let (window, _shell) = shell_window(cx);
        assert_eq!(pill(window, cx), None, "there's a pill with nothing to offer");

        koyo.announce("9.0.0");
        Koyo::an_hour_on(cx);
        for tabs in [Tabs::Sidebar, Tabs::TopRail] {
            cx.update(|cx| settings::update(cx, |settings| settings.tabs = tabs));
            let pill = pill(window, cx).expect("an offer shows no pill");
            let toggle = VisualTestContext::from_window(window, cx)
                .debug_bounds("toggle-sidebar")
                .unwrap();
            assert!(pill.left() >= toggle.right(), "the pill isn't after the sidebar's toggle");
            assert!(pill.bottom() <= px(theme::TITLE_BAR_HEIGHT), "the pill isn't in the top row");
        }

        let at = pill(window, cx).unwrap().center();
        let held = koyo.hold_download();
        VisualTestContext::from_window(window, cx).simulate_click(at, Modifiers::none());
        assert!(
            matches!(cx.read(updates::shown), Shown::Working { .. }),
            "a click on the pill didn't start the update"
        );
        assert!(pill(window, cx).is_some(), "the pill went while the update is under way");

        drop(held);
        until_it_says("Retry Update", cx);
        assert!(pill(window, cx).is_some(), "a failed update shows no pill");
    }
}
