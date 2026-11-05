//! Finding a link in a line's text, around the character the pointer is on: a URL, or a path
//! to a file as tools print them, Claude Code's `Read(src/main.rs)` and `src/a.rs:42:7` and a
//! path framed in box drawing among them. Which paths are files is the caller's to say.

use std::ops::Range;
use std::path::PathBuf;

use super::LinkTarget;

/// The longest run of text a link is looked for in: anything longer isn't a link anyone
/// points at, and looking through it would cost every move of the pointer.
const LONGEST: usize = 1024;

/// How many of the brackets and separators on each side of the pointer a path may start or
/// end at.
const SPLITS: usize = 8;

/// What a URL may start with.
const SCHEMES: [&str; 3] = ["https://", "http://", "file://"];

/// Says which file, if any, a path as printed names: one already there, as an absolute path.
pub(crate) type Resolve<'a> = dyn FnMut(&str) -> Option<PathBuf> + 'a;

/// The link at character `at` of `text`, and the characters it covers.
pub(crate) fn find(
    text: &[char],
    at: usize,
    file: &mut Resolve<'_>,
) -> Option<(Range<usize>, LinkTarget)> {
    if text.get(at).is_none_or(|c| ends_path(*c)) {
        return None;
    }
    if let Some(range) = url(text, at) {
        let uri: String = text.get(range.clone())?.iter().collect();
        return Some((range, target(&uri, file)?));
    }
    path(text, at, file)
}

/// What a URI opens: a web page, or a file that's there (`file://`, at a line when its
/// fragment names one, `#L42`). Nothing for other schemes.
pub(crate) fn target(uri: &str, file: &mut Resolve<'_>) -> Option<LinkTarget> {
    if uri.starts_with("https://") || uri.starts_with("http://") {
        return Some(LinkTarget::Url(uri.to_owned()));
    }
    let (path, line) = super::file_uri(uri)?;
    located(&path, file).map(|(_, found)| super::at_line(found, line))
}

