//! The shell's side of the pinned pages (`docs/arc-sidebar.md` 11), tiles at the Arc sidebar's
//! head and buttons in the Orca layout's top rail: their region in either layout (`region.rs`),
//! a page's tab pinned as it's dropped on it, where they made room for it, a pinned page shown
//! as its tile is picked, moved among them, closed and still pinned, pinned at another address,
//! unpinned onto a checkout's tabs or among the space's loose pages, or removed, as its menu
//! offers (`menu.rs`); and the zone that says where one goes while none is, which the person can
//! put away. What a pinned page's own events do goes as a checkout's page's do
//! (`browser_tabs.rs`).

use koyo_browser_ipc::Session;
use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Context, Div, FontWeight, IntoElement, ParentElement as _, SharedString, Styled as _, div,
    img, px,
};

use super::splits::{Dragged, DraggedPane};
use super::tab_face::FAVICON_CORNERS;
use super::{Shell, icon};
use crate::browser;
use crate::browser::profiles::By;
use crate::history;
use crate::model::{Pane, Unloaded};
use crate::settings;
use crate::theme::Palette;

mod menu;
mod region;

pub use menu::CopyToWorktree;
#[cfg(test)]
pub use menu::{ClosePinned, PinnedMenu, RemovePinned, RepinAtCurrent, Unpin};
pub use region::{hides_pinned_zone, pinned_zone_says};

/// How much of its strength the picture of a pinned page that isn't open keeps.
const CLOSED: f32 = 0.6;

/// The first letter of the host of the page at `address`, which a site with no icon shows.
fn initial(address: &str) -> Option<SharedString> {
    let host = history::host(address)?;
    let first = host.chars().find(|first| first.is_alphanumeric())?;
    Some(first.to_uppercase().collect::<String>().into())
}

/// A pinned page's picture, `size` points across, on its tile in the Arc sidebar and its button
/// in the Orca rail: its site's icon, its corners a little rounded; with none, its host's
/// initial; a globe for a page with no host, as a new tab's start page. Fainter while its page
/// isn't open, closed or not opened since the window came back.
pub fn pinned_picture(page: &Pane, size: f32, p: Palette) -> Div {
    let picture = if let Some(favicon) = page.favicon() {
        img(favicon)
            .size(px(size))
            .rounded(px(FAVICON_CORNERS))
            .into_any_element()
    } else {
        let muted = div().text_color(p.muted);
        match initial(page.address().unwrap_or_default()) {
            Some(initial) => muted
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .child(initial)
                .into_any_element(),
            None => muted.child(icon(IconName::Globe, size)).into_any_element(),
        }
    };
    let open = page.view().is_some();
    div().when(!open, |face| face.opacity(CLOSED)).child(picture)
}

impl Shell {
    /// Where the checkout's tab holding pane `pane` is, as `(project, branch, tab)`, when that's
    /// its single pane and a page, not a page's `DevTools` in a tab of their own: what the
    /// pinned and the loose pages take from a checkout's list.
    pub(crate) fn page_tab_of(&self, pane: u64) -> Option<(usize, usize, usize)> {
        let (at, _) = self.workspace.find_pane(|found| found.id == pane)?;
        let tools = (self.workspace.tab(at)).is_some_and(|tab| tab.first().is_inspector());
        (self.workspace.is_page_tab(at) && !tools).then_some(at)
    }

    /// Whether what's in hand is something the pinned pages take, once it has left where it was
    /// taken from: what their region lights up for, and what a zone put away shows for. They
    /// take a page's tab off a checkout's list and a loose page's row off theirs; not while it's
    /// still on its list.
    pub(crate) fn pinned_taking(&self) -> bool {
        (self.tab_hand.left_list()).is_some_and(|pane| {
            self.workspace.loose_at(pane).is_some() || self.page_tab_of(pane).is_some()
        })
    }

    /// Whether what's in hand is something the Arc sidebar's loose pages take, once it has left
    /// where it was taken from: what their rows light up for, and what their strip shows for.
    /// They take a page's tab off a checkout's list (`loose_taking_tab`), and a pinned page's
    /// tile well away from the others; not while it's still on the grid it was taken from.
    pub(crate) fn loose_taking(&self) -> bool {
        self.loose_taking_tab() || self.pinned_hand.left_grid()
    }

