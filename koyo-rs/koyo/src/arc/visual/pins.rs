//! The pinned pages at the sidebar's head and the loose pages under them (`docs/arc-sidebar.md`
//! 11), drawn as a person uses them: the zone that says where a pinned page goes while none is;
//! a page's tab in hand over it, and let go, its tile; three more pinned, one with its site's
//! icon; a page's tab taken off its list and let go on the strip for the space's own pages, and
//! another opened from it; a tile in hand among the others, taken well away as a chip onto a
//! loose page's row, and that row back onto the tiles; a pinned page shown as the space changes
//! under it, a space with no project among them; and the zone put away, back for a page's tab
//! taken off its list.

use std::io::Cursor;
use std::sync::Arc;

use gpui_kit::{Bounds, Image, ImageFormat, Modifiers, MouseButton, Pixels, Point, point, px};
use image::{ImageBuffer, Rgba};

use super::acts::{Part, drag, save_with, settle};
use super::headers::away;
use super::web_page;
use crate::arc::hover::Hovered;
use crate::model::{Pane, Tab, Unloaded};
use crate::settings;
use crate::visual::{Drawn, Result, checkout, noted};

/// The pages pinned after the first: their addresses and titles.
const MORE: [(&str, &str); 3] = [
    ("https://calendar.example.com/week", "Calendar"),
    ("https://github.com/pulls", "Pull requests"),
    ("http://localhost:3000/", "App"),
];

/// A site's icon for the check, as the browser fetches one: a PNG, a disc on a square.
fn site_icon() -> Result<Arc<Image>> {
    let picture = ImageBuffer::from_fn(64, 64, |x, y| {
        let (dx, dy) = (x as f32 - 31.5, y as f32 - 31.5);
        if dx.hypot(dy) < 18. {
            Rgba([255u8, 255, 255, 255])
        } else {
            Rgba([234u8, 88, 12, 255])
        }
    });
    let mut png = Cursor::new(Vec::new());
    picture
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    Ok(Arc::new(Image::from_bytes(
        ImageFormat::Png,
        png.into_inner(),
    )))
}

/// Where the region of the pinned pages was drawn.
fn region() -> Result<Bounds<Pixels>> {
    noted("arc-pinned").ok_or_else(|| "the pinned pages' region isn't drawn".into())
}

/// Where the row of the tab at `tab` of the selected worktree was drawn.
fn row(drawn: &Drawn, tab: usize) -> Result<Point<Pixels>> {
    let project = drawn.cx.read(|cx| {
        let project = drawn.shell.read(cx).workspace.project();
        project.map(|project| project.active_branch)
    });
    let branch = project.ok_or("no project is shown")?;
    let row = noted(&format!("arc-tab-{branch}-{tab}")).ok_or("the tab's row isn't drawn")?;
    Ok(row.center())
}

/// Where the tile of the pinned page in slot `slot` was drawn.
fn tile(drawn: &Drawn, slot: usize) -> Result<Point<Pixels>> {
    let drawn_at = drawn.cx.read(|cx| {
        let shell = drawn.shell.read(cx);
        let key = shell.workspace.pinned().get(slot)?.key();
        shell.arc_hover.drawn_at(Hovered::Pin(key))
    });
    Ok(drawn_at
        .ok_or("the pinned page's tile isn't drawn")?
        .center())
}

/// The pinned pages by their titles, in the order of their tiles, and the place of the one shown.
fn pinned(drawn: &Drawn) -> (Vec<String>, Option<usize>) {
    drawn.cx.read(|cx| {
        let workspace = &drawn.shell.read(cx).workspace;
        let titles = (workspace.pinned().iter())
            .map(|pinned| pinned.tab.first().title.to_string())
            .collect();
        (titles, workspace.pinned_shown())
    })
}

/// What the address bar says, the page the panes show, and the space and project the sidebar
/// lists.
fn showing(drawn: &Drawn) -> String {
    drawn.cx.read(|cx| {
        let shell = drawn.shell.read(cx);
        let said = crate::arc::top::said(shell, cx).0;
        let page = (shell.workspace.tab_shown()).map(|tab| tab.first().title.to_string());
        let project = (shell.workspace.project()).map(|project| project.name.to_string());
        let space = settings::get(cx).space;
        format!("space {space} lists {project:?}; the panes show {page:?}, the bar says {said:?}")
    })
}

