use std::fmt;
use std::path::Path;

use tree_sitter::{Language as Grammar, LanguageError, Node, Parser, Tree};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Rust,
    TypeScript,
    Tsx,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Code,
    PlainComment,
    DocComment,
}

#[derive(Debug)]
pub enum ParseFailed {
    Grammar(LanguageError),
    Abandoned,
}

impl fmt::Display for ParseFailed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Grammar(failure) => write!(formatter, "the grammar does not load: {failure}"),
            Self::Abandoned => write!(formatter, "the parser gave up on the file"),
        }
    }
}

impl Language {
    pub fn of_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()? {
            "rs" => Some(Self::Rust),
            "ts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            _ => None,
        }
    }

    fn grammar(self) -> Grammar {
        match self {
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        }
    }

    pub fn parse(self, source: &str) -> Result<Tree, ParseFailed> {
        let mut parser = Parser::new();
        parser
            .set_language(&self.grammar())
            .map_err(ParseFailed::Grammar)?;
        parser.parse(source, None).ok_or(ParseFailed::Abandoned)
    }

    pub fn role(self, node: Node<'_>, source: &str) -> Role {
        match (self, node.kind()) {
            (Self::Rust, "line_comment" | "block_comment") => {
                if node.named_child_count() == 0 {
                    Role::PlainComment
                } else {
                    Role::DocComment
                }
            }
            (Self::TypeScript | Self::Tsx, "comment") => {
                let text = &source[node.byte_range()];
                if text.starts_with("/**") && text != "/**/" {
                    Role::DocComment
                } else {
                    Role::PlainComment
                }
            }
            _ => Role::Code,
        }
    }
}