    /// Whether a page's tab is in hand that has left the checkout's list it was taken from: what
    /// the loose pages take, along the top rail and down the Arc sidebar, and what the spot that
    /// takes the first of them shows for. Not while it's still on its list.
    pub(crate) fn loose_taking_tab(&self) -> bool {
        (self.tab_hand.left_list()).is_some_and(|pane| {
            self.workspace.loose_at(pane).is_none() && self.page_tab_of(pane).is_some()
        })
    }

    /// Pins the tab holding pane `pane`, a checkout's or a loose page's, at the address its page
    /// shows, in slot `place` among those pinned already or after them: it goes on showing if it
    /// was the tab shown. Whether it was one a pin takes.
    pub(crate) fn pin_tab_of(&mut self, pane: u64, place: usize, cx: &mut Context<Self>) -> bool {
        let home = (self.workspace.pane(pane)).and_then(Pane::address);
        let home = home.unwrap_or_default().to_owned();
        let pinned = if let Some(at) = self.workspace.loose_at(pane) {
            self.workspace.pin_loose(at, home, place)
        } else {
            let tab = self.page_tab_of(pane);
            tab.and_then(|at| self.workspace.pin_tab(at, home, place))
        };
        if pinned.is_some() {
            cx.notify();
        }
        pinned.is_some()
    }

    /// What's in `held` let go on the pinned pages' region, in either layout: a page's tab or a
    /// loose page is pinned where the tiles made room for it, or after them, as on the zone
    /// that's there while none is pinned.
    fn pin_dropped(&mut self, held: &DraggedPane, cx: &mut Context<Self>) {
        if let (Dragged::Tab(..) | Dragged::Loose, [face]) = (held.what, held.faces.as_slice()) {
            let place = self.pinned_hand.lands().unwrap_or(usize::MAX);
            self.pin_tab_of(face.id, place, cx);
        }
    }

    /// Whether the pinned pages' region shows, in either layout: their tiles while any is
    /// pinned, else the zone that says where one goes, unless the person put it away
    /// (`settings.json` `pinned_zone_dismissed`) and nothing it takes is in hand, as `taking`
    /// says.
    fn lists_pinned(&self, taking: bool, cx: &App) -> bool {
        !self.workspace.pinned().is_empty() || taking || !settings::get(cx).pinned_zone_dismissed
    }

    /// Puts away the zone that says where a pinned page goes, as its × does: it's back only
    /// while a page's tab is in hand, until one is pinned.
    fn dismiss_pinned_zone(cx: &mut App) {
        settings::update(cx, |settings| settings.pinned_zone_dismissed = true);
    }

    /// A pinned page's tile let go over another slot: it moves there among them.
    pub(crate) fn move_pinned(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        if self.workspace.move_pinned(from, to) {
            cx.notify();
        }
    }

    /// The selected checkout, as `(project, branch)`.
    pub(crate) fn checkout_shown(&self) -> Option<(usize, usize)> {
        let (project, branch, _) = self.workspace.shown_tab()?;
        Some((project, branch))
    }

    /// Unpins the page known by `key`, as its menu does: onto the selected checkout, after its
    /// tabs, shown there; with no checkout selected, as in a space with no project, among the
    /// space's loose pages.
    pub(crate) fn unpin(&mut self, key: u64, cx: &mut Context<Self>) {
        match self.checkout_shown() {
            Some(to) => self.unpin_onto(key, to, usize::MAX, cx),
            None => self.loosen_pinned(key, usize::MAX, cx),
        }
    }

    /// Unpins the page known by `key` onto the checkout `to`, as `(project, branch)`, at `place`
    /// among its tabs, as its tile's let go there: a tab of that checkout, shown.
    pub(crate) fn unpin_onto(
        &mut self,
        key: u64,
        to: (usize, usize),
        place: usize,
        cx: &mut Context<Self>,
    ) {
        let at = self.workspace.pinned_at(key);
        if at.is_some_and(|at| self.workspace.unpin(at, to, place)) {
            cx.notify();
        }
    }

