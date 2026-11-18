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
    /// Index into [`Code::scopes`]: the innermost function or closure around the token.
    pub scope: Option<usize>,
}

/// A function or a closure, and what tells whether the code in it may block.
pub struct Scope<'source> {
    /// The function or closure it's written in.
    pub parent: Option<usize>,
    pub kind: Kind<'source>,
    pub parameters: Vec<Parameter<'source>>,
}

pub enum Kind<'source> {
    Function,
    /// A closure, with the path of the function it's handed to when it's written as an
    /// argument of a call to one by its path, as `blocking::spawn(executor, |may| ...)`.
    Closure {
        callee: Vec<&'source str>,
    },
}

/// A function's or closure's parameter: the name it binds, if it's a plain name, and the path
/// of its type with references and type arguments taken off, empty where that isn't a path
/// (`impl Trait`, a tuple) or none is written.
pub struct Parameter<'source> {
    pub name: Option<&'source str>,
    pub ty: Vec<&'source str>,
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
    /// The functions and closures tokens sit in, each after the one around it.
    pub scopes: Vec<Scope<'source>>,
    pub modules: Vec<Declared>,
}

/// The tokens and `mod` items of one Rust file, with everything the shipped build leaves out
/// left out, and comments too.
pub fn read(source: &str) -> Result<Code<'_>, ParseFailed> {
    let tree = Language::Rust.parse(source)?;
    let mut reader = Reader {
        source,
        site: 0,
        scope: None,
        code: Code {
            tokens: Vec::new(),
            sites: vec![Vec::new()],
            scopes: Vec::new(),
            modules: Vec::new(),
        },
    };
    reader.visit(tree.root_node());
    Ok(reader.code)
}

struct Reader<'source> {
    source: &'source str,
    site: usize,
    scope: Option<usize>,
    code: Code<'source>,
}

impl<'source> Reader<'source> {
    fn visit(&mut self, node: Node<'_>) {
        if node.child_count() == 0 {
            self.code.tokens.push(Token {
                kind: node.kind(),
                text: &self.source[node.byte_range()],
                start: node.start_byte(),
                line: node.start_position().row + 1,
                site: self.site,
                scope: self.scope,
            });
            return;
        }
        if self.left_out_from_within(node) {
            return;
        }
        let outer = self.scope;
        if let Some(scope) = self.scope_of(node) {
            self.scope = Some(self.code.scopes.len());
            self.code.scopes.push(scope);
        }
        self.visit_children(node);
        self.scope = outer;
    }

    fn visit_children(&mut self, node: Node<'_>) {
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
        let outer_scope = self.scope.take();
        self.code.sites.push(inner);
        self.visit(body);
        self.site = outer;
        self.scope = outer_scope;
    }

    /// The scope `node` opens, if it's a function or a closure.
    fn scope_of(&self, node: Node<'_>) -> Option<Scope<'source>> {
        let kind = match node.kind() {
            "function_item" => Kind::Function,
            "closure_expression" => Kind::Closure {
                callee: self.callee(node),
            },
            _ => return None,
        };
        let mut cursor = node.walk();
        let parameters = node
            .child_by_field_name("parameters")
            .map(|list| {
                list.named_children(&mut cursor)
                    .filter_map(|parameter| self.parameter(parameter))
                    .collect()
            })
            .unwrap_or_default();
        Some(Scope {
            parent: self.scope,
            kind,
            parameters,
        })
    }

    /// The path of the function `closure` is an argument of, empty where it isn't one.
    fn callee(&self, closure: Node<'_>) -> Vec<&'source str> {
        let function = closure
            .parent()
            .filter(|arguments| arguments.kind() == "arguments")
            .and_then(|arguments| arguments.parent())
            .filter(|call| call.kind() == "call_expression")
            .and_then(|call| call.child_by_field_name("function"));
        let function = match function {
            Some(generic) if generic.kind() == "generic_function" => {
                generic.child_by_field_name("function")
            }
            function => function,
        };
        function
            .filter(|function| matches!(function.kind(), "identifier" | "scoped_identifier"))
            .map(|path| self.segments(path))
            .unwrap_or_default()
    }

    fn parameter(&self, node: Node<'_>) -> Option<Parameter<'source>> {
        match node.kind() {
            "parameter" => Some(Parameter {
                name: node
                    .child_by_field_name("pattern")
                    .filter(|pattern| pattern.kind() == "identifier")
                    .map(|name| &self.source[name.byte_range()]),
                ty: node
                    .child_by_field_name("type")
                    .map(|ty| self.type_path(ty))
                    .unwrap_or_default(),
            }),
            "identifier" => Some(Parameter {
                name: Some(&self.source[node.byte_range()]),
                ty: Vec::new(),
            }),
            "attribute_item" => None,
            _ => Some(Parameter {
                name: None,
                ty: Vec::new(),
            }),
        }
    }

    fn type_path(&self, ty: Node<'_>) -> Vec<&'source str> {
        match ty.kind() {
            "reference_type" | "generic_type" => ty
                .child_by_field_name("type")
                .map(|inner| self.type_path(inner))
                .unwrap_or_default(),
            "type_identifier" | "scoped_type_identifier" | "identifier" | "scoped_identifier" => {
                self.segments(ty)
            }
            _ => Vec::new(),
        }
    }

    fn segments(&self, path: Node<'_>) -> Vec<&'source str> {
        self.source[path.byte_range()]
            .split("::")
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
            .collect()
    }
}
