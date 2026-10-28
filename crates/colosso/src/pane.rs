//! Placeholder tab contents: what each pane kind will roughly look like.

use gpui_kit::component::Icon;
use gpui_kit::{
    AnyElement, IntoElement, ParentElement as _, SharedString, Styled as _, div, px, relative,
};

use crate::model::{Pane, PaneKind, Tab};
use crate::theme;

const MONO: &str = "Menlo";

pub fn tab(tab: &Tab, context: SharedString) -> AnyElement {
    match tab {
        Tab::Single(pane) => self::pane(pane, context),
        Tab::Split(left, right) => div()
            .size_full()
            .flex()
            .child(div().flex_1().min_w_0().child(pane(left, context.clone())))
            .child(div().w(px(1.)).h_full().bg(theme::hairline()))
            .child(div().flex_1().min_w_0().child(pane(right, context)))
            .into_any_element(),
    }
}

fn pane(pane: &Pane, context: SharedString) -> AnyElement {
    match pane.kind {
        PaneKind::Chat => chat(pane, context),
        PaneKind::Terminal => terminal(),
        PaneKind::Browser => browser(pane),
        PaneKind::Editor => editor(pane),
    }
}

fn chat(pane: &Pane, context: SharedString) -> AnyElement {
    let message = |width: f32, lines: usize| {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .children((0..lines).map(move |line| {
                let width = if line + 1 == lines {
                    width * 0.6
                } else {
                    width
                };
                div()
                    .h(px(10.))
                    .w(relative(width))
                    .rounded(px(5.))
                    .bg(theme::raised())
            }))
    };
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(
            div()
                .px_6()
                .pt_4()
                .text_sm()
                .text_color(theme::muted())
                .child(format!("{context}  /  {}", pane.title)),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .max_w(px(760.))
                .mx_auto()
                .px_6()
                .py_6()
                .flex()
                .flex_col()
                .gap_8()
                .child(
                    div()
                        .self_end()
                        .w(relative(0.5))
                        .p_3()
                        .rounded(px(10.))
                        .bg(theme::raised())
                        .child(message(1., 2)),
                )
                .child(message(1., 4))
                .child(message(0.9, 3)),
        )
        .child(
            div()
                .w_full()
                .max_w(px(760.))
                .mx_auto()
                .px_6()
                .pb_4()
                .child(
                    div()
                        .h(px(96.))
                        .p_3()
                        .flex()
                        .flex_col()
                        .justify_between()
                        .rounded(px(14.))
                        .border_1()
                        .border_color(theme::hairline())
                        .bg(theme::raised())
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme::muted())
                                .child("Ask anything, @tag files, or / for commands"),
                        )
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .items_center()
                                .text_xs()
                                .text_color(theme::muted())
                                .child("Claude Opus 5.5  ·  Extra High")
                                .child(
                                    div()
                                        .size(px(26.))
                                        .rounded_full()
                                        .bg(theme::text())
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            Icon::new(gpui_kit::assets::IconName::ArrowUp)
                                                .size(px(14.))
                                                .text_color(theme::card()),
                                        ),
                                ),
                        ),
                ),
        )
        .into_any_element()
}

fn terminal() -> AnyElement {
    div()
        .size_full()
        .p_4()
        .bg(theme::sunken())
        .font_family(MONO)
        .text_sm()
        .text_color(theme::text())
        .flex()
        .flex_col()
        .gap_1()
        .child("$ cargo test -p ledger")
        .child(div().text_color(theme::muted()).child("running 42 tests"))
        .child(
            div()
                .text_color(theme::muted())
                .child("test result: ok. 42 passed"),
        )
        .child("$ ")
        .into_any_element()
}

fn browser(pane: &Pane) -> AnyElement {
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(
            div()
                .h(px(40.))
                .px_3()
                .flex()
                .items_center()
                .border_b_1()
                .border_color(theme::hairline())
                .child(
                    div()
                        .flex_1()
                        .h(px(26.))
                        .px_3()
                        .flex()
                        .items_center()
                        .gap_2()
                        .rounded(px(8.))
                        .bg(theme::raised())
                        .text_sm()
                        .text_color(theme::muted())
                        .child(Icon::new(gpui_kit::assets::IconName::Globe).size(px(13.)))
                        .child(pane.title.clone()),
                ),
        )
        .child(
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(theme::muted())
                .child("Browser pane: arrives with CEF"),
        )
        .into_any_element()
}

fn editor(pane: &Pane) -> AnyElement {
    let lines = [
        "[workspace]",
        "resolver = \"3\"",
        "members = [\"crates/*\"]",
        "",
        "[workspace.package]",
        "edition = \"2024\"",
    ];
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(
            div()
                .h(px(34.))
                .px_4()
                .flex()
                .items_center()
                .border_b_1()
                .border_color(theme::hairline())
                .text_sm()
                .text_color(theme::muted())
                .child(pane.title.clone()),
        )
        .child(
            div()
                .p_4()
                .font_family(MONO)
                .text_sm()
                .flex()
                .flex_col()
                .children(lines.iter().enumerate().map(|(index, line)| {
                    div()
                        .flex()
                        .gap_4()
                        .child(
                            div()
                                .w(px(24.))
                                .text_color(theme::muted())
                                .child((index + 1).to_string()),
                        )
                        .child(div().text_color(theme::text()).child(*line))
                })),
        )
        .into_any_element()
}
