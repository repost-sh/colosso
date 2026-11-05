//! Opening a pane of any kind on a checkout, as the New Tab menu would, for `tab_open`,
//! `pane_split` and `file_open`: a terminal, new or one the daemon runs already; a browser
//! page; a file in an editor, at a line and column; the checkout's Changes; a chat.

use std::path::{Path, PathBuf};

use koyo_app_control::ops::shared::{PaneKind, PaneSpec};
use koyo_app_control::{ControlError, ErrorCode};
use koyo_term_control::terminal::{is_terminal_name, login_shell_line};
use koyo_term_control::Handle;
use gpui_kit::{App, AsyncApp, Context, Task, Window};
use serde_json::Value;

use super::Exec;
use crate::files::{At, Place};
use crate::model::{self, Pane, Tab};
use crate::navigation::Route;
use crate::shell::Shell;

/// A pane opened: now, or once the terminal daemon has answered.
pub(crate) enum Opened {
    Now(Result<u64, ControlError>),
    Later(Task<Result<u64, ControlError>>),
}

/// Selects the checkout at `(project, branch)` in the window and shows it: its folder.
pub(crate) fn show_worktree(
    shell: &mut Shell,
    (project, branch): (usize, usize),
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> PathBuf {
    shell.workspace.select_branch(project, branch);
    shell.follow_selection(window, cx);
    shell.workspace.projects[project].branches[branch]
        .worktree
        .path
        .clone()
}

impl Opened {
    /// The operation's answer: `then` given the pane, once it's open.
    pub(crate) fn then(
        self,
        cx: &mut App,
        then: impl FnOnce(u64, &mut App) -> Result<Value, ControlError> + 'static,
    ) -> Exec {
        match self {
            Opened::Now(Ok(pane)) => Exec::Ready(then(pane, cx)),
            Opened::Now(Err(error)) => Exec::Ready(Err(error)),
            Opened::Later(opening) => Exec::Later(cx.spawn(async move |cx: &mut AsyncApp| {
                let pane = opening.await?;
                cx.update(|cx| then(pane, cx))
            })),
        }
    }
}

/// Refuses fields of `spec` its kind doesn't take.
pub(crate) fn check(spec: &PaneSpec) -> Result<(), ControlError> {
    let terminal = spec.terminal.is_some() || spec.name.is_some() || spec.command.is_some();
    let file = spec.file.is_some() || spec.line.is_some() || spec.column.is_some();
    let stray = match spec.kind {
        PaneKind::Terminal => (spec.terminal.is_some()
            && (spec.name.is_some() || spec.command.is_some()))
        .then_some("`terminal` (an existing terminal) with `name` or `command` (a new one)")
        .or_else(|| (spec.url.is_some() || file).then_some("`url`, `file`, `line` or `column`")),
        PaneKind::Browser => (terminal || file).then_some("terminal or file fields"),
        PaneKind::Editor => (terminal || spec.url.is_some()).then_some("terminal fields or `url`"),
        PaneKind::Changes | PaneKind::Chat => {
            (terminal || file || spec.url.is_some()).then_some("terminal, file or browser fields")
        }
    };
    let positional = spec.file.is_none() && (spec.line.is_some() || spec.column.is_some());
    match stray {
        Some(fields) => Err(ControlError::invalid_input(format!(
            "A {} pane takes no {fields}.",
            spec.kind.id()
        ))),
        None if positional => Err(ControlError::invalid_input(
            "`line` and `column` need a `file`.",
        )),
        None => match &spec.name {
            // A name the daemon refuses would leave a tab whose terminal never opens.
            Some(name) if !is_terminal_name(name) => Err(ControlError::invalid_input(format!(
                "“{name}” isn't a terminal name: lowercase letters, digits and dashes, up to 32, \
                 starting with a letter or digit."
            ))),
            _ => Ok(()),
        },
    }
}

/// Opens the pane `spec` describes on the selected checkout, at `root`, in a tab of its own:
/// after the shown tab when `beside`, else after the last.
pub(crate) fn open(
    shell: &mut Shell,
    root: &Path,
    spec: &PaneSpec,
    beside: bool,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> Opened {
    match spec.kind {
        PaneKind::Terminal => {
            if let Some(handle) = &spec.terminal {
                return Opened::Now(
                    shell
                        .reveal_terminal(&Handle(handle.clone()), cx)
                        .ok_or_else(|| {
                            ControlError::new(
                                ErrorCode::TerminalNotFound,
                                format!(
                                    "Koyo runs no terminal {handle}; terminal_list lists them."
                                ),
                            )
                        }),
                );
            }
            let command = (spec.command.clone()).map(login_shell_line).unwrap_or_default();
            let opening = shell.open_terminal(root.to_path_buf(), spec.name.clone().into(), command, None, cx);
            Opened::Later(cx.spawn_in(window, async move |this, cx| {
                let pane = opening.await.ok_or_else(|| {
                    ControlError::new(
                        ErrorCode::Unsupported,
                        "Koyo couldn't reach its terminal daemon to open a terminal.",
                    )
                })?;
                // The terminal starts with its view, which the window makes once it draws the
                // tab; a caller may go on to use the terminal before that, or with the window
                // hidden, so its view is made now.
                this.update_in(cx, |shell, window, cx| {
                    let terminal = (shell.workspace.find_pane(|found| found.id == pane)).and_then(
                        |((project, branch, _), _)| {
                            let unit = &shell.workspace.projects[project].branches[branch];
                            unit.pane(pane)?.terminal
                        },
                    );
                    if let Some(terminal) = terminal {
                        shell.terminal_view(terminal, window, cx);
                    }
                })
                .ok();
                Ok(pane)
            }))
        }
        PaneKind::Browser => Opened::Now(
            shell
                .open_browser_tab(spec.url.clone(), beside, window, cx)
                .ok_or_else(|| {
                    ControlError::not_found("No checkout is selected to open a page on.")
                }),
        ),
        PaneKind::Editor => match &spec.file {
            Some(file) => Opened::Now(open_file(
                shell,
                root,
                file,
                (spec.line, spec.column),
                true,
                window,
                cx,
            )),
            None => Opened::Now(Ok(push(
                shell,
                Pane::new(model::PaneKind::Editor),
                beside,
                cx,
            ))),
        },
        PaneKind::Changes => Opened::Now(open_changes(shell, window, cx)),
        PaneKind::Chat => Opened::Now(Ok(push(
            shell,
            Pane::new(model::PaneKind::Chat),
            beside,
            cx,
        ))),
    }
}

/// Adds `pane` in a tab of its own on the selected checkout and shows it.
fn push(shell: &mut Shell, pane: Pane, beside: bool, cx: &mut Context<Shell>) -> u64 {
    let id = pane.id;
    if beside {
        shell.workspace.open_beside_shown(Tab::Single(pane));
    } else {
        shell.workspace.push_tab(Tab::Single(pane));
    }
    shell.navigate(Route::Workspace, cx);
    cx.notify();
    id
}

/// Shows the file at `file`, relative to the selected checkout at `root` unless it's
/// absolute, as the tree does, at a line and a column counted from 1, kept open unless
/// `kept` is false and previews are on; its pane.
pub(crate) fn open_file(
    shell: &mut Shell,
    root: &Path,
    file: &str,
    (line, column): (Option<u32>, Option<u32>),
    kept: bool,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> Result<u64, ControlError> {
    let path = root.join(file);
    if !path.is_file() {
        return Err(ControlError::not_found(format!(
            "No file is at {}.",
            path.display()
        )));
    }
    let at = line.map(|line| {
        at(
            &path,
            line.saturating_sub(1),
            column.map(|column| column.saturating_sub(1)),
        )
    });
    shell.open_file(
        Place {
            path: path.clone(),
            at,
        },
        kept,
        true,
        window,
        cx,
    );
    shown_file(shell, &path, cx)
        .ok_or_else(|| ControlError::internal(format!("{} didn't open.", path.display())))
}

/// Where line `line` and column `column` of the file at `path` are, both counted from 0, the
/// column in characters: bytes of its text when it can be read, else the line's start.
fn at(path: &Path, line: u32, column: Option<u32>) -> At {
    let Some(column) = column else {
        return At::Line(line);
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return At::Line(line);
    };
    let start = (text.split_inclusive('\n'))
        .take(usize::try_from(line).unwrap_or(usize::MAX))
        .map(str::len)
        .sum::<usize>();
    let rest = text.get(start..).unwrap_or_default();
    let in_line = rest.split('\n').next().unwrap_or_default();
    let offset = (in_line.char_indices())
        .nth(usize::try_from(column).unwrap_or(usize::MAX))
        .map_or(in_line.len(), |(offset, _)| offset);
    At::Bytes(start + offset..start + offset)
}

/// The pane on the selected checkout showing the file at `path`.
fn shown_file(shell: &Shell, path: &Path, cx: &App) -> Option<u64> {
    let unit = shell.workspace.branch()?;
    let mut panes = unit.tabs.iter().flat_map(Tab::panes);
    panes
        .find(|pane| {
            pane.file
                .as_ref()
                .is_some_and(|file| file.read(cx).path == path)
        })
        .map(|pane| pane.id)
}

/// Shows the selected checkout's Changes tab, opening it if it isn't open; its pane.
pub(crate) fn open_changes(
    shell: &mut Shell,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> Result<u64, ControlError> {
    let panel = shell.changes_panel(window, cx).ok_or_else(|| {
        ControlError::not_found("No checkout is selected to show the changes of.")
    })?;
    shell.show_changes(&panel, cx);
    let unit = shell
        .workspace
        .branch()
        .ok_or_else(|| ControlError::not_found("No checkout is selected."))?;
    (unit.tabs.iter().flat_map(Tab::panes))
        .find(|pane| pane.changes.is_some())
        .map(|pane| pane.id)
        .ok_or_else(|| ControlError::internal("The Changes tab didn't open."))
}
