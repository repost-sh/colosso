//! LSP snippets: the text a completion inserts, with the places to fill in.
//!
//! `$1` and `${1}` are tab stops, `${1:name}` one with a default, `${1|a,b|}`
//! one with choices (the first is inserted), and `$0` is where the caret ends
//! up; a backslash makes the next character literal. `$NAME` is a variable
//! the editor has no value for, so it inserts nothing, or `${NAME:default}`
//! its default. What's parsed is the plain text to insert and its stops, in
//! the order Tab visits them.
//!
//! https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#snippet_syntax

use std::collections::BTreeMap;
use std::ops::Range;

/// A snippet parsed: its text, and its tab stops as byte ranges of it.
#[derive(Clone, Debug)]
pub(crate) struct Snippet {
    pub(crate) text: String,
    /// The stops in the order Tab visits them: by number, with `$0` last;
    /// each with every place it's at, more than one where it's repeated.
    /// There is always a last stop: `$0`, or the end of the text.
    pub(crate) stops: Vec<Vec<Range<usize>>>,
}

impl Snippet {
    pub(crate) fn parse(snippet: &str) -> Snippet {
        let mut parser = Parser::default();
        parser.parse(snippet, 0, false);
        let Parser { text, mut stops } = parser;
        let last = stops.remove(&0).unwrap_or_else(|| vec![text.len()..text.len()]);
        let mut stops: Vec<Vec<Range<usize>>> = stops.into_values().collect();
        stops.push(last);
        Snippet { text, stops }
    }
}

#[derive(Default)]
struct Parser {
    text: String,
    /// By number, so they come out in Tab's order.
    stops: BTreeMap<u32, Vec<Range<usize>>>,
}

impl Parser {
    /// Parses `source` from `i` into the text. Inside a placeholder's braces
    /// it stops at the closing one; either way it returns where it stopped.
    fn parse(&mut self, source: &str, mut i: usize, nested: bool) -> usize {
        while let Some(ch) = source[i..].chars().next() {
            match ch {
                '\\' if i + 1 < source.len() => i = self.escaped(source, i, true),
                '$' => i = self.placeholder(source, i + 1),
                '}' if nested => return i + 1,
                _ => {
                    self.text.push(ch);
                    i += ch.len_utf8();
                }
            }
        }
        i
    }

    /// Just past a `$`: a stop, a variable, or either in braces with what
    /// fills it. A `$` that begins none of them is literal.
    fn placeholder(&mut self, source: &str, i: usize) -> usize {
        let rest = &source[i..];
        if let Some((number, len)) = leading_number(rest) {
            let at = self.text.len();
            self.stop(number, at..at);
            return i + len;
        }
        if let Some(len) = leading_name(rest) {
            return i + len;
        }
        if let Some(inner) = rest.strip_prefix('{') {
            let start = self.text.len();
            if let Some((number, len)) = leading_number(inner) {
                let after = i + 1 + len;
                let end = match source.as_bytes().get(after) {
                    Some(b'}') => Some(after + 1),
                    Some(b':') => Some(self.parse(source, after + 1, true)),
                    Some(b'|') => Some(self.choice(source, after + 1)),
                    _ => None,
                };
                if let Some(end) = end {
                    self.stop(number, start..self.text.len());
                    return end;
                }
            } else if let Some(len) = leading_name(inner) {
                let after = i + 1 + len;
                match source.as_bytes().get(after) {
                    Some(b'}') => return after + 1,
                    Some(b':') => return self.parse(source, after + 1, true),
                    _ => {}
                }
            }
        }
        self.text.push('$');
        i
    }

    /// Choices, from just past their opening `|`: the first goes into the
    /// text; returns where the `|}` closing them ends.
    fn choice(&mut self, source: &str, mut i: usize) -> usize {
        let mut first = true;
        while let Some(ch) = source[i..].chars().next() {
            match ch {
                '\\' if i + 1 < source.len() => i = self.escaped(source, i, first),
                ',' => {
                    first = false;
                    i += 1;
                }
                '|' => {
                    i += 1;
                    if source[i..].starts_with('}') {
                        i += 1;
                    }
                    return i;
                }
                _ => {
                    if first {
                        self.text.push(ch);
                    }
                    i += ch.len_utf8();
                }
            }
        }
        i
    }

