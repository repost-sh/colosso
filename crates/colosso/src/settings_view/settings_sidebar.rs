//! The settings sidebar: a search field over the pages, grouped under headings. Picking a
//! page navigates to it, so Back returns to the page before.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Context, Entity, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, div, px,
};

use super::{SIDEBAR_GROUPS, SettingsPage};
use crate::navigation::{Navigate, NavigationHistory, Route};
use crate::shell::{icon, row, section_label};
use crate::theme;

pub struct SettingsSidebar {
    history: Entity<NavigationHistory>,
    search: Entity<InputState>,
    query: SharedString,
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
            query: SharedString::default(),
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
                self.query = search.read(cx).value();
                cx.notify();
            }
            InputEvent::PressEnter { .. } => {
                if let Some(page) = self.matching_pages().next() {
                    window.dispatch_action(Box::new(Navigate(Route::Settings(page))), cx);
                }
            }
            _ => {}
        }
    }

    fn matching_pages(&self) -> impl Iterator<Item = SettingsPage> + '_ {
        SIDEBAR_GROUPS
            .iter()
            .flat_map(|(_, pages)| pages.iter().copied())
            .filter(|page| page.matches(&self.query))
    }
}

impl Render for SettingsSidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let selected = match self.history.read(cx).current() {
            Route::Settings(page) => Some(page),
            Route::Workspace => None,
        };
        let groups: Vec<_> = SIDEBAR_GROUPS
            .iter()
            .filter_map(|(heading, pages)| {
                let pages: Vec<_> = pages
                    .iter()
                    .copied()
                    .filter(|page| page.matches(&self.query))
                    .collect();
                (!pages.is_empty()).then_some((*heading, pages))
            })
            .collect();
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
                        .child(format!("No settings match “{}”.", self.query.trim())),
                )
            })
    }
}
