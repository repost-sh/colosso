//! How a place says what goes on in it (`docs/activity.md`): the dot for what it has for the
//! person, and after it the spinner for its work, with how many subagents work up and right of it
//! once there's more than one, as an exponent is written. Each shows whatever the other does.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Div, Hsla, InteractiveElement as _, IntoElement as _, ParentElement as _, SharedString,
    Styled as _, div, px, relative,
};
use koyo_agent::work::Work;

use super::spinner::{SIZE, spinner};
use crate::pane::dot;
use crate::theme;

/// Between the dot and the spinner.
const APART: f32 = 4.;
/// How large the count is.
const COUNT: f32 = 8.;
/// How far into the spinner's corner, which its ring leaves empty, the count starts.
const CLOSER: f32 = 0.5;
/// How far up from the spinner's top the count is.
const RAISED: f32 = 5.;

/// What a place says: at least one of the two.
pub struct Sign {
    dot: Option<Div>,
    work: Option<Div>,
}

impl Sign {
    /// What the place `named` so says of `work`, and, in a dot of the colour given, of having
    /// something for the person; none while there's nothing to say. Its parts are known to a test
    /// as its name with `-dot`, `-spinner` and `-subagents` after it.
    pub fn new(
        named: impl FnOnce() -> String,
        work: Work,
        waits: Option<Hsla>,
        cx: &mut App,
    ) -> Option<Sign> {
        let pace = work.pace();
        if pace.is_none() && waits.is_none() {
            return None;
        }
        let name = SharedString::from(named());
        let known = |part: &'static str| {
            let name = name.clone();
            move || format!("{name}-{part}")
        };
        let work = pace.map(|pace| {
            let spinner = spinner(name.clone(), (SIZE, pace), cx);
            let muted = theme::palette(cx).muted;
            let count = work.beside.subagents;
            (div().flex().flex_shrink_0().items_start())
                .child(spinner.debug_selector(known("spinner")))
                .when(count > 1, |work| {
                    let count = div()
                        .debug_selector(known("subagents"))
                        .flex_shrink_0()
                        .mt(px(-RAISED))
                        .ml(px(-CLOSER))
                        .text_size(px(COUNT))
                        .line_height(relative(1.))
                        .text_color(muted)
                        .whitespace_nowrap()
                        .child(format!("x{count}"));
                    work.child(count)
                })
        });
        let dot = waits.map(|colour| dot(colour).debug_selector(known("dot")));
        Some(Sign { dot, work })
    }

    /// The sign in a line, as wide as what it says: on a rail tab.
    pub fn in_line(self) -> Div {
        (div().flex().flex_shrink_0().items_center())
            .gap(px(APART))
            .children(self.dot)
            .children(self.work)
    }

    /// The sign at a row's end, in the one column the rows' signs share: as wide as the spinner,
    /// or as the dot alone, so either is in the column's middle, and the rest hangs out of it, the
    /// dot before the spinner and the count after.
    pub fn in_column(self) -> Div {
        let Some(work) = self.work else {
            return (div().flex_shrink_0()).children(self.dot);
        };
        let dot = self.dot.map(|dot| {
            (div().absolute().top_0().bottom_0().right(px(SIZE + APART)))
                .flex()
                .items_center()
                .child(dot)
                .into_any_element()
        });
        (div().relative().flex_shrink_0().w(px(SIZE)))
            .children(dot)
            .child(work)
    }
}
