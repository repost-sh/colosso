//! What the client tells a server it can do, in one place. A server only offers what the
//! client says it can take, so a feature missing here is a feature no server will offer.

use lsp_types::{
    ClientCapabilities, CodeActionCapabilityResolveSupport, CodeActionClientCapabilities,
    CodeActionKind, CodeActionKindLiteralSupport, CodeActionLiteralSupport,
    CompletionClientCapabilities, CompletionItemCapability, CompletionItemCapabilityResolveSupport,
    CompletionItemTag, DiagnosticTag, DidChangeWatchedFilesClientCapabilities,
    DocumentSymbolClientCapabilities, DynamicRegistrationClientCapabilities,
    GeneralClientCapabilities, GotoCapability, HoverClientCapabilities,
    InlayHintClientCapabilities, InlayHintResolveClientCapabilities, InsertTextMode,
    InsertTextModeSupport, MarkupKind, ParameterInformationSettings, PositionEncodingKind,
    PrepareSupportDefaultBehavior, PublishDiagnosticsClientCapabilities, RenameClientCapabilities,
    ResourceOperationKind, SemanticTokenModifier, SemanticTokenType,
    SemanticTokensClientCapabilities, SemanticTokensClientCapabilitiesRequests,
    SemanticTokensFullOptions, ShowDocumentClientCapabilities, SignatureHelpClientCapabilities,
    SignatureInformationSettings, TagSupport, TextDocumentClientCapabilities,
    TextDocumentSyncClientCapabilities, TokenFormat, WindowClientCapabilities,
    WorkspaceClientCapabilities, WorkspaceEditClientCapabilities,
    WorkspaceSymbolClientCapabilities, WorkspaceSymbolResolveSupportCapability,
};
use serde_json::json;

/// Everything the client says it supports when it initializes a server.
pub(crate) fn client_capabilities() -> ClientCapabilities {
    ClientCapabilities {
        general: Some(general()),
        window: Some(window()),
        workspace: Some(workspace()),
        text_document: Some(text_document()),
        // rust-analyzer's own: it says when it's loaded and indexed, and when it reloads,
        // which `Client::ready` waits on.
        experimental: Some(json!({"serverStatusNotification": true})),
        ..ClientCapabilities::default()
    }
}

fn general() -> GeneralClientCapabilities {
    GeneralClientCapabilities {
        // UTF-8 first: editors index text by bytes, so a server that agrees spares every
        // position a conversion. UTF-16 is the protocol's default, which every server has.
        position_encodings: Some(vec![
            PositionEncodingKind::UTF8,
            PositionEncodingKind::UTF16,
        ]),
        ..GeneralClientCapabilities::default()
    }
}

fn window() -> WindowClientCapabilities {
    WindowClientCapabilities {
        // Indexing and checking report progress, so the app can say what the server is doing.
        work_done_progress: Some(true),
        show_document: Some(ShowDocumentClientCapabilities { support: true }),
        show_message: None,
    }
}

fn workspace() -> WorkspaceClientCapabilities {
    WorkspaceClientCapabilities {
        apply_edit: Some(true),
        workspace_edit: Some(WorkspaceEditClientCapabilities {
            document_changes: Some(true),
            resource_operations: Some(vec![
                ResourceOperationKind::Create,
                ResourceOperationKind::Rename,
                ResourceOperationKind::Delete,
            ]),
            ..WorkspaceEditClientCapabilities::default()
        }),
        did_change_configuration: Some(dynamic(false)),
        // The app watches the checkout already; the server asks for what it wants told about.
        did_change_watched_files: Some(DidChangeWatchedFilesClientCapabilities {
            dynamic_registration: Some(true),
            relative_pattern_support: Some(true),
        }),
        symbol: Some(WorkspaceSymbolClientCapabilities {
            resolve_support: Some(WorkspaceSymbolResolveSupportCapability {
                properties: vec!["location.range".to_owned()],
            }),
            ..WorkspaceSymbolClientCapabilities::default()
        }),
        workspace_folders: Some(true),
        configuration: Some(true),
        ..WorkspaceClientCapabilities::default()
    }
}

fn text_document() -> TextDocumentClientCapabilities {
    TextDocumentClientCapabilities {
        synchronization: Some(TextDocumentSyncClientCapabilities {
            did_save: Some(true),
            ..TextDocumentSyncClientCapabilities::default()
        }),
        completion: Some(completion()),
        hover: Some(HoverClientCapabilities {
            dynamic_registration: None,
            content_format: Some(markup()),
        }),
        signature_help: Some(SignatureHelpClientCapabilities {
            dynamic_registration: None,
            signature_information: Some(SignatureInformationSettings {
                documentation_format: Some(markup()),
                parameter_information: Some(ParameterInformationSettings {
                    label_offset_support: Some(true),
                }),
                active_parameter_support: Some(true),
            }),
            context_support: Some(true),
        }),
        declaration: Some(goto()),
        definition: Some(goto()),
        type_definition: Some(goto()),
        implementation: Some(goto()),
        references: Some(dynamic(false)),
        document_highlight: Some(dynamic(false)),
        document_symbol: Some(DocumentSymbolClientCapabilities {
            hierarchical_document_symbol_support: Some(true),
            ..DocumentSymbolClientCapabilities::default()
        }),
        formatting: Some(dynamic(false)),
        range_formatting: Some(dynamic(false)),
        rename: Some(RenameClientCapabilities {
            dynamic_registration: None,
            prepare_support: Some(true),
            prepare_support_default_behavior: Some(PrepareSupportDefaultBehavior::IDENTIFIER),
            honors_change_annotations: None,
        }),
        code_action: Some(code_action()),
        publish_diagnostics: Some(PublishDiagnosticsClientCapabilities {
            related_information: Some(true),
            tag_support: Some(TagSupport {
                value_set: vec![DiagnosticTag::UNNECESSARY, DiagnosticTag::DEPRECATED],
            }),
            // Diagnostics name the version they were computed for, so stale ones can be told.
            version_support: Some(true),
            code_description_support: Some(true),
            data_support: Some(true),
        }),
        inlay_hint: Some(InlayHintClientCapabilities {
            dynamic_registration: None,
            resolve_support: Some(InlayHintResolveClientCapabilities {
                properties: ["tooltip", "textEdits", "label.tooltip", "label.location"]
                    .map(str::to_owned)
                    .to_vec(),
            }),
        }),
        semantic_tokens: Some(semantic_tokens()),
        ..TextDocumentClientCapabilities::default()
    }
}

