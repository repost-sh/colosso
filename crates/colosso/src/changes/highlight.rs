//! Syntax highlighting for diff lines. Lines are tagged with a handful of token kinds rather
//! than colours, so the view colours them from its palette, light or dark.
//!
//! syntect parses, with bat's syntaxes from `two-face`; a token's kind comes from the innermost
//! of its scopes that names one. Highlighting runs within a budget of lines, since it is the
//! slowest part of showing a diff: files past it show plain.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::OnceLock;

use colosso_git::{Content, Lines};
use syntect::parsing::{ParseState, Scope, ScopeStack, ScopeStackOp, SyntaxReference, SyntaxSet};

/// Files with more lines than this on a side show plain.
const LONGEST_HIGHLIGHTED: usize = 20_000;

/// Lines longer than this show plain: minified code costs the parser a lot and says little.
const LONGEST_LINE: usize = 1_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Token {
    Comment,
    Keyword,
    String,
    /// Numbers too.
    Constant,
    Function,
    Type,
    Tag,
    Attribute,
}

/// A line's highlighted spans, as byte ranges with their kind. Unhighlighted text is plain.
pub type LineTokens = Vec<(Range<u32>, Token)>;

/// The tokens of both sides of a file, a line's empty if it shows plain.
#[derive(Clone, Debug, Default)]
pub struct Highlights {
    pub old: Vec<LineTokens>,
    pub new: Vec<LineTokens>,
}

/// The highlights for `content`, a file at `path`, or `None` if its kind of file isn't known or
/// it's too long to highlight.
pub fn highlight(path: &str, content: &Content) -> Option<Highlights> {
    let Content::Text { old, new, .. } = content else {
        return None;
    };
    if old.len().max(new.len()) > LONGEST_HIGHLIGHTED {
        return None;
    }
    let syntaxes = syntaxes();
    let syntax = syntax_for(syntaxes, path, new.iter().next().or(old.iter().next()))?;
    Some(Highlights {
        old: side(syntaxes, syntax, old),
        new: side(syntaxes, syntax, new),
    })
}

/// bat's syntaxes, loaded the first time they're asked for.
fn syntaxes() -> &'static SyntaxSet {
    static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAXES.get_or_init(two_face::syntax::extra_no_newlines)
}

/// The syntax for `path`, by its name (`Makefile`), its extension, or its first line (`#!`).
fn syntax_for<'a>(
    syntaxes: &'a SyntaxSet,
    path: &str,
    first_line: Option<&str>,
) -> Option<&'a SyntaxReference> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let extension = name.rsplit_once('.').map(|(_, extension)| extension);
    syntaxes
        .find_syntax_by_extension(name)
        .or_else(|| extension.and_then(|extension| syntaxes.find_syntax_by_extension(extension)))
        .or_else(|| first_line.and_then(|line| syntaxes.find_syntax_by_first_line(line)))
        .filter(|syntax| syntax.name != "Plain Text")
}

fn side(syntaxes: &SyntaxSet, syntax: &SyntaxReference, lines: &Lines) -> Vec<LineTokens> {
    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    lines
        .iter()
        .map(|line| {
            if line.len() > LONGEST_LINE {
                return Vec::new();
            }
            let Ok(ops) = state.parse_line(line, syntaxes) else {
                return Vec::new();
            };
            let mut tokens: LineTokens = Vec::new();
            let mut start = 0;
            for (end, op) in ops.into_iter().chain([(line.len(), ScopeStackOp::Noop)]) {
                if end > start
                    && let Some(token) = token_of(&stack)
                {
                    match tokens.last_mut() {
                        Some((range, last)) if *last == token && range.end == start as u32 => {
                            range.end = end as u32;
                        }
                        _ => tokens.push((start as u32..end as u32, token)),
                    }
                }
                start = end;
                // A malformed stack only costs this line's colours.
                let _ = stack.apply(&op);
            }
            tokens
        })
        .collect()
}

/// The kind the innermost scope that names one gives, if any does.
fn token_of(stack: &ScopeStack) -> Option<Token> {
    thread_local! {
        static KINDS: RefCell<HashMap<Scope, Option<Token>>> = RefCell::new(HashMap::new());
    }
    KINDS.with_borrow_mut(|kinds| {
        stack.as_slice().iter().rev().find_map(|scope| {
            *kinds
                .entry(*scope)
                .or_insert_with(|| kind_of(&scope.build_string()))
        })
    })
}

/// The kind a `TextMate` scope name like `entity.name.function.rust` stands for.
fn kind_of(scope: &str) -> Option<Token> {
    const KINDS: [(&str, Token); 15] = [
        ("comment", Token::Comment),
        ("string", Token::String),
        ("constant.character.escape", Token::String),
        ("constant", Token::Constant),
        ("keyword.operator", Token::Keyword),
        ("keyword", Token::Keyword),
        ("storage.type", Token::Keyword),
        ("storage.modifier", Token::Keyword),
        ("entity.name.function", Token::Function),
        ("support.function", Token::Function),
        ("entity.name.type", Token::Type),
        ("entity.name.class", Token::Type),
        ("support.type", Token::Type),
        ("entity.name.tag", Token::Tag),
        ("entity.other.attribute-name", Token::Attribute),
    ];
    KINDS
        .iter()
        .find(|(prefix, _)| {
            scope
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
        })
        .map(|(_, kind)| *kind)
}

#[cfg(test)]
mod tests {
    use colosso_git::Hunk;

    use super::*;

    fn text(old: &str, new: &str) -> Content {
        Content::Text {
            old: Lines::new(old),
            new: Lines::new(new),
            hunks: vec![Hunk {
                old: 0..1,
                new: 0..1,
            }],
        }
    }

    fn kinds<'a>(line: &'a str, tokens: &LineTokens) -> Vec<(&'a str, Token)> {
        tokens
            .iter()
            .map(|(range, token)| (&line[range.start as usize..range.end as usize], *token))
            .collect()
    }

    #[test]
    fn rust_is_tagged_by_kind() {
        let line = "fn main() { let n = 42; // done";
        let highlights = highlight("src/main.rs", &text("", line)).unwrap();
        let tokens = kinds(line, &highlights.new[0]);
        assert!(tokens.contains(&("fn", Token::Keyword)), "{tokens:?}");
        assert!(tokens.contains(&("main", Token::Function)), "{tokens:?}");
        assert!(tokens.contains(&("42", Token::Constant)), "{tokens:?}");
        assert!(tokens.contains(&("// done", Token::Comment)), "{tokens:?}");
    }

    #[test]
    fn a_string_spanning_lines_stays_a_string() {
        let source = "const A = `one\ntwo`;";
        let highlights = highlight("a.ts", &text("", source)).unwrap();
        let second = kinds("two`;", &highlights.new[1]);
        assert_eq!(second.first(), Some(&("two`", Token::String)));
    }

    #[test]
    fn files_are_known_by_name_extension_or_first_line() {
        assert!(highlight("Makefile", &text("", "all:\n\techo")).is_some());
        assert!(highlight("crates/app/Cargo.toml", &text("", "[package]")).is_some());
        assert!(highlight("bin/run", &text("", "#!/bin/bash\necho hi")).is_some());
        assert!(highlight("notes.unknownext", &text("", "plain words")).is_none());
    }

    #[test]
    fn binary_and_long_files_are_not_highlighted() {
        assert!(highlight("a.rs", &Content::Binary).is_none());
        let long = "x\n".repeat(LONGEST_HIGHLIGHTED + 1);
        assert!(highlight("a.rs", &text("", &long)).is_none());
    }
}
