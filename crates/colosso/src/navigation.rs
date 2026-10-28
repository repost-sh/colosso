//! Where the window is: the workspace or a settings page, with back and forward history
//! like a browser's. The shell owns the history; views move through it with actions.

use gpui_kit::base::History;
use gpui_kit::{Action, App, KeyBinding, actions};

use crate::settings_view::SettingsPage;

/// How many steps back the history remembers.
const HISTORY_LIMIT: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    Workspace,
    Settings(SettingsPage),
}

actions!(navigation, [GoBack, GoForward, OpenSettings, CloseSettings]);

/// Goes to `0`, recording where the window was so Back returns there.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = navigation, no_json)]
pub struct Navigate(pub Route);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-[", GoBack, None),
        KeyBinding::new("cmd-]", GoForward, None),
        KeyBinding::new("escape", CloseSettings, Some("Settings")),
    ]);
}

pub struct NavigationHistory {
    routes: History<Route>,
    /// The settings page Open Settings returns to.
    last_settings_page: SettingsPage,
}

impl NavigationHistory {
    pub fn new() -> Self {
        let mut routes = History::new().max_entries(HISTORY_LIMIT);
        routes.push(Route::Workspace);
        NavigationHistory {
            routes,
            last_settings_page: SettingsPage::General,
        }
    }

    pub fn current(&self) -> Route {
        // Never empty: it starts on the workspace and Back keeps the first route.
        self.routes.current().copied().unwrap_or(Route::Workspace)
    }

    pub fn last_settings_page(&self) -> SettingsPage {
        self.last_settings_page
    }

    pub fn can_go_back(&self) -> bool {
        self.routes.can_back()
    }

    pub fn can_go_forward(&self) -> bool {
        self.routes.can_forward()
    }

    /// Goes to `route` and drops the forward history; returns whether the route changed.
    pub fn push(&mut self, route: Route) -> bool {
        if route == self.current() {
            return false;
        }
        self.routes.push(route);
        self.arrived(route);
        true
    }

    pub fn go_back(&mut self) -> bool {
        self.routes
            .back()
            .map(|route| self.arrived(route))
            .is_some()
    }

    pub fn go_forward(&mut self) -> bool {
        self.routes
            .forward()
            .map(|route| self.arrived(route))
            .is_some()
    }

    fn arrived(&mut self, route: Route) {
        if let Route::Settings(page) = route {
            self.last_settings_page = page;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn back_and_forward_retrace_the_pushed_routes() {
        let mut history = NavigationHistory::new();
        let appearance = Route::Settings(SettingsPage::Appearance);
        assert!(history.push(Route::Settings(SettingsPage::General)));
        assert!(history.push(appearance));
        assert!(history.go_back());
        assert_eq!(history.current(), Route::Settings(SettingsPage::General));
        assert!(history.go_back());
        assert_eq!(history.current(), Route::Workspace);
        assert!(!history.go_back());
        assert!(history.go_forward());
        assert!(history.go_forward());
        assert_eq!(history.current(), appearance);
        assert!(!history.can_go_forward());
    }

    #[test]
    fn pushing_drops_the_forward_history() {
        let mut history = NavigationHistory::new();
        history.push(Route::Settings(SettingsPage::About));
        history.go_back();
        assert!(history.can_go_forward());
        history.push(Route::Settings(SettingsPage::General));
        assert!(!history.can_go_forward());
    }

    #[test]
    fn pushing_the_current_route_records_nothing() {
        let mut history = NavigationHistory::new();
        assert!(!history.push(Route::Workspace));
        assert!(!history.can_go_back());
    }

    #[test]
    fn open_settings_returns_to_the_last_page_shown() {
        let mut history = NavigationHistory::new();
        history.push(Route::Settings(SettingsPage::KeyboardShortcuts));
        history.push(Route::Workspace);
        assert_eq!(
            history.last_settings_page(),
            SettingsPage::KeyboardShortcuts
        );
    }
}
