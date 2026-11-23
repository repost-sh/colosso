//! The pages that are no checkout's in the Orca layout's top rail (`docs/arc-sidebar.md` 1, 11),
//! drawn as a person uses them: the pinned pages' pill, the space's loose pages as tabs and the
//! hairline before the checkout's tabs, a checkout's tab shown and then a pinned page; a page's
//! tab in hand over the pill, the buttons making room for it where it would land, and let go
//! there; and with none pinned, the zone that says where one goes, its × under the pointer, and,
//! put away, back for a page's tab in hand. Each saved whole, and the rail again larger.

use gpui_kit::{Modifiers, Pixels, Point, point, px};

use super::acts::{Part, save_with, settle};
use super::pins::{carry, let_go, middle, remove_pins, take_up};
use super::web_page;
use crate::settings::{self, Layout};
use crate::theme::UseDarkTheme;
use crate::visual::{Drawn, Result, checkout, noted};

/// The pages pinned, opened in the space and left as the checkout's tabs: their addresses and
/// titles.
const PINNED: [(&str, &str); 3] = [
    ("https://mail.example.com/inbox", "Inbox"),
    ("https://calendar.example.com/week", "Calendar"),
    ("https://github.com/pulls", "Pull requests"),
];
const LOOSE: [(&str, &str); 2] = [
    ("https://news.example.com/", "News"),
    ("https://maps.example.com/", "Maps"),
];
const TABS: [(&str, &str); 2] = [
    ("https://docs.example.com/guide", "Guide"),
    ("http://localhost:3000/", "App"),
];

/// Where the checkout's tab titled `title` was drawn in the rail, by its middle.
fn tab_titled(drawn: &Drawn, title: &str) -> Result<Point<Pixels>> {
    let at = drawn.cx.read(|cx| {
        let unit = drawn.shell.read(cx).workspace.branch()?;
        (unit.tabs.iter()).position(|tab| tab.first().title.as_ref() == title)
    });
    middle(&format!("orca-tab-{}", at.ok_or_else(|| format!("no tab is titled {title}"))?))
}

/// The pinned pages and the space's loose pages, by their titles, in order.
fn listed(drawn: &Drawn) -> (Vec<String>, Vec<String>) {
    drawn.cx.read(|cx| {
        let workspace = &drawn.shell.read(cx).workspace;
        let title = |tab: &crate::model::Tab| tab.first().title.to_string();
        let pinned = workspace.pinned().iter().map(|pinned| title(&pinned.tab));
        let loose = workspace.loose().into_iter().map(|page| title(&page.tab));
        (pinned.collect(), loose.collect())
    })
}

/// The Orca layout, dark, on this checkout's project in the first space: three pages pinned,
/// two loose ones, and two pages among the checkout's tabs, the last of them shown.
fn start(drawn: &mut Drawn) -> Result<()> {
    let (shell, root) = (drawn.shell.clone(), checkout()?);
    drawn.cx.update(|cx| {
        cx.dispatch_action(&UseDarkTheme);
        settings::update(cx, |settings| {
            settings.layout = Layout::Orca;
            settings.pinned_zone_dismissed = false;
        });
        shell.update(cx, |shell, cx| {
            shell.show_space(0, cx);
            if let Some(project) = shell.workspace.project_index(&root) {
                shell.select_project(project, cx);
                shell.workspace.select_worktree(project, &root);
            }
            remove_pins(shell, cx);
            let loose: Vec<u64> = shell.workspace.loose().iter().map(|page| page.key()).collect();
            for key in loose {
                shell.close_loose(key, cx);
            }
            let mut opened = |(address, title): (&str, &str)| {
                let tab = web_page(address, title);
                let key = tab.first().id;
                shell.workspace.push_tab(tab);
                key
            };
            let pinned: Vec<u64> = PINNED.into_iter().map(&mut opened).collect();
            let loose: Vec<u64> = LOOSE.into_iter().map(&mut opened).collect();
            for tab in TABS {
                opened(tab);
            }
            for key in pinned {
                shell.pin_tab_of(key, usize::MAX, cx);
            }
            for key in loose {
                shell.loosen_tab_of(key, usize::MAX, cx);
            }
            if let Some((project, branch, _)) = shell.workspace.shown_tab() {
                let last = (shell.workspace.branch()).map_or(0, |unit| unit.tabs.len() - 1);
                shell.pick_tab(project, branch, last, cx);
            }
        });
    });
    settle(drawn)
}

