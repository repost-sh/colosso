//! A project's worktrees under their headers (`docs/arc-sidebar.md` 5), drawn as a person sees
//! and uses them: the four states of Arc's folder header (open, open under the pointer,
//! collapsed under the pointer, collapsed), light and dark; the rows sliding closed and open
//! again, caught along the way; a worktree dragged to the end, caught in hand; and the window
//! opened again on the settings file, the order and the collapsed worktree as they were left,
//! and Orca listing them in that order. Each picture is saved whole, and the worktrees' part of
//! the sidebar three times its size (`acts::Part::Worktree`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::{Action, Bounds, Modifiers, Pixels, Point, point, px};

use super::acts::{Part, drag, frames, save_with, settle};
use super::{project_beside, titled, web_page};
use crate::model::PaneKind;
use crate::projects::read_worktrees;
use crate::settings::{self, Layout};
use crate::test_support::try_git;
use crate::theme::{UseDarkTheme, UseLightTheme};
use crate::visual::{Drawn, Result, noted};

/// The worktrees beside the project's main checkout: their branches and folders' endings, one
/// with a name too long for the sidebar.
const WORKTREES: [(&str, &str); 3] = [
    ("feat/login", "login"),
    ("fix-payouts", "payouts"),
    (
        "schulzfel/a-rather-long-worktree-name-to-see-it-cut-short",
        "long",
    ),
];
/// The tabs the login worktree has, as the references' worktree has theirs.
const TABS: [(&str, &str); 5] = [
    (
        "https://mail.example.com/invoice",
        "Acme GmbH - Invoice 012588-000001 for October",
    ),
    ("https://mail.example.com/", "Gmail"),
    ("https://mail.example.com/u/1", "Gmail"),
    ("https://github.com/", "GitHub"),
    ("https://claude.ai/new", "New session - Claude"),
];
/// How long a frame lasts while the rows slide, caught each time.
const FRAME: Duration = Duration::from_millis(60);

/// The project `acme` beside the pictures, with its worktrees: its main checkout's folder.
fn acme() -> Result<PathBuf> {
    let root = project_beside("acme")?;
    for (branch, ending) in WORKTREES {
        let folder = root.with_file_name(format!("acme-{ending}"));
        if !folder.exists() {
            let folder = folder.to_string_lossy();
            try_git(&root, &["worktree", "add", "-q", "-b", branch, &folder])?;
        }
    }
    Ok(root)
}

/// The place among the selected project's worktrees of the one on `branch`.
fn place_of(drawn: &Drawn, branch: &str) -> Result<usize> {
    let place = drawn.cx.read(|cx| {
        let units = &drawn.shell.read(cx).workspace.project()?.branches;
        units.iter().position(|unit| unit.name() == branch)
    });
    place.ok_or_else(|| format!("no worktree on {branch}"))
}

/// Where the header of the worktree at `place` was drawn.
fn header(place: usize) -> Result<Bounds<Pixels>> {
    noted(&format!("arc-header-{place}")).ok_or_else(|| "the header isn't drawn".into())
}

/// The selected project's worktrees, by name, in the order they're listed.
fn listed(drawn: &Drawn) -> Vec<String> {
    drawn.cx.read(|cx| {
        let project = drawn.shell.read(cx).workspace.project();
        (project.into_iter().flat_map(|project| &project.branches))
            .map(|unit| unit.name().to_string())
            .collect()
    })
}

/// Where the pointer rests when it's on nothing of the sidebar's: over the panes.
fn away() -> Point<Pixels> {
    point(px(1200.), px(500.))
}

/// The pointer moved to `at`, as a hover, and what it eases in or out of come to rest.
fn rest_at(drawn: &mut Drawn, at: Point<Pixels>) -> Result<()> {
    (drawn.cx).simulate_mouse_move(drawn.window, at, None, Modifiers::default());
    settle(drawn)
}

/// The project shown with its worktrees, the login one with the references' tabs, each of
/// its header's four states saved light and dark, its rows caught sliding, a worktree
/// dragged to the end, and the window opened again from the settings file.
pub fn headers(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    let root = acme()?;
    let shell = drawn.shell.clone();
    let read = read_worktrees(&root);
    let login = root.with_file_name("acme-login");
    let payouts = root.with_file_name("acme-payouts");
    drawn.cx.update(|cx| {
        settings::update(cx, |settings| settings.layout = Layout::Arc);
        shell.update(cx, |shell, cx| {
            let project = shell.workspace.add_project(root.clone());
            shell.show_worktrees(&root, read, cx);
            shell.save_projects(cx);
            shell.workspace.select_worktree(project, &login);
            for (address, title) in TABS {
                shell.workspace.push_tab(web_page(address, title));
            }
            // A tab on the payouts worktree, and one on the main checkout, which is shown, so no
            // tab of the login worktree's is a pill, as none of the references' is.
            shell.workspace.select_worktree(project, &payouts);
            let chat = titled(PaneKind::Chat, "Payouts retry when the bank says 429");
            shell.workspace.push_tab(chat);
            shell.workspace.select_worktree(project, &root);
            shell
                .workspace
                .push_tab(titled(PaneKind::Editor, "README.md"));
            cx.notify();
        });
    });
    rest_at(drawn, away())?;
    let place = place_of(drawn, "feat/login")?;
    said.push(format!(
        "the worktrees, as git lists them: {:?}",
        listed(drawn)
    ));
    let themes: [(&dyn Action, &str); 2] = [(&UseLightTheme, "light"), (&UseDarkTheme, "dark")];
    for (theme, name) in themes {
        drawn.cx.update(|cx| cx.dispatch_action(theme));
        rest_at(drawn, away())?;
        let saved = save_with(drawn, &format!("arc-worktrees-{name}"), Part::Rows)?;
        said.push(format!("the worktrees, {name}: {saved}"));
        let bounds = header(place)?;
        let (top, on) = (bounds.top(), bounds.center());
        let states = [
            (away(), false, "open"),
            (on, false, "open-hovered"),
            (on, true, "collapsed-hovered"),
            (away(), false, "collapsed"),
        ];
        for (at, click, state) in states {
            if click {
                (drawn.cx).simulate_click(drawn.window, at, Modifiers::default());
            }
            rest_at(drawn, at)?;
            let rows = if state.starts_with("open") { 6 } else { 1 };
            let part = Part::Worktree { top, rows };
            let saved = save_with(drawn, &format!("arc-header-{state}-{name}"), part)?;
            said.push(format!("{state}, {name}: {saved}"));
        }
        // Open again, for the next appearance.
        (drawn.cx).simulate_click(drawn.window, on, Modifiers::default());
        rest_at(drawn, away())?;
    }
    slide(drawn, place, said)?;
    reorder(drawn, &root, said)
}

