//! A composer's draft (`docs/chat.md` 5.2): what it shows of the chat's when it opens, what it
//! writes as things change. A composer on a chat another has open starts empty and keeps nothing.

use gpui_kit::{App, Context, Focusable as _, Window};
use koyo_agent::protocol::ids::ThreadId;
use koyo_agent::protocol::model::{Attachment, Prompt};

use super::ChatComposer;
use crate::chat::drafts::{self, Draft, DraftOf};

impl ChatComposer {
    /// Takes up its chat's draft, and shows what's in it.
    pub(super) fn bring_back_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let of = self
            .thread_id(cx)
            .map_or_else(|| DraftOf::NewChat(self.cwd.clone()), DraftOf::Thread);
        let Some(draft) = drafts::take_up(&of, cx) else {
            return;
        };
        self.draft = Some(of);
        let Draft {
            text,
            cursor,
            attachments,
        } = draft;
        if !text.is_empty() {
            self.text.update(cx, |typed, cx| {
                typed.set_value(text, window, cx);
                typed.set_selected_range(cursor..cursor, cx);
            });
        }
        self.give_back_those_there(attachments);
    }

    /// Takes back what a message said, to say again: its words after what's typed, its files with
    /// what's attached. The caret goes to its end, for a person to change it.
    pub(in crate::chat) fn take_back(
        &mut self,
        prompt: Prompt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.after_typed(&prompt.text, window, cx);
        self.give_back_those_there(prompt.attachments);
        window.focus(&self.focus_handle(cx), cx);
        cx.notify();
    }

    /// `text` after what's typed so neither is lost, with the caret at its end.
    pub(super) fn after_typed(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.text.update(cx, |typed, cx| {
            let end = typed.value().len();
            let words = if end == 0 {
                text.to_owned()
            } else {
                format!("\n\n{text}")
            };
            typed.set_selected_range(end..end, cx);
            typed.replace(words, window, cx);
        });
    }

    /// A file that's gone would only stop the message going.
    fn give_back_those_there(&mut self, attachments: Vec<Attachment>) {
        let folder = self.attachments.as_deref();
        let there = |kept: &Attachment| folder.is_some_and(|in_it| in_it.join(&kept.file).exists());
        self.give_back_attached(attachments.into_iter().filter(there).collect());
    }

    /// Its draft is what it shows now, if that's changed. This is asked with every blink of the
    /// caret, so what's typed is only read out for a change.
    pub(super) fn keep_draft(&mut self, cx: &mut App) {
        let Some(of) = &self.draft else {
            return;
        };
        let text = self.text.read(cx);
        let written = Some((text.version(), text.cursor()));
        if written == self.draft_written
            && self.kept_attachments().eq(drafts::attachments_of(of, cx))
        {
            return;
        }
        self.draft_written = written;
        let draft = Draft {
            text: text.value().to_string(),
            cursor: text.cursor(),
            attachments: self.kept_attachments().cloned().collect(),
        };
        drafts::write(of, draft, cx);
    }

    /// A new chat has its thread: anything written since it was asked for belongs to that.
    pub(super) fn draft_goes_to(&mut self, id: &ThreadId, cx: &mut Context<Self>) {
        if let Some(new_chat) = self.draft.take() {
            drafts::give_up(&new_chat, cx);
        }
        let of = DraftOf::Thread(id.clone());
        self.draft = drafts::take_up(&of, cx).map(|_| of);
        self.draft_written = None;
        self.keep_draft(cx);
    }

    /// It's closing. Its draft names what's attached, for when the chat next opens; without one,
    /// no message will.
    pub(super) fn let_go(&mut self, cx: &mut App) {
        if let Some(of) = self.draft.take() {
            drafts::put_down(&of, cx);
        } else {
            let gone = std::mem::take(&mut self.attached);
            self.forget(gone, cx);
        }
    }
}
