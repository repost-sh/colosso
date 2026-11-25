//! The sidebar with the tabs in it (`docs/layout.md`, Sidebar) as the app draws it, saved as PNGs
//! in `.artifacts/qa/sidebar/`: this checkout's project with a few tabs and another in a space, a
//! project in a space of its own, and an empty space, light and dark; then what a person does there
//! (`acts`): a space's mark saying a chat in it works and then waits, a tab's row saying the same,
//! the address bar following the pane pressed in, ⌘L's launcher on a page, ⌥← and ⌥→ between
//! projects and ⌥⇧← and ⌥⇧→ between spaces, in a composer and on the shortcuts page (`keys`), the
//! pages pinned at the sidebar's head and the space's loose pages under them, pinned and kept by a
//! drop, a tab in hand making room among the tiles, dragged, shown as the space changes, closed and
//! put away (`pins`), the loose pages half-way through a swipe, each space's own and shared
//! (`unpinned_swipe`), and the same pinned and loose pages with the tabs on the top rail, a tab in
//! hand making room among the pinned ones there, and their zone (`rail`). It shows a chat, so it
//! runs on a data folder and an agent host of its own:
//!
//! ```sh
//! Q=$PWD/.artifacts/qa/sidebar && mkdir -p $Q/tmp &&
//!   KOYO_AGENT_PROVIDER=fake KOYO_DATA_DIR=$Q/data TMPDIR=$Q/tmp \
//!   KOYO_CONFIG_DIR=$Q/config KOYO_LOG_DIR=$Q/logs \
//!   script/cargo run -p koyo --features visual-check -- --visual-check-sidebar
//! ```

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use gpui_kit::{Action, point, px};

use crate::browser::profiles::By;
use crate::model::{Browser, Content, Pane, PaneKind, Tab, Unloaded};
use crate::settings::{self, Tabs};
use crate::shell::Shell;
use crate::shell::ToggleRightPanel;
use crate::test_support::try_git;
use crate::theme::{UseDarkTheme, UseLightTheme};
use crate::visual::{Drawn, Result, checkout};

mod acts;
mod keys;
mod pins;
mod rail;
mod unpinned_swipe;

/// Where the pictures go, relative to the checkout.
const OUT: &str = ".artifacts/qa/sidebar";

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
        restored: true,
        session: koyo_browser_ipc::Session::default(),
    };
    let content = Content::Browser(Browser::waiting(unloaded, By::Person));
    Tab::Single(Pane {
        title: title.to_owned().into(),
        ..Pane::of(content)
    })
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
        .push_tab(titled(PaneKind::Chat, "Sidebar on the glass"));
    shell
        .workspace
        .push_tab(titled(PaneKind::Terminal, "cargo test"));
    shell.workspace.push_tab(Tab::beside(
        web_page("https://x.com/history", "History / X"),
        web_page("https://normas.example/", "Normas"),
    ));
    shell.workspace.push_tab(Tab::beside(
        web_page("https://github.com/", "GitHub"),
        web_page("https://www.example.com/", "Example Domain"),
    ));
    if let Some(unit) = shell.workspace.branch_mut() {
        unit.active_tab = unit.tabs.len() - 1;
    }
}

/// More tabs than the sidebar's height, scrolled between their ends: they fade at both.
fn edge_fade(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    let shell = drawn.shell.clone();
    drawn.cx.update(|cx| {
        settings::update(cx, |settings| settings.tabs = Tabs::Sidebar);
        shell.update(cx, |shell, cx| {
            shell.show_space(0, cx);
            for at in 0..60 {
                let tab = titled(PaneKind::Editor, &format!("Tab {at}"));
                shell.workspace.push_tab(tab);
            }
            cx.notify();
        });
    });
    acts::settle(drawn)?;
    drawn.cx.update(|cx| {
        shell.update(cx, |shell, cx| {
            shell.sidebar_scroll.set_offset(point(px(0.), px(-307.)));
            cx.notify();
        });
    });
    acts::settle(drawn)?;
    let path = drawn.save("sidebar-edge-fade.png")?;
    said.push(format!("saved {} (rows fading at both ends)", path.display()));
    Ok(())
}

fn draw() -> Result<Vec<String>> {
    acts::needs_its_own_host()?;
    let (api, site) = (project_beside("api")?, project_beside("site")?);
    let mut drawn = Drawn::open_on_checkout(OUT, &[&api, &site])?;
    let shell = drawn.shell.clone();
    let mut said = Vec::new();
    drawn.cx.update(|cx| {
        settings::update(cx, |settings| {
            settings.tabs = Tabs::Sidebar;
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
            settings.add_space();
            settings.space = 0;
        });
        shell.update(cx, |shell, cx| {
            open_tabs(shell);
            cx.notify();
        });
    });
    let root = checkout()?;
    let fetched = drawn.until(Duration::from_secs(20), |drawn| {
        Ok(drawn
            .cx
            .update(|cx| crate::shell::switcher::avatar::picture(&root, cx))
            .is_some())
    });
    said.push(match fetched {
        Ok(()) => "the project's avatar came from its git host".into(),
        Err(why) => format!("no avatar ({why}): the project shows initials"),
    });
    (drawn.cx)
        .update_window(drawn.window, |_, window, cx| {
            window.dispatch_action(Box::new(ToggleRightPanel), cx);
        })
        .map_err(|error| error.to_string())?;
    let themes: [(&dyn Action, &str); 2] = [(&UseLightTheme, "light"), (&UseDarkTheme, "dark")];
    for (theme, name) in themes {
        drawn.cx.update(|cx| cx.dispatch_action(theme));
        drawn.draw()?;
        let path = drawn.save(&format!("sidebar-work-{name}.png"))?;
        said.push(format!("saved {}", path.display()));
    }
    for (space, name) in [(1, "side"), (2, "empty")] {
        drawn
            .cx
            .update(|cx| shell.update(cx, |shell, cx| shell.show_space(space, cx)));
        acts::settle(&mut drawn)?;
        let path = drawn.save(&format!("sidebar-{name}-dark.png"))?;
        said.push(format!("saved {}", path.display()));
    }
    acts::pills(&mut drawn, &site, &mut said)?;
    acts::tab_marks(&mut drawn, &mut said)?;
    acts::address(&mut drawn, &mut said)?;
    keys::space_keys(&mut drawn, &mut said)?;
    pins::pins(&mut drawn, &mut said)?;
    unpinned_swipe::loose_swipe(&mut drawn, &mut said)?;
    acts::stop_host(&mut said);
    drawn.cx.update(|cx| {
        settings::update(cx, |settings| settings.tabs = Tabs::TopRail);
        shell.update(cx, |shell, cx| shell.show_space(0, cx));
    });
    let path = drawn.save("rail-window-dark.png")?;
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
    said.push(format!("the second space's project: {}", held(&site)));
    rail::rail_pages(&mut drawn, &mut said)?;
    edge_fade(&mut drawn, &mut said)?;
    Ok(said)
}
