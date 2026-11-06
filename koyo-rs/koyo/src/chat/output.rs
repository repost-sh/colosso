//! What a tool gave back, in the box it shows in: a terminal's text in the theme's colours, or a
//! diff coloured as the changes tab colours it (`docs/chat.md` 5.3). Its text can be selected and
//! copied, a diff's code without its line numbers.

mod terminal;

use gpui_kit::base::SelectableText;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Div, ElementId, HighlightStyle, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, SharedString, Stateful, StatefulInteractiveElement as _,
    Styled as _, div, px,
};
use koyo_agent::protocol::ids::ItemId;
use koyo_agent::protocol::model::Hunk;
use koyo_git::changed_words;

use crate::changes::{
    CODE_LEFT, CODE_RIGHT, LINE_TINT, LineTokens, NUMBER_RIGHT, NUMBERS_TINT, Span, WORDS_TINT,
    highlight_lines, number_width, spans, token_colour,
};
use crate::theme::Palette;
use terminal::{colours_of, terminal_colour};

/// The tallest a tool's output gets before it scrolls.
const OUTPUT_HEIGHT: f32 = 240.;
/// The tallest a diff gets before it scrolls: a change is read rather than glanced at.
const DIFF_HEIGHT: f32 = 432.;
const DIFF_FONT_SIZE: f32 = 12.;
const DIFF_LINE_HEIGHT: f32 = 18.;

/// A sunken box, which scrolls past a height.
pub(super) fn scroll_box(id: impl Into<ElementId>, p: Palette) -> Stateful<Div> {
    div()
        .id(id)
        .max_h(px(OUTPUT_HEIGHT))
        .overflow_y_scroll()
        .rounded(px(8.))
        .bg(p.sunken)
}

/// The box a tool's text goes in.
fn boxed(id: String, p: Palette) -> Stateful<Div> {
    scroll_box(SharedString::from(id), p)
        .font_family(p.mono)
        .text_xs()
        .text_color(p.muted)
}

/// `text` as a terminal would colour it.
pub(super) fn block(id: String, text: &str, p: Palette) -> Stateful<Div> {
    let (plain, spans) = colours_of(text.trim_end());
    let highlights = spans.into_iter().map(|(range, colour)| {
        let style = HighlightStyle {
            color: Some(terminal_colour(colour, p)),
            ..HighlightStyle::default()
        };
        (range, style)
    });
    let shown = SelectableText::new(SharedString::from(format!("{id}-text")), plain);
    boxed(id, p).p_2().child(shown.with_highlights(highlights))
}

/// The box a patch's lines go in, in the size they're drawn at.
fn lines_box(id: String, p: Palette) -> Stateful<Div> {
    boxed(id, p)
        .max_h(px(DIFF_HEIGHT))
        .py_1()
        .text_size(px(DIFF_FONT_SIZE))
        .line_height(px(DIFF_LINE_HEIGHT))
}

/// A change not made yet, whose lines have no numbers to go by, each starting `+` or `-`.
pub(super) fn change(id: String, lines: &[String], p: Palette) -> Stateful<Div> {
    let drawn: Vec<_> = (lines.iter().enumerate())
        .map(|(at, line)| {
            let tint = if mark(line) == Mark::Removed {
                p.removed
            } else {
                p.added
            };
            diff_line(
                format!("{id}-{at}"),
                line,
                None,
                Some(tint),
                px(0.),
                None,
                p,
            )
        })
        .collect();
    lines_box(id, p).children(drawn)
}

/// What a line of a patch is, by its first character.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    Added,
    Removed,
    Context,
    /// "\ No newline at end of file".
    Note,
}

fn mark(line: &str) -> Mark {
    match line.chars().next() {
        Some('+') => Mark::Added,
        Some('-') => Mark::Removed,
        Some('\\') => Mark::Note,
        _ => Mark::Context,
    }
}

/// How a patch's lines are coloured, worked out once and off the main thread: their syntax, and
/// the words each line that replaced another changed. By hunk, and line as the hunk has them.
#[derive(Debug, Default)]
pub(super) struct PatchColours {
    hunks: Vec<Vec<LineColours>>,
}

/// A line's code in the stretches it's coloured by.
#[derive(Clone, Debug, Default)]
struct LineColours {
    spans: Vec<Span>,
}

/// The colours of `patch`, a change to the file at path `file`: each hunk's lines as that
/// stretch of the file had them and has them now, and the nth line a run of removed ones replaced
/// paired with the nth of the added ones after them, as the changes tab pairs them.
pub(super) fn patch_colours(file: &str, patch: &[Hunk]) -> PatchColours {
    let hunks = patch.iter().map(|hunk| hunk_colours(file, &hunk.lines));
    PatchColours {
        hunks: hunks.collect(),
    }
}

