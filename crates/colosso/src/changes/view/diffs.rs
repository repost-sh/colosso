//! The changes' diffs in a workspace tab: each changed file's on a card of its own, one
//! under the other, scrolled to the file picked in the changes panel's list, and the list
//! following the file they're scrolled to. The panel holds what they show and where; the
//! tab draws it.

use std::rc::Rc;

use colosso_git::{Content, Status};
use gpui_kit::assets::IconName;
use gpui_kit::component::{Selectable as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, Div, Empty, Entity, Font, FontWeight, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div, font, list, px,
};

use super::super::file_view::FileView;
use super::super::highlight::Token;
use super::super::line::{CodeLine, Half, Look, runs};
use super::super::rows::{Layout, Row};
use super::{ChangesPanel, DIGIT_WIDTH, LINE_HEIGHT, counts, split_path, status_letter};
use crate::right_panel::panel_header;
use crate::shell::{ellipsized, icon, icon_button};
use crate::theme::{self, Palette};

/// The changes panel's diffs, as a workspace tab shows them.
pub struct ChangesTab {
    panel: Entity<ChangesPanel>,
    /// Drawn again as the panel changes: its files read again, or scrolled to another.
    _panel: Subscription,
}

impl ChangesTab {
    pub fn new(panel: Entity<ChangesPanel>, cx: &mut Context<Self>) -> Self {
        let _panel = cx.observe(&panel, |_, _, cx| cx.notify());
        ChangesTab { panel, _panel }
    }
}

impl Render for ChangesTab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.panel.update(cx, |panel, cx| panel.diffs(window, cx))
    }
}

