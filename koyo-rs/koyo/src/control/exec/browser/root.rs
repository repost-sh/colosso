//! The one folder of the person's an agent's browser tools let it hand a page or be shown
//! one from: the worktree of the Koyo chat or terminal it names, as Koyo has that chat or
//! terminal, never the folder its hello names. It's read on the main thread as written
//! ([`Claimed::of`]) and resolved, links followed, off it ([`Claimed::root`]). A caller that
//! names no chat or terminal has no worktree here, and is refused uploads and `file:` pages and
//! told why.
//!
//! An agent opens a `file:` page only from its worktree (`opening`), and every protocol command
//! a browser tool runs, reading or acting, is refused on a page whose address is a `file:` one
//! outside it (`page::call_within`), checked before the command and again once it's answered
//! if the page moved meanwhile, so a `file:` page going to another doesn't get round it. What
//! this doesn't see: what a `file:` page of the worktree shows of other files in itself (an
//! image, a frame), which Chromium lets one `file:` page do.
//!
//! This keeps a well-meaning agent, or one a page steers, to its own worktree. It is no
//! boundary against an agent set on getting out: Koyo's tools let an agent run commands (a
//! terminal with a command, a chat) in any worktree and see the agent desktop, and anything
//! may name any chat or terminal.

use std::fs;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use gpui_kit::{App, AsyncApp};
use koyo_agent::protocol::ids::ThreadId;
use koyo_app_control::ControlError;
use koyo_term_control::Handle;

use super::super::select::holding;
use super::super::{Caller, Exec, main_shell, window_closed};
use crate::blocking::{self, Blocking};

/// Where the chat or terminal a caller names runs, as Koyo has it, and every checkout Koyo
/// has: what its worktree is found from, off the main thread.
pub(crate) struct Claimed {
    /// Its chat's folder, or its terminal's worktree.
    at: PathBuf,
    checkouts: Vec<PathBuf>,
}

