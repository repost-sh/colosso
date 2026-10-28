//! Placeholder tab contents: what each pane kind will roughly look like.

use gpui_kit::assets::IconName;
use gpui_kit::{
    AnyElement, IntoElement, ParentElement as _, SharedString, Styled as _, div, px, relative,
};

use crate::model::{Pane, PaneKind, Tab};
use crate::shell::icon;
use crate::theme::{MONO, Palette};

pub fn tab(tab: &Tab, context: SharedString, p: Palette) -> AnyElement {
    match tab {
        Tab::Single(pane) => self::pane(pane, context, p),
        Tab::Split(left, right) => div()
            .size_full()
            .flex()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(pane(left, context.clone(), p)),
            )
            .child(div().w(px(1.)).h_full().bg(p.hairline))
            .child(div().flex_1().min_w_0().child(pane(right, context, p)))
            .into_any_element(),
    }
}

fn pane(pane: &Pane, context: SharedString, p: Palette) -> AnyElement {
    match pane.kind {
        PaneKind::Chat => chat(pane, context, p),
        PaneKind::Terminal => terminal(p),
        PaneKind::Browser => browser(pane, p),
        PaneKind::Editor => editor(pane, p),
    }
}

fn chat(pane: &Pane, context: SharedString, p: Palette) -> AnyElement {
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
                    .bg(p.raised)
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
                .text_color(p.muted)
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
                        .bg(p.raised)
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
                        .border_color(p.hairline)
                        .bg(p.raised)
                        .child(
                            div()
                                .text_sm()
                                .text_color(p.muted)
                                .child("Ask anything, @tag files, or / for commands"),
                        )
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .items_center()
                                .text_xs()
                                .text_color(p.muted)
                                .child("Claude Opus 5.5  ·  Extra High")
                                .child(
                                    div()
                                        .size(px(26.))
                                        .rounded_full()
                                        .bg(p.text)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(icon(IconName::ArrowUp, 14.).text_color(p.card)),
                                ),
                        ),
                ),
        )
        .into_any_element()
}

fn terminal(p: Palette) -> AnyElement {
    div()
        .size_full()
        .p_4()
        .bg(p.sunken)
        .font_family(MONO)
        .text_sm()
        .text_color(p.text)
        .flex()
        .flex_col()
        .gap_1()
        .child("$ cargo test -p ledger")
        .child(div().text_color(p.muted).child("running 42 tests"))
        .child(
            div()
                .text_color(p.muted)
                .child("test result: ok. 42 passed"),
        )
        .child("$ ")
        .into_any_element()
}

fn browser(pane: &Pane, p: Palette) -> AnyElement {
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
                .border_color(p.hairline)
                .child(
                    div()
                        .flex_1()
                        .h(px(26.))
                        .px_3()
                        .flex()
                        .items_center()
                        .gap_2()
                        .rounded(px(8.))
                        .bg(p.raised)
                        .text_sm()
                        .text_color(p.muted)
                        .child(icon(IconName::Globe, 13.))
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
                .text_color(p.muted)
                .child("Browser pane: arrives with CEF"),
        )
        .into_any_element()
}

fn editor(pane: &Pane, p: Palette) -> AnyElement {
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
                .border_color(p.hairline)
                .text_sm()
                .text_color(p.muted)
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
                                .text_color(p.muted)
                                .child((index + 1).to_string()),
                        )
                        .child(div().text_color(p.text).child(*line))
                })),
        )
        .into_any_element()
}