/// Takes up what's at `from` and carries it well off its list or grid, as a person dragging it
/// elsewhere does: what takes it elsewhere shows from then.
fn take_up(drawn: &mut Drawn, from: Point<Pixels>) -> Result<()> {
    let (window, none, held) = (drawn.window, Modifiers::default(), Some(MouseButton::Left));
    (drawn.cx).simulate_mouse_down(window, from, MouseButton::Left, none);
    (drawn.cx).simulate_mouse_move(window, from + point(px(4.), px(4.)), held, none);
    drawn.draw()?;
    (drawn.cx).simulate_mouse_move(window, away(), held, none);
    settle(drawn)
}

/// Carries what's in hand over `to`, without letting go.
fn carry(drawn: &mut Drawn, to: Point<Pixels>) -> Result<()> {
    let (none, held) = (Modifiers::default(), Some(MouseButton::Left));
    (drawn.cx).simulate_mouse_move(drawn.window, to, held, none);
    settle(drawn)
}

/// Lets go at `at` of what's in hand.
fn let_go(drawn: &mut Drawn, at: Point<Pixels>) -> Result<()> {
    (drawn.cx).simulate_mouse_up(drawn.window, at, MouseButton::Left, Modifiers::default());
    settle(drawn)
}

/// Where what `name` names was drawn, by its middle.
fn middle(name: &str) -> Result<Point<Pixels>> {
    let at = noted(name).ok_or_else(|| format!("{name} isn't drawn"))?;
    Ok(at.center())
}

/// The loose pages of the space shown, by their titles, and the place of the one shown.
fn loose(drawn: &Drawn) -> (Vec<String>, Option<usize>) {
    drawn.cx.read(|cx| {
        let workspace = &drawn.shell.read(cx).workspace;
        let titles = (workspace.loose().iter())
            .map(|page| page.tab.first().title.to_string())
            .collect();
        (titles, workspace.loose_shown())
    })
}

/// The first space shown on this checkout's project, with two pages first among its tabs,
/// none pinned and none loose yet.
fn start(drawn: &mut Drawn) -> Result<()> {
    let (shell, root) = (drawn.shell.clone(), checkout()?);
    drawn.cx.update(|cx| {
        // The zone as a person first finds it, whatever the check's last run left put away.
        settings::update(cx, |settings| settings.pinned_zone_dismissed = false);
        shell.update(cx, |shell, cx| {
            shell.show_space(0, cx);
            if let Some(project) = shell.workspace.project_index(&root) {
                shell.select_project(project, cx);
                shell.workspace.select_worktree(project, &root);
            }
            if let Some(unit) = shell.workspace.branch_mut() {
                unit.tabs
                    .insert(0, web_page("https://docs.example.com/guide", "Guide"));
                unit.tabs
                    .insert(0, web_page("https://mail.example.com/inbox", "Inbox (3)"));
                unit.active_tab += 2;
            }
            cx.notify();
        });
    });
    settle(drawn)
}

/// The sidebar in its new order, the zone for pinned pages at its head; a page's tab dragged
/// onto the zone, which says so and takes it: its tile. Then three more pinned, the first of
/// them with its site's icon.
fn pin_by_drop(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    let shell = drawn.shell.clone();
    said.push(format!(
        "no page pinned, the zone at the sidebar's head says where one goes: {}",
        save_with(drawn, "arc-pinned-empty", Part::Rows)?
    ));
    take_up(drawn, row(drawn, 0)?)?;
    let zone = region()?.center();
    carry(drawn, zone)?;
    said.push(format!(
        "a page's tab in hand over the zone: {}",
        save_with(drawn, "arc-pinned-tab-over-the-zone", Part::Rows)?
    ));
    let_go(drawn, zone)?;
    said.push(format!(
        "let go there, it's pinned: {:?}; {}",
        pinned(drawn),
        save_with(drawn, "arc-pinned-one", Part::Rows)?
    ));
    let icon = site_icon()?;
    drawn.cx.update(|cx| {
        shell.update(cx, |shell, cx| {
            // On a tab of the worktree's, where a page opened is the worktree's.
            if let Some((project, branch, tab)) = shell.workspace.shown_tab() {
                shell.pick_tab(project, branch, tab, cx);
            }
            for (at, (address, title)) in MORE.into_iter().enumerate() {
                let unloaded = Unloaded {
                    address: address.into(),
                    favicon: (at == 0).then(|| icon.clone()),
                };
                let pane = Pane::unloaded(unloaded, title.into());
                let key = pane.id;
                shell.workspace.push_tab(Tab::Single(pane));
                shell.pin_tab_of(key, cx);
            }
        });
    });
    settle(drawn)?;
    said.push(format!(
        "four pinned, the last still shown: {:?}; {}",
        pinned(drawn),
        save_with(drawn, "arc-pinned-tiles", Part::Rows)?
    ));
    Ok(())
}

