//! What could be done about the code under the cursor: the server's actions for the
//! selection and the diagnostics in it, listed by title with the preferred first, and the
//! one chosen filled in by the server and applied, to this file in the editor and to any
//! other on disk.

use std::ops::Range;

use colosso_lsp::lsp_types::request::{CodeActionRequest, CodeActionResolveRequest};
use colosso_lsp::lsp_types::{
    CodeAction, CodeActionContext, CodeActionKind, CodeActionOrCommand, CodeActionParams,
    CodeActionProviderCapability, CodeActionTriggerKind, Command, PartialResultParams,
    WorkDoneProgressParams,
};
use gpui_kit::component::input::{CodeActionProvider, EditorState};
use gpui_kit::{App, AppContext as _, Entity, SharedString, Task, Window};

use super::edits;
use super::providers::Hooks;
use crate::checkout::Document;

impl CodeActionProvider for Hooks {
    fn id(&self) -> SharedString {
        self.kind.name().into()
    }

    fn code_actions(
        &self,
        state: Entity<EditorState>,
        range: Range<usize>,
        _: &mut Window,
        cx: &mut App,
    ) -> Task<gpui_kit::Result<Vec<CodeAction>>> {
        let rope = state.read(cx).text().clone();
        let Some(asked) = self.ask(&rope, cx) else {
            return Task::ready(Ok(Vec::new()));
        };
        let Some(server) = self.server.upgrade() else {
            return Task::ready(Ok(Vec::new()));
        };
        let diagnostics = server.read(cx).published.get(&self.path).1.within(&range);
        cx.background_spawn(async move {
            let params = CodeActionParams {
                text_document: asked.document.clone(),
                range: asked.for_server(range),
                context: CodeActionContext {
                    diagnostics,
                    only: None,
                    trigger_kind: Some(CodeActionTriggerKind::INVOKED),
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            };
            let answer = asked.ask::<CodeActionRequest>(&params).await.flatten();
            Ok(listed(answer.unwrap_or_default()))
        })
    }

    /// Applies `action`, filled in by the server first when it came without its edit:
    /// this file's edits in the editor, the others' on disk.
    fn perform_code_action(
        &self,
        state: Entity<EditorState>,
        action: CodeAction,
        _: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Task<gpui_kit::Result<()>> {
        let rope = state.read(cx).text().clone();
        let Some(asked) = self.ask(&rope, cx) else {
            return Task::ready(Ok(()));
        };
        let resolves = (self.capabilities(cx)).is_some_and(|capabilities| {
            matches!(
                &capabilities.code_action_provider,
                Some(CodeActionProviderCapability::Options(options))
                    if options.resolve_provider == Some(true)
            )
        });
        let (path, checkout) = (self.path.clone(), self.checkout.clone());
        let editor = state.downgrade();
        window.spawn(cx, async move |cx| {
            let title = action.title.clone();
            let (asked, action) = cx
                .background_spawn(async move {
                    let action = if action.edit.is_none() && resolves {
                        (asked.ask::<CodeActionResolveRequest>(&action).await).unwrap_or(action)
                    } else {
                        action
                    };
                    (asked, action)
                })
                .await;
            let Some(edit) = action.edit else {
                log::warn!("{title}: nothing to apply; a command isn't run yet");
                return Ok(());
            };
            let elsewhere = cx.update(|window, cx| {
                edits::apply(edit, &asked, &path, &editor, &checkout, window, cx)
            })?;
            let files = match elsewhere {
                Ok(files) => files,
                Err(why) => {
                    log::warn!("{title}: {why}");
                    return Ok(());
                }
            };
            if files.is_empty() {
                return Ok(());
            }
            let encoding = asked.encoding;
            let written = cx
                .background_spawn(async move {
                    (files.into_iter())
                        .filter_map(|(file, edits)| match edits::on_disk(&file, &edits, encoding) {
                            Ok(()) => Some(file),
                            Err(error) => {
                                log::warn!("{title}: couldn't edit {}: {error}", file.display());
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            // A file shown reads again now, rather than once the watcher says it changed.
            cx.update(|_, cx| {
                let Some(checkout) = checkout.upgrade() else {
                    return;
                };
                for file in written {
                    log::info!("edited {}", file.display());
                    if let Some(document) = checkout.read(cx).open_document(&file) {
                        document.update(cx, Document::reread);
                    }
                }
            })?;
            Ok(())
        })
    }
}

/// The actions to list: those that can be done, the preferred first, then fixes before
/// refactorings before the rest. A command isn't listed, bare or as an action without an
/// edit, as it isn't run yet; an action without either may get its edit when filled in.
fn listed(answer: Vec<CodeActionOrCommand>) -> Vec<CodeAction> {
    let mut actions: Vec<CodeAction> = (answer.into_iter())
        .filter_map(|either| match either {
            CodeActionOrCommand::CodeAction(action)
                if action.edit.is_some() || action.command.is_none() =>
            {
                Some(action)
            }
            CodeActionOrCommand::CodeAction(CodeAction { title, .. })
            | CodeActionOrCommand::Command(Command { title, .. }) => {
                log::debug!("{title}: a command, which isn't run yet");
                None
            }
        })
        .filter(|action| action.disabled.is_none())
        .collect();
    actions.sort_by_key(|action| (!action.is_preferred.unwrap_or(false), order(action)));
    actions
}

/// Fixes first, then refactorings, then source-wide actions, then whatever else.
fn order(action: &CodeAction) -> u8 {
    let kind = action.kind.as_ref().map_or("", CodeActionKind::as_str);
    let first = [
        CodeActionKind::QUICKFIX,
        CodeActionKind::REFACTOR,
        CodeActionKind::SOURCE,
    ];
    (first.iter().position(|known| kind.starts_with(known.as_str()))).map_or(3, |at| at as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use colosso_lsp::lsp_types::CodeActionDisabled;

    fn action(title: &str, kind: Option<CodeActionKind>, preferred: bool) -> CodeActionOrCommand {
        CodeActionOrCommand::CodeAction(CodeAction {
            title: title.to_owned(),
            kind,
            is_preferred: Some(preferred),
            ..CodeAction::default()
        })
    }

    #[test]
    fn actions_are_listed_preferred_first_then_fixes_then_refactorings() {
        let mut disabled = CodeAction {
            title: "Can't".to_owned(),
            ..CodeAction::default()
        };
        disabled.disabled = Some(CodeActionDisabled {
            reason: "not here".to_owned(),
        });
        let run = Command::new("Run".to_owned(), "run".to_owned(), None);
        let runs = CodeAction {
            title: "Runs".to_owned(),
            command: Some(run.clone()),
            ..CodeAction::default()
        };
        let answer = vec![
            action("Extract", Some(CodeActionKind::REFACTOR_EXTRACT), false),
            action("Organize", Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS), false),
            action("Other", None, false),
            action("Import", Some(CodeActionKind::QUICKFIX), false),
            action("Fix now", Some(CodeActionKind::QUICKFIX), true),
            CodeActionOrCommand::CodeAction(disabled),
            CodeActionOrCommand::CodeAction(runs),
            CodeActionOrCommand::Command(run),
        ];
        let titles: Vec<String> = listed(answer).into_iter().map(|action| action.title).collect();
        assert_eq!(titles, ["Fix now", "Import", "Extract", "Organize", "Other"]);
    }
}
