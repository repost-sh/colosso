//! Files an agent gives a page (`browser_upload`): only from its own worktree's checkout, the
//! files it works on, never anything else of the person's. Its worktree is the one of the chat
//! or the terminal it names, as Koyo has it ([`Claimed`]), never the folder its hello names: a
//! caller that names neither gives no files, and is told why. That keeps an agent's mistakes,
//! and a page steering it, inside its worktree; it doesn't hold against an agent bent on
//! getting out, which Koyo's tools let run commands, and anything may name any chat or
//! terminal. Each path is checked off the main thread and the page given a copy (`given`): set
//! on a file input over the protocol (`DOM.setFileInputFiles`), or given to the file chooser the
//! page waits on for it, that same one (`browser/agent/choosing.rs`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::App;
use koyo_agent::protocol::ids::ThreadId;
use koyo_app_control::ControlError;
use koyo_app_control::ops::browser::BrowserUploadInput;
use koyo_browser_ipc::FileMode;
use koyo_term_control::Handle;

use super::super::select::holding;
use super::super::{Caller, Exec, main_shell, output, window_closed};
use super::acts::on_page;
use super::frames::{Named, set_files};
use super::page::now;
use super::refusals;
use crate::blocking::{self, Blocking};

mod given;

pub(crate) fn upload(
    caller: &Caller,
    input: BrowserUploadInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    if input.paths.is_empty() {
        return Err(ControlError::invalid_input("Give the files in `paths`."));
    }
    let claimed = Claimed::of(caller, cx);
    let to_input = input.element.is_some() || input.selector.is_some();
    on_page(
        caller,
        input.pane,
        true,
        cx,
        async move |page, actor, cx| {
            let claimed = claimed?;
            let actor = actor.as_ref();
            let asked = if to_input {
                None
            } else {
                let asked =
                    cx.update(|cx| Ok::<_, ControlError>(page.view()?.read(cx).chooser_asked()))?;
                Some(asked.ok_or_else(|| nothing_asked(page.pane()))?)
            };
            let mode = asked.map(|asked| asked.mode);
            fits(mode, input.paths.len())?;
            let folder = mode == Some(FileMode::Folder);
            let paths = input.paths;
            let copied = blocking::spawn(cx.background_executor(), move |may| {
                let root = claimed.root(may)?;
                let data = data_folder(may).ok_or_else(|| {
                    ControlError::internal("Koyo has no data folder to copy the files to.")
                })?;
                let into = given::folder_for_upload(&data, may)?;
                let files = given::given(&root, &paths, (folder, given::Budget::whole()), &into, may)?;
                Ok::<_, ControlError>((files, data))
            });
            let (files, data) = copied.await?;
            cx.update(|cx| forget_on_quit(data, cx));
            if to_input {
                let named = Named::new(input.element.as_deref(), input.selector.as_deref())?;
                set_files(&page, actor, &named, files, cx).await?;
            } else if let Some(asked) = asked {
                let actor = actor
                    .map(Caller::actor)
                    .ok_or_else(|| ControlError::internal("An upload acts."))?;
                cx.update(|cx| {
                    (page
                        .view()?
                        .update(cx, |view, cx| view.agent_chooses(&actor, asked, files, cx)))
                    .map_err(|why| refusals::unanswered(why, page.pane()))
                })?;
            }
            output(&cx.update(|cx| now(&page, cx))?)
        },
    )
}

/// Where the chat or terminal a caller names runs, as Koyo has it, and every checkout Koyo
/// has: what its worktree is found from, off the main thread.
struct Claimed {
    /// Its chat's folder, or its terminal's worktree.
    at: PathBuf,
    checkouts: Vec<PathBuf>,
}

impl Claimed {
    /// Where the chat or terminal `caller` names runs.
    fn of(caller: &Caller, cx: &App) -> Result<Claimed, ControlError> {
        let (_, shell) = main_shell(cx).ok_or_else(window_closed)?;
        let shell = shell.read(cx);
        let at = if let Some(id) = &caller.chat {
            let chats = shell.chats().read(cx);
            (chats.summary(&ThreadId(id.clone()))).map(|chat| PathBuf::from(&chat.cwd))
        } else if let Some(handle) = &caller.terminal {
            let named = shell.terminal_named(&Handle(handle.clone()));
            named.and_then(|(_, worktree)| worktree.map(Path::to_path_buf))
        } else {
            return Err(unnamed());
        };
        let checkouts = (shell.workspace.projects.iter())
            .flat_map(|project| project.branches.iter())
            .map(|unit| unit.worktree.path.clone())
            .collect();
        Ok(Claimed {
            at: at.ok_or_else(nowhere)?,
            checkouts,
        })
    }

    /// The checkout of the worktree.
    fn root(&self, may: Blocking<'_>) -> Result<PathBuf, ControlError> {
        let at = holding(&self.checkouts, &self.at, may).ok_or_else(nowhere)?;
        Ok(self.checkouts[at].clone())
    }
}

/// What a caller that names no Koyo chat or terminal is told.
fn unnamed() -> ControlError {
    ControlError::not_found(
        "You name no Koyo chat or terminal (KOYO_CHAT, KOYO_TERMINAL), so no worktree is known as yours, and what a client says of its folder doesn't make one: browser_upload, which gives a page only your worktree's files, is refused.",
    )
}

fn nowhere() -> ControlError {
    ControlError::not_found(
        "The chat or terminal you run in is in no checkout of a project Koyo has, so no worktree is known as yours: browser_upload, which gives a page only your worktree's files, is refused.",
    )
}

fn nothing_asked(pane: u64) -> ControlError {
    ControlError::not_found(format!(
        "Pane {pane}'s page isn't asking for files: name the `<input type=file>` with `ref` or `selector`, or have the page open its file chooser first, as a browser_click on the input does."
    ))
}

/// Whether `count` files answer a chooser asking `asked`: one for a single file or a folder,
/// any for several; a chooser asking where to save takes none of an agent's.
fn fits(asked: Option<FileMode>, count: usize) -> Result<(), ControlError> {
    match asked {
        Some(FileMode::Save) => Err(ControlError::invalid_input(
            "The page asks where to save a file, which browser_upload doesn't answer: browser_dialog with dismiss cancels it.",
        )),
        Some(FileMode::Open | FileMode::Folder) if count != 1 => Err(ControlError::invalid_input(
            "The page's file chooser takes one path.",
        )),
        _ => Ok(()),
    }
}

/// Has this run's copies of what agents gave pages, in Koyo's data folder `data`, go as Koyo
/// quits, once.
fn forget_on_quit(data: PathBuf, cx: &mut App) {
    static ASKED: AtomicBool = AtomicBool::new(false);
    if ASKED.swap(true, Ordering::Relaxed) {
        return;
    }
    blocking::on_quit(cx, move |may, _| given::forget(&data, may));
}

/// Koyo's data folder, where the copies go, as it's found on disk.
#[cfg(not(test))]
fn data_folder(_: Blocking<'_>) -> Option<PathBuf> {
    crate::settings::data_dir()
}

/// A scratch folder for the copies: tests write none of the person's.
#[cfg(test)]
#[expect(clippy::unnecessary_wraps, reason = "the same as the shipped build's, which may have none")]
fn data_folder(_: Blocking<'_>) -> Option<PathBuf> {
    Some(Path::new(env!("KOYO_TARGET_DIR")).join("test-data"))
}