impl Claimed {
    /// Where the chat or terminal `caller` names runs.
    pub(crate) fn of(caller: &Caller, cx: &App) -> Result<Claimed, ControlError> {
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

    /// The worktree, links followed.
    pub(crate) fn root(&self, may: Blocking<'_>) -> Result<PathBuf, ControlError> {
        let at = holding(&self.checkouts, &self.at, may).ok_or_else(nowhere)?;
        let checkout = &self.checkouts[at];
        fs::canonicalize(checkout).map_err(|why| {
            ControlError::not_found(format!(
                "Your worktree {} can't be read: {why}",
                checkout.display()
            ))
        })
    }
}

/// What a caller that names no Koyo chat or terminal is told.
fn unnamed() -> ControlError {
    ControlError::not_found(
        "You name no Koyo chat or terminal (KOYO_CHAT, KOYO_TERMINAL), so no worktree is known as yours, and what a client says of its folder doesn't make one: browser_upload and file: pages, which are only ever your worktree's, are refused.",
    )
}

fn nowhere() -> ControlError {
    ControlError::not_found(
        "The chat or terminal you run in is in no checkout of a project Koyo has, so no worktree is known as yours: browser_upload and file: pages, which are only ever your worktree's, are refused.",
    )
}

/// Whether a page at `url` shows a file: a `file:` address, or its source (`view-source:`).
pub(crate) fn shows_a_file(url: &str) -> bool {
    file_address(url).is_some()
}

/// The `file:` address a page at `url` shows, read as Chromium reads it: tabs and line breaks
/// dropped and the ends trimmed (the URL standard's parse), and `view-source:` taken off.
fn file_address(url: &str) -> Option<url::Url> {
    let mut url = url::Url::parse(url).ok()?;
    while url.scheme() == "view-source" {
        url = url::Url::parse(url.path()).ok()?;
    }
    (url.scheme() == "file").then_some(url)
}

/// Refuses a page at `url`, a `file:` one, unless the file it shows is in the worktree at
/// `root`, links followed, and has no other name (a hard link) that could be elsewhere. A file
/// that isn't there can't be told to be in it, so it's refused too.
fn inside(url: &str, root: &Path, _: Blocking<'_>) -> Result<(), ControlError> {
    let path = file_address(url).and_then(|address| address.to_file_path().ok());
    let real = path.and_then(|path| fs::canonicalize(path).ok());
    let lone = |real: &Path| fs::metadata(real).is_ok_and(|meta| meta.is_dir() || meta.nlink() == 1);
    match real {
        Some(real) if real.starts_with(root) && lone(&real) => Ok(()),
        _ => Err(ControlError::invalid_input(format!(
            "{url} is outside your worktree {}, or isn't there, or has another name elsewhere (a hard link): the browser tools open and read only file: pages of it.",
            root.display()
        ))),
    }
}

/// Refuses `caller` a page at `url` that shows a file outside its worktree, or any `file:`
/// page while it has none; any other page it may be shown.
pub(crate) async fn shown_to(caller: &Caller, url: String, cx: &mut AsyncApp) -> Result<(), ControlError> {
    if !shows_a_file(&url) {
        return Ok(());
    }
    let claimed = cx.update(|cx| Claimed::of(caller, cx))?;
    blocking::spawn(cx.background_executor(), move |may| {
        let root = claimed.root(may)?;
        inside(&url, &root, may)
    })
    .await
}

/// Carries out `open`, which opens `url` for `caller`, once `shown_to` let it: at once for no
/// page, or one that shows no file.
pub(crate) fn opening(
    caller: &Caller,
    url: Option<String>,
    cx: &mut App,
    open: impl FnOnce(&Caller, &mut App) -> Exec + 'static,
) -> Exec {
    let Some(url) = url.filter(|url| shows_a_file(url)) else {
        return open(caller, cx);
    };
    let caller = caller.clone();
    Exec::Later(cx.spawn(async move |cx: &mut AsyncApp| {
        shown_to(&caller, url, cx).await?;
        match cx.update(|cx| open(&caller, cx)) {
            Exec::Ready(answer) => answer,
            Exec::Later(opened) => opened.await,
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::{inside, shows_a_file};
    use crate::blocking;
    use crate::test_support::scratch;

    #[test]
    fn a_file_page_is_one_of_the_worktrees_only_where_its_file_is_links_followed() {
        let root = std::fs::canonicalize(scratch("root-files")).unwrap();
        let outside = scratch("root-files-outside");
        std::fs::write(root.join("index.html"), "<p>").unwrap();
        std::fs::write(outside.join("id_ed25519"), "key").unwrap();
        std::os::unix::fs::symlink(outside.join("id_ed25519"), root.join("key")).unwrap();
        let checked = |path: &std::path::Path| {
            let url = url::Url::from_file_path(path).unwrap().to_string();
            blocking::in_test(|may| inside(&url, &root, may))
        };
        assert!(checked(&root.join("index.html")).is_ok());
        assert!(checked(&root).is_ok(), "the worktree's own listing");
        assert!(checked(&root.join("not-yet.html")).is_err(), "no file, so no telling where it is");
        std::fs::hard_link(outside.join("id_ed25519"), root.join("another-name")).unwrap();
        assert!(checked(&root.join("another-name")).is_err(), "another name of a file elsewhere");
        assert!(checked(&outside.join("id_ed25519")).is_err());
        assert!(checked(&root.join("key")).is_err(), "a link out of it");
        assert!(checked(&root.join("../root-files-outside/id_ed25519")).is_err());
        let source = format!("view-source:{}", url::Url::from_file_path(outside.join("id_ed25519")).unwrap());
        assert!(blocking::in_test(|may| inside(&source, &root, may)).is_err(), "its source too");
    }

    #[test]
    fn only_a_file_address_or_its_source_shows_a_file() {
        for url in [
            "file:///etc/hosts",
            "FILE:///etc/hosts",
            " file:/etc/hosts",
            "view-source:file:///etc/hosts",
            "VIEW-SOURCE:file:///etc/hosts",
            "view-sou\trce:file:///etc/passwd",
            "view-source\n:file:///etc/passwd",
            "\tview-source:fi\rle:///etc/passwd",
            "view-source:view-source:file:///etc/passwd",
        ] {
            assert!(shows_a_file(url), "{url}");
        }
        for url in ["https://example.com/", "data:text/html,<p>", "about:blank", "/etc/hosts", ""] {
            assert!(!shows_a_file(url), "{url}");
        }
    }
}
