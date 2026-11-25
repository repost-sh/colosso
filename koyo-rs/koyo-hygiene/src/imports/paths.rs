use crate::shipped::Token;

/// Where a path starts: the crate root, or `levels` modules up from where it's written, `0`
/// for the module itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Base {
    Crate,
    Up(usize),
}

/// The first segment after the base: an item in it, or the module itself (`self`, `*`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment {
    Item(String),
    Itself,
}

/// A path by its base and its first segment after that base. A use list gives one for each
/// name at its top level.
pub struct Reach {
    pub site: usize,
    pub base: Base,
    pub segment: Segment,
    /// The names each way the path goes on after the segment, as a use list gives it several:
    /// `[b]` and `[c, d]` for `a::{b, c::d}`.
    pub onward: Vec<Vec<String>>,
    pub line: usize,
    pub start: usize,
}

/// Every path in the tokens rooted at `crate::`, `super::`, `self::` or a bare name, in macro
/// arguments too. A bare `name::` is the module's own item `name`; the names in a use list
/// after `path::` are the path's, not the module's. `$crate` is a macro's own crate, not a path
/// into this one.
pub fn reaches(tokens: &[Token<'_>]) -> Vec<Reach> {
    let mut found = Vec::new();
    let mut index = 0;
    while let Some(token) = tokens.get(index) {
        let previous = index
            .checked_sub(1)
            .and_then(|before| kind_at(tokens, before));
        index = match (previous, root_at(tokens, index)) {
            (Some("::"), _) if token.kind == "{" => past_list(tokens, index),
            (Some("::" | "$" | "."), _) => index + 1,
            (_, Some((base, colons))) => follow(tokens, colons, base, token.site, &mut found),
            (_, None) => {
                if token.kind == "identifier" && kind_at(tokens, index + 1) == Some("::") {
                    record(tokens, index, token.site, Base::Up(0), &mut found);
                }
                index + 1
            }
        };
    }
    found
}

/// The index after the use list opening at `index`.
fn past_list(tokens: &[Token<'_>], mut index: usize) -> usize {
    let mut depth = 0_usize;
    while let Some(token) = tokens.get(index) {
        index += 1;
        match token.kind {
            "{" => depth += 1,
            "}" if depth <= 1 => break,
            "}" => depth -= 1,
            _ => {}
        }
    }
    index
}

fn kind_at(tokens: &[Token<'_>], index: usize) -> Option<&'static str> {
    tokens.get(index).map(|token| token.kind)
}

/// The base of a path whose first token is at `index`, and the index of the `::` after it.
fn root_at(tokens: &[Token<'_>], index: usize) -> Option<(Base, usize)> {
    let (base, colons) = match kind_at(tokens, index)? {
        "crate" => (Base::Crate, index + 1),
        "super" => climb(tokens, index),
        "self"
            if kind_at(tokens, index + 1) == Some("::")
                && kind_at(tokens, index + 2) == Some("super") =>
        {
            climb(tokens, index + 2)
        }
        "self" => (Base::Up(0), index + 1),
        _ => return None,
    };
    (kind_at(tokens, colons) == Some("::")).then_some((base, colons))
}

fn climb(tokens: &[Token<'_>], first: usize) -> (Base, usize) {
    let mut levels = 1;
    let mut after = first + 1;
    while kind_at(tokens, after) == Some("::") && kind_at(tokens, after + 1) == Some("super") {
        levels += 1;
        after += 2;
    }
    (Base::Up(levels), after)
}

/// Records what the path reaches after the `::` at `colons`, and returns the index of the
/// first token after the part of the path it read.
fn follow(
    tokens: &[Token<'_>],
    colons: usize,
    base: Base,
    site: usize,
    found: &mut Vec<Reach>,
) -> usize {
    let at = colons + 1;
    match tokens.get(at) {
        Some(token) if token.kind == "{" => listed(tokens, at + 1, base, site, found),
        Some(_) => {
            record(tokens, at, site, base, found);
            at + 1
        }
        None => at,
    }
}

/// Each name at the top level of the use list opening before `index`. An element that climbs
/// further, as `super::{super::x}` does, is followed from where it climbs to.
fn listed(
    tokens: &[Token<'_>],
    index: usize,
    base: Base,
    site: usize,
    found: &mut Vec<Reach>,
) -> usize {
    elements(tokens, index, |element| {
        if let (Base::Up(levels), Some((Base::Up(more), colons))) = (base, root_at(tokens, element))
        {
            follow(tokens, colons, Base::Up(levels + more), site, found)
        } else {
            record(tokens, element, site, base, found);
            element
        }
    })
}

/// Hands `read` the index each element at the top level of the use list opening before `index`
/// starts at, for the index after the part of it `read` read, and returns the index after the
/// list.
fn elements(tokens: &[Token<'_>], mut index: usize, mut read: impl FnMut(usize) -> usize) -> usize {
    let mut depth = 1;
    let mut element_starts = true;
    while let Some(token) = tokens.get(index) {
        if element_starts && depth == 1 {
            element_starts = false;
            let after = read(index);
            if after > index {
                index = after;
                continue;
            }
        }
        match token.kind {
            "{" => depth += 1,
            "}" if depth == 1 => return index + 1,
            "}" => depth -= 1,
            "," if depth == 1 => element_starts = true,
            _ => {}
        }
        index += 1;
    }
    index
}

/// The path from `base` whose first segment is the token at `index`, if the token names one.
fn record(tokens: &[Token<'_>], index: usize, site: usize, base: Base, found: &mut Vec<Reach>) {
    let Some(token) = tokens.get(index) else {
        return;
    };
    let segment = match (name(token), token.kind) {
        (Some(name), _) => Segment::Item(name),
        (None, "self" | "*") => Segment::Itself,
        (None, _) => return,
    };
    let mut onward = Vec::new();
    go_on(tokens, index + 1, &mut Vec::new(), &mut onward);
    found.push(Reach {
        site,
        base,
        segment,
        onward,
        line: token.line,
        start: token.start,
    });
}

/// Adds to `found` the names each way the path goes on at `index`, the end of one of its
/// segments, each after `names`, and returns the index after the path.
fn go_on(
    tokens: &[Token<'_>],
    index: usize,
    names: &mut Vec<String>,
    found: &mut Vec<Vec<String>>,
) -> usize {
    let next = (kind_at(tokens, index) == Some("::"))
        .then(|| tokens.get(index + 1))
        .flatten();
    match next {
        Some(token) if token.kind == "{" => elements(tokens, index + 2, |element| {
            match kind_at(tokens, element) {
                Some("}" | ",") | None => element,
                Some(_) => step(tokens, element, names, found),
            }
        }),
        Some(_) => step(tokens, index + 1, names, found),
        None => {
            found.push(names.clone());
            index
        }
    }
}

/// Goes on with a path through the token at `index`: past the name it is, or no further.
fn step(
    tokens: &[Token<'_>],
    index: usize,
    names: &mut Vec<String>,
    found: &mut Vec<Vec<String>>,
) -> usize {
    let Some(next) = tokens.get(index).and_then(name) else {
        found.push(names.clone());
        return index;
    };
    names.push(next);
    let after = go_on(tokens, index + 1, names, found);
    names.pop();
    after
}

fn name(token: &Token<'_>) -> Option<String> {
    matches!(token.kind, "identifier" | "type_identifier").then(|| {
        token
            .text
            .strip_prefix("r#")
            .unwrap_or(token.text)
            .to_owned()
    })
}