/// A page's tab taken off its list: a dotted strip under the pinned pages takes it for the
/// space, and the tiles' outline for the pinned. Let go on the strip, it's the space's first
/// loose page, shown. Another opened from it comes after it; the first picked again.
fn loose_pages(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    let (shell, window) = (drawn.shell.clone(), drawn.window);
    take_up(drawn, row(drawn, 0)?)?;
    let strip = middle("arc-loose-strip")?;
    carry(drawn, strip)?;
    said.push(format!(
        "a page's tab off its list, over the strip for the space's own pages: {}",
        save_with(drawn, "arc-loose-tab-over-the-strip", Part::Rows)?
    ));
    let_go(drawn, strip)?;
    said.push(format!(
        "let go there, it's a loose page of the space, shown: {:?}; {}",
        loose(drawn),
        save_with(drawn, "arc-loose-one", Part::Rows)?
    ));
    // Opened where the person is on a loose page, a page is a loose one, after it.
    let opened = (drawn.cx).update_window(window, |_, window, cx| {
        shell.update(cx, |shell, cx| {
            shell.open_browser_tab(
                Some("https://news.example.com/".into()),
                false,
                None,
                window,
                cx,
            )
        })
    });
    opened.map_err(|error| error.to_string())?;
    settle(drawn)?;
    drawn
        .cx
        .simulate_click(window, middle("arc-loose-0")?, Modifiers::default());
    settle(drawn)?;
    said.push(format!(
        "another opened from it, and the first picked again: {:?}; {}; {}",
        loose(drawn),
        showing(drawn),
        save_with(drawn, "arc-loose-pages", Part::Rows)?
    ));
    Ok(())
}

/// A tile dragged onto the last one's slot, the others stepping out of its way; then one taken
/// well away, where it's a chip and its tile has gone back, onto a loose page's row: let go
/// there it's a loose page; and that row dragged back onto the tiles is pinned again.
fn drag_tiles(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    let before = pinned(drawn).0;
    let (first, last) = (tile(drawn, 0)?, tile(drawn, before.len() - 1)?);
    let pictures = ["arc-pinned-tile-in-hand", "arc-pinned-tile-over-its-slot"];
    drag(drawn, (first, last), Some((pictures, Part::Rows)), said)?;
    said.push(format!(
        "tiles {before:?} dragged first onto last: {:?}",
        pinned(drawn).0
    ));
    take_up(drawn, tile(drawn, 0)?)?;
    let over = middle("arc-loose-1")?;
    carry(drawn, over)?;
    said.push(format!(
        "a tile taken well away, a chip over a loose page's row: {}",
        save_with(drawn, "arc-pinned-tile-away", Part::Rows)?
    ));
    let_go(drawn, over)?;
    said.push(format!(
        "let go there, it's a loose page: pinned {:?}, loose {:?}",
        pinned(drawn),
        loose(drawn)
    ));
    take_up(drawn, middle("arc-loose-1")?)?;
    let tiles = region()?.center();
    carry(drawn, tiles)?;
    said.push(format!(
        "its row in hand over the tiles: {}",
        save_with(drawn, "arc-loose-row-over-the-tiles", Part::Rows)?
    ));
    let_go(drawn, tiles)?;
    said.push(format!(
        "let go there, it's pinned again: pinned {:?}, loose {:?}",
        pinned(drawn),
        loose(drawn)
    ));
    Ok(())
}