    /// Closes the pinned page known by `key`, as ⌘W and its menu's Close do: its page and its
    /// `DevTools` go, and it stays pinned, to open again at the address it's pinned at once its
    /// tile is picked. Shown, what the space shows under it shows in its place.
    pub(crate) fn close_pinned(&mut self, key: u64, cx: &mut Context<Self>) {
        self.let_apart_devtools_go(key, cx);
        let Some(at) = self.workspace.pinned_at(key) else {
            return;
        };
        let home = self.workspace.pinned()[at].home.clone();
        let (title, closed) = self.rested(at, home, cx);
        self.workspace.close_pinned_page(at, title, closed);
        cx.notify();
    }

    /// What the pane of the pinned page at `at` is titled and holds once it has no page of its
    /// own and opens at `address`: its own title and icon while that's the page it shows, else
    /// the address's, and the icon kept for its site.
    fn rested(&self, at: usize, address: String, cx: &mut App) -> (SharedString, Unloaded) {
        let page = self.workspace.pinned()[at].tab.first();
        let (title, favicon) = if page.address() == Some(address.as_str()) {
            (page.title.clone(), page.favicon())
        } else {
            let rested = browser::page_brought_back(&address, "", By::Person, Session::default(), cx);
            (rested.title.clone(), rested.favicon())
        };
        let session = Session::default();
        (title, Unloaded { address, favicon, restored: true, session })
    }

    /// Whether the pinned page known by `key` has something to close: a page open, or, brought
    /// back with the window and not opened yet, an address other than the one it's pinned at.
    pub(crate) fn pinned_is_open(&self, key: u64) -> bool {
        self.workspace.pinned_of(key).is_some_and(|pinned| {
            let page = pinned.tab.first();
            page.view().is_some() || page.address() != Some(pinned.home.as_str())
        })
    }

    /// The address the pinned page known by `key` shows, when it isn't the one it's pinned at:
    /// what Replace Pinned URL with Current pins it at.
    pub(crate) fn pinned_strayed(&self, key: u64) -> Option<String> {
        let pinned = self.workspace.pinned_of(key)?;
        let shows = pinned.tab.first().address()?;
        (!shows.is_empty() && shows != pinned.home).then(|| shows.to_owned())
    }

    /// The pinned page known by `key` is pinned at the address it shows from now on.
    pub(crate) fn repin_at_current(&mut self, key: u64, cx: &mut Context<Self>) {
        if let Some(shows) = self.pinned_strayed(key) {
            self.repin_at(key, shows, cx);
        }
    }

    /// The pinned page known by `key` is pinned at `address` from now on, as typed in an
    /// address or as a page shows it. Its page, open, stays where it is; closed, it opens there.
    pub(crate) fn repin_at(&mut self, key: u64, address: String, cx: &mut Context<Self>) {
        let Some(at) = self.workspace.pinned_at(key) else {
            return;
        };
        if !self.workspace.set_pinned_home(at, address.clone()) {
            return;
        }
        if self.workspace.pinned()[at].tab.first().view().is_none() {
            let (title, rested) = self.rested(at, address, cx);
            self.workspace.rest_pinned(at, title, rested);
        }
        cx.notify();
    }

    /// Removes the pinned page known by `key`: it's pinned no longer, its page and its
    /// `DevTools` gone with it.
    pub(crate) fn remove_pinned(&mut self, key: u64, cx: &mut Context<Self>) {
        self.let_apart_devtools_go(key, cx);
        if let Some(at) = self.workspace.pinned_at(key) {
            self.workspace.remove_pinned(at);
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::initial;

    #[test]
    fn a_site_with_no_icon_shows_its_hosts_first_letter() {
        let letter = |address| initial(address).map(|letter| letter.to_string());
        assert_eq!(letter("https://www.example.com/docs").as_deref(), Some("E"));
        assert_eq!(letter("http://localhost:3000/").as_deref(), Some("L"));
        assert_eq!(letter("https://127.0.0.1:8080/").as_deref(), Some("1"));
        assert_eq!(letter("about:blank"), None, "no host: a globe");
        assert_eq!(letter(""), None);
    }
}
