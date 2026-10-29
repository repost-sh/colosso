//! How the palette looks: the field, then a row for each file found, its icon, its name in
//! bold and its folder dimmed after it, the characters that matched in the accent colour;
//! and under them how many matched. Each row is made the first time it's shown and drawn
//! from that after, so moving the pick redraws the rows on screen without making them again.

use std::ops::Range;

use colosso_project::Results;
use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::Input;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, FontWeight, HighlightStyle, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    StyledText, Window, div, px, uniform_list,
};

use super::view::GoToFile;
use crate::files::{ROW, file_icon};
use crate::modal;
use crate::right_panel::{PageDown, PageUp, SelectNext, SelectPrevious, note};
use crate::shell::{self, ellipsized, icon};
use crate::theme::{self, Palette};

/// A file found, as its row draws it: its icon, its name and its folder, each with the
/// bytes in it that matched.
pub(super) struct Row {
    icon: IconName,
    name: SharedString,
    name_matched: Vec<Range<usize>>,
    folder: SharedString,
    folder_matched: Vec<Range<usize>>,
    recent: bool,
}

impl Row {
    /// The row of the file `results` found at `index`, if it found one there.
    pub(super) fn of(results: &Results, index: usize) -> Option<Row> {
        let found = results.found.get(index)?;
        let matched = results.highlights(index);
        let path = &*found.path;
        let (name, folder) = (found.name..path.len(), 0..found.name.saturating_sub(1));
        let part = |range: &Range<usize>| path.get(range.clone()).unwrap_or_default();
        Some(Row {
            icon: file_icon(part(&name)),
            name: part(&name).to_owned().into(),
            name_matched: within(&matched, &name),
            folder: part(&folder).to_owned().into(),
            folder_matched: within(&matched, &folder),
            recent: found.recent,
        })
    }
}

impl Render for GoToFile {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let list = if self.results.found.is_empty() {
            note(p).child(self.note())
        } else {
            div().flex_1().min_h_0().child(
                uniform_list(
                    "go-to-file",
                    self.results.found.len(),
                    cx.processor(|this, range: Range<usize>, _, cx| {
                        let p = theme::palette(cx);
                        range
                            .map(|index| this.row(index, p, cx))
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.scroll)
                .size_full(),
            )
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .key_context("GoToFile")
            .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &PageUp, _, cx| this.page(-1, cx)))
            .on_action(cx.listener(|this, _: &PageDown, _, cx| this.page(1, cx)))
            .child(modal::field(
                Input::new(&self.input)
                    .small()
                    .prefix(icon(IconName::Search, 14.).text_color(p.muted)),
                p,
            ))
            .child(list)
            .child(
                modal::small_line()
                    .text_color(p.muted)
                    .child(self.status.clone()),
            )
    }
}

impl GoToFile {
    fn row(&mut self, index: usize, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let selected = index == self.selected;
        let Some(slot) = self.rows.get_mut(index) else {
            return div().into_any_element();
        };
        if slot.is_none() {
            *slot = Row::of(&self.results, index);
        }
        let Some(row) = slot.as_ref() else {
            return div().into_any_element();
        };
        shell::row(("go-to-file-row", index), selected, p)
            .w_full()
            .h(px(ROW))
            .gap_1p5()
            .rounded(px(5.))
            .on_click(cx.listener(move |this, _, window, cx| this.open(index, window, cx)))
            .child(icon(row.icon, 14.).text_color(p.muted))
            .child(
                div()
                    .flex_shrink_0()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(styled(&row.name, &row.name_matched, p.accent)),
            )
            .child(
                ellipsized(styled(&row.folder, &row.folder_matched, p.accent))
                    .flex_1()
                    .text_color(p.muted),
            )
            .when(row.recent, |row| {
                row.child(div().text_xs().text_color(p.muted).child("recent"))
            })
            .into_any_element()
    }

    /// What the list says when it has no rows.
    fn note(&self) -> &'static str {
        let typed = !self.results.query.text.is_empty();
        if typed && self.listed {
            "No files match."
        } else if typed || !self.listed {
            "Listing files…"
        } else {
            "This checkout has no files."
        }
    }

    /// How many files matched, or how many there are with nothing typed: made when what's
    /// shown changes, not each frame.
    pub(super) fn status_line(&self) -> SharedString {
        let (results, files) = (&self.results, self.results.files);
        let line = (results.query.line).map_or(String::new(), |line| format!(" · line {line}"));
        let counted = if results.query.text.is_empty() {
            format!("{files} files, the ones opened lately first")
        } else {
            format!("{} of {files} files match", results.matched)
        };
        let listing = if self.listed { "" } else { " · listing…" };
        format!("{counted}{line}{listing}").into()
    }
}

/// Those of `matched` within `part`, as ranges of it.
fn within(matched: &[Range<usize>], part: &Range<usize>) -> Vec<Range<usize>> {
    (matched.iter())
        .map(|range| range.start.max(part.start)..range.end.min(part.end))
        .filter(|range| range.start < range.end)
        .map(|range| range.start - part.start..range.end - part.start)
        .collect()
}

/// `text` with its `matched` bytes in `colour`.
fn styled(text: &SharedString, matched: &[Range<usize>], colour: Hsla) -> StyledText {
    let style = HighlightStyle {
        color: Some(colour),
        ..HighlightStyle::default()
    };
    let highlights = matched.iter().map(|range| (range.clone(), style));
    StyledText::new(text.clone()).with_highlights(highlights)
}