/// A pinned page shown, and the space switched under it: the sidebar lists the other space's
/// loose pages and project, then a space with no project, which shows no dot, and the panes
/// still show the pinned page. Back on the first space, ⌘W closes it, and its tile stays.
fn across_spaces(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    let (shell, window) = (drawn.shell.clone(), drawn.window);
    let at = tile(drawn, 0)?;
    drawn.cx.simulate_click(window, at, Modifiers::default());
    settle(drawn)?;
    let path = drawn.save("arc-pinned-shown.png")?;
    said.push(format!(
        "a tile picked: {}; saved {}",
        showing(drawn),
        path.display()
    ));
    for (space, name) in [
        (1, "arc-pinned-shown-next-space"),
        (2, "arc-pinned-shown-empty-space"),
    ] {
        (drawn.cx).update(|cx| shell.update(cx, |shell, cx| shell.slide_to_space(space, cx)));
        settle(drawn)?;
        settle(drawn)?;
        let path = drawn.save(&format!("{name}.png"))?;
        said.push(format!(
            "{}; its loose pages {:?}; saved {}",
            showing(drawn),
            loose(drawn).0,
            path.display()
        ));
    }
    // In the space with no project, ⌘W closes the pinned page, and a page opened is the
    // space's own: nothing of another space's shows.
    drawn.cx.simulate_keystrokes(window, "cmd-w");
    settle(drawn)?;
    let opened = (drawn.cx).update_window(window, |_, window, cx| {
        shell.update(cx, |shell, cx| {
            shell.open_browser_tab(
                Some("https://maps.example.com/".into()),
                false,
                None,
                window,
                cx,
            )
        })
    });
    opened.map_err(|error| error.to_string())?;
    settle(drawn)?;
    said.push(format!(
        "⌘W on it, then a page opened in the space with no project: {}; still pinned {:?}; loose {:?}; {}",
        showing(drawn),
        pinned(drawn),
        loose(drawn),
        save_with(drawn, "arc-loose-in-an-empty-space", Part::Rows)?
    ));
    (drawn.cx).update(|cx| shell.update(cx, |shell, cx| shell.slide_to_space(0, cx)));
    settle(drawn)?;
    settle(drawn)?;
    said.push(format!(
        "back on the first space: {}; loose {:?}; {}",
        showing(drawn),
        loose(drawn),
        save_with(drawn, "arc-first-space-again", Part::Rows)?
    ));
    Ok(())
}

/// Every pin removed, the zone is back; put away with its ×, it's gone, and shows again only
/// once a page's tab is taken off its list.
fn put_away(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    let (shell, window) = (drawn.shell.clone(), drawn.window);
    drawn.cx.update(|cx| {
        shell.update(cx, |shell, cx| {
            let keys: Vec<u64> = (shell.workspace.pinned().iter())
                .map(crate::model::Pinned::key)
                .collect();
            for key in keys {
                shell.remove_pinned(key, cx);
            }
        });
    });
    settle(drawn)?;
    // The pointer on the zone shows its ×, at its end.
    let zone = region()?;
    let end = point(zone.right() - px(18.), zone.center().y);
    (drawn.cx).simulate_mouse_move(window, end, None, Modifiers::default());
    settle(drawn)?;
    said.push(format!(
        "none pinned, the pointer on the zone's ×: {}",
        save_with(drawn, "arc-pinned-zone-hovered", Part::Rows)?
    ));
    drawn.cx.simulate_click(window, end, Modifiers::default());
    settle(drawn)?;
    let away = drawn.cx.read(|cx| settings::get(cx).pinned_zone_dismissed);
    said.push(format!(
        "its × pressed, the zone is put away ({away}): {}",
        save_with(drawn, "arc-pinned-put-away", Part::Rows)?
    ));
    // A page's tab taken off its list brings it back, to take it.
    take_up(drawn, row(drawn, 0)?)?;
    let zone = region()?.center();
    carry(drawn, zone)?;
    said.push(format!(
        "a page's tab off its list, the zone is back for it: {}",
        save_with(drawn, "arc-pinned-put-away-tab-in-hand", Part::Rows)?
    ));
    let_go(drawn, zone)?;
    said.push(format!("let go there: pinned {:?}", pinned(drawn)));
    Ok(())
}

/// The pinned and the loose pages, from none to several and back, each state saved whole and
/// the sidebar's head larger.
pub fn pins(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    start(drawn)?;
    pin_by_drop(drawn, said)?;
    loose_pages(drawn, said)?;
    drag_tiles(drawn, said)?;
    across_spaces(drawn, said)?;
    put_away(drawn, said)
}
