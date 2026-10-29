//! The types and parameter names the server infers, shown greyed in the text where they'd
//! go (`textDocument/inlayHint`): asked for the rows in view by the editor, each hint put
//! at its byte, its kind deciding which side of it the caret and typing fall.

use std::ops::Range;

use colosso_lsp::lsp_types::request::InlayHintRequest;
use colosso_lsp::lsp_types::{
    InlayHint, InlayHintKind, InlayHintLabel, InlayHintParams, OneOf, WorkDoneProgressParams,
};
use gpui_kit::component::input::{Bias, Inlay, InlayHintProvider, Rope};
use gpui_kit::{App, AppContext as _, SharedString, Task, Window};

use super::providers::Hooks;

impl InlayHintProvider for Hooks {
    fn inlay_hints(
        &self,
        text: &Rope,
        range: Range<usize>,
        _: &mut Window,
        cx: &mut App,
    ) -> Task<gpui_kit::Result<Vec<Inlay>>> {
        let offers = (self.capabilities(cx)).is_some_and(|capabilities| {
            !matches!(
                capabilities.inlay_hint_provider,
                None | Some(OneOf::Left(false))
            )
        });
        let Some(asked) = self.ask(text, cx).filter(|_| offers) else {
            return Task::ready(Ok(Vec::new()));
        };
        cx.background_spawn(async move {
            let params = InlayHintParams {
                work_done_progress_params: WorkDoneProgressParams::default(),
                text_document: asked.document.clone(),
                range: asked.for_server(range),
            };
            let hints = (asked.ask::<InlayHintRequest>(&params).await)
                .flatten()
                .unwrap_or_default();
            Ok((hints.iter())
                .map(|hint| Inlay {
                    offset: asked.offset(hint.position),
                    label: label(hint),
                    bias: bias(hint.kind),
                })
                .collect())
        })
    }
}

/// The hint's label in one piece, with the padding it asks for as spaces.
fn label(hint: &InlayHint) -> SharedString {
    let mut label = String::new();
    if hint.padding_left == Some(true) {
        label.push(' ');
    }
    match &hint.label {
        InlayHintLabel::String(text) => label.push_str(text),
        InlayHintLabel::LabelParts(parts) => parts.iter().for_each(|part| label.push_str(&part.value)),
    }
    if hint.padding_right == Some(true) {
        label.push(' ');
    }
    label.into()
}

/// Which text the hint belongs to: a parameter's name to the value after it, so it stays
/// put as the value is typed; a type, or anything else, to the name before it, so it moves
/// along as the name is typed.
fn bias(kind: Option<InlayHintKind>) -> Bias {
    if kind == Some(InlayHintKind::PARAMETER) {
        Bias::Left
    } else {
        Bias::Right
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use colosso_lsp::lsp_types::{InlayHintLabelPart, Position};

    fn hint(label: InlayHintLabel, kind: Option<InlayHintKind>) -> InlayHint {
        InlayHint {
            position: Position::new(0, 0),
            label,
            kind,
            text_edits: None,
            tooltip: None,
            padding_left: None,
            padding_right: None,
            data: None,
        }
    }

    #[test]
    fn a_label_is_one_piece_padded_as_asked_and_a_parameter_s_stays_with_its_value() {
        let mut typed = hint(": u32".to_owned().into(), Some(InlayHintKind::TYPE));
        typed.padding_left = Some(true);
        assert_eq!(label(&typed).as_ref(), " : u32");
        let parts = vec![
            InlayHintLabelPart {
                value: "name".to_owned(),
                ..InlayHintLabelPart::default()
            },
            InlayHintLabelPart {
                value: ":".to_owned(),
                ..InlayHintLabelPart::default()
            },
        ];
        let mut parameter = hint(parts.into(), Some(InlayHintKind::PARAMETER));
        parameter.padding_right = Some(true);
        assert_eq!(label(&parameter).as_ref(), "name: ");
        assert_eq!(bias(parameter.kind), Bias::Left);
        assert_eq!(bias(typed.kind), Bias::Right);
        assert_eq!(bias(None), Bias::Right, "rust-analyzer's chaining hints end a line");
    }
}
