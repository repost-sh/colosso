use std::mem;

use tree_sitter::Node;

use crate::language::{Language, ParseFailed};

use super::attributes::{left_out, path_value};

/// Nodes whose own leading attributes apply to them, rather than to the sibling after them.
const SELF_ATTRIBUTED: [&str; 4] = [
    "match_arm",
    "last_match_arm",
    "field_initializer",
    "shorthand_field_initializer",
];

/// One token of the code the shipped build compiles, in source order.
pub struct Token<'source> {
    pub kind: &'static str,
    pub text: &'source str,
    pub start: usize,
    pub line: usize,
    /// Index into [`Code::sites`]: the inline modules around the token.
    pub site: usize,
}

pub enum Body {
    Inline,
    File { path: Option<String> },
}

/// A `mod` item the shipped build compiles.
pub struct Declared {
    /// The inline modules around the item, outermost first.
    pub within: Vec<String>,
    pub name: String,
    pub line: usize,
    pub body: Body,
}

pub struct Code<'source> {
    pub tokens: Vec<Token<'source>>,
    /// The inline module paths tokens sit in: the first is the file's own module, `[]`.
    pub sites: Vec<Vec<String>>,
    pub modules: Vec<Declared>,
}

/// The tokens and `mod` items of one Rust file, with everything the shipped build leaves out
/// left out, and comments too.
pub fn read(source: &str) -> Result<Code<'_>, ParseFailed> {
    let tree = Language::Rust.parse(source)?;
    let mut reader = Reader {
        source,
        site: 0,
        code: Code {
            tokens: Vec::new(),
            sites: vec![Vec::new()],
            modules: Vec::new(),
        },
    };
    reader.visit(tree.root_node());
    Ok(reader.code)
}

struct Reader<'source> {
    source: &'source str,
    site: usize,
    code: Code<'source>,
}

impl Reader<'_> {
    fn visit(&mut self, node: Node<'_>) {
        if node.child_count() == 0 {
            self.code.tokens.push(Token {
                kind: node.kind(),
                text: &self.source[node.byte_range()],
                start: node.start_byte(),
                line: node.start_position().row + 1,
                site: self.site,
            });
            return;
        }
        if self.left_out_from_within(node) {
            return;
        }
        let mut cursor = node.walk();
        let mut attributes = Vec::new();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "line_comment" | "block_comment" => {}
                "attribute_item" => attributes.push(child),
                _ => {
                    let run = mem::take(&mut attributes);
                    if run.iter().any(|item| left_out(*item, self.source)) {
                        continue;
                    }
                    for item in &run {
                        self.visit(*item);
                    }
                    if child.kind() == "mod_item" {
                        let path = run.iter().find_map(|item| path_value(*item, self.source));
                        self.module(child, path);
                    } else {
                        self.visit(child);
                    }
                }
            }
        }
        for item in attributes {
            self.visit(item);
        }
    }

    fn left_out_from_within(&self, node: Node<'_>) -> bool {
        let applies_to_node = SELF_ATTRIBUTED.contains(&node.kind());
        let mut cursor = node.walk();
        node.children(&mut cursor).any(|child| {
            let own = match child.kind() {
                "inner_attribute_item" => true,
                "attribute_item" => applies_to_node,
                _ => false,
            };
            own && left_out(child, self.source)
        })
    }

    fn module(&mut self, node: Node<'_>, path: Option<String>) {
        let Some(name) = node.child_by_field_name("name") else {
            return;
        };
        let name = self.source[name.byte_range()].to_owned();
        let within = self.code.sites[self.site].clone();
        let line = node.start_position().row + 1;
        let Some(body) = node.child_by_field_name("body") else {
            self.code.modules.push(Declared {
                within,
                name,
                line,
                body: Body::File { path },
            });
            return;
        };
        let mut inner = within.clone();
        inner.push(name.clone());
        self.code.modules.push(Declared {
            within,
            name,
            line,
            body: Body::Inline,
        });
        let outer = mem::replace(&mut self.site, self.code.sites.len());
        self.code.sites.push(inner);
        self.visit(body);
        self.site = outer;
    }
}
