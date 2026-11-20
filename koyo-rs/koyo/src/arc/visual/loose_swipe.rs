//! The loose pages under two fingers moving to the next space (`docs/arc-sidebar.md` 11.1,
//! 11.4), the swipe caught half-way: each space's own, they slide out with it; shared, they stay
//! where they are under the pinned pages as the space under them slides, and the page shown
//! stays shown once the next space is in.

use gpui_kit::{
    Bounds, Modifiers, Pixels, ScrollDelta, ScrollEvent, TouchPhase, point, px,
};

use super::acts::{Part, save_with, settle};
use super::web_page;
use crate::model::Loose;
use crate::settings::{self, LoosePages};
use crate::visual::{Drawn, Result, checkout, noted};

/// Where the first loose page's row was drawn.
fn first_row() -> Result<Bounds<Pixels>> {
    noted("arc-loose-0").ok_or_else(|| "no loose page's row is drawn".into())
}

/// Two fingers on the trackpad over the pinned pages, in `phase`, moved `dx` across.
fn fingers(drawn: &mut Drawn, phase: TouchPhase, dx: f32) -> Result<()> {
    let over = noted("arc-pinned").ok_or("the pinned pages aren't drawn")?;
    let event = ScrollEvent {
        position: over.center(),
        delta: ScrollDelta::Pixels(point(px(dx), px(0.))),
        modifiers: Modifiers::none(),
        touch_phase: phase,
        ..ScrollEvent::default()
    };
    drawn.cx.simulate_event(drawn.window, event);
    drawn.draw()
}

/// What the panes show, by its title, and whether it's a loose page.
fn showing(drawn: &Drawn) -> (String, bool) {
    drawn.cx.read(|cx| {
        let workspace = &drawn.shell.read(cx).workspace;
        let title = (workspace.tab_shown()).map(|tab| tab.first().title.to_string());
        (title.unwrap_or_default(), workspace.loose_shown().is_some())
    })
}

/// The first space shown, with two loose pages of its own at least, the first of them shown.
fn start(drawn: &mut Drawn) -> Result<()> {
    let (shell, root) = (drawn.shell.clone(), checkout()?);
    drawn.cx.update(|cx| {
        shell.update(cx, |shell, cx| {
            shell.show_space(0, cx);
            for (address, title) in [
                ("https://radio.example.com/", "Radio"),
                ("https://notes.example.com/", "Notes"),
            ] {
                let page = Loose {
                    tab: web_page(address, title),
                    project: root.clone(),
                };
                shell.workspace.add_loose(page, usize::MAX, false);
            }
            shell.workspace.show_loose(0);
            cx.notify();
        });
    });
    settle(drawn)
}

/// The fingers half-way to the next space, with the loose pages listed as `pages`, saved as
/// `arc-loose-<name>-mid-swipe`; then let go, on in the next space, saved as
/// `arc-loose-<name>-next-space`; then back on the first.
fn swipe(drawn: &mut Drawn, pages: LoosePages, name: &str, said: &mut Vec<String>) -> Result<()> {
    let shell = drawn.shell.clone();
    drawn.cx.update(|cx| {
        settings::update(cx, |settings| settings.loose_pages = pages);
    });
    settle(drawn)?;
    let rest = first_row()?;
    fingers(drawn, TouchPhase::Started, 0.)?;
    for _ in 0..4 {
        fingers(drawn, TouchPhase::Moved, -30.)?;
    }
    let along = drawn.cx.read(|cx| shell.read(cx).swipe.offset);
    let mid = first_row()?;
    let saved = save_with(drawn, &format!("arc-loose-{name}-mid-swipe"), Part::Rows)?;
    said.push(format!(
        "{name}: the space {along:.0} pt along under two fingers, the first loose row from x {} to x {}, showing {:?}: {saved}",
        rest.origin.x.as_f32(),
        mid.origin.x.as_f32(),
        showing(drawn),
    ));
    fingers(drawn, TouchPhase::Ended, 0.)?;
    settle(drawn)?;
    settle(drawn)?;
    let space = drawn.cx.read(|cx| settings::get(cx).space);
    let saved = save_with(drawn, &format!("arc-loose-{name}-next-space"), Part::Rows)?;
    said.push(format!(
        "{name}: let go, on space {space}, showing {:?}: {saved}",
        showing(drawn)
    ));
    drawn.cx.update(|cx| shell.update(cx, |shell, cx| shell.slide_to_space(0, cx)));
    settle(drawn)?;
    settle(drawn)?;
    said.push(format!(
        "{name}: back on the first space, showing {:?}",
        showing(drawn)
    ));
    if space != 1 {
        return Err(format!("{name}: the swipe didn't go on to the next space"));
    }
    Ok(())
}

/// The loose pages mid-swipe per space and shared, then per space again.
pub fn loose_swipe(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    start(drawn)?;
    swipe(drawn, LoosePages::PerSpace, "per-space", said)?;
    swipe(drawn, LoosePages::Shared, "shared", said)?;
    drawn.cx.update(|cx| {
        settings::update(cx, |settings| settings.loose_pages = LoosePages::PerSpace);
    });
    settle(drawn)
}
