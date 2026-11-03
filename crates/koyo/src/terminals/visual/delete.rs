//! Deleting a worktree with a terminal running in it (`docs/terminals.md` 5.5): its dialog
//! names the terminal, and confirming stops it in the daemon before git takes the folder.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use koyo_term_control::{IfExists, OpenRequest, RequestBody, ResponseBody, Selector};

use super::drive::{act, wait_until};
use crate::delete_worktree::DeleteWorktreeDialog;
use crate::modal;
use crate::projects::{DeleteWorktree, read_worktrees};
use crate::test_support::try_git;
use crate::visual::{Drawn, Result, checkout};

pub(super) fn delete(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    let shell = drawn.shell.clone();
    let repo = (drawn
        .cx
        .read(|cx| shell.read(cx).workspace.project().map(|p| p.path.clone())))
    .ok_or("no project is selected")?;
    let made = repo.with_file_name("feat-gone");
    let _ = std::fs::remove_dir_all(&made);
    try_git(&repo, &["worktree", "prune"])?;
    let _ = try_git(&repo, &["branch", "-q", "-D", "feat-gone"]);
    let target = made.to_str().ok_or("the worktree's path isn't text")?;
    try_git(&repo, &["worktree", "add", "-q", "-b", "feat-gone", target])?;
    let gone = made.canonicalize().map_err(|error| error.to_string())?;
    let client = drawn.cx.update(|cx| {
        shell.update(cx, |shell, cx| {
            shell.show_worktrees(&repo, read_worktrees(&repo), cx);
            shell.terminal_client()
        })
    });
    let open = OpenRequest {
        worktree: Some(Selector::Path(gone.clone())),
        name: "server".into(),
        command: ["/bin/sh", "-c", "echo serving; sleep 60"]
            .map(str::to_owned)
            .to_vec(),
        cwd: None,
        env: BTreeMap::new(),
        size: None,
        labels: BTreeSet::new(),
        if_exists: IfExists::Attach,
        respawn: false,
    };
    let client = client.ok_or("the app has no terminal client")?;
    match client.request(RequestBody::Open(open)) {
        Ok(ResponseBody::Opened { .. }) => {}
        other => return Err(format!("opening a terminal in the worktree: {other:?}")),
    }
    wait_until(drawn, "the worktree's terminal known to the app", |drawn| {
        Ok(has_terminals(drawn, &gone))
    })?;
    act(drawn, Box::new(DeleteWorktree(gone.clone())))?;
    wait_until(drawn, "the delete dialog", |drawn| {
        Ok(drawn
            .cx
            .read(modal::current::<DeleteWorktreeDialog>)
            .is_some())
    })?;
    let dialog = (drawn.cx.read(modal::current::<DeleteWorktreeDialog>)).ok_or("no dialog")?;
    let window = drawn
        .cx
        .read(modal::window)
        .ok_or("the dialog has no window")?;
    let named = drawn
        .cx
        .read(|cx| dialog.read(cx).named_terminals().join(", "));
    if !named.starts_with("server (") {
        return Err(format!("the dialog named the terminals as {named:?}"));
    }
    (drawn.cx)
        .update_window(window, |_, window, cx| window.draw(cx).clear(cx))
        .map_err(|error| error.to_string())?;
    let image = (drawn.cx.capture_screenshot(window)).map_err(|error| error.to_string())?;
    let path = checkout()?.join(super::OUT).join("delete-worktree.png");
    image.save(&path).map_err(|error| error.to_string())?;
    said.push(format!("{}", path.display()));
    (drawn.cx)
        .update_window(window, |_, window, cx| {
            dialog.update(cx, |dialog, cx| dialog.confirm(window, cx));
        })
        .map_err(|error| error.to_string())?;
    wait_until(drawn, "the worktree gone with its terminal", |drawn| {
        Ok(!gone.exists() && !has_terminals(drawn, &gone))
    })?;
    let list = RequestBody::ListTerminals {
        worktree: None,
        status: None,
    };
    let Ok(ResponseBody::Terminals { terminals }) = client.request(list) else {
        return Err("the daemon didn't list its terminals".into());
    };
    let left = (terminals.iter()).any(|summary| {
        (summary.record.worktree.as_ref()).is_some_and(|worktree| worktree.path == gone)
    });
    if left {
        return Err("the deleted worktree's terminal is still in the daemon".into());
    }
    said.push(format!(
        "deleting the worktree named its terminal ({named}) and stopped it"
    ));
    Ok(())
}

fn has_terminals(drawn: &mut Drawn, worktree: &Path) -> bool {
    let shell = drawn.shell.clone();
    drawn.cx.read(|cx| shell.read(cx).has_terminals(worktree))
}
