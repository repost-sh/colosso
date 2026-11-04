//! What a chat says when Claude Code isn't there to run it (`docs/chat.md` O2): the command that
//! installs it and a way to look again, as sending would only say so after the fact.

use gpui_kit::component::Sizable as _;
use gpui_kit::{ClickEvent, ClipboardItem, Context, Div, ParentElement as _, Styled as _, div, px};

use super::ChatComposer;
use crate::shell::glass;
use crate::theme::{MONO, Palette};

const INSTALL: &str = "curl -fsSL https://claude.ai/install.sh | bash";

impl ChatComposer {
    /// The note, if Claude Code is missing.
    pub(super) fn claude_missing_note(&self, p: Palette, cx: &Context<Self>) -> Option<Div> {
        let missing = *self.chats.read(cx).claude_missing.read(cx);
        missing.then(|| {
            let copy = glass("chat-claude-install-copy", p.text, cx)
                .small()
                .label("Copy")
                .on_click(cx.listener(|_, _: &ClickEvent, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(INSTALL.to_owned()));
                }));
            let again = glass("chat-claude-check-again", p.text, cx)
                .small()
                .label("Check again")
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.chats.update(cx, |chats, cx| chats.ask_for_provider(true, cx));
                }));
            div()
                .pb_3()
                .flex()
                .flex_col()
                .gap_2()
                .text_sm()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(div().text_color(p.text).child("Claude Code isn't installed"))
                        .child(again),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .pl_3()
                        .pr(px(6.))
                        .py(px(6.))
                        .rounded(px(8.))
                        .bg(p.sunken)
                        .child(div().flex_1().min_w_0().text_xs().font_family(MONO).child(INSTALL))
                        .child(copy),
                )
        })
    }
}
