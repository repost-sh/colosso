//! The pieces settings pages are built from: the scrolling page column, titled sections of
//! rows, and the buttons a row's control is made of.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::base::ElementExt as _;
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Bounds, ClickEvent, Div, ElementId, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement, Pixels, RenderOnce, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, Window, div, point, px,
};

use super::SettingsPage;
use crate::shell::icon;
use crate::theme::{self, Palette};

/// A settings page: its title over its sections, in a column centered in the card that
/// scrolls when it runs past the bottom.
#[derive(IntoElement)]
pub struct SettingsPageColumn {
    page: SettingsPage,
    sections: Vec<AnyElement>,
}

impl SettingsPageColumn {
    pub fn new(page: SettingsPage) -> Self {
        SettingsPageColumn {
            page,
            sections: Vec::new(),
        }
    }
}

impl ParentElement for SettingsPageColumn {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.sections.extend(elements);
    }
}

impl RenderOnce for SettingsPageColumn {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = theme::palette(cx);
        // Each page keeps a scroll position of its own.
        div()
            .id(("settings-page", self.page as usize))
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .w_full()
                    .max_w(px(theme::SETTINGS_PAGE_WIDTH))
                    .mx_auto()
                    .px_8()
                    .pt(px(64.))
                    .pb(px(48.))
                    .flex()
                    .flex_col()
                    .gap(px(36.))
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text)
                            .child(self.page.title()),
                    )
                    .children(self.sections),
            )
    }
}

/// A titled card of setting rows, a hairline between each two.
#[derive(IntoElement)]
pub struct SettingsSection {
    title: SharedString,
    rows: Vec<SettingRow>,
}

impl SettingsSection {
    pub fn new(title: impl Into<SharedString>) -> Self {
        SettingsSection {
            title: title.into(),
            rows: Vec::new(),
        }
    }

    pub fn row(mut self, row: SettingRow) -> Self {
        self.rows.push(row);
        self
    }

    pub fn rows(mut self, rows: impl IntoIterator<Item = SettingRow>) -> Self {
        self.rows.extend(rows);
        self
    }

    /// Whether the section's title or any of its rows mentions `query`, which must already be
    /// lowercase.
    pub fn matches(&self, query: &str) -> bool {
        let mentions = |text: &str| text.to_lowercase().contains(query);
        mentions(&self.title)
            || self
                .rows
                .iter()
                .any(|row| mentions(&row.title) || row.description.as_deref().is_some_and(mentions))
    }
}

impl RenderOnce for SettingsSection {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = theme::palette(cx);
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(self.title),
            )
            .child(
                div()
                    .rounded(px(10.))
                    .border_1()
                    .border_color(p.hairline)
                    .bg(p.raised)
                    .flex()
                    .flex_col()
                    .children(self.rows.into_iter().enumerate().map(|(index, row)| {
                        div()
                            .when(index > 0, |row| {
                                row.child(div().mx_4().h(px(1.)).bg(p.hairline))
                            })
                            .child(row)
                    })),
            )
    }
}

/// One setting: its name and what it does on the left, the controls that change it on the
/// right.
#[derive(IntoElement)]
pub struct SettingRow {
    title: SharedString,
    description: Option<SharedString>,
    controls: Vec<AnyElement>,
}

impl SettingRow {
    pub fn new(title: impl Into<SharedString>) -> Self {
        SettingRow {
            title: title.into(),
            description: None,
            controls: Vec::new(),
        }
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Adds a control after any added before it.
    pub fn control(mut self, control: impl IntoElement) -> Self {
        self.controls.push(control.into_any_element());
        self
    }
}

impl RenderOnce for SettingRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = theme::palette(cx);
        div()
            .min_h(px(56.))
            .px_4()
            .py(px(10.))
            .flex()
            .items_center()
            .gap_6()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().text_sm().child(self.title))
                    .when_some(self.description, |text, description| {
                        text.child(div().text_xs().text_color(p.muted).child(description))
                    }),
            )
            .when(!self.controls.is_empty(), |row| {
                row.child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .gap_2()
                        .children(self.controls),
                )
            })
    }
}

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A small rounded button with a text label, for actions a setting offers.
#[derive(IntoElement)]
pub struct PillButton {
    id: ElementId,
    label: SharedString,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

impl PillButton {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        PillButton {
            id: id.into(),
            label: label.into(),
            disabled: false,
            on_click: None,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for PillButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = theme::palette(cx);
        pill(self.id, self.disabled, p)
            .when_some(self.on_click.filter(|_| !self.disabled), |pill, handler| {
                pill.on_click(move |event, window, cx| handler(event, window, cx))
            })
            .child(self.label)
    }
}

/// A pill naming the current choice; pressing it lists the choices in a native menu below it.
#[derive(IntoElement)]
pub struct MenuButton {
    id: ElementId,
    label: SharedString,
    menu: Rc<dyn Fn(&App) -> NativeMenu>,
}

impl MenuButton {
    /// `menu` builds the menu when it opens, so its checkmarks show the choice at that moment.
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        menu: impl Fn(&App) -> NativeMenu + 'static,
    ) -> Self {
        MenuButton {
            id: id.into(),
            label: label.into(),
            menu: Rc::new(menu),
        }
    }
}

impl RenderOnce for MenuButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = theme::palette(cx);
        let bounds = Rc::new(Cell::new(Bounds::<Pixels>::default()));
        let measured = bounds.clone();
        let menu = self.menu;
        pill(self.id, false, p)
            .gap_1()
            .pr(px(6.))
            .on_click(move |_, window, cx| {
                let below = bounds.get().bottom_left() + point(px(0.), px(4.));
                menu(cx).show(below, window, cx);
            })
            .child(self.label)
            .child(icon(IconName::ChevronDown, 12.).text_color(p.muted))
            .on_prepaint(move |bounds, _, _| measured.set(bounds))
    }
}

fn pill(id: ElementId, disabled: bool, p: Palette) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(26.))
        .px(px(10.))
        .flex()
        .items_center()
        .rounded(px(7.))
        .border_1()
        .border_color(p.hairline)
        .bg(p.raised)
        .text_sm()
        .when(disabled, |pill| pill.text_color(p.muted))
        .when(!disabled, |pill| {
            pill.cursor_pointer().hover(|style| style.bg(p.selected))
        })
}