    /// A backslash at `i`: the character after it goes into the text as it
    /// is, if `kept`; returns where it ends.
    fn escaped(&mut self, source: &str, i: usize, kept: bool) -> usize {
        let literal = source[i + 1..].chars().next().unwrap_or('\\');
        if kept {
            self.text.push(literal);
        }
        i + 1 + literal.len_utf8()
    }

    fn stop(&mut self, number: u32, range: Range<usize>) {
        self.stops.entry(number).or_default().push(range);
    }
}

/// The number `text` begins with, and how many bytes it takes.
fn leading_number(text: &str) -> Option<(u32, usize)> {
    let len = text.bytes().take_while(u8::is_ascii_digit).count();
    (len > 0).then(|| (text[..len].parse().unwrap_or(u32::MAX), len))
}

/// How many bytes the variable name `text` begins with takes, if any.
fn leading_name(text: &str) -> Option<usize> {
    let first = text.chars().next()?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    Some(
        text.bytes()
            .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            .count(),
    )
}

/// The stops of a snippet inserted, while Tab moves through them.
#[derive(Clone, Debug)]
pub(crate) struct SnippetSession {
    /// Each stop's places as bytes of the text at `version`.
    pub(crate) stops: Vec<Vec<Range<usize>>>,
    pub(crate) current: usize,
    pub(crate) version: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(snippet: &str) -> (String, Vec<Vec<Range<usize>>>) {
        let snippet = Snippet::parse(snippet);
        (snippet.text, snippet.stops)
    }

    #[test]
    fn stops_come_in_tab_order_with_the_end_last() {
        let (text, stops) = parsed("greet(${1:name})$0");
        assert_eq!(text, "greet(name)");
        assert_eq!(stops, vec![vec![6..10], vec![11..11]]);

        let (text, stops) = parsed("for $2 in $1 {\n\t$0\n}");
        assert_eq!(text, "for  in  {\n\t\n}");
        assert_eq!(stops, vec![vec![8..8], vec![4..4], vec![12..12]]);
    }

    #[test]
    fn a_snippet_without_a_final_stop_ends_at_its_end() {
        let (text, stops) = parsed("${1:a}.b");
        assert_eq!(text, "a.b");
        assert_eq!(stops, vec![vec![0..1], vec![3..3]]);
        assert_eq!(parsed("plain"), ("plain".to_owned(), vec![vec![5..5]]));
    }

    #[test]
    fn a_repeated_stop_has_every_place() {
        let (text, stops) = parsed("let ${1:x} = $1;");
        assert_eq!(text, "let x = ;");
        assert_eq!(stops, vec![vec![4..5, 8..8], vec![9..9]]);
    }

    #[test]
    fn choices_insert_the_first_and_nesting_fills_in() {
        let (text, stops) = parsed("${1|one,two\\,three|}!");
        assert_eq!(text, "one!");
        assert_eq!(stops, vec![vec![0..3], vec![4..4]]);

        let (text, stops) = parsed("${1:outer ${2:inner}} end");
        assert_eq!(text, "outer inner end");
        assert_eq!(stops, vec![vec![0..11], vec![6..11], vec![15..15]]);
    }

    #[test]
    fn escapes_and_variables_and_stray_dollars() {
        assert_eq!(parsed("\\$1 \\} \\\\").0, "$1 } \\");
        assert_eq!(parsed("$TM_FILENAME-${TM_LINE:7}-${UNKNOWN}").0, "-7-");
        assert_eq!(parsed("cost: $ and ${").0, "cost: $ and ${");
        assert_eq!(parsed("héllo ${1:wörld}").1, vec![vec![7..13], vec![13..13]]);
    }
}
