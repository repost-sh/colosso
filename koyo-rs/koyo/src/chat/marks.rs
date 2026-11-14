//! The rows that say what became of a run (`docs/chat.md` 5.3): a compaction, a notice, a failure,
//! a stop, and the files it changed.

use std::path::Path;

use gpui_kit::assets::IconName;
use gpui_kit::{
    AnyElement, Div, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use koyo_agent::protocol::model::{
    FileChange, Item, ItemBody, ItemStatus, NoticeLevel, Run, StoppedBy,
};
use koyo_agent::words::{compacted, failed};

use super::rows::Drawing;
use super::view::ChatViewEvent;
use crate::changes::counts;
use crate::files::relative;
use crate::shell::{ellipsized, icon};

impl Drawing {
    pub(super) fn compaction(&self, item: &Item) -> AnyElement {
        let ItemBody::Compaction(compaction) = &item.body else {
            return div().into_any_element();
        };
        if item.status == ItemStatus::Running {
            self.divider("Compacting context…".to_owned())
        } else {
            self.divider(compacted(compaction))
        }
    }

    /// A line across the column with `text` at its centre.
    fn divider(&self, text: String) -> AnyElement {
        let p = self.p;
        let line = || div().flex_1().h(px(1.)).bg(p.hairline);
        div()
            .flex()
            .items_center()
            .gap_3()
            .text_xs()
            .text_color(p.muted)
            .child(line())
            .child(text)
            .child(line())
            .into_any_element()
    }

    pub(super) fn notice(&self, item: &Item) -> impl IntoElement {
        let p = self.p;
        let (text, level) = match &item.body {
            ItemBody::Notice(notice) => (notice.text.as_str(), notice.level),
            _ => (
                "This takes a newer version of Koyo to show.",
                NoticeLevel::Info,
            ),
        };
        let (mark, color) = match level {
            NoticeLevel::Info => (IconName::Info, p.muted),
            NoticeLevel::Warning => (IconName::TriangleAlert, p.modified),
            NoticeLevel::Error => (IconName::CircleX, p.danger),
        };
        said(mark, color, text.to_owned())
    }

    pub(super) fn failure(&self, run: &Run) -> impl IntoElement {
        let text = run
            .failure
            .as_ref()
            .map_or_else(|| "This response failed.".to_owned(), failed);
        said(IconName::CircleX, self.p.danger, text)
    }

    pub(super) fn stopped(&self, run: &Run) -> AnyElement {
        let by_host = run
            .stop
            .as_ref()
            .is_some_and(|stop| stop.by == StoppedBy::Host);
        self.divider(
            if by_host {
                "This response was interrupted"
            } else {
                "You stopped this response"
            }
            .to_owned(),
        )
    }

    /// What the run changed: the sum of it, then a row for each file, which opens it.
    pub(super) fn files(&self, run: &Run) -> impl IntoElement {
        let p = self.p;
        let added: u64 = run.files.iter().map(|file| file.added).sum();
        let removed: u64 = run.files.iter().map(|file| file.removed).sum();
        let count = run.files.len();
        let noun = if count == 1 { "file" } else { "files" };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .px_3()
            .py_2()
            .rounded(px(10.))
            .border_1()
            .border_color(p.hairline)
            .text_sm()
            .child(
                div()
                    .flex()
                    .gap_2()
                    .text_color(p.muted)
                    .child(format!("{count} {noun} changed"))
                    .child(counts(added, removed, p)),
            )
            .children(run.files.iter().map(|file| self.file_row(file)))
    }

    fn file_row(&self, file: &FileChange) -> impl IntoElement {
        let p = self.p;
        let cwd = Path::new(&*self.cwd);
        let path = cwd.join(&file.path);
        let shown = relative(cwd, &path);
        div()
            .id(SharedString::from(format!("chat-file-{}", file.path)))
            .flex()
            .justify_between()
            .gap_3()
            .text_color(p.muted)
            .cursor_pointer()
            .hover(|row| row.text_color(p.text))
            .child(ellipsized(shown))
            .child(counts(file.added, file.removed, p))
            .on_click(
                self.on_timeline(move |_, _, cx| cx.emit(ChatViewEvent::OpenFile(path.clone()))),
            )
    }
}

/// `text` after `mark`, in `color`, wrapped at the column's width however long it is: text
/// straight in a row is as wide as it's long, and runs out of the column.
fn said(mark: IconName, color: Hsla, text: String) -> Div {
    marked(mark, color).child(div().min_w_0().child(text))
}

/// A row of small text in `color`, with `mark` first.
pub(super) fn marked(mark: IconName, color: Hsla) -> Div {
    div()
        .flex()
        .items_start()
        .gap_2()
        .text_sm()
        .text_color(color)
        .child(div().pt(px(3.)).flex_shrink_0().child(icon(mark, 13.)))
}
