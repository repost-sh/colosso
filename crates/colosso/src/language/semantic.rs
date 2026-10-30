//! The server's own reading of the code colours it over the grammar's
//! (`textDocument/semanticTokens/full`): what it knows to be a method, a field, a type in
//! a macro, a parameter, a doc comment; and in a language with no grammar here, Python,
//! the names. The server's token types are mapped onto the names the theme colours, so
//! the theme decides the colours and a switch of it recolours; a type the theme has no
//! name for, and the few the grammar colours as well (punctuation, operators, namespaces),
//! are left to the grammar. The positions come in the server's encoding and go to the
//! editor in characters.

use std::ops::Range;

use colosso_lsp::lsp_types::request::SemanticTokensFullRequest;
use colosso_lsp::lsp_types::{
    self, PartialResultParams, Position, SemanticToken, SemanticTokenType, SemanticTokens,
    SemanticTokensFullOptions, SemanticTokensLegend, SemanticTokensParams, SemanticTokensResult,
    SemanticTokensServerCapabilities, WorkDoneProgressParams,
};
use gpui_kit::component::input::{DocumentRangeSemanticTokensProvider, Rope};
use gpui_kit::{App, AppContext as _, Task, Window};

use super::providers::Hooks;

/// The names the theme colours that a server's token types map onto: the editor's legend.
const NAMES: [&str; 19] = [
    "type",
    "variable",
    "variable.parameter",
    "property",
    "constant",
    "function",
    "function.method",
    "function.macro",
    "keyword",
    "attribute",
    "comment",
    "comment.doc",
    "string",
    "string.escape",
    "string.special",
    "string.regex",
    "number",
    "boolean",
    "label",
];

impl DocumentRangeSemanticTokensProvider for Hooks {
    fn legend(&self) -> SemanticTokensLegend {
        SemanticTokensLegend {
            token_types: NAMES.iter().map(|name| SemanticTokenType::new(name)).collect(),
            token_modifiers: Vec::new(),
        }
    }

    /// The whole file's tokens, whatever `range` (the editor asks for all of it), in the
    /// editor's legend. None when the server doesn't colour; an error, which keeps what
    /// the editor has, while the server isn't running or has another text, or when it
    /// fails to answer.
    fn semantic_tokens(
        &self,
        text: &Rope,
        _: Range<usize>,
        _: &mut Window,
        cx: &mut App,
    ) -> Task<gpui_kit::Result<SemanticTokens>> {
        let Some(capabilities) = self.capabilities(cx) else {
            return Task::ready(not_now("the server isn't running"));
        };
        let Some(legend) = legend(capabilities.semantic_tokens_provider.as_ref()) else {
            return Task::ready(Ok(SemanticTokens::default()));
        };
        let Some(asked) = self.ask(text, cx) else {
            return Task::ready(not_now("the server has another text"));
        };
        let mapping = Mapping::new(legend);
        cx.background_spawn(async move {
            let params = SemanticTokensParams {
                text_document: asked.document.clone(),
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            };
            let data = match asked.ask::<SemanticTokensFullRequest>(&params).await.flatten() {
                Some(SemanticTokensResult::Tokens(tokens)) => tokens.data,
                Some(SemanticTokensResult::Partial(partial)) => partial.data,
                None => return not_now("the server gave no tokens"),
            };
            Ok(SemanticTokens {
                result_id: None,
                data: mapping.for_editor(&data, |range| asked.for_editor(range)),
            })
        })
    }
}

/// Why the tokens weren't fetched, for the editor to keep the ones it has.
fn not_now<T>(why: &str) -> gpui_kit::Result<T> {
    Err(std::io::Error::other(why).into())
}

/// The server's legend, when it colours whole files.
fn legend(provider: Option<&SemanticTokensServerCapabilities>) -> Option<&SemanticTokensLegend> {
    let options = match provider? {
        SemanticTokensServerCapabilities::SemanticTokensOptions(options) => options,
        SemanticTokensServerCapabilities::SemanticTokensRegistrationOptions(registration) => {
            &registration.semantic_tokens_options
        }
    };
    let full = matches!(
        options.full,
        Some(SemanticTokensFullOptions::Bool(true) | SemanticTokensFullOptions::Delta { .. })
    );
    full.then_some(&options.legend)
}

/// How the server's legend maps onto the editor's: each of its token types to an index of
/// [`NAMES`], or nothing for one the theme has no name for; the same with its
/// `documentation` modifier on, which makes a comment a doc comment; and that modifier's
/// bit.
struct Mapping {
    types: Vec<Option<u32>>,
    documented: Vec<Option<u32>>,
    documentation: u32,
}

impl Mapping {
    fn new(legend: &SemanticTokensLegend) -> Mapping {
        let names: Vec<&str> = (legend.token_types.iter()).map(|kind| named(kind.as_str())).collect();
        let types = names.iter().map(|name| index(name)).collect();
        let documented = (names.iter())
            .map(|name| index(if *name == "comment" { "comment.doc" } else { name }))
            .collect();
        let documentation = (legend.token_modifiers.iter())
            .position(|modifier| modifier.as_str() == "documentation")
            .map_or(0, |bit| 1 << bit);
        Mapping {
            types,
            documented,
            documentation,
        }
    }

