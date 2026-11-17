use std::ops::Range;

use tree_sitter::{Node, Point, Tree};

use crate::language::{Language, ParseFailed, Role};
use crate::text::alone_on_line;

const SAFETY_HEAD: &str = "// SAFETY:";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    Plain,
    SafetyHead { attached: bool },
    SafetyTail,
}

#[derive(Clone, Debug)]
pub struct Comment {
    pub range: Range<usize>,
    pub start: Point,
    pub standing: Standing,
}

pub struct Scanned {
    pub tree: Tree,
    pub comments: Vec<Comment>,
}

pub fn scan(language: Language, source: &str) -> Result<Scanned, ParseFailed> {
    let tree = language.parse(source)?;
    let comments = comments_of(language, &tree, source);
    Ok(Scanned { tree, comments })
}

fn comments_of(language: Language, tree: &Tree, source: &str) -> Vec<Comment> {
    let mut plain = Vec::new();
    let mut unsafe_keywords = Vec::new();
    walk(tree, |node| match language.role(node, source) {
        Role::PlainComment => {
            plain.push(node);
            false
        }
        Role::DocComment => false,
        Role::Code => {
            if language == Language::Rust && node.kind() == "unsafe" {
                unsafe_keywords.push(node.start_byte());
            }
            true
        }
    });
    let standings = if language == Language::Rust {
        rust_standings(&plain, source, &unsafe_keywords)
    } else {
        vec![Standing::Plain; plain.len()]
    };
    plain
        .iter()
        .zip(standings)
        .map(|(node, standing)| Comment {
            range: comment_range(language, *node, source),
            start: node.start_position(),
            standing,
        })
        .collect()
}

fn comment_range(language: Language, node: Node<'_>, source: &str) -> Range<usize> {
    let range = node.byte_range();
    if language == Language::Rust && source[range.clone()].ends_with('\r') {
        range.start..range.end - 1
    } else {
        range
    }
}

fn walk<'tree>(tree: &'tree Tree, mut visit: impl FnMut(Node<'tree>) -> bool) {
    let mut cursor = tree.walk();
    loop {
        if visit(cursor.node()) && cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return;
            }
        }
    }
}

fn rust_standings(plain: &[Node<'_>], source: &str, unsafe_keywords: &[usize]) -> Vec<Standing> {
    let mut standings = vec![Standing::Plain; plain.len()];
    let mut run_open = false;
    for (index, node) in plain.iter().enumerate() {
        if starts_safety_run(*node, source) {
            standings[index] = Standing::SafetyHead { attached: false };
            run_open = true;
        } else if run_open && continues_run(plain[index - 1], *node, source) {
            standings[index] = Standing::SafetyTail;
        } else {
            run_open = false;
        }
    }
    for head in 0..plain.len() {
        if matches!(standings[head], Standing::SafetyHead { .. }) {
            let mut last = head;
            while standings.get(last + 1) == Some(&Standing::SafetyTail) {
                last += 1;
            }
            standings[head] = Standing::SafetyHead {
                attached: attached_to_unsafe(plain[head], plain[last], source, unsafe_keywords),
            };
        }
    }
    standings
}

fn starts_safety_run(node: Node<'_>, source: &str) -> bool {
    node.kind() == "line_comment" && source[node.byte_range()].starts_with(SAFETY_HEAD)
}

fn continues_run(previous: Node<'_>, node: Node<'_>, source: &str) -> bool {
    node.kind() == "line_comment"
        && node.start_position().row == previous.end_position().row + 1
        && alone_on_line(source, node.start_byte())
}

fn attached_to_unsafe(
    head: Node<'_>,
    last: Node<'_>,
    source: &str,
    unsafe_keywords: &[usize],
) -> bool {
    alone_on_line(source, head.start_byte())
        && (first_thing_in_unsafe_block(head)
            || described_range(last).is_some_and(|range| holds_unsafe(unsafe_keywords, &range)))
}

fn described_range(last: Node<'_>) -> Option<Range<usize>> {
    let next = next_substantive_sibling(last)?;
    let parent = next.parent()?;
    let end = match parent.kind() {
        "token_tree" => statement_end_in_token_tree(next, parent),
        "field_expression" if next.kind() == "." => chain_segment_end(parent),
        _ => own_code_end(next),
    };
    Some(next.start_byte()..end)
}

fn own_code_end(node: Node<'_>) -> usize {
    let holds_other_code = matches!(
        node.kind(),
        "function_item" | "impl_item" | "trait_item" | "mod_item" | "foreign_mod_item"
    );
    node.child_by_field_name("body")
        .filter(|_| holds_other_code)
        .map_or(node.end_byte(), |body| body.start_byte())
}

fn statement_end_in_token_tree(first: Node<'_>, token_tree: Node<'_>) -> usize {
    let mut token = Some(first);
    while let Some(current) = token {
        if current.kind() == ";" {
            return current.end_byte();
        }
        token = current.next_sibling();
    }
    token_tree.end_byte()
}

fn chain_segment_end(field_expression: Node<'_>) -> usize {
    match field_expression.parent() {
        Some(call) if call.kind() == "call_expression" => call.end_byte(),
        _ => field_expression.end_byte(),
    }
}

fn next_substantive_sibling(node: Node<'_>) -> Option<Node<'_>> {
    let mut next = node.next_sibling();
    while let Some(sibling) = next {
        if !matches!(
            sibling.kind(),
            "line_comment" | "block_comment" | "attribute_item"
        ) {
            return Some(sibling);
        }
        next = sibling.next_sibling();
    }
    None
}

fn holds_unsafe(unsafe_keywords: &[usize], range: &Range<usize>) -> bool {
    let first = unsafe_keywords.partition_point(|&offset| offset < range.start);
    unsafe_keywords
        .get(first)
        .is_some_and(|&offset| offset < range.end)
}

fn first_thing_in_unsafe_block(head: Node<'_>) -> bool {
    let in_unsafe_block = head.parent().is_some_and(|block| {
        block.kind() == "block"
            && block
                .parent()
                .is_some_and(|owner| owner.kind() == "unsafe_block")
    });
    if !in_unsafe_block {
        return false;
    }
    let mut before = head.prev_sibling();
    while let Some(sibling) = before {
        match sibling.kind() {
            "{" => return true,
            "line_comment" | "block_comment" => before = sibling.prev_sibling(),
            _ => return false,
        }
    }
    false
}