/// The login worktree's rows sliding closed and open again, each caught a few frames in.
fn slide(drawn: &mut Drawn, place: usize, said: &mut Vec<String>) -> Result<()> {
    let bounds = header(place)?;
    let part = Part::Worktree {
        top: bounds.top(),
        rows: 6,
    };
    for way in ["closing", "opening"] {
        (drawn.cx).simulate_click(drawn.window, bounds.center(), Modifiers::default());
        for frame in 1..=3 {
            frames(drawn, 1, FRAME)?;
            let saved = save_with(drawn, &format!("arc-header-{way}-{frame}"), part)?;
            said.push(format!("{way}, {} ms in: {saved}", frame * 60));
        }
        settle(drawn)?;
    }
    rest_at(drawn, away())
}

/// The main checkout dragged by its header past the last worktree, caught in hand; the long
/// one collapsed; and the window opened again from the settings file, both as they were left.
fn reorder(drawn: &mut Drawn, root: &Path, said: &mut Vec<String>) -> Result<()> {
    let long = place_of(drawn, WORKTREES[2].0)?;
    (drawn.cx).simulate_click(drawn.window, header(long)?.center(), Modifiers::default());
    rest_at(drawn, away())?;
    let main = header(place_of(drawn, "main")?)?;
    let last = header(listed(drawn).len() - 1)?;
    // Past the middle of the last worktree: its tab's row.
    let to = last.center() + point(px(0.), px(38.));
    let part = Part::Worktree {
        top: main.top(),
        rows: 12,
    };
    let pictures = ["arc-header-on-the-way", "arc-header-in-hand"];
    drag(drawn, (main.center(), to), Some((pictures, part)), said)?;
    rest_at(drawn, away())?;
    let saved = save_with(drawn, "arc-header-moved", part)?;
    said.push(format!("let go: {:?}, {saved}", listed(drawn)));

    // Kept: once the settings are written, the window opens again on them.
    drawn.until(Duration::from_secs(5), |drawn| {
        drawn.cx.advance_clock(Duration::from_millis(600));
        Ok(kept(drawn, root).is_some_and(|kept| kept.contains("acme-long")))
    })?;
    said.push(format!(
        "the settings file keeps {}",
        kept(drawn, root).unwrap_or_default()
    ));
    drawn.reopen()?;
    drawn.until(Duration::from_secs(20), |drawn| {
        let shell = drawn.shell.clone();
        Ok(drawn.cx.update(|cx| {
            shell.update(cx, |shell, cx| {
                let Some(project) = shell.workspace.project_index(root) else {
                    return false;
                };
                let listed = shell.workspace.projects[project].branches.len();
                if listed <= WORKTREES.len() {
                    return false;
                }
                shell.select_project(project, cx);
                true
            })
        }))
    })?;
    rest_at(drawn, away())?;
    let part = Part::Worktree {
        top: header(0)?.top(),
        rows: 8,
    };
    let saved = save_with(drawn, "arc-header-reopened", part)?;
    said.push(format!("opened again: {:?}, {saved}", listed(drawn)));
    // Orca lists them in the same order: it's the project's.
    drawn
        .cx
        .update(|cx| settings::update(cx, |settings| settings.layout = Layout::Orca));
    let path = drawn.save("orca-worktrees-moved-dark.png")?;
    said.push(format!("in Orca: saved {}", path.display()));
    Ok(())
}

/// What the settings file keeps of how the worktrees of the project at `root` show: their
/// order and those collapsed, by their folders' names.
fn kept(drawn: &Drawn, root: &Path) -> Option<String> {
    let path = drawn.cx.read(|cx| settings::path(cx).map(Path::to_owned))?;
    let file: settings::Settings = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    let look = file.project_looks.get(root)?;
    let (order, collapsed) = (names(&look.worktrees), names(&look.collapsed));
    Some(format!("order {order:?}, collapsed {collapsed:?}"))
}

/// The names of the folders at `paths`.
fn names<'a>(paths: impl IntoIterator<Item = &'a PathBuf>) -> Vec<String> {
    (paths.into_iter())
        .filter_map(|path| Some(path.file_name()?.to_string_lossy().into_owned()))
        .collect()
}
