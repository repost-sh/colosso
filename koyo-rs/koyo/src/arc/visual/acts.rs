//! What a person does in the Arc sidebar, drawn as they do it (`docs/arc-sidebar.md` 2, 3, 7): a
//! space that isn't shown saying on its mark that a chat in it works, then waits; a project's
//! tile dragged from one row of the grid to the other; and a chat beside a page, the address
//! bar following the pane pressed in, ⌘L opening the launcher on the page.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::{Modifiers, MouseButton, Pixels, Point, point, px};
use image::imageops::{self, FilterType};
use koyo_agent::protocol::ids::{ThreadId, Time};
use koyo_agent::protocol::model::{Status, ThreadSummary};

use super::{OUT, project_beside, titled, web_page};
use crate::arc::hover::Hovered;
use crate::modal;
use crate::model::{PaneKind, Tab};
use crate::projects::read_worktrees;
use crate::settings;
use crate::shell::ToggleRightPanel;
use crate::theme;
use crate::visual::{Drawn, Result, checkout};

/// The check shows a chat, which reaches the agent host of the app's data folder: that has to be
/// the check's own, on the fake provider, or it would be the person's, with their chats.
pub fn needs_its_own_host() -> Result<()> {
    let own = std::env::var_os("KOYO_DATA_DIR").is_some();
    let fake = std::env::var_os("KOYO_AGENT_PROVIDER").is_some_and(|provider| provider == "fake");
    if own && fake {
        Ok(())
    } else {
        Err("set KOYO_DATA_DIR and KOYO_AGENT_PROVIDER=fake, so the chat the check shows has an agent host of its own".into())
    }
}

/// Stops the agent host the chat shown started in the check's data folder, where it would run
/// on as a person's does.
pub fn stop_host(said: &mut Vec<String>) {
    let Some(data) = settings::data_dir() else {
        return;
    };
    match koyo_agent::client::stop_host(&koyo_agent::client::host_dir(&data)) {
        Ok(()) => said.push("stopped the agent host the chat shown started".into()),
        Err(error) => said.push(format!("the agent host is left running: {error}")),
    }
}

/// Lets what moves in time come to rest: a space's switch, a mark's fade, tiles sliding. Each
/// step is a frame of the display's, which is what a fade or a spring under way waits for to
/// draw again.
pub fn settle(drawn: &mut Drawn) -> Result<()> {
    let window = drawn.window;
    for _ in 0..8 {
        drawn.cx.advance_clock(Duration::from_millis(100));
        (drawn.cx)
            .update_window(window, |_, window, cx| window.simulate_next_frame(cx))
            .map_err(|error| error.to_string())?;
        drawn.draw()?;
    }
    Ok(())
}

/// Saves the window as `name`, and the sidebar's lower left, four times its size, as
/// `name`-foot: the marks are a few points across.
fn save_with_foot(drawn: &mut Drawn, name: &str) -> Result<String> {
    let path = drawn.save(&format!("{name}.png"))?;
    let picture = image::open(&path).map_err(|error| error.to_string())?;
    let (width, height) = (picture.width(), picture.height());
    let scale = width / 1600;
    let sidebar = drawn.cx.read(|cx| settings::get(cx).sidebar_width());
    let across = ((sidebar + 2. * theme::GUTTER) as u32 * scale).min(width);
    let down = (110 * scale).min(height);
    let foot = imageops::crop_imm(&picture.to_rgba8(), 0, height - down, across, down).to_image();
    let foot = imageops::resize(&foot, across * 4, down * 4, FilterType::Nearest);
    let beside = path.with_file_name(format!("{name}-foot.png"));
    foot.save(&beside).map_err(|error| error.to_string())?;
    Ok(format!("saved {} and {}", path.display(), beside.display()))
}

/// The chats' host says of the one chat in `folder` that it's at `status`, read or not.
fn hear(drawn: &mut Drawn, folder: &Path, status: Status, unread: bool) {
    let summary = ThreadSummary {
        id: ThreadId("0001".into()),
        cwd: folder.to_string_lossy().into_owned(),
        title: "A chat in the other space".into(),
        provider: "claude".into(),
        model: None,
        status,
        needs: None,
        background: false,
        queued: 0,
        unread,
        tab_open: true,
        updated_at: Time("2026-11-10T10:00:00Z".into()),
    };
    let chats = drawn.cx.read(|cx| drawn.shell.read(cx).chats().clone());
    (drawn.cx).update(|cx| chats.update(cx, |chats, cx| chats.hear_summaries(vec![summary], cx)));
}