    /// `tokens`, as the server encoded them, in the editor's legend and at the editor's
    /// positions, whose columns count characters, which `for_editor` converts a range of
    /// the server's to. The ones the theme has no name for are left out, and so is one
    /// the server put out of order or over a line's end.
    fn for_editor(
        &self,
        tokens: &[SemanticToken],
        for_editor: impl Fn(lsp_types::Range) -> lsp_types::Range,
    ) -> Vec<SemanticToken> {
        let mut out = Vec::with_capacity(tokens.len());
        let (mut read, mut last) = (Position::default(), Position::default());
        for token in tokens {
            if token.delta_line > 0 {
                read = Position::new(read.line + token.delta_line, token.delta_start);
            } else {
                read.character += token.delta_start;
            }
            let documented = token.token_modifiers_bitset & self.documentation != 0;
            let kinds = if documented { &self.documented } else { &self.types };
            let Some(kind) = kinds.get(token.token_type as usize).copied().flatten() else {
                continue;
            };
            let end = Position::new(read.line, read.character + token.length);
            let lsp_types::Range { start, end } = for_editor(lsp_types::Range::new(read, end));
            if start < last || end.line != start.line || end.character <= start.character {
                continue;
            }
            let delta_line = start.line - last.line;
            let delta_start = if delta_line == 0 { start.character - last.character } else { start.character };
            out.push(SemanticToken {
                delta_line,
                delta_start,
                length: end.character - start.character,
                token_type: kind,
                token_modifiers_bitset: 0,
            });
            last = start;
        }
        out
    }
}

/// The index of `name` in [`NAMES`]; none when the theme has no name for it.
fn index(name: &str) -> Option<u32> {
    (NAMES.iter().position(|known| *known == name)).map(|index| index as u32)
}

/// A server's token type as the theme names it: the protocol's, rust-analyzer's and
/// basedpyright's own names for what the theme names otherwise, the rest as they are.
fn named(kind: &str) -> &str {
    match kind {
        "class" | "enum" | "interface" | "struct" | "typeParameter" | "typeAlias" | "union"
        | "builtinType" | "selfTypeKeyword" => "type",
        "parameter" | "selfParameter" | "clsParameter" => "variable.parameter",
        "const" | "static" | "enumMember" | "builtinConstant" => "constant",
        "magicFunction" => "function",
        "method" => "function.method",
        "macro" | "procMacro" => "function.macro",
        "selfKeyword" | "modifier" => "keyword",
        "decorator" | "builtinAttribute" | "derive" | "deriveHelper" => "attribute",
        "character" => "string",
        "escapeSequence" => "string.escape",
        "formatSpecifier" => "string.special",
        "regexp" => "string.regex",
        "lifetime" => "label",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use colosso_lsp::lsp_types::SemanticTokenModifier;
    use colosso_lsp::{Encoding, IndexedText};

    use super::*;

    fn token(delta_line: u32, delta_start: u32, length: u32, kind: u32, modifiers: u32) -> SemanticToken {
        SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type: kind,
            token_modifiers_bitset: modifiers,
        }
    }

    #[test]
    fn a_server_s_type_is_named_as_the_theme_names_it_or_left_to_the_grammar() {
        assert_eq!(named("method"), "function.method");
        assert_eq!(named("selfParameter"), "variable.parameter");
        assert_eq!(named("enumMember"), "constant");
        assert!(index(named("keyword")).is_some());
        assert_eq!(index(named("namespace")), None);
        assert_eq!(index(named("brace")), None);
    }

    #[test]
    fn tokens_come_to_the_editor_in_its_legend_at_character_columns_without_the_uncoloured() {
        let legend = SemanticTokensLegend {
            token_types: vec![
                SemanticTokenType::COMMENT,
                SemanticTokenType::NAMESPACE,
                SemanticTokenType::KEYWORD,
            ],
            token_modifiers: vec![SemanticTokenModifier::STATIC, SemanticTokenModifier::DOCUMENTATION],
        };
        let mapping = Mapping::new(&legend);
        assert_eq!(mapping.types, [index("comment"), None, index("keyword")]);
        assert_eq!(mapping.documented, [index("comment.doc"), None, index("keyword")]);
        assert_eq!(mapping.documentation, 0b10);
        let kind = |name| index(name).unwrap();
        // UTF-16 positions: the crab counts two there, one to the editor.
        let text = IndexedText::new("/// doc\nlet s = \"héllo 🦀\"; let n\n".to_owned());
        let for_editor = |range| text.range(text.offsets(range, Encoding::Utf16), Encoding::Utf32);
        let tokens = [
            token(0, 0, 7, 0, 0b10),
            token(1, 0, 3, 2, 0),
            token(0, 4, 1, 1, 0),
            token(0, 16, 3, 2, 0),
        ];
        assert_eq!(
            mapping.for_editor(&tokens, for_editor),
            [
                token(0, 0, 7, kind("comment.doc"), 0),
                token(1, 0, 3, kind("keyword"), 0),
                token(0, 19, 3, kind("keyword"), 0),
            ]
        );
    }
}