fn completion() -> CompletionClientCapabilities {
    CompletionClientCapabilities {
        completion_item: Some(CompletionItemCapability {
            snippet_support: Some(true),
            documentation_format: Some(markup()),
            deprecated_support: Some(true),
            preselect_support: Some(true),
            tag_support: Some(TagSupport {
                value_set: vec![CompletionItemTag::DEPRECATED],
            }),
            insert_replace_support: Some(true),
            // The list comes back light; what a row needs only once it's chosen comes later.
            resolve_support: Some(CompletionItemCapabilityResolveSupport {
                properties: ["documentation", "detail", "additionalTextEdits"]
                    .map(str::to_owned)
                    .to_vec(),
            }),
            insert_text_mode_support: Some(InsertTextModeSupport {
                value_set: vec![InsertTextMode::AS_IS, InsertTextMode::ADJUST_INDENTATION],
            }),
            label_details_support: Some(true),
            ..CompletionItemCapability::default()
        }),
        context_support: Some(true),
        ..CompletionClientCapabilities::default()
    }
}

fn code_action() -> CodeActionClientCapabilities {
    let kinds = [
        CodeActionKind::EMPTY,
        CodeActionKind::QUICKFIX,
        CodeActionKind::REFACTOR,
        CodeActionKind::REFACTOR_EXTRACT,
        CodeActionKind::REFACTOR_INLINE,
        CodeActionKind::REFACTOR_REWRITE,
        CodeActionKind::SOURCE,
        CodeActionKind::SOURCE_ORGANIZE_IMPORTS,
    ];
    CodeActionClientCapabilities {
        code_action_literal_support: Some(CodeActionLiteralSupport {
            code_action_kind: CodeActionKindLiteralSupport {
                value_set: kinds.iter().map(|kind| kind.as_str().to_owned()).collect(),
            },
        }),
        is_preferred_support: Some(true),
        disabled_support: Some(true),
        data_support: Some(true),
        // Actions are listed without their edits, which are worked out for the one chosen.
        resolve_support: Some(CodeActionCapabilityResolveSupport {
            properties: vec!["edit".to_owned()],
        }),
        ..CodeActionClientCapabilities::default()
    }
}

fn semantic_tokens() -> SemanticTokensClientCapabilities {
    SemanticTokensClientCapabilities {
        dynamic_registration: None,
        requests: SemanticTokensClientCapabilitiesRequests {
            range: Some(true),
            full: Some(SemanticTokensFullOptions::Delta { delta: Some(true) }),
        },
        token_types: vec![
            SemanticTokenType::NAMESPACE,
            SemanticTokenType::TYPE,
            SemanticTokenType::CLASS,
            SemanticTokenType::ENUM,
            SemanticTokenType::INTERFACE,
            SemanticTokenType::STRUCT,
            SemanticTokenType::TYPE_PARAMETER,
            SemanticTokenType::PARAMETER,
            SemanticTokenType::VARIABLE,
            SemanticTokenType::PROPERTY,
            SemanticTokenType::ENUM_MEMBER,
            SemanticTokenType::EVENT,
            SemanticTokenType::FUNCTION,
            SemanticTokenType::METHOD,
            SemanticTokenType::MACRO,
            SemanticTokenType::KEYWORD,
            SemanticTokenType::MODIFIER,
            SemanticTokenType::COMMENT,
            SemanticTokenType::STRING,
            SemanticTokenType::NUMBER,
            SemanticTokenType::REGEXP,
            SemanticTokenType::OPERATOR,
            SemanticTokenType::DECORATOR,
        ],
        token_modifiers: vec![
            SemanticTokenModifier::DECLARATION,
            SemanticTokenModifier::DEFINITION,
            SemanticTokenModifier::READONLY,
            SemanticTokenModifier::STATIC,
            SemanticTokenModifier::DEPRECATED,
            SemanticTokenModifier::ABSTRACT,
            SemanticTokenModifier::ASYNC,
            SemanticTokenModifier::MODIFICATION,
            SemanticTokenModifier::DOCUMENTATION,
            SemanticTokenModifier::DEFAULT_LIBRARY,
        ],
        formats: vec![TokenFormat::RELATIVE],
        overlapping_token_support: Some(false),
        multiline_token_support: Some(false),
        server_cancel_support: Some(true),
        augments_syntax_tokens: Some(true),
    }
}

fn goto() -> GotoCapability {
    // Links carry the target's whole range and its name's, so a peek can show the item.
    GotoCapability {
        dynamic_registration: None,
        link_support: Some(true),
    }
}

fn markup() -> Vec<MarkupKind> {
    vec![MarkupKind::Markdown, MarkupKind::PlainText]
}

fn dynamic(dynamic_registration: bool) -> DynamicRegistrationClientCapabilities {
    DynamicRegistrationClientCapabilities {
        dynamic_registration: Some(dynamic_registration),
    }
}