/// The first space shown, a chat in the second space's project works, then is done and not
/// seen, then seen: what the second space's mark says each time.
pub fn pills(drawn: &mut Drawn, site: &Path, said: &mut Vec<String>) -> Result<()> {
    let shell = drawn.shell.clone();
    (drawn.cx).update(|cx| shell.update(cx, |shell, cx| shell.show_space(0, cx)));
    settle(drawn)?;
    let steps = [
        (Status::Working, false, "arc-pill-working"),
        (Status::Idle, true, "arc-pill-attention"),
        (Status::Idle, false, "arc-pill-seen"),
    ];
    for (status, unread, name) in steps {
        hear(drawn, site, status, unread);
        settle(drawn)?;
        let says = (drawn.cx).read(|cx| shell.read(cx).space_activity(1, cx));
        said.push(format!(
            "the second space says {says:?}: {}",
            save_with_foot(drawn, name)?
        ));
    }
    Ok(())
}

/// Four more repositories beside the pictures, for a grid of two rows.
fn more_projects() -> Result<Vec<PathBuf>> {
    (["api", "docs", "infra", "mobile"].into_iter())
        .map(project_beside)
        .collect()
}

/// The tiles of the space shown, by their projects' names, in the order they're drawn.
fn tiles(drawn: &Drawn) -> Vec<String> {
    drawn.cx.read(|cx| {
        let shell = drawn.shell.read(cx);
        (shell.space_projects(cx).into_iter())
            .map(|index| shell.workspace.projects[index].name.to_string())
            .collect()
    })
}

/// Where the tile of the project in slot `slot` of the space shown is drawn.
fn tile_at(drawn: &Drawn, slot: usize) -> Result<Point<Pixels>> {
    let drawn_at = drawn.cx.read(|cx| {
        let shell = drawn.shell.read(cx);
        let index = *shell.space_projects(cx).get(slot)?;
        shell.arc_hover.drawn_at(Hovered::Tile(index))
    });
    Ok(drawn_at.ok_or("the tile isn't drawn")?.center())
}

/// Drags the tile in slot `from` of the space shown onto the one in slot `to`, as a person
/// would: taken up, half way there, over its slot, let go. With `pictures`, half way and over
/// its slot are saved under those names.
fn drag(
    drawn: &mut Drawn,
    (from, to): (usize, usize),
    pictures: Option<[&str; 2]>,
    said: &mut Vec<String>,
) -> Result<()> {
    let window = drawn.window;
    let (from, to) = (tile_at(drawn, from)?, tile_at(drawn, to)?);
    let none = Modifiers::default();
    let held = Some(MouseButton::Left);
    (drawn.cx).simulate_mouse_down(window, from, MouseButton::Left, none);
    (drawn.cx).simulate_mouse_move(window, from + point(px(4.), px(4.)), held, none);
    drawn.draw()?;
    // Half way, then there: the tiles it passes step into the slot it left.
    let half = point((from.x + to.x) / 2., (from.y + to.y) / 2.);
    for (step, at) in [half, to].into_iter().enumerate() {
        (drawn.cx).simulate_mouse_move(window, at, held, none);
        settle(drawn)?;
        if let Some(names) = pictures {
            let path = drawn.save(&format!("{}.png", names[step]))?;
            said.push(format!("saved {}", path.display()));
        }
    }
    (drawn.cx).simulate_mouse_up(window, to, MouseButton::Left, none);
    settle(drawn)
}

/// The sidebar's order as the settings keep it, by the projects' folders.
fn kept_order(drawn: &Drawn) -> Vec<String> {
    drawn.cx.read(|cx| {
        (settings::get(cx).projects.iter())
            .filter_map(|path| Some(path.file_name()?.to_string_lossy().into_owned()))
            .collect()
    })
}

/// Five projects in the space shown, three to a row, and the first one's tile dragged onto the
/// last one's, in the row below; then back up to where it was, which leaves the order as the
/// check found it.
pub fn drag_tile(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    let shell = drawn.shell.clone();
    let read: Vec<_> = (more_projects()?.into_iter())
        .map(|root| (read_worktrees(&root), root))
        .collect();
    drawn.cx.update(|cx| {
        shell.update(cx, |shell, cx| {
            let selected = shell.workspace.active_project();
            for (worktrees, root) in read {
                shell.workspace.add_project(root.clone());
                shell.show_worktrees(&root, worktrees, cx);
            }
            shell.workspace.select_project(selected);
            shell.save_projects(cx);
            cx.notify();
        });
    });
    settle(drawn)?;
    let before = tiles(drawn);
    let last = before.len() - 1;
    let pictures = ["arc-tile-in-hand", "arc-tile-over-its-slot"];
    drag(drawn, (0, last), Some(pictures), said)?;
    let path = drawn.save("arc-tile-moved.png")?;
    said.push(format!(
        "tiles {before:?} dragged first onto last: {:?}; the sidebar's order saved as {:?}; saved {}",
        tiles(drawn),
        kept_order(drawn),
        path.display()
    ));
    drag(drawn, (last, 0), None, said)?;
    said.push(format!(
        "and the last dragged back onto the first: {:?}; saved as {:?}",
        tiles(drawn),
        kept_order(drawn)
    ));
    Ok(())
}

