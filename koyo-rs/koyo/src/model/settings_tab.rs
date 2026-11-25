//! The Settings tab (`docs/sidebar.md` 11.9): Koyo's settings in a tab of their own among
//! the loose pages, one at most. Open, it's shown over whatever tab was, the selected checkout's,
//! a pinned or a loose page, and that one shows again once it closes. It's open only while it's
//! shown: showing anything else closes it. It keeps the pages it showed for Back and Forward, as
//! a page keeps its history. It's no checkout's and no space's, it's never pinned, moved or kept
//! with the window, and closed it isn't among the tabs ⌘⇧T brings back, since it would close
//! again as soon as anything else showed.

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
    tab: Option<SettingsTab>,
    /// The page the last Settings tab showed as it closed.
    last: SettingsPage,
}

impl Settings {
    /// Closes the Settings tab, if it's open: the next opens on the page it showed. Whether it
    /// was open.
    pub(super) fn close(&mut self) -> bool {
        let Some(tab) = self.tab.take() else {
            return false;
        };
        self.last = tab.page;
        true
    }
}

impl Workspace {
    /// The Settings tab, while it's open and so shown.
    pub fn settings(&self) -> Option<&SettingsTab> {
        self.apart.settings.tab.as_ref()
    }

    /// The Settings tab, to show another page in it.
    pub fn settings_mut(&mut self) -> Option<&mut SettingsTab> {
        self.apart.settings.tab.as_mut()
    }

    /// Opens the Settings tab on `page`, over whatever tab is shown, or shows `page` in it if
    /// it's open already. With no page asked for, it stays on the page it shows, or a new one
    /// opens on the page the last one showed. Whether it opened or shows another page.
    pub fn open_settings(&mut self, page: Option<SettingsPage>) -> bool {
        let settings = &mut self.apart.settings;
        match (&mut settings.tab, page) {
            (Some(tab), Some(page)) => tab.go(page),
            (Some(_), None) => false,
            (None, page) => {
                settings.tab = Some(SettingsTab::on(page.unwrap_or(settings.last)));
                true
            }
        }
    }

    /// Closes the Settings tab: the tab it was shown over shows again. Whether it was open.
    pub fn close_settings(&mut self) -> bool {
        self.apart.settings.close()
    }

    /// Puts back `tab`, the Settings tab as it was before an opening out of sight passed through
    /// what it was shown over, which closed it: shown over what's shown, its pages to go back to
    /// with it.
    pub fn bring_back_settings(&mut self, tab: SettingsTab) {
        self.apart.settings.tab = Some(tab);
    }
}

#[cfg(test)]
mod tests;