impl ChangesPanel {
    /// The diffs, under a bar saying how much they change, with how they're laid out.
    fn diffs(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::palette(cx);
        self.glide(window);
        let body = self.state(p, cx).unwrap_or_else(|| {
            // How the diff lines look this frame, shared by them all; the code's size is
            // what `text_xs` gives.
            let look = Rc::new(look(self.wrap, p, window.rem_size() * 0.75));
            let rows = cx.processor(move |this, index, _, cx| this.row(index, &look, cx));
            (div().flex_1().min_w_0().min_h_0())
                .child(list(self.list.clone(), rows).size_full())
                .into_any_element()
        });
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.diffs_bar(p, cx))
            .child(body)
            .into_any_element()
    }

    /// How many files changed and by how many lines, and whether the diffs are side by side
    /// and wrap their lines.
    fn diffs_bar(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let toggle = |id: &'static str, glyph: IconName, tip: &'static str, on: bool| {
            icon_button(id, glyph, tip, cx).xsmall().selected(on)
        };
        let (added, removed) = self.checkout.read(cx).totals();
        let files = match self.files.len() {
            1 => "1 file".to_owned(),
            files => format!("{files} files"),
        };
        panel_header(p)
            .gap_2()
            .child(div().text_sm().text_color(p.muted).child(files))
            .child(counts(added, removed, p))
            .child(div().flex_1())
            .child(
                toggle(
                    "changes-split",
                    IconName::Columns2,
                    "Side by side",
                    self.layout == Layout::Split,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.layout = match this.layout {
                        Layout::Unified => Layout::Split,
                        Layout::Split => Layout::Unified,
                    };
                    this.rebuild(cx);
                })),
            )
            .child(
                toggle("changes-wrap", IconName::TextWrap, "Wrap lines", self.wrap).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.wrap = !this.wrap;
                        this.list.remeasure();
                        cx.notify();
                    }),
                ),
            )
    }

    /// The `index`th row of the diffs, framed as part of its file's card.
    fn row(&self, index: usize, look: &Rc<Look>, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::palette(cx);
        let row = self.rows[index];
        let file = &self.files[row.file()];
        // Each row draws its stretch of the card's sides, so the list can lay them out apart.
        let card = || {
            div()
                .mx_3()
                .border_x_1()
                .border_color(p.hairline)
                .bg(p.sunken)
                .overflow_hidden()
        };
        match row {
            Row::File(index) => {
                let collapsed = self.shown.collapsed.contains(&file.key());
                div()
                    .pt_3()
                    .child(
                        card()
                            .border_t_1()
                            .rounded_t(px(8.))
                            .when(collapsed, |card| card.border_b_1().rounded_b(px(8.)))
                            .child(self.file_header(index, collapsed, p, cx)),
                    )
                    .into_any_element()
            }
            Row::End(_) => div()
                .when(index + 1 == self.rows.len(), Styled::pb_3)
                .child(card().h(px(4.)).border_b_1().rounded_b(px(8.)))
                .into_any_element(),
            Row::Note(_) => card()
                .border_t_1()
                .border_color(p.hairline)
                .px_3()
                .py_2()
                .text_xs()
                .text_color(p.muted)
                .child(note_for(file))
                .into_any_element(),
            Row::Fold { file, new, len, .. } => card()
                .child(
                    div()
                        .id(("fold", index))
                        .h(px(26.))
                        .px_3()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_xs()
                        .text_color(p.muted)
                        .bg(p.hover)
                        .cursor_pointer()
                        .hover(|style| style.bg(p.selected).text_color(p.text))
                        .on_click(cx.listener(move |this, _, _, cx| this.unfold(file, new, cx)))
                        .child(icon(IconName::UnfoldVertical, 12.))
                        .child(match len {
                            1 => "1 unmodified line".to_owned(),
                            len => format!("{len} unmodified lines"),
                        }),
                )
                .into_any_element(),
            Row::Line { old, new, .. } => {
                let (side, line, tint) = match (old, new) {
                    (Some(old), None) => (Side::Old, old, Some(p.removed)),
                    (None, Some(new)) => (Side::New, new, Some(p.added)),
                    (_, Some(new)) => (Side::New, new, None),
                    // Every line is on one side at least.
                    (None, None) => return Empty.into_any_element(),
                };
                let half = half(file, side, line, tint, vec![old, new], &look.font, p);
                CodeLine::new(vec![half], look.clone()).into_any_element()
            }
            Row::Pair {
                old, new, changed, ..
            } => {
                let side = |side, line: Option<u32>, tint| match line {
                    Some(line) => {
                        let tint = changed.then_some(tint);
                        half(file, side, line, tint, vec![Some(line)], &look.font, p)
                    }
                    None => Half::missing(px(number_width(file))),
                };
                let halves = vec![
                    side(Side::Old, old, p.removed),
                    side(Side::New, new, p.added),
                ];
                CodeLine::new(halves, look.clone()).into_any_element()
            }
        }
    }

    fn file_header(
        &self,
        file: usize,
        collapsed: bool,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let diff = &self.files[file].diff;
        let (dir, name) = split_path(&diff.change.path);
        let (added, removed) = diff.content.line_counts();
        div()
            .id(("header", file))
            .h(px(36.))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .text_sm()
            .cursor_pointer()
            .hover(|style| style.bg(p.hover))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_collapsed(file, cx)))
            .child(
                icon(
                    if collapsed {
                        IconName::ChevronRight
                    } else {
                        IconName::ChevronDown
                    },
                    14.,
                )
                .text_color(p.muted),
            )
            .child(status_letter(&diff.change.status, p))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_baseline()
                    .gap_1p5()
                    .child(
                        div()
                            .flex_shrink_0()
                            .font_weight(FontWeight::MEDIUM)
                            .child(name.to_owned()),
                    )
                    .when_some(renamed_from(&diff.change.status), |path, from| {
                        path.child(
                            ellipsized(format!("from {from}"))
                                .text_xs()
                                .text_color(p.muted),
                        )
                    })
                    .when(!dir.is_empty(), |path| {
                        path.child(ellipsized(dir.to_owned()).text_xs().text_color(p.muted))
                    }),
            )
            // A file partly staged has a card for each part.
            .when(diff.change.staged, |header| {
                header.child(
                    div()
                        .flex_shrink_0()
                        .px_1p5()
                        .rounded(px(4.))
                        .border_1()
                        .border_color(p.hairline)
                        .text_xs()
                        .text_color(p.muted)
                        .child("Staged"),
                )
            })
            .child(div().flex_1())
            .child(counts(added, removed, p))
            .child(stats_bar(added, removed, p))
    }
}

#[derive(Clone, Copy)]
enum Side {
    Old,
    New,
}

