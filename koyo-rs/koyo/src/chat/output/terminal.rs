//! A terminal's text as it's left on a screen: its escapes gone, a progress bar at its last
//! state, and the stretches it coloured, in the theme's colours.

use std::iter::Peekable;
use std::ops::Range;
use std::str::Chars;

use gpui_kit::Hsla;
use koyo_termd::skip_escape;

use crate::theme::Palette;

/// The theme's colour for a terminal's colour, by its number, 0 to 7.
pub(super) fn terminal_colour(colour: u8, p: Palette) -> Hsla {
    match colour {
        1 => p.removed,
        2 => p.added,
        3 => p.modified,
        4 | 6 => p.code.function,
        5 => p.code.keyword,
        7 => p.text,
        _ => p.muted,
    }
}

/// `text` as it's left on a screen, and which of it was in which colour, by its number.
pub(super) fn colours_of(text: &str) -> (String, Vec<(Range<usize>, u8)>) {
    let mut plain = String::with_capacity(text.len());
    let mut spans = Vec::new();
    let mut open = None;
    for line in text.split_inclusive('\n') {
        let (body, ended) = line
            .strip_suffix('\n')
            .map_or((line, false), |body| (body, true));
        let mut chars = overwritten(body).chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\u{1b}' if chars.peek() == Some(&'[') => {
                    chars.next();
                    let pen =
                        style_escape(&mut chars).map_or(Pen::Keep, |numbers| pen_of(&numbers));
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
