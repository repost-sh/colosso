//! Opening a pane of any kind on a checkout, as the New Tab menu would, for `tab_open`,
//! `pane_split` and `file_open`: a terminal, new or one the daemon runs already; a browser
//! page, in responsive mode at the size its call or the person's default says
//! (`page_mode`); a file in an editor, at a line and column; the checkout's Changes; a chat.
//! Shown, or out of sight with the window kept where it was (`kept`).

use std::path::{Path, PathBuf};

use gpui_kit::{App, AsyncApp, Context, Task, Window};
use koyo_app_control::ops::shared::{PaneKind, PaneSpec};
use koyo_app_control::{ControlError, ErrorCode};
use koyo_term_control::Handle;
use koyo_term_control::terminal::{is_terminal_name, login_shell_line};
use serde_json::Value;

use super::super::{Caller, policy};
use super::kept::Kept;
use super::{Exec, browser};
use crate::browser::BrowserView;
use crate::browser::Responsive;
use crate::browser::agent::{self, SetBy};
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

/// Runs `opening` on the checkout at `(project, branch)`, which an opening goes through as
/// the person's menus do: selected, and followed by the right panel when the opening reads
/// it there (`panel`: a file, the Changes) or shows. Not `shown`, the window is put back as it
/// was before anything is drawn ([`Kept`]), and a checkout the person never looked at gets no
/// panels unless the opening needs them.
pub(crate) fn on_checkout<T>(
    shell: &mut Shell,
    (project, branch): (usize, usize),
    (shown, panel): (bool, bool),
    window: &mut Window,
    cx: &mut Context<Shell>,
    opening: impl FnOnce(&mut Shell, &Path, &mut Window, &mut Context<Shell>) -> T,
) -> T {
    let kept = (!shown).then(|| Kept::of(shell, (project, branch), window, cx));
    let root = if shown || panel {
        show_worktree(shell, (project, branch), window, cx)
    } else {
        shell.workspace.select_branch(project, branch);
        shell.workspace.projects[project].branches[branch]
            .worktree
            .path
            .clone()
    };
    let opened = opening(shell, &root, window, cx);
    if let Some(kept) = kept {
        kept.restore(shell, window, cx);
    }
    opened
}

