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

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AbsoluteLength, App, Div, EdgesRefinement, ElementId, HighlightStyle, InteractiveElement as _,
    Pixels, SharedString, Stateful, StatefulInteractiveElement as _, StyleRefinement, Styled as _,
    Window, div, px, rems,
};

use crate::{
    ActiveTheme, ThemeStyled as _,
    styled::popover_ring,
    text::{TextView, TextViewStyle},
};

/// A card's corners.
pub(super) const RADIUS: Pixels = px(10.);
/// Between two cards, and between a card and what it's about.
pub(super) const GAP: Pixels = px(4.);
/// The least space kept between a card and the window's edge.
pub(super) const SNAP_TO_EDGE: Pixels = px(8.);

/// A card's frame: the popover surface, rounded, clipping what's in it. Over a backdrop the
/// card is clear, and the shadow's ring that edges it would tint it all through; a hairline
/// border in its ink edges it instead.
pub(super) fn framed(id: impl Into<ElementId>, backdrop: bool, cx: &App) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .overflow_hidden()
        .popover_style(cx)
        .rounded(RADIUS)
        .when(backdrop, |mut this| {
            let hairline = Some(AbsoluteLength::Pixels(px(0.5)));
            this.style().border_widths = EdgesRefinement {
                top: hairline,
                right: hairline,
                bottom: hairline,
                left: hairline,
            };
            this.bg(cx.theme().transparent)
                .shadow_none()
                .border_color(popover_ring(cx))
        })
}

/// A card's body: padded, in the UI's font, scrolling past its height.
pub(super) fn card_body(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .py_2()
        .text_xs()
        .font_family(cx.theme().font_family.clone())
        .overflow_y_scroll()
}

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

