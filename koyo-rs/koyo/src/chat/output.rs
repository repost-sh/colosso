//! What a tool gave back, in the box it shows in: a terminal's text in the theme's colours, or a
//! diff in the lines the changes tab draws (`docs/chat.md` 5.3).

use std::iter::Peekable;
use std::ops::Range;
use std::rc::Rc;
use std::str::Chars;

use gpui_kit::{
    Div, ElementId, HighlightStyle, Hsla, InteractiveElement as _, ParentElement as _, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, StyledText, TextRun, div, font, px,
    transparent_black,
};
use koyo_agent::protocol::ids::ItemId;
use koyo_agent::protocol::model::Hunk;
use koyo_termd::skip_escape;

use crate::changes::{CodeLine, Half, Look, number_width};
use crate::theme::{MONO, Palette};

/// The tallest a tool's output gets before it scrolls.
const OUTPUT_HEIGHT: f32 = 240.;
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
        .font_family(MONO)
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
    boxed(id, p).p_2().child(StyledText::new(plain).with_highlights(highlights))
}

/// The look of a patch's lines.
fn look(p: Palette) -> Rc<Look> {
    Rc::new(Look {
        font: font(MONO),
        font_size: px(DIFF_FONT_SIZE),
        line_height: px(DIFF_LINE_HEIGHT),
        inset: px(0.),
        card: transparent_black(),
        hairline: transparent_black(),
        numbers: p.muted,
        missing: p.hover,
        wrap: true,
    })
}

/// A change not made yet, whose lines have no numbers to go by, each starting `+` or `-`.
pub(super) fn change(id: String, lines: &[String], p: Palette) -> Stateful<Div> {
    let look = look(p);
    let lines = lines.iter().map(|line| {
        let tint = if line.starts_with('-') { p.removed } else { p.added };
        diff_line(line, None, Some(tint), px(0.), &look, p)
    });
    boxed(id, p).py_1().children(lines.collect::<Vec<_>>())
}

/// `patch`, each line numbered as the file has it now, or had it if it's gone.
pub(super) fn diff(id: &ItemId, patch: &[Hunk], p: Palette) -> Stateful<Div> {
    let look = look(p);
    let longest = patch.iter().map(|hunk| hunk.new_start + hunk.new_lines).max();
    let width = px(number_width(usize::try_from(longest.unwrap_or(1)).unwrap_or(usize::MAX)));
    let mut lines = Vec::new();
    for hunk in patch {
        let (mut old, mut new) = (hunk.old_start, hunk.new_start);
        for line in &hunk.lines {
            let (number, tint) = match line.chars().next() {
                Some('+') => (&mut new, Some(p.added)),
                Some('-') => (&mut old, Some(p.removed)),
                // "\ No newline at end of file".
                Some('\\') => continue,
                _ => {
                    old += 1;
                    (&mut new, None)
                }
            };
            lines.push(diff_line(line, Some(*number), tint, width, &look, p));
            *number += 1;
        }
    }
    boxed(format!("diff-{id}"), p).py_1().children(lines)
}

