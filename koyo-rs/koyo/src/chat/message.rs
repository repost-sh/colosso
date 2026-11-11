//! A message a person sent, as the timeline shows it (`docs/chat.md` 5.3): a bubble of markdown,
//! drawn as an answer is, cut to a few lines until it's opened, what it attached above its words,
//! and as the pointer passes, when it was sent, going back to it and copying it.

use std::path::Path;

use gpui_kit::assets::IconName;
use gpui_kit::component::text::{TextView, TextViewState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Div, Entity, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, div,
};
use koyo_agent::protocol::ids::RunId;
use koyo_agent::protocol::model::{Delivered, Item, ItemBody, Run};
use koyo_agent::timeline::RowId;
use koyo_agent::words::clock;

use super::rows::{Drawing, bubble, hover_meta, markdown};
use super::timeline::GoBackTo;
use crate::pane::tab_button;
use crate::shell::icon;

/// How many lines of a message show until it's opened.
const COLLAPSED_LINES: usize = 8;
/// About how many characters a line of a bubble holds, to tell a long message without laying it
/// out. A guess only says whether "Show more" is offered: it never hides what can't be opened.
const BUBBLE_LINE: usize = 72;

impl Drawing {
    /// The message `item` said, its words drawn by `text`.
    pub(super) fn user(
        &self,
        item: &Item,
        text: Option<&Entity<TextViewState>>,
        run: &Run,
        id: &RowId,
        index: usize,
    ) -> Option<Div> {
        let ItemBody::UserMessage(message) = &item.body else {
            return None;
        };
        let p = self.p;
        let long = longer_than(&message.text, COLLAPSED_LINES);
        let open = self.open.contains(id);
        let steered = message.delivered == Delivered::Steer;
        let group: SharedString = format!("user-{}", item.id).into();
        // A steer joined the run: when the run was asked for isn't when it was said.
        let sent = if steered {
            None
        } else {
            clock(&run.requested_at)
        };
        let said = bubble()
            .bg(p.wash)
            .when(!message.attachments.is_empty(), |said| {
                said.child(self.attached(&item.id.0, &message.attachments))
            })
            .children(text.filter(|_| !message.text.is_empty()).map(|text| {
                markdown(TextView::new(text), Path::new(&*self.cwd))
                    .when(long && !open, |text| text.max_lines(COLLAPSED_LINES))
            }))
            .when(long, |said| {
                said.child(self.show_more(id.clone(), index, open))
            });
        Some(
            div()
                .group(group.clone())
                .flex()
                .flex_col()
                .items_end()
                .gap_1()
                .when(steered, |row| {
                    row.child(div().text_xs().text_color(p.muted).child("↷ Steered"))
                })
                .child(said)
                .child(
                    hover_meta(&group, p)
                        .children(sent)
                        .when(self.can_go_back && !steered, |meta| {
                            meta.child(self.go_back_button(&run.id))
                        })
                        .child(self.copy_button(&item.id)),
                ),
        )
    }

    /// A small button that goes back to the message that started `run`.
    fn go_back_button(&self, run: &RunId) -> impl IntoElement {
        let id = SharedString::from(format!("go-back-{run}"));
        let run = run.clone();
        tab_button(id, self.p)
            .cursor_pointer()
            .tooltip(|window, cx| Tooltip::new("Go back to this message").build(window, cx))
            .child(icon(IconName::Undo2, 12.))
            .on_click(self.on_timeline(move |_, _, cx| cx.emit(GoBackTo(run.clone()))))
    }
}

/// Whether `text` takes more than `lines` lines of a bubble, counting those that wrap.
pub(super) fn longer_than(text: &str, lines: usize) -> bool {
    let took: usize = text
        .lines()
        .map(|line| 1 + line.chars().count() / BUBBLE_LINE)
        .sum();
    took > lines
}

#[cfg(test)]
mod tests {
    use super::{COLLAPSED_LINES, longer_than};

    #[test]
    fn a_message_is_long_past_eight_lines_counting_those_that_wrap() {
        let is_long = |text: &str| longer_than(text, COLLAPSED_LINES);
        assert!(!is_long(&"line\n".repeat(8)));
        assert!(is_long(&"line\n".repeat(9)));
        assert!(is_long(&"word ".repeat(200)));
        assert!(!is_long(&"語".repeat(200)), "characters, not their bytes");
    }
}
