use std::ops::Range;

use crate::text::{is_horizontal_space, line_start};

pub struct Rewritten {
    pub text: String,
    pub seams: Vec<usize>,
}

struct Cut {
    range: Range<usize>,
    insert: &'static str,
    whole_lines: bool,
}

pub fn rewrite(source: &str, doomed: &[Range<usize>]) -> Rewritten {
    let mut text = String::with_capacity(source.len());
    let mut seams = Vec::new();
    let mut copied = 0;
    for span in neighbours_merged(source, doomed) {
        let cut = cut_for(source, &span);
        text.push_str(&source[copied..cut.range.start]);
        if cut.whole_lines {
            seams.push(text.len());
        }
        text.push_str(cut.insert);
        copied = cut.range.end;
    }
    text.push_str(&source[copied..]);
    Rewritten { text, seams }
}

fn neighbours_merged(source: &str, doomed: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut spans: Vec<Range<usize>> = Vec::new();
    for range in doomed {
        match spans.last_mut() {
            Some(last)
                if source[last.end..range.start]
                    .chars()
                    .all(is_horizontal_space) =>
            {
                last.end = range.end;
            }
            _ => spans.push(range.clone()),
        }
    }
    spans
}

fn cut_for(source: &str, span: &Range<usize>) -> Cut {
    let line_begin = line_start(source, span.start);
    let line_end = source[span.end..]
        .find('\n')
        .map_or(source.len(), |newline| span.end + newline);
    let before = &source[line_begin..span.start];
    let after = &source[span.end..line_end];
    let leading = before.chars().all(is_horizontal_space);
    let trailing = after
        .chars()
        .all(|character| is_horizontal_space(character) || character == '\r');
    match (leading, trailing) {
        (true, true) => Cut {
            range: line_begin..(line_end + 1).min(source.len()),
            insert: "",
            whole_lines: true,
        },
        (true, false) => Cut {
            range: span.start
                ..span.end + after.len() - after.trim_start_matches(is_horizontal_space).len(),
            insert: "",
            whole_lines: false,
        },
        (false, true) => Cut {
            range: span.start - before.len() + before.trim_end_matches(is_horizontal_space).len()
                ..line_end - usize::from(after.ends_with('\r')),
            insert: "",
            whole_lines: false,
        },
        (false, false) => cut_between_code(before, after, span),
    }
}

fn cut_between_code(before: &str, after: &str, span: &Range<usize>) -> Cut {
    let code_before = before.trim_end_matches(is_horizontal_space);
    let range = span.start - (before.len() - code_before.len())..span.end;
    let glued = match (code_before.chars().next_back(), after.chars().next()) {
        (Some(left), Some(right)) => would_glue(left, right),
        _ => false,
    };
    Cut {
        range,
        insert: if glued { " " } else { "" },
        whole_lines: false,
    }
}

fn would_glue(left: char, right: char) -> bool {
    let word =
        |character: char| character.is_alphanumeric() || character == '_' || character == '$';
    let operator = |character: char| "+-*/%&|^<>=!.:?@~#'".contains(character);
    (word(left) && word(right)) || (operator(left) && operator(right))
}
