use gpui::prelude::FluentBuilder as _;
use gpui::{AnyElement, App, IntoElement, ParentElement as _, Styled, Window, div, px};

use crate::{
    ActiveTheme as _, Icon, IconName,
    highlighter::{DiagnosticEntry, DiagnosticSeverity},
    input::popovers::render_markdown,
};

/// A diagnostic as a card shows it: its severity's icon in its colour, what it says, and,
/// muted, where it comes from.
pub(crate) fn render_diagnostic(
    diagnostic: &DiagnosticEntry,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let severity = diagnostic.severity;
    let icon = match severity {
        DiagnosticSeverity::Error => IconName::CircleX,
        DiagnosticSeverity::Warning => IconName::TriangleAlert,
        DiagnosticSeverity::Info | DiagnosticSeverity::Hint => IconName::Info,
    };
    let source = match (&diagnostic.source, &diagnostic.code) {
        (Some(source), Some(code)) => Some(format!("{source}({code})")),
        (Some(source), None) => Some(source.to_string()),
        (None, Some(code)) => Some(code.to_string()),
        (None, None) => None,
    };
    let muted = cx.theme().muted_foreground;
    div()
        .flex()
        .items_start()
        .gap_2()
        .child(
            Icon::new(icon)
                .text_color(crate::highlighter::diagnostic_foreground(severity, cx))
                .size_3p5()
                .flex_none()
                .mt(px(2.)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(render_markdown(
                    "diagnostic",
                    diagnostic.message.clone(),
                    window,
                    cx,
                ))
                .when_some(source, |this, source| {
                    this.child(div().text_color(muted).child(source))
                }),
        )
        .into_any_element()
}