/// A line of a patch without its mark.
fn text(line: &str) -> &str {
    line.get(1..).unwrap_or_default()
}

fn hunk_colours(path: &str, lines: &[String]) -> Vec<LineColours> {
    let side = |kept: Mark| -> Vec<&str> {
        let on_side = |line: &&String| [Mark::Context, kept].contains(&mark(line));
        lines
            .iter()
            .filter(on_side)
            .map(|line| text(line))
            .collect()
    };
    let old = highlight_lines(path, &side(Mark::Removed)).unwrap_or_default();
    let new = highlight_lines(path, &side(Mark::Added)).unwrap_or_default();
    let (mut old, mut new) = (old.into_iter(), new.into_iter());
    let tokens: Vec<LineTokens> = (lines.iter())
        .map(|line| {
            let tokens = match mark(line) {
                Mark::Removed => old.next(),
                Mark::Added => new.next(),
                Mark::Context => {
                    old.next();
                    new.next()
                }
                Mark::Note => None,
            };
            tokens.unwrap_or_default()
        })
        .collect();
    let mut words = vec![Vec::new(); lines.len()];
    let mut at = 0;
    while at < lines.len() {
        let removed = at..at + run_of(&lines[at..], Mark::Removed);
        let added = removed.end..removed.end + run_of(&lines[removed.end..], Mark::Added);
        at = added.end.max(at + 1);
        for (old, new) in removed.zip(added) {
            if let Some((removed, added)) = changed_words(text(&lines[old]), text(&lines[new])) {
                words[old] = removed;
                words[new] = added;
            }
        }
    }
    (lines.iter().zip(tokens).zip(words))
        .map(|((line, tokens), words)| LineColours {
            spans: spans(text(line).trim_end().len(), &tokens, &words),
        })
        .collect()
}

/// How many of `lines` are `kept` from the first.
fn run_of(lines: &[String], kept: Mark) -> usize {
    lines.iter().take_while(|line| mark(line) == kept).count()
}

/// The number of lines `patch` adds and removes.
pub(super) fn patch_counts(patch: &[Hunk]) -> (u64, u64) {
    let marks = patch
        .iter()
        .flat_map(|hunk| hunk.lines.iter().map(|line| mark(line)));
    marks.fold((0, 0), |(added, removed), mark| match mark {
        Mark::Added => (added + 1, removed),
        Mark::Removed => (added, removed + 1),
        _ => (added, removed),
    })
}

/// `patch`, each line numbered as the file has it now, or had it if it's gone, and coloured once
/// its `colours` are worked out; a gap between hunks, as the lines between them didn't change.
pub(super) fn diff(
    id: &ItemId,
    patch: &[Hunk],
    colours: Option<&PatchColours>,
    p: Palette,
) -> Stateful<Div> {
    let longest = patch
        .iter()
        .map(|hunk| hunk.new_start + hunk.new_lines)
        .max();
    let width = px(number_width(
        usize::try_from(longest.unwrap_or(1)).unwrap_or(usize::MAX),
    ));
    let mut lines: Vec<AnyElement> = Vec::new();
    for (at, hunk) in patch.iter().enumerate() {
        if at > 0 {
            lines.push(gap(width, p).into_any_element());
        }
        let coloured = colours.and_then(|colours| colours.hunks.get(at));
        let (mut old, mut new) = (hunk.old_start, hunk.new_start);
        for (line_at, line) in hunk.lines.iter().enumerate() {
            let (number, tint) = match mark(line) {
                Mark::Added => (&mut new, Some(p.added)),
                Mark::Removed => (&mut old, Some(p.removed)),
                Mark::Note => continue,
                Mark::Context => {
                    old += 1;
                    (&mut new, None)
                }
            };
            let colour = coloured.and_then(|coloured| coloured.get(line_at));
            let line_id = format!("diff-{id}-{at}-{line_at}");
            let drawn = diff_line(line_id, line, Some(*number), tint, width, colour, p);
            lines.push(drawn.into_any_element());
            *number += 1;
        }
    }
    lines_box(format!("diff-{id}"), p).children(lines)
}

/// Where a patch leaves out the lines between two hunks.
fn gap(number_width: Pixels, p: Palette) -> Div {
    div()
        .h(px(DIFF_LINE_HEIGHT))
        .pl(number_width + px(CODE_LEFT))
        .text_color(p.muted)
        .child("⋯")
}

