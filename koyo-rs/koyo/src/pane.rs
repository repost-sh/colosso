//! Tab contents: each pane's chat, terminal, page, file, diffs, the agent desktop or a device,
//! and placeholders for what a pane shows until its view is made.

use gpui_kit::assets::IconName;
use gpui_kit::{
    AnyElement, Context, Div, ElementId, Entity, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Stateful, Styled as _, div, px,
};

use crate::model::{Content, Editor, EditorFile, Pane};
use crate::shell::{Shell, icon};
use crate::theme::Palette;

/// How wide a tab's close button draws its X.
pub const CLOSE: f32 = 12.;
/// A button in a row down the sidebar, a worktree's header's `…` or `+` or a tab's ×: Arc's
/// size, bigger than a tab bar's.
pub const ROW_BUTTON: f32 = 24.;
/// What a sidebar row's button draws.
pub const ROW_GLYPH: f32 = 18.;

/// A tab's close button: an X, lit when the pointer's over it.
pub fn close_button(id: impl Into<ElementId>, p: Palette) -> Stateful<Div> {
    sized_close_button(id, CLOSE, p)
}

/// A close button whose X is `glyph` wide, for a row taller than a tab.
pub fn sized_close_button(id: impl Into<ElementId>, glyph: f32, p: Palette) -> Stateful<Div> {
    tab_button(id, p).child(icon(IconName::X, glyph))
}

/// A round mark of `color`, as a tab or a row says how something's doing.
pub fn dot(color: Hsla) -> Div {
    div().size(px(7.)).flex_shrink_0().rounded_full().bg(color)
}

/// The dot that marks a file with unsaved edits, on its tab and in the switcher.
pub fn unsaved_dot(p: Palette) -> Div {
    dot(p.text)
}

/// `button`, a tab's, at a sidebar row's size.
pub fn row_sized(button: Stateful<Div>) -> Stateful<Div> {
    button.size(px(ROW_BUTTON)).rounded(px(6.))
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

impl Shell {
    /// What `pane` shows: its chat's, its terminal's, its page's, the agent desktop's or a
    /// device's view, its file or the checkout's diffs, once made (`Shell::make_shown`); until
    /// then, what stands in for it.
    pub fn pane_content(&self, pane: &Pane, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        match &pane.content {
            Content::Chat(_) => (self.chat_tabs.view(pane.id)).map_or_else(
                || div().size_full().into_any_element(),
                |chat| chat.clone().into_any_element(),
            ),
            Content::Terminal(id) => {
                (id.and_then(|id| self.terminal_pane(id, p, cx))).unwrap_or_else(|| terminal(p))
            }
            Content::Browser(browser) => view_or_empty(browser.view()),
            Content::Editor(Editor::File(EditorFile::Open(file))) => {
                file.clone().into_any_element()
            }
            Content::Editor(Editor::Changes(Some(changes))) => changes.clone().into_any_element(),
            Content::Editor(
                Editor::Untitled | Editor::File(EditorFile::Unopened(_)) | Editor::Changes(None),
            ) => editor(&pane.title, p),
            Content::Desktop(view) => view_or_empty(view.as_ref()),
            Content::Device(device) => view_or_empty(device.view()),
        }
    }
}

/// `view`, or nothing while there's none.
fn view_or_empty<V: Render>(view: Option<&Entity<V>>) -> AnyElement {
    view.cloned().map_or_else(
        || gpui_kit::Empty.into_any_element(),
        IntoElement::into_any_element,
    )
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

fn editor(title: &SharedString, p: Palette) -> AnyElement {
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
                .child(title.clone()),
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
