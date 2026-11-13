//! The Arc sidebar (`docs/arc-sidebar.md`) as the app draws it, saved as PNGs in
//! `.superpowers/qa/arc/`: this checkout's project with a few tabs in a space, a second project in
//! a space of its own with its own colours, and an empty space, light and dark; then what a
//! person does there (`acts`): a space's mark saying a chat in it works and then waits, a tab's
//! row and its tile saying the same, a tile dragged to another row and back, the address bar
//! following the pane pressed in, ⌘L's launcher on a page, ⌥← and ⌥→ between spaces, in a
//! composer and on the shortcuts page (`keys`), the pages pinned at the sidebar's head and the
//! space's loose pages under them, pinned and kept by a drop, dragged, shown as the space
//! changes, closed and put away (`pins`), and a project's worktrees under their headers,
//! collapsed, opened, dragged and kept (`headers`). It shows a chat, so it runs on a data folder
//! and an agent host of its own:
//!
//! ```sh
//! Q=$PWD/.superpowers/qa/arc && mkdir -p $Q/tmp &&
//!   KOYO_AGENT_PROVIDER=fake KOYO_DATA_DIR=$Q/data TMPDIR=$Q/tmp \
//!   KOYO_CONFIG_DIR=$Q/config KOYO_LOG_DIR=$Q/logs \
//!   script/cargo run -p koyo --features visual-check -- --visual-check-arc
//! ```

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use gpui_kit::{Action, point};

use crate::gradient::GradientTheme;
use crate::model::{Pane, PaneKind, Tab, Unloaded};
use crate::settings::{self, Layout};
use crate::shell::Shell;
use crate::shell::ToggleRightPanel;
use crate::test_support::try_git;
use crate::theme::{UseDarkTheme, UseLightTheme};
use crate::visual::{Drawn, Result, checkout};

mod acts;
mod headers;
mod keys;
mod pins;

/// Where the pictures go, relative to the checkout.
const OUT: &str = ".superpowers/qa/arc";

pub fn run() -> ExitCode {
    crate::visual::run(draw)
}

/// A repository of its own called `name`, beside the pictures: another project for the check.
fn project_beside(name: &str) -> Result<PathBuf> {
    let folder = checkout()?.join(OUT).join(name);
    if !folder.join(".git").exists() {
        std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
        try_git(&folder, &["init", "-q", "-b", "main"])?;
        try_git(&folder, &["commit", "-q", "--allow-empty", "-m", "start"])?;
    }
    folder.canonicalize().map_err(|error| error.to_string())
}

/// A tab of a page at `address`, titled `title`, with no page until it's shown.
fn web_page(address: &str, title: &str) -> Tab {
    let unloaded = Unloaded {
        address: address.into(),
        favicon: None,
    };
    Tab::Single(Pane::unloaded(unloaded, title.to_owned().into()))
}

/// A tab of `kind`, titled `title`.
fn titled(kind: PaneKind, title: &str) -> Tab {
    let mut pane = Pane::new(kind);
    pane.title = title.to_owned().into();
    Tab::Single(pane)
}

/// The tabs the checkout shows: pages, a chat and a terminal, a split listed and one shown, the
/// last of them.
fn open_tabs(shell: &mut Shell) {
    shell
        .workspace
        .push_tab(web_page("https://www.example.com/docs", "Example Domain"));
    shell
        .workspace
        .push_tab(titled(PaneKind::Chat, "Sidebar like Arc's"));
    shell
        .workspace
        .push_tab(titled(PaneKind::Terminal, "cargo test"));
    // A split listed, as Arc lists one: its halves side by side.
    shell.workspace.push_tab(Tab::beside(
        web_page("https://x.com/history", "History / X"),
        web_page("https://normas.example/", "Normas"),
    ));
    // And one shown, each pane an island, the glass between them.
    shell.workspace.push_tab(Tab::beside(
        web_page("https://github.com/", "GitHub"),
        web_page("https://www.example.com/", "Example Domain"),
    ));
    if let Some(unit) = shell.workspace.branch_mut() {
        unit.active_tab = unit.tabs.len() - 1;
    }
}