/// The rail with the pinned pages' pill, the loose pages and the hairline before the
/// checkout's tabs, one of those shown; then the first pinned page picked, its button lit.
fn rail(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    said.push(format!(
        "in Orca, pinned and loose {:?}, a checkout's tab shown: {}",
        listed(drawn),
        save_with(drawn, "orca-rail-dark", Part::Rail)?
    ));
    let window = drawn.window;
    drawn
        .cx
        .simulate_click(window, middle("orca-pinned-0")?, Modifiers::default());
    settle(drawn)?;
    said.push(format!(
        "the first pinned page picked, its button lit: {}",
        save_with(drawn, "orca-rail-pinned-shown-dark", Part::Rail)?
    ));
    Ok(())
}

/// A page's tab of the checkout taken off the rail and brought over the pill between the first
/// two buttons: they make room for it there, the slot lit; let go, it's pinned there.
fn landing(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    take_up(drawn, tab_titled(drawn, TABS[0].1)?)?;
    let between = middle("orca-pinned-1")?;
    for _ in 0..3 {
        carry(drawn, between)?;
    }
    said.push(format!(
        "a page's tab in hand over the pill, between the first two buttons: {}",
        save_with(drawn, "orca-rail-tab-landing-dark", Part::Rail)?
    ));
    let_go(drawn, between)?;
    said.push(format!(
        "let go there, it's pinned second: {:?}; {}",
        listed(drawn).0,
        save_with(drawn, "orca-rail-tab-landed-dark", Part::Rail)?
    ));
    Ok(())
}

/// Every pin removed: the zone in the pill's place, then its × under the pointer; put away, it's
/// back while a page's tab is in hand, saying to let it go there.
fn zone(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    let (shell, window) = (drawn.shell.clone(), drawn.window);
    drawn.cx.update(|cx| shell.update(cx, remove_pins));
    settle(drawn)?;
    said.push(format!(
        "none pinned, the zone in the pill's place: {}",
        save_with(drawn, "orca-rail-zone-dark", Part::Rail)?
    ));
    let zone = noted("orca-pinned").ok_or("the zone isn't drawn")?;
    let end = point(zone.right() - px(14.), zone.center().y);
    (drawn.cx).simulate_mouse_move(window, end, None, Modifiers::default());
    settle(drawn)?;
    said.push(format!(
        "the pointer on the zone, its × shown: {}",
        save_with(drawn, "orca-rail-zone-hovered-dark", Part::Rail)?
    ));
    drawn.cx.simulate_click(window, end, Modifiers::default());
    settle(drawn)?;
    let put_away = drawn.cx.read(|cx| settings::get(cx).pinned_zone_dismissed);
    said.push(format!(
        "its × pressed, put away ({put_away}): {}",
        save_with(drawn, "orca-rail-zone-put-away-dark", Part::Rail)?
    ));
    let tab = tab_titled(drawn, TABS[1].1)?;
    take_up(drawn, tab)?;
    said.push(format!(
        "a page's tab in hand, the zone back for it: {}",
        save_with(drawn, "orca-rail-zone-tab-in-hand-dark", Part::Rail)?
    ));
    carry(drawn, tab)?;
    let_go(drawn, tab)
}

/// The pages that are no checkout's in the Orca layout's top rail, from several to none.
pub fn rail_pages(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    start(drawn)?;
    rail(drawn, said)?;
    landing(drawn, said)?;
    zone(drawn, said)
}