/// What the address bar says, and the address of the page the sidebar's controls and ⌘L act on.
fn bar(drawn: &Drawn) -> (String, Option<String>) {
    drawn.cx.read(|cx| {
        let shell = drawn.shell.read(cx);
        let said = crate::arc::top::said(shell, cx).0.to_string();
        let page = shell.shown_page(cx).map(|page| page.read(cx).address());
        (said, page)
    })
}

/// A chat on the left and a page on the right of one tab: the bar says the pane pressed in,
/// and ⌘L opens the launcher on the page, where an address typed loads in it.
pub fn address(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    let (shell, window) = (drawn.shell.clone(), drawn.window);
    // Without the panel beside them, the panes share the window right of the sidebar.
    (drawn.cx)
        .update_window(window, |_, window, cx| {
            window.dispatch_action(Box::new(ToggleRightPanel), cx);
        })
        .map_err(|error| error.to_string())?;
    drawn.cx.update(|cx| {
        shell.update(cx, |shell, cx| {
            let chat = titled(PaneKind::Chat, "Plan the release");
            let page = web_page("https://www.example.com/docs?page=2", "Example Domain");
            shell.workspace.push_tab(Tab::beside(chat, page));
            cx.notify();
        });
    });
    settle(drawn)?;
    let sidebar = drawn.cx.read(|cx| settings::get(cx).sidebar_width());
    let left = sidebar + 2. * theme::GUTTER;
    let share = |share: f32| point(px(left + (1600. - left) * share), px(500.));
    let (in_chat, in_page) = (share(0.25), share(0.75));
    let none = Modifiers::default();
    for (at, name) in [(in_page, "arc-bar-in-the-page"), (in_chat, "arc-bar-in-the-chat")] {
        drawn.cx.simulate_click(window, at, none);
        settle(drawn)?;
        let (bar, page) = bar(drawn);
        let path = drawn.save(&format!("{name}.png"))?;
        said.push(format!(
            "pressed {name}: the bar says {bar:?}, its page is {page:?}; saved {}",
            path.display()
        ));
    }

    // ⌘L from the chat: the launcher on the tab's page, its address filled in and selected.
    drawn.cx.simulate_keystrokes(window, "cmd-l");
    drawn.draw()?;
    let launcher = (drawn.cx.read(modal::window)).ok_or("⌘L opened no launcher")?;
    (drawn.cx)
        .update_window(launcher, |_, window, cx| window.draw(cx).clear(cx))
        .map_err(|error| error.to_string())?;
    drawn.cx.run_until_parked();
    let picture = (drawn.cx.capture_screenshot(launcher)).map_err(|error| error.to_string())?;
    let path = checkout()?.join(OUT).join("arc-cmd-l.png");
    picture.save(&path).map_err(|error| error.to_string())?;
    said.push(format!("⌘L opened the launcher: saved {}", path.display()));
    let tabs = |drawn: &Drawn| {
        (drawn.cx).read(|cx| (shell.read(cx).workspace.branch()).map_or(0, |unit| unit.tabs.len()))
    };
    let before = tabs(drawn);
    drawn.cx.simulate_input(launcher, "example.org/next");
    drawn.cx.simulate_keystrokes(launcher, "enter");
    settle(drawn)?;
    let (bar, page) = bar(drawn);
    let keyboard = (drawn.cx)
        .update_window(window, |_, window, _| {
            let context = window.context_stack();
            context.iter().any(|context| context.contains("BrowserPage"))
        })
        .map_err(|error| error.to_string())?;
    let path = drawn.save("arc-after-cmd-l.png")?;
    said.push(format!(
        "typed example.org/next and Enter: the page is {page:?}, the bar says {bar:?}, tabs {before} -> {}, the page has the keyboard: {keyboard}; saved {}",
        tabs(drawn),
        path.display()
    ));
    Ok(())
}
