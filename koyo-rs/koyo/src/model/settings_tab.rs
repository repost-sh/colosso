//! The Settings tab (`docs/sidebar.md` 11.9): Koyo's settings in a tab of their own among
//! the loose pages, one at most. Opened or picked, it's shown over whatever tab was, the selected
//! checkout's, a pinned or a loose page, and that one shows again once it closes. Anything else
//! shown leaves it open behind, where its tab picks it again. It keeps the pages it showed for
//! Back and Forward, as a page keeps its history. It's no checkout's and no space's, it's never
//! pinned, moved or kept with the window, and closed it isn't among the tabs ⌘⇧T brings back.

use gpui_kit::assets::IconName;

use super::Workspace;
use crate::navigation::SettingsPage;

/// How many pages back a Settings tab remembers.
const PAGES_LIMIT: usize = 50;

/// The Settings tab: the page it shows, and the pages Back and Forward go to.
#[derive(Clone, Debug)]
pub struct SettingsTab {
    page: SettingsPage,
    /// The pages it showed before, the latest last.
    back: Vec<SettingsPage>,
    /// The pages Back left, the nearest last.
    forward: Vec<SettingsPage>,
}

impl SettingsTab {
    /// What its row says.
    pub const TITLE: &str = "Settings";
    /// What its row shows where a page's own icon goes.
    pub const ICON: IconName = IconName::Settings;

    fn on(page: SettingsPage) -> Self {
        SettingsTab {
            page,
            back: Vec::new(),
            forward: Vec::new(),
        }
    }

    /// The page it shows.
    pub fn page(&self) -> SettingsPage {
        self.page
    }

    /// Shows `page`, forgetting the pages Forward went to. Whether that's another page.
    pub fn go(&mut self, page: SettingsPage) -> bool {
        if page == self.page {
            return false;
        }
        self.back.push(std::mem::replace(&mut self.page, page));
        if self.back.len() > PAGES_LIMIT {
            self.back.remove(0);
        }
        self.forward.clear();
        true
    }

    /// Shows the page it showed before this one, as Back does. False at the first.
    pub fn back(&mut self) -> bool {
        let Some(page) = self.back.pop() else {
            return false;
        };
        self.forward.push(std::mem::replace(&mut self.page, page));
        true
    }

    /// Shows the page Back left, as Forward does. False with none.
    pub fn forward(&mut self) -> bool {
        let Some(page) = self.forward.pop() else {
            return false;
        };
        self.back.push(std::mem::replace(&mut self.page, page));
        true
    }
}

/// The Settings tab while it's open, and the page the next one opens on.
#[derive(Clone, Debug, Default)]
pub(super) struct Settings {
    open: Option<Open>,
    /// The page the last Settings tab showed as it closed.
    last: SettingsPage,
}

/// The Settings tab, open.
#[derive(Clone, Debug)]
struct Open {
    tab: SettingsTab,
    /// Whether it's shown, over the tab it covers, rather than open behind it.
    shown: bool,
}

impl Settings {
    /// Leaves the Settings tab, if it's shown, for what it covers: it stays open behind.
    pub(super) fn leave(&mut self) {
        if let Some(open) = &mut self.open {
            open.shown = false;
        }
    }
}

impl Workspace {
    /// The Settings tab, while it's open, shown or not.
    pub fn settings(&self) -> Option<&SettingsTab> {
        self.apart.settings.open.as_ref().map(|open| &open.tab)
    }

    /// The Settings tab, while it's the tab shown.
    pub fn settings_shown(&self) -> Option<&SettingsTab> {
        (self.apart.settings.open.as_ref())
            .filter(|open| open.shown)
            .map(|open| &open.tab)
    }

    /// The Settings tab while it's the tab shown, to show another page in it.
    pub fn settings_shown_mut(&mut self) -> Option<&mut SettingsTab> {
        (self.apart.settings.open.as_mut())
            .filter(|open| open.shown)
            .map(|open| &mut open.tab)
    }

    /// Shows the Settings tab over whatever tab is shown, opened if it isn't open, on `page`. With
    /// no page asked for, it's on the page it showed, or a new one on the page the last one
    /// showed. Whether it wasn't shown on that page already.
    pub fn open_settings(&mut self, page: Option<SettingsPage>) -> bool {
        let settings = &mut self.apart.settings;
        let Some(open) = &mut settings.open else {
            let tab = SettingsTab::on(page.unwrap_or(settings.last));
            settings.open = Some(Open { tab, shown: true });
            return true;
        };
        let came = !std::mem::replace(&mut open.shown, true);
        page.is_some_and(|page| open.tab.go(page)) || came
    }

    /// Leaves the Settings tab, if it's shown, for the tab it covers: it stays open behind.
    pub fn leave_settings(&mut self) {
        self.apart.settings.leave();
    }

    /// Closes the Settings tab: the tab it covered, if it was shown, shows again, and the next
    /// Settings tab opens on the page this one showed. Whether it was open.
    pub fn close_settings(&mut self) -> bool {
        let settings = &mut self.apart.settings;
        let Some(open) = settings.open.take() else {
            return false;
        };
        settings.last = open.tab.page;
        true
    }
}

#[cfg(test)]
mod tests;
