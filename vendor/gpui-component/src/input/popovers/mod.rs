mod caret_anchored;
mod code_action_menu;
mod completion_menu;
mod cards;
mod diagnostic;

pub(crate) use caret_anchored::*;
pub(crate) use code_action_menu::*;
pub(crate) use completion_menu::*;
pub(crate) use cards::EditorCards;
pub use cards::{PopoverBackdrop, set_popover_backdrop};
pub(crate) use diagnostic::render_diagnostic;

use gpui::{
    App, Div, ElementId, HighlightStyle, InteractiveElement as _, SharedString, Stateful, StyleRefinement,
    Styled as _, Window, div, px, rems,
};

use crate::{
    ActiveTheme, ThemeStyled as _,
    text::{TextView, TextViewStyle},
};

pub(super) fn render_markdown(
    id: impl Into<ElementId>,
    markdown: impl Into<SharedString>,
    _: &mut Window,
    cx: &mut App,
) -> TextView {
    let theme = cx.theme();
    // Prose in the UI's font, code in the editor's: a code block is the signature or an
    // example, set in place without a box or a language label, and inline code is only
    // tinted.
    TextView::markdown(id, markdown)
        .style(
            TextViewStyle::default()
                .paragraph_gap(rems(0.5))
                .heading_font_size(|level, rem_size| match level {
                    1..=3 => rem_size * 1,
                    4 => rem_size * 0.9,
                    _ => rem_size * 0.8,
                })
                .inline_code(HighlightStyle {
                    background_color: Some(theme.muted),
                    ..HighlightStyle::default()
                })
                .code_block(
                    StyleRefinement::default()
                        .bg(theme.transparent)
                        .border_0()
                        .p_0()
                        .py_0p5()
                        .text_size(px(12.)),
                ),
        )
        .code_block_actions(|_, _, _| div())
        .font_family(theme.font_family.clone())
        .selectable(true)
}

pub(super) fn editor_popover(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .occlude()
        .popover_style(cx)
        .shadow_md()
        .text_xs()
        .p_1()
}
