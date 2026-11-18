use super::code::Token;

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
                    record(&mut found, token.site, Base::Up(0), token);
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
        Some(token) => {
            record(found, site, base, token);
            at + 1
        }
        None => at,
    }
}

/// Each name at the top level of the use list opening before `index`. An element that climbs
/// further, as `super::{super::x}` does, is followed from where it climbs to.
fn listed(
    tokens: &[Token<'_>],
    mut index: usize,
    base: Base,
    site: usize,
    found: &mut Vec<Reach>,
) -> usize {
    let mut depth = 1;
    let mut element_starts = true;
    while let Some(token) = tokens.get(index) {
        if element_starts && depth == 1 {
            element_starts = false;
            if let (Base::Up(levels), Some((Base::Up(more), colons))) =
                (base, root_at(tokens, index))
            {
                index = follow(tokens, colons, Base::Up(levels + more), site, found);
                continue;
            }
            record(found, site, base, token);
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

/// The path from `base` whose first segment is `token`, if the token names one.
fn record(found: &mut Vec<Reach>, site: usize, base: Base, token: &Token<'_>) {
    let segment = match token.kind {
        "identifier" | "type_identifier" => {
            let name = token.text.strip_prefix("r#").unwrap_or(token.text);
            Segment::Item(name.to_owned())
        }
        "self" | "*" => Segment::Itself,
        _ => return,
    };
    found.push(Reach {
        site,
        base,
        segment,
        line: token.line,
        start: token.start,
    });
}
