//! What a person does in the Arc sidebar, drawn as they do it (`docs/arc-sidebar.md` 2, 3, 5,
//! 7): a space that isn't shown saying on its mark that a chat in it works, then waits; a tab's
//! row and its project's tile saying the same of a chat in the space shown; a project's tile
//! dragged from one row of the grid to the other; and a chat beside a page, the address bar
//! following the pane pressed in, ⌘L opening the launcher on the page.

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
    frames(drawn, 8, Duration::from_millis(100))
}

/// `count` frames of the display's, `step` apart, each drawn.
pub fn frames(drawn: &mut Drawn, count: usize, step: Duration) -> Result<()> {
    let window = drawn.window;
    for _ in 0..count {
        drawn.cx.advance_clock(step);
        (drawn.cx)
            .update_window(window, |_, window, cx| window.simulate_next_frame(cx))
            .map_err(|error| error.to_string())?;
        drawn.draw()?;
    }
    Ok(())
}

/// A part of the sidebar a picture shows again, larger: what's said there is a few points
/// across.
#[derive(Clone, Copy)]
pub enum Part {
    /// The tiles and the worktrees' rows under them.
    Rows,
    /// The spaces' marks.
    Foot,
    /// A worktree's rows, `rows` of them down from `top`, a little room round them.
    Worktree { top: Pixels, rows: u32 },
}

/// Saves the window as `name`, and `part` of the sidebar, three or four times its size, as
/// `name`-rows, `name`-foot or `name`-part.
pub fn save_with(drawn: &mut Drawn, name: &str, part: Part) -> Result<String> {
    let path = drawn.save(&format!("{name}.png"))?;
    let picture = image::open(&path).map_err(|error| error.to_string())?;
    let (width, height) = (picture.width(), picture.height());
    let scale = width / 1600;
    let sidebar = drawn.cx.read(|cx| settings::get(cx).sidebar_width());
    let across = ((sidebar + 2. * theme::GUTTER) as u32 * scale).min(width);
    let (label, top, down, times) = match part {
        Part::Rows => ("rows", (80 * scale).min(height), 600 * scale, 3),
        Part::Foot => ("foot", height.saturating_sub(110 * scale), 110 * scale, 4),
        // Each row is 38 points apart.
        Part::Worktree { top, rows } => {
            let top = (top.as_f32() as u32).saturating_sub(8) * scale;
            ("part", top.min(height), (rows * 38 + 16) * scale, 3)
        }
    };
    let down = down.min(height - top);
    let part = imageops::crop_imm(&picture.to_rgba8(), 0, top, across, down).to_image();
    let part = imageops::resize(&part, across * times, down * times, FilterType::Nearest);
    let beside = path.with_file_name(format!("{name}-{label}.png"));
    part.save(&beside).map_err(|error| error.to_string())?;
    Ok(format!("saved {} and {}", path.display(), beside.display()))
}

/// The chats' host says of chat `id`, in `folder`, that it's at `status`, read or not.
fn hear(drawn: &mut Drawn, (id, folder): (&str, &Path), status: Status, unread: bool) {
    let summary = ThreadSummary {
        id: ThreadId(id.into()),
        cwd: folder.to_string_lossy().into_owned(),
        title: "A chat at work".into(),
        provider: "claude".into(),
        model: None,
        effort: None,
        fast: false,
        permission: None,
        plan: false,
        status,
        needs: None,
        background: false,
        queued: 0,
        unread,
        tab_open: true,
        updated_at: Time("2026-11-10T10:00:00Z".into()),
        said: None,
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
        hear(drawn, ("0001", site), status, unread);
        settle(drawn)?;
        let says = (drawn.cx).read(|cx| shell.read(cx).space_activity(1, cx));
        said.push(format!(
            "the second space says {says:?}: {}",
            save_with(drawn, name, Part::Foot)?
        ));
    }
    Ok(())
}

/// A chat in a tab of the space shown works, then is done and not seen, then seen: what its
/// row says at its end each time, and its project's tile in its corner.
pub fn tab_marks(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    let shell = drawn.shell.clone();
    let root = checkout()?;
    // The chat among the tabs, which isn't the one shown, so nothing marks it seen.
    let found = (drawn.cx).update(|cx| {
        shell.update(cx, |shell, cx| {
            let tabs = &mut shell.workspace.branch_mut()?.tabs;
            let chat = (tabs.iter_mut()).find(|tab| tab.first().kind == PaneKind::Chat)?;
            if let Tab::Single(pane) = chat {
                pane.chat = Some(ThreadId("0002".into()));
            }
            cx.notify();
            Some(())
        })
    });
    found.ok_or("no chat among the tabs shown")?;
    let steps = [
        (Status::Working, false, "arc-tab-working"),
        (Status::Idle, true, "arc-tab-waiting"),
        (Status::Idle, false, "arc-tab-seen"),
    ];
    for (status, unread, name) in steps {
        hear(drawn, ("0002", &root), status, unread);
        settle(drawn)?;
        let says = (drawn.cx).read(|cx| {
            let shell = shell.read(cx);
            let chat = |tab: &&Tab| tab.first().kind == PaneKind::Chat;
            let tab = shell.workspace.branch()?.tabs.iter().find(chat)?;
            Some(shell.tab_activity(tab, cx))
        });
        said.push(format!(
            "the chat's tab says {says:?}: {}",
            save_with(drawn, name, Part::Rows)?
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

/// Drags what's at `from` to `to`, as a person would: taken up, half way there, over where it
/// goes, let go. With `pictures`, half way and over where it goes are saved under those names,
/// with `part` of the sidebar.
pub fn drag(
    drawn: &mut Drawn,
    (from, to): (Point<Pixels>, Point<Pixels>),
    pictures: Option<([&str; 2], Part)>,
    said: &mut Vec<String>,
) -> Result<()> {
    let window = drawn.window;
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
        if let Some((names, part)) = pictures {
            said.push(save_with(drawn, names[step], part)?);
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
    let (first, end) = (tile_at(drawn, 0)?, tile_at(drawn, last)?);
    drag(drawn, (first, end), Some((pictures, Part::Rows)), said)?;
    let path = drawn.save("arc-tile-moved.png")?;
    said.push(format!(
        "tiles {before:?} dragged first onto last: {:?}; the sidebar's order saved as {:?}; saved {}",
        tiles(drawn),
        kept_order(drawn),
        path.display()
    ));
    let (first, end) = (tile_at(drawn, 0)?, tile_at(drawn, last)?);
    drag(drawn, (end, first), None, said)?;
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
    for (at, name) in [
        (in_page, "arc-bar-in-the-page"),
        (in_chat, "arc-bar-in-the-chat"),
    ] {
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
            context
                .iter()
                .any(|context| context.contains("BrowserPage"))
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