/// What a file's card says in place of lines it has none to show.
fn note_for(file: &FileView) -> String {
    match (&file.diff.content, &file.diff.change.status) {
        (Content::Binary, _) => "Binary file".to_owned(),
        (Content::TooLarge { bytes }, _) => {
            format!(
                "Too large to show ({:.1} MB)",
                *bytes as f64 / f64::from(1 << 20)
            )
        }
        (Content::Unreadable { reason }, _) => format!("Couldn't be read: {reason}"),
        (_, Status::Renamed { .. }) => "Moved, unchanged".to_owned(),
        (_, Status::Added | Status::Untracked) => "Empty file".to_owned(),
        (_, Status::Deleted) => "Deleted empty file".to_owned(),
        _ => "Only its file mode changed".to_owned(),
    }
}


fn renamed_from(status: &Status) -> Option<&str> {
    match status {
        Status::Renamed { from } => Some(from),
        _ => None,
    }
}


/// Five blocks shared between added and removed lines, as a glance at the size of a change.
fn stats_bar(added: u32, removed: u32, p: Palette) -> Div {
    const BLOCKS: u32 = 5;
    let total = added + removed;
    // Rounded to the nearest block; a change with no lines fills none.
    let green = (added * BLOCKS + total / 2).checked_div(total);
    let red = green.map_or(0, |green| BLOCKS - green);
    let green = green.unwrap_or(0);
    div()
        .flex_shrink_0()
        .flex()
        .gap(px(2.))
        .children((0..BLOCKS).map(|block| {
            let colour = if block < green {
                p.added
            } else if block < green + red {
                p.removed
            } else {
                p.selected
            };
            div().size(px(7.)).rounded(px(1.5)).bg(colour)
        }))
}


/// The width of a line-number column wide enough for every line of `file`.
fn number_width(file: &FileView) -> f32 {
    let lines = match &file.diff.content {
        Content::Text { old, new, .. } => old.len().max(new.len()),
        _ => 0,
    };
    let digits = lines.max(1).ilog10() + 1;
    digits.max(2) as f32 * DIGIT_WIDTH + 12.
}

fn token_colour(token: Token, p: Palette) -> Hsla {
    match token {
        Token::Comment => p.code.comment,
        Token::Keyword => p.code.keyword,
        Token::String => p.code.string,
        Token::Constant => p.code.number,
        Token::Function => p.code.function,
        Token::Type => p.code.kind,
        Token::Tag => p.code.tag,
        Token::Attribute => p.code.attribute,
    }
}

/// How the diff's lines look in palette `p`, wrapped or not, with code `font_size` big.
/// Line `line` of `side` of `view`, numbered with `numbers`, coloured by its tokens and,
/// if it's `tint`ed as added or removed, with the words it changed marked.
fn half(
    view: &FileView,
    side: Side,
    line: u32,
    tint: Option<Hsla>,
    numbers: Vec<Option<u32>>,
    font: &Font,
    p: Palette,
) -> Half {
    let number_width = px(number_width(view));
    let Content::Text { old, new, .. } = &view.diff.content else {
        return Half::missing(number_width);
    };
    let colours = view.colours();
    let highlights = colours.and_then(|colours| colours.highlights.as_ref());
    let (lines, tokens, words) = match side {
        Side::Old => (
            old,
            highlights.map(|highlights| &highlights.old),
            colours.map(|colours| &colours.old_words),
        ),
        Side::New => (
            new,
            highlights.map(|highlights| &highlights.new),
            colours.map(|colours| &colours.new_words),
        ),
    };
    let text = lines.line(line as usize);
    let tokens =
        (tokens.and_then(|tokens| tokens.get(line as usize))).map_or(&[][..], Vec::as_slice);
    let words = (tint.and(words).and_then(|words| words.get(&line))).map_or(&[][..], Vec::as_slice);
    let colour = |kind: Option<Token>| kind.map_or(p.text, |kind| token_colour(kind, p));
    let marked = tint.unwrap_or(p.text).alpha(0.3);
    Half {
        numbers,
        number_width,
        runs: runs(text.len(), tokens, words, colour, marked, font),
        text: SharedString::from(text.to_owned()),
        tint,
        missing: false,
    }
}

fn look(wrap: bool, p: Palette, font_size: Pixels) -> Look {
    Look {
        font: font(theme::MONO),
        font_size,
        line_height: px(LINE_HEIGHT),
        inset: px(12.),
        card: p.sunken,
        hairline: p.hairline,
        numbers: p.muted,
        missing: p.hover,
        wrap,
    }
}

