//! Tab contents: a file opened from the file tree in its editor, and placeholders for what
//! each other pane kind will roughly look like.

use gpui_kit::assets::IconName;
use gpui_kit::{
    AnyElement, Div, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
    Stateful, Styled as _, div, px,
};

use crate::model::{Pane, PaneKind};
use crate::shell::icon;
use crate::theme::Palette;

/// A tab's close button: an X, lit when the pointer's over it.
pub fn close_button(id: impl Into<ElementId>, p: Palette) -> Stateful<Div> {
    tab_button(id, p).child(icon(IconName::X, 12.))
}

/// A round mark of `color`, as a tab or a row says how something's doing.
pub fn dot(color: Hsla) -> Div {
    div().size(px(7.)).flex_shrink_0().rounded_full().bg(color)
}

/// The dot that marks a file with unsaved edits, on its tab and in the switcher.
pub fn unsaved_dot(p: Palette) -> Div {
    dot(p.text)
}

/// A small square button in a tab, lit when hovered, without what's drawn in it.
pub fn tab_button(id: impl Into<ElementId>, p: Palette) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(18.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .hover(|style| style.bg(p.hover))
}

/// What a pane shows: its chat, terminal, page, file or the checkout's diffs.
pub fn pane(pane: &Pane, p: Palette) -> AnyElement {
    match pane.kind {
        // Its view is drawn in its place, once the shell has made it.
        PaneKind::Chat => div().size_full().into_any_element(),
        PaneKind::Terminal => terminal(p),
        PaneKind::Browser => browser(pane),
        PaneKind::Editor => match (&pane.file, &pane.changes) {
            (Some(file), _) => file.clone().into_any_element(),
            (None, Some(changes)) => changes.clone().into_any_element(),
            (None, None) => editor(pane, p),
        },
    }
}

fn terminal(p: Palette) -> AnyElement {
    div()
        .size_full()
        .p_4()
        .bg(p.sunken)
        .font_family(p.mono)
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

/// A browser pane is its page's view, made by the time its tab is shown
/// (`Shell::make_shown_browsers`).
fn browser(pane: &Pane) -> AnyElement {
    (pane.browser.clone()).map_or_else(
        || gpui_kit::Empty.into_any_element(),
        IntoElement::into_any_element,
    )
}

fn editor(pane: &Pane, p: Palette) -> AnyElement {
    let lines = [
        "[workspace]",
        "resolver = \"3\"",
        "members = [\"koyo\", \"koyo-*\"]",
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
                .font_family(p.mono)
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