/// A line of a patch, its first character saying what its tint says, numbered from one if it can
/// be.
fn diff_line(
    line: &str,
    number: Option<u64>,
    tint: Option<Hsla>,
    number_width: gpui_kit::Pixels,
    look: &Rc<Look>,
    p: Palette,
) -> CodeLine {
    let text: SharedString = line.get(1..).unwrap_or_default().trim_end().to_owned().into();
    let run = TextRun {
        len: text.len(),
        font: look.font.clone(),
        color: p.text,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let half = Half {
        numbers: vec![number.and_then(|number| u32::try_from(number.saturating_sub(1)).ok())],
        number_width,
        text,
        runs: vec![run],
        tint,
        missing: false,
    };
    CodeLine::new(vec![half], Rc::clone(look))
}

/// The theme's colour for a terminal's colour, by its number, 0 to 7.
fn terminal_colour(colour: u8, p: Palette) -> Hsla {
    match colour {
        1 => p.removed,
        2 => p.added,
        3 => p.modified,
        4..=6 => p.accent,
        7 => p.text,
        _ => p.muted,
    }
}

/// `text` as it's left on a screen, and which of it was in which colour, by its number.
fn colours_of(text: &str) -> (String, Vec<(Range<usize>, u8)>) {
    let mut plain = String::with_capacity(text.len());
    let mut spans = Vec::new();
    let mut open = None;
    for line in text.split_inclusive('\n') {
        let (body, ended) = line.strip_suffix('\n').map_or((line, false), |body| (body, true));
        let mut chars = overwritten(body).chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\u{1b}' if chars.peek() == Some(&'[') => {
                    chars.next();
                    let pen = style_escape(&mut chars).map_or(Pen::Keep, |numbers| pen_of(&numbers));
                    if pen != Pen::Keep {
                        close(&mut open, plain.len(), &mut spans);
                    }
                    if let Pen::Colour(colour) = pen {
                        open = Some((plain.len(), colour));
                    }
                }
                '\u{1b}' => skip_escape(&mut chars),
                c if c.is_control() && c != '\t' => {}
                c => plain.push(c),
            }
        }
        if ended {
            plain.push('\n');
        }
    }
    close(&mut open, plain.len(), &mut spans);
    (plain, spans)
}

/// What's left of a line once its carriage returns started it over, as a progress bar's do.
fn overwritten(line: &str) -> &str {
    let line = line.trim_end_matches('\r');
    line.rsplit('\r').next().unwrap_or(line)
}

/// Reads an escape to its final byte, past its `[`: its numbers, if it's one of style.
fn style_escape(chars: &mut Peekable<Chars<'_>>) -> Option<String> {
    let mut numbers = String::new();
    for c in chars.by_ref() {
        if ('@'..='~').contains(&c) {
            return (c == 'm').then_some(numbers);
        }
        numbers.push(c);
    }
    None
}

/// Ends the colour that's `open` where the text has got to.
fn close(open: &mut Option<(usize, u8)>, end: usize, spans: &mut Vec<(Range<usize>, u8)>) {
    if let Some((from, colour)) = open.take()
        && from < end
    {
        spans.push((from..end, colour));
    }
}

/// What a style escape does to the colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pen {
    Keep,
    Reset,
    Colour(u8),
}

/// What a style escape's numbers do. The colours of 256 and of millions aren't among the theme's.
fn pen_of(numbers: &str) -> Pen {
    let mut pen = Pen::Keep;
    for number in numbers.split(';') {
        match number.parse::<u8>().unwrap_or(0) {
            0 | 39 => pen = Pen::Reset,
            38 | 48 => break,
            code @ 30..=37 => pen = Pen::Colour(code - 30),
            code @ 90..=97 => pen = Pen::Colour(code - 90),
            _ => {}
        }
    }
    pen
}

#[cfg(test)]
mod tests {
    use super::colours_of;

    #[test]
    fn a_terminals_colours_leave_their_text_and_where_they_were() {
        let (plain, spans) = colours_of("\u{1b}[31mred\u{1b}[0m and \u{1b}[1;92mgreen\u{1b}[m!");
        assert_eq!(plain, "red and green!");
        assert_eq!(spans, vec![(0..3, 1), (8..13, 2)]);
    }

    #[test]
    fn what_isnt_a_colour_goes_and_changes_nothing() {
        let (plain, spans) = colours_of("\u{1b}[2Kdone \u{1b}[38;2;31;32;33mtrue colour\u{1b}[");
        assert_eq!(plain, "done true colour");
        assert!(spans.is_empty());
    }

    #[test]
    fn a_colour_left_open_runs_to_the_end() {
        let (plain, spans) = colours_of("\u{1b}[34mblue");
        assert_eq!(plain, "blue");
        assert_eq!(spans, vec![(0..4, 4)]);
    }

    #[test]
    fn a_links_address_goes_and_a_progress_bar_leaves_its_last_state() {
        let (plain, _) = colours_of("\u{1b}]8;;https://x.dev\u{1b}\\here\u{1b}]8;;\u{7}\n");
        assert_eq!(plain, "here\n");
        let (plain, _) = colours_of("10%\r50%\r100%\r\ndone\ta\u{7}b");
        assert_eq!(plain, "100%\ndone\tab");
    }
}