/// A line of a patch, its first character saying what its tint says, numbered from one if it can
/// be, in its syntax's colours with the words it changed marked, once those are worked out. Its
/// code can be selected, and a drag over several lines copies them; its number can't, so what's
/// copied is the code alone.
fn diff_line(
    id: String,
    line: &str,
    number: Option<u64>,
    tint: Option<Hsla>,
    number_width: Pixels,
    colours: Option<&LineColours>,
    p: Palette,
) -> Div {
    let marked = tint.unwrap_or(p.text).alpha(WORDS_TINT);
    let spans = colours.map_or(&[][..], |colours| &colours.spans[..]);
    let highlights = spans.iter().scan(0, move |start, span| {
        let range = *start..*start + span.len;
        *start = range.end;
        let style = HighlightStyle {
            color: Some(span.kind.map_or(p.text, |kind| token_colour(kind, p))),
            background_color: span.marked.then_some(marked),
            ..HighlightStyle::default()
        };
        Some((range, style))
    });
    let number = div()
        .flex_shrink_0()
        .w(number_width)
        .pr(px(NUMBER_RIGHT))
        .flex()
        .justify_end()
        .text_color(tint.map_or(p.muted, |tint| tint.alpha(0.8)))
        .when_some(tint, |number, tint| number.bg(tint.alpha(NUMBERS_TINT)))
        .children(number.map(|number| number.to_string()));
    let id = SharedString::from(id);
    let selector = id.clone();
    let code =
        SelectableText::new(id, text(line).trim_end().to_owned()).with_highlights(highlights);
    div()
        .flex()
        .min_h(px(DIFF_LINE_HEIGHT))
        .text_color(p.text)
        .when_some(tint, |row, tint| row.bg(tint.alpha(LINE_TINT)))
        .child(number)
        .child(
            div()
                .debug_selector(move || format!("{selector}-code"))
                .flex_1()
                .min_w_0()
                .pl(px(CODE_LEFT))
                .pr(px(CODE_RIGHT))
                .child(code),
        )
}

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use koyo_agent::protocol::model::Hunk;

    use super::{LineColours, patch_colours, patch_counts};

    fn hunk(lines: &[&str]) -> Hunk {
        Hunk {
            old_start: 1,
            old_lines: 0,
            new_start: 1,
            new_lines: 0,
            lines: lines.iter().map(|line| (*line).to_owned()).collect(),
        }
    }

    /// Whether some of `line` is in a syntax's colours.
    fn in_colour(line: &LineColours) -> bool {
        line.spans.iter().any(|span| span.kind.is_some())
    }

    /// The words of `text` that `line` marks as changed.
    fn marked_in<'a>(line: &LineColours, text: &'a str) -> Vec<&'a str> {
        let mut start = 0;
        let mut words: Vec<Range<usize>> = Vec::new();
        for span in &line.spans {
            let range = start..start + span.len;
            start = range.end;
            match words.last_mut() {
                Some(last) if span.marked && last.end == range.start => last.end = range.end,
                _ if span.marked => words.push(range),
                _ => {}
            }
        }
        words.into_iter().map(|range| &text[range]).collect()
    }

    #[test]
    fn a_patch_is_coloured_by_its_syntax_and_a_replaced_line_by_the_words_it_changed() {
        let patch = [hunk(&[
            " fn main() {",
            "-    let n = 1;",
            "-    let m = 2;",
            "+    let n = 10;",
            "+    run();",
            "+    done();",
            " }",
        ])];
        let colours = patch_colours("src/main.rs", &patch);
        let lines = &colours.hunks[0];
        assert_eq!(lines.len(), 7);
        assert!(in_colour(&lines[0]), "fn is a keyword");
        let marked = |at: usize| marked_in(&lines[at], &patch[0].lines[at][1..]);
        assert_eq!(
            marked(1),
            ["1"],
            "the first removed line pairs with the first added"
        );
        assert_eq!(marked(3), ["10"]);
        assert!(
            marked(5).is_empty(),
            "an added line with none to pair has no words marked"
        );
        assert!(marked(0).is_empty(), "nor has a line that stayed");
        assert_eq!(patch_counts(&patch), (3, 2));
    }

    #[test]
    fn a_file_of_no_known_kind_is_coloured_by_its_words_only() {
        let patch = [hunk(&[
            "-one two",
            "+one three",
            "\\ No newline at end of file",
        ])];
        let colours = patch_colours("notes.unknownext", &patch);
        assert!(!colours.hunks[0].iter().any(in_colour));
        assert_eq!(marked_in(&colours.hunks[0][0], "one two"), ["two"]);
        assert_eq!(patch_counts(&patch), (1, 1));
    }
}