/// Opens the pane `spec` describes for `caller` on the checkout at `at`, as [`open`] does:
/// shown, the window switching to it, or out of sight, the window kept where it was; a page
/// in the mode its `viewport` or the person's default for agents' pages says. Fields its kind
/// doesn't take, or a `viewport` no page takes, are refused before anything opens.
pub(crate) fn open_on(
    shell: &mut Shell,
    caller: &Caller,
    at: (usize, usize),
    spec: &PaneSpec,
    (beside, shown): (bool, bool),
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> Result<Opened, ControlError> {
    check(spec)?;
    let mode = page_mode(caller, spec, window, cx)?;
    // A file opens through the checkout's right panel, and so do the Changes.
    let panel = spec.kind == PaneKind::Changes || spec.file.is_some();
    Ok(on_checkout(
        shell,
        at,
        (shown, panel),
        window,
        cx,
        |shell, root, window, cx| open(shell, root, spec, mode, (beside, shown), window, cx),
    ))
}

/// The responsive mode the page `spec` opens for `caller` starts in, in `window`: as its
/// `viewport` asks, else as the person set pages agents open to; `None` when it fills its
/// pane, or isn't a page.
fn page_mode(
    caller: &Caller,
    spec: &PaneSpec,
    window: &Window,
    cx: &App,
) -> Result<Option<Responsive>, ControlError> {
    if spec.kind != PaneKind::Browser {
        return Ok(None);
    }
    let asked = (spec.viewport.as_ref())
        .map(browser::opening_change)
        .transpose()?;
    let default = policy::pages_open_at(cx).size();
    let by = SetBy::Agent(caller.label());
    agent::opening(asked.as_ref(), default, window.scale_factor(), &by)
        .map_err(ControlError::invalid_input)
}

/// Opens browser pane `pane`'s page, which an agent placed, in the browser engine: the
/// browser tools reach a page only once it's open. A tab `window` shows and draws opens it as
/// it's drawn, at its pane's size; any other page opens at once, as a window minimized or on
/// another desktop draws nothing. A page `opener` made is then the one its next call without
/// a `pane` means.
pub(crate) fn open_agents_page(
    shell: &mut Shell,
    opener: Option<&Caller>,
    pane: u64,
    window: &Window,
    cx: &mut Context<Shell>,
) {
    let workspace = &shell.workspace;
    let Some((at, _)) = workspace.find_pane(|found| found.id == pane) else {
        return;
    };
    let view = (workspace.projects[at.0].branches[at.1].pane(pane))
        .and_then(|found| found.view().cloned());
    let Some(view) = view else {
        return;
    };
    let drawn = shell.route(cx) == Route::Workspace
        && shell.workspace.shown_checkout_tab() == Some(at)
        && window.is_visible();
    if !drawn {
        view.update(cx, BrowserView::open_out_of_sight);
    }
    if let Some(opener) = opener {
        browser::acted(opener, pane, cx);
    }
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
fn check(spec: &PaneSpec) -> Result<(), ControlError> {
    let terminal = spec.terminal.is_some() || spec.name.is_some() || spec.command.is_some();
    let file = spec.file.is_some() || spec.line.is_some() || spec.column.is_some();
    let page = spec.url.is_some() || spec.viewport.is_some();
    let stray = match spec.kind {
        PaneKind::Terminal => (spec.terminal.is_some()
            && (spec.name.is_some() || spec.command.is_some()))
        .then_some("`terminal` (an existing terminal) with `name` or `command` (a new one)")
        .or_else(|| (page || file).then_some("`url`, `viewport`, `file`, `line` or `column`")),
        PaneKind::Browser => (terminal || file).then_some("terminal or file fields"),
        PaneKind::Editor => (terminal || page).then_some("terminal fields, `url` or `viewport`"),
        PaneKind::Changes | PaneKind::Chat | PaneKind::Desktop | PaneKind::Device => {
            (terminal || file || page).then_some("terminal, file or browser fields")
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
/// after the shown tab when `beside`, else after the last; a page starting in `mode`. Unless
/// `shown`, a terminal's tab
/// comes without being shown and a file's editor without the keyboard, the window being
/// put back by whoever opens it out of sight.
fn open(
    shell: &mut Shell,
    root: &Path,
    spec: &PaneSpec,
    mode: Option<Responsive>,
    (beside, shown): (bool, bool),
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
            let command = (spec.command.clone())
                .map(login_shell_line)
                .unwrap_or_default();
            let opening = shell.open_terminal(
                root.to_path_buf(),
                spec.name.clone().into(),
                command,
                None,
                shown,
                cx,
            );
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
                .open_browser_tab(spec.url.clone(), beside, mode, window, cx)
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
                (true, shown),
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
        // There is one agent desktop: the pane that shows it, or a new one.
        PaneKind::Desktop => {
            let pane = shell.desktop_pane(shown, cx).ok_or_else(|| {
                ControlError::not_found("No checkout is selected to open the agent desktop on.")
            });
            shell.navigate(Route::Workspace, cx);
            cx.notify();
            Opened::Now(pane)
        }
        PaneKind::Chat => Opened::Now(Ok(push(
            shell,
            Pane::new(model::PaneKind::Chat),
            beside,
            cx,
        ))),
        // A device's tab opens as the picker; device_open opens one device.
        PaneKind::Device => {
            let pane = shell.device_pane(None, shown, cx).ok_or_else(|| {
                ControlError::not_found("No checkout is selected to open a device's tab on.")
            });
            shell.navigate(Route::Workspace, cx);
            cx.notify();
            Opened::Now(pane)
        }
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
/// `kept` is false and previews are on, its editor taking the keyboard when `focus`; its
/// pane.
pub(crate) fn open_file(
    shell: &mut Shell,
    root: &Path,
    file: &str,
    (line, column): (Option<u32>, Option<u32>),
    (kept, focus): (bool, bool),
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
        focus,
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
        .find(|pane| pane.shows_changes())
        .map(|pane| pane.id)
        .ok_or_else(|| ControlError::internal("The Changes tab didn't open."))
}