/// The URL around `at`: from the scheme before it to the end of its run of characters, less
/// the punctuation and unmatched closing brackets a sentence puts after a URL.
fn url(text: &[char], at: usize) -> Option<Range<usize>> {
    let run = run(text, at, ends_url)?;
    let start = (run.start..=at).rev().find(|&i| {
        SCHEMES
            .iter()
            .any(|scheme| starts_with(text.get(i..run.end).unwrap_or_default(), scheme))
    })?;
    let mut end = run.end;
    while end > start {
        let Some(&last) = text.get(end - 1) else {
            break;
        };
        let unmatched = |open: char| {
            let inside = text.get(start..end).unwrap_or_default();
            let count = |c: char| inside.iter().filter(|&&other| other == c).count();
            count(last) > count(open)
        };
        let trailing = matches!(last, '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '"')
            || (last == ')' && unmatched('('))
            || (last == ']' && unmatched('['));
        if !trailing {
            break;
        }
        end -= 1;
    }
    let scheme = SCHEMES
        .iter()
        .find(|scheme| starts_with(text.get(start..end).unwrap_or_default(), scheme))
        .map_or(0, |scheme| scheme.len());
    (at < end && end > start + scheme).then_some(start..end)
}

/// The file path around `at`. Its run of characters is tried whole and in the pieces its
/// brackets and separators make, longest first, so `Read(src/a.rs)` finds `src/a.rs` while
/// `app/(auth)/page.tsx` is found whole: the first that's a file is the one.
fn path(
    text: &[char],
    at: usize,
    file: &mut Resolve<'_>,
) -> Option<(Range<usize>, LinkTarget)> {
    let run = run(text, at, ends_path)?;
    let splits = |i: &usize| {
        text.get(*i)
            .is_some_and(|c| matches!(c, '(' | ')' | '[' | ']' | '{' | '}' | ',' | '=' | ';'))
    };
    let starts: Vec<usize> = std::iter::once(run.start)
        .chain((run.start..at).rev().filter(splits).take(SPLITS).map(|i| i + 1))
        .collect();
    let ends: Vec<usize> = std::iter::once(run.end)
        .chain((at + 1..run.end).filter(splits).take(SPLITS))
        .collect();
    let mut pieces: Vec<Range<usize>> = (starts.iter())
        .flat_map(|&start| ends.iter().map(move |&end| start..end))
        .filter(|piece| piece.start <= at && at < piece.end)
        .collect();
    pieces.sort_by_key(|piece| std::cmp::Reverse(piece.len()));
    pieces.into_iter().find_map(|piece| {
        let shown: String = text.get(piece.clone())?.iter().collect();
        let (covers, target) = located(&shown, file)?;
        let range = piece.start..piece.start + covers;
        range.contains(&at).then_some((range, target))
    })
}

/// The file `shown` names, and how many of its characters name it: as it is, with a line or
/// a line and column after it (`a.rs:42`, `a.rs:42:7`), and the same without the full stops
/// and colons a sentence or a compiler puts after it.
pub(crate) fn located(shown: &str, file: &mut Resolve<'_>) -> Option<(usize, LinkTarget)> {
    let trimmed = shown.trim_end_matches(['.', ':']);
    let trimmed = (trimmed.len() < shown.len()).then_some(trimmed);
    std::iter::once(shown).chain(trimmed).find_map(|text| {
        let with_line = numbered(text).map(|(path, line, column)| (path, Some(line), column));
        with_line
            .into_iter()
            .chain(std::iter::once((text, None, None)))
            .find_map(|(path, line, column)| {
                let path = file(path)?;
                Some((text.chars().count(), LinkTarget::File { path, line, column }))
            })
    })
}

/// `text` as a path, a line and maybe a column: `a.rs:42` or `a.rs:42:7`.
fn numbered(text: &str) -> Option<(&str, u32, Option<u32>)> {
    let (rest, last) = number_at_end(text)?;
    Some(match number_at_end(rest) {
        Some((path, line)) => (path, line, Some(last)),
        None => (rest, last, None),
    })
}

/// The number after the last colon of `text`, and what's before the colon.
fn number_at_end(text: &str) -> Option<(&str, u32)> {
    let head = text.trim_end_matches(|c: char| c.is_ascii_digit());
    let number = text.get(head.len()..).filter(|digits| !digits.is_empty())?;
    let head = head.strip_suffix(':').filter(|head| !head.is_empty())?;
    Some((head, number.parse().ok()?))
}

/// The run of characters around `at` that `ends` doesn't stop, when it isn't too long to be
/// a link.
fn run(text: &[char], at: usize, ends: fn(char) -> bool) -> Option<Range<usize>> {
    let goes = |i: &usize| text.get(*i).is_some_and(|c| !ends(*c));
    if !goes(&at) {
        return None;
    }
    let start = (0..at)
        .rev()
        .take(LONGEST)
        .take_while(goes)
        .last()
        .unwrap_or(at);
    let end = (at..text.len())
        .take(LONGEST)
        .take_while(goes)
        .last()
        .map_or(at, |last| last + 1);
    (end - start < LONGEST).then_some(start..end)
}

fn starts_with(text: &[char], prefix: &str) -> bool {
    prefix.chars().count() <= text.len() && prefix.chars().zip(text).all(|(a, b)| a == *b)
}

/// What a URL can't hold, unescaped.
fn ends_url(c: char) -> bool {
    c.is_whitespace()
        || matches!(c, '"' | '<' | '>' | '`' | '{' | '}' | '|' | '\\' | '^')
        || symbol(c)
}

/// What ends a path as tools print it: spaces, quotes and the frames and bullets around it.
fn ends_path(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>' | '|') || symbol(c)
}

/// The arrows, box drawing, shapes, bullets and icons terminal programs draw with, which no
/// path or URL holds: Claude Code's `⏺` and `⎿`, a frame's `│`, a list's `•`.
fn symbol(c: char) -> bool {
    matches!(c,
        '\u{2190}'..='\u{21FF}' // Arrows
        | '\u{2300}'..='\u{23FF}' // Miscellaneous technical: ⏺, ⎿
        | '\u{2500}'..='\u{27BF}' // Box drawing, blocks, shapes, symbols, dingbats
        | '\u{E000}'..='\u{F8FF}' // Private use: the icons of Nerd Fonts
        | '•' | '·' | '…' | '‹' | '›' | '«' | '»'
    )
}
