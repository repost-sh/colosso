//! The Settings tab's content (`docs/arc-sidebar.md` 11.9): Settings' own pages listed at its
//! left, a search field over them, grouped under headings, and the page picked beside them.
//! Picking a page shows it in the tab, which keeps the page before for Back. It's content, drawn
//! on the card as a tab's is, not the sidebar's glass.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, App, AppContext as _, Context, Div, Entity, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, Stateful, StatefulInteractiveElement as _,
    Styled as _, Subscription, Window, div, px,
};

use super::{PAGE_GROUPS, SettingsPage};
use crate::model::SettingsTab;
use crate::shell::{icon, row, section_label};
use crate::theme;

/// How wide the list of pages is.
const WIDTH: f32 = 220.;

/// Shows a page of Settings in the Settings tab, as its link in the list does.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct ShowPage(pub SettingsPage);

/// The list of pages' search field, a view of its own so the window drawing the Settings tab
/// again doesn't build it again; and what's typed in it, which the list keeps between the times
/// the tab is open.
pub struct PageList {
    search: Entity<InputState>,
    /// The search text, trimmed and lowercased for matching.
    query: String,
    _search: Subscription,
}

impl PageList {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let _search = cx.subscribe_in(&search, window, Self::on_search_event);
        PageList {
            search,
            query: String::new(),
            _search,
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
                    window.dispatch_action(Box::new(ShowPage(page)), cx);
                }
            }
            _ => {}
        }
    }

    /// The list's headings with the pages under each that match the search, leaving out
    /// headings with none.
    fn matching_groups(&self, cx: &App) -> Vec<(&'static str, Vec<SettingsPage>)> {
        PAGE_GROUPS
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

impl Render for PageList {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
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
            .child(Input::new(&self.search).appearance(false).flex_1())
    }
}

/// The Settings tab's content on `page`: the pages `list` lists at its left, and the page beside
/// them.
pub fn tab(list: &Entity<PageList>, page: SettingsPage, cx: &App) -> impl IntoElement {
    div()
        .size_full()
        .flex()
        .child(pages(list, page, cx))
        .child(div().flex_1().min_w_0().h_full().child(super::page(page, cx)))
}

/// The list of pages, `page` picked, under `list`'s search field.
fn pages(list: &Entity<PageList>, page: SettingsPage, cx: &App) -> Stateful<Div> {
    let p = theme::palette(cx);
    let groups = list.read(cx).matching_groups(cx);
    let nothing_found = groups.is_empty();
    div()
        .id("settings-pages")
        .w(px(WIDTH))
        .h_full()
        .flex_shrink_0()
        .p_2()
        .border_r_1()
        .border_color(p.hairline)
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
                .child(SettingsTab::TITLE),
        )
        .child(list.clone())
        .children(groups.into_iter().map(|(heading, listed)| {
            div()
                .mt_3()
                .flex()
                .flex_col()
                .gap_px()
                .child(section_label(p).child(heading))
                .children(listed.into_iter().map(|listed| {
                    let (picked, at) = (listed == page, listed as usize);
                    row(("settings-page-link", at), picked, p)
                        .debug_selector(move || format!("settings-page-link-{at}"))
                        .when(!picked, |row| row.text_color(p.text))
                        .on_click(move |_, window, cx| {
                            window.dispatch_action(Box::new(ShowPage(listed)), cx);
                        })
                        .child(icon(listed.icon(), 14.).text_color(p.muted))
                        .child(listed.title())
                }))
        }))
        .when(nothing_found, |pages| {
            pages.child(
                div()
                    .mt_3()
                    .px_2()
                    .text_sm()
                    .text_color(p.muted)
                    .child(format!(
                        "No settings match “{}”.",
                        list.read(cx).search.read(cx).value().trim()
                    )),
            )
        })
}