fn draw() -> Result<Vec<String>> {
    acts::needs_its_own_host()?;
    // A second project, for the second space.
    let site = project_beside("site")?;
    let mut drawn = Drawn::open_on_checkout(OUT, &[&site])?;
    let shell = drawn.shell.clone();
    let mut said = Vec::new();
    drawn.cx.update(|cx| {
        settings::update(cx, |settings| {
            settings.layout = Layout::Arc;
            settings.spaces.clear();
            settings.space = 0;
            settings.project_looks.clear();
        });
        shell.update(cx, Shell::save_projects);
        settings::update(cx, |settings| {
            settings.spaces[0].name = "Work".into();
            let second_space = settings.add_space();
            settings.spaces[second_space].name = "Side projects".into();
            settings.move_project(&site, second_space);
            let mut colours = GradientTheme::default();
            colours.move_primary(point(300., 120.));
            settings.spaces[second_space].dark_theme = Some(colours.clone());
            settings.spaces[second_space].light_theme = Some(colours);
            settings.add_space();
            settings.space = 0;
        });
        shell.update(cx, |shell, cx| {
            open_tabs(shell);
            cx.notify();
        });
    });
    // The tile shows the git host's avatar once it's fetched; offline, the initials stay.
    let root = checkout()?;
    let fetched = drawn.until(Duration::from_secs(20), |drawn| {
        Ok(drawn
            .cx
            .update(|cx| super::avatar::picture(&root, cx))
            .is_some())
    });
    said.push(match fetched {
        Ok(()) => "the project's avatar came from its git host".into(),
        Err(why) => format!("no avatar ({why}): the tile shows initials"),
    });
    // The panel beside the panes, an island of its own.
    (drawn.cx)
        .update_window(drawn.window, |_, window, cx| {
            window.dispatch_action(Box::new(ToggleRightPanel), cx);
        })
        .map_err(|error| error.to_string())?;
    let themes: [(&dyn Action, &str); 2] = [(&UseLightTheme, "light"), (&UseDarkTheme, "dark")];
    for (theme, name) in themes {
        drawn.cx.update(|cx| cx.dispatch_action(theme));
        drawn.draw()?;
        let path = drawn.save(&format!("arc-work-{name}.png"))?;
        said.push(format!("saved {}", path.display()));
    }
    for (space, name) in [(1, "side"), (2, "empty")] {
        drawn
            .cx
            .update(|cx| shell.update(cx, |shell, cx| shell.show_space(space, cx)));
        // The switch moves in time: the space fades in and its mark stretches.
        acts::settle(&mut drawn)?;
        let path = drawn.save(&format!("arc-{name}-dark.png"))?;
        said.push(format!("saved {}", path.display()));
    }
    // What a person does there: a space's mark saying a chat in it waits, a tab's row saying
    // it, a tile dragged, the address bar following the pane and ⌘L's launcher on a page.
    acts::pills(&mut drawn, &site, &mut said)?;
    acts::tab_marks(&mut drawn, &mut said)?;
    acts::drag_tile(&mut drawn, &mut said)?;
    acts::address(&mut drawn, &mut said)?;
    keys::space_keys(&mut drawn, &mut said)?;
    // The pages pinned at the sidebar's head and the space's loose ones, from none and back.
    pins::pins(&mut drawn, &mut said)?;
    acts::stop_host(&mut said);
    // The Orca layout keeps one card, hairlines between its parts: no islands there.
    drawn.cx.update(|cx| {
        settings::update(cx, |settings| settings.layout = Layout::Orca);
        shell.update(cx, |shell, cx| shell.show_space(0, cx));
    });
    let path = drawn.save("orca-dark.png")?;
    said.push(format!("saved {}", path.display()));
    let shown = drawn.cx.read(|cx| {
        let settings = settings::get(cx);
        (settings.spaces.iter())
            .map(|space| format!("{} ({} projects)", space.name, space.projects.len()))
            .collect::<Vec<_>>()
            .join(", ")
    });
    said.push(format!("spaces: {shown}"));
    let held = |path: &Path| path.display().to_string();
    said.push(format!("second project: {}", held(&site)));
    // Back in Arc, a project's worktrees under their headers, which Orca then lists as moved.
    headers::headers(&mut drawn, &mut said)?;
    Ok(said)
}
