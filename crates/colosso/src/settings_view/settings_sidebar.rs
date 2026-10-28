//! The settings sidebar: a search field over the pages, grouped under headings. Picking a
//! page navigates to it, so Back returns to the page before.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Subscription, Window,
    div, px,
};

use super::{SIDEBAR_GROUPS, SettingsPage};
use crate::navigation::{Navigate, NavigationHistory, Route};
use crate::shell::{icon, row, section_label};
use crate::theme;

pub struct SettingsSidebar {
    history: Entity<NavigationHistory>,
    search: Entity<InputState>,
    /// The search text, trimmed and lowercased for matching.
    query: String,
    _subscriptions: [Subscription; 2],
}

impl SettingsSidebar {
    pub fn new(
        history: Entity<NavigationHistory>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let _subscriptions = [
            cx.observe(&history, |_, _, cx| cx.notify()),
            cx.subscribe_in(&search, window, Self::on_search_event),
        ];
        SettingsSidebar {
            history,
            search,
            query: String::new(),
            _subscriptions,
        }
    }

    /// Typing filters the pages; Enter opens the first page left.
    fn on_search_event(
        &mut self,
        search: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                self.query = search.read(cx).value().trim().to_lowercase();
                cx.notify();
            }
            InputEvent::PressEnter { .. } => {
                let first = self.matching_groups(cx).first().map(|(_, pages)| pages[0]);
                if let Some(page) = first {
                    window.dispatch_action(Box::new(Navigate(Route::Settings(page))), cx);
                }
            }
            _ => {}
        }
    }

    /// The sidebar's headings with the pages under each that match the search, leaving out
    /// headings with none.
    fn matching_groups(&self, cx: &App) -> Vec<(&'static str, Vec<SettingsPage>)> {
        SIDEBAR_GROUPS
            .iter()
            .filter_map(|(heading, pages)| {
                let pages: Vec<_> = pages
                    .iter()
                    .copied()
                    .filter(|page| page.matches(&self.query, cx))
                    .collect();
                (!pages.is_empty()).then_some((*heading, pages))
            })
            .collect()
    }
}

impl Render for SettingsSidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let selected = match self.history.read(cx).current() {
            Route::Settings(page) => Some(page),
            Route::Workspace => None,
        };
        let groups = self.matching_groups(cx);
        let nothing_found = groups.is_empty();

        div()
            .id("settings-sidebar")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_px()
            .child(
                div()
                    .h(px(28.))
                    .px_2()
                    .flex()
                    .items_center()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Settings"),
            )
            .child(
                div()
                    .h(px(theme::ROW_HEIGHT))
                    .flex_shrink_0()
                    .pl_2()
                    .flex()
                    .items_center()
                    .gap_1()
                    .rounded(px(8.))
                    .bg(p.hover)
                    .text_sm()
                    .child(icon(IconName::Search, 14.).text_color(p.muted))
                    .child(Input::new(&self.search).appearance(false).flex_1()),
            )
            .children(groups.into_iter().map(|(heading, pages)| {
                div()
                    .mt_3()
                    .flex()
                    .flex_col()
                    .gap_px()
                    .child(section_label(p).child(heading))
                    .children(pages.into_iter().map(|page| {
                        let is_selected = selected == Some(page);
                        row(("settings-page-link", page as usize), is_selected, p)
                            .when(!is_selected, |row| row.text_color(p.text))
                            .on_click(move |_, window, cx| {
                                window
                                    .dispatch_action(Box::new(Navigate(Route::Settings(page))), cx)
                            })
                            .child(icon(page.icon(), 14.).text_color(p.muted))
                            .child(page.title())
                    }))
            }))
            .when(nothing_found, |sidebar| {
                sidebar.child(
                    div()
                        .mt_3()
                        .px_2()
                        .text_sm()
                        .text_color(p.muted)
                        .child(format!(
                            "No settings match “{}”.",
                            self.search.read(cx).value().trim()
                        )),
                )
            })
    }
}
