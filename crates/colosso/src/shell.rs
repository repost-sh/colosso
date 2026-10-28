//! The window: translucent rails around a rounded content card, in Orca or Arc layout.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, App, Context, Div, ElementId, FocusHandle, InteractiveElement as _, IntoElement,
    KeyBinding, MouseButton, MouseDownEvent, ParentElement as _, Render, SharedString, Stateful,
    Styled as _, Window, actions, div, px,
};

use crate::model::Workspace;
use crate::theme::{CycleTheme, Palette, ThemePreference};
use crate::{arc, orca, pane, theme};

actions!(
    colosso,
    [
        ToggleSidebar,
        ToggleSidebarMode,
        ToggleRightPanel,
        ToggleBottomPanel,
        NewChat
    ]
);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-b", ToggleSidebar, None),
        KeyBinding::new("cmd-alt-b", ToggleRightPanel, None),
        KeyBinding::new("cmd-j", ToggleBottomPanel, None),
        KeyBinding::new("cmd-t", NewChat, None),
        KeyBinding::new("cmd-shift-m", ToggleSidebarMode, None),
    ]);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SidebarMode {
    Orca,
    Arc,
}

pub struct Shell {
    pub workspace: Workspace,
    mode: SidebarMode,
    sidebar_open: bool,
    right_panel_open: bool,
    bottom_panel_open: bool,
    /// Horizontal travel of a two-finger swipe in progress over the Arc sidebar.
    pub swipe: Option<f32>,
    focus: FocusHandle,
}

impl Shell {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Shell {
            workspace: Workspace::sample(),
            mode: SidebarMode::Orca,
            sidebar_open: true,
            right_panel_open: false,
            bottom_panel_open: false,
            swipe: None,
            focus,
        }
    }

    fn toggle_sidebar(&mut self, _: &ToggleSidebar, _: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_open = !self.sidebar_open;
        cx.notify();
    }

    fn toggle_sidebar_mode(
        &mut self,
        _: &ToggleSidebarMode,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.mode = match self.mode {
            SidebarMode::Orca => SidebarMode::Arc,
            SidebarMode::Arc => SidebarMode::Orca,
        };
        cx.notify();
    }

    fn toggle_right_panel(&mut self, _: &ToggleRightPanel, _: &mut Window, cx: &mut Context<Self>) {
        self.right_panel_open = !self.right_panel_open;
        cx.notify();
    }

    fn toggle_bottom_panel(
        &mut self,
        _: &ToggleBottomPanel,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.bottom_panel_open = !self.bottom_panel_open;
        cx.notify();
    }

    fn new_chat(&mut self, _: &NewChat, _: &mut Window, cx: &mut Context<Self>) {
        self.workspace.new_chat();
        cx.notify();
    }

    fn top_rail(&self) -> impl IntoElement {
        div()
            .h(px(theme::TITLE_BAR_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_1()
            .pr(px(theme::GUTTER))
            .child(drag_area("traffic-lights").w(px(theme::TRAFFIC_LIGHTS_WIDTH)))
            .child(action_button(
                "toggle-sidebar",
                IconName::PanelLeft,
                "Toggle sidebar",
                ToggleSidebar,
            ))
            .child(icon_button("back", IconName::ArrowLeft, "Back"))
            .child(icon_button("forward", IconName::ArrowRight, "Forward"))
            .child(drag_area("title").flex_1())
            .child(action_button(
                "toggle-bottom",
                IconName::PanelBottom,
                "Toggle bottom panel",
                ToggleBottomPanel,
            ))
            .child(action_button(
                "toggle-right",
                IconName::PanelRight,
                "Toggle right panel",
                ToggleRightPanel,
            ))
    }

    fn icon_rail(&self, cx: &App) -> impl IntoElement {
        let other_mode = match self.mode {
            SidebarMode::Orca => "Switch to Arc layout",
            SidebarMode::Arc => "Switch to Orca layout",
        };
        div()
            .w(px(theme::ICON_RAIL_WIDTH))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .pb(px(theme::GUTTER))
            .child(icon_button("rail-chats", IconName::MessagesSquare, "Chats"))
            .child(icon_button(
                "rail-changes",
                IconName::GitPullRequest,
                "Changes",
            ))
            .child(div().flex_1())
            .child(action_button(
                "rail-mode",
                IconName::PanelsTopLeft,
                other_mode,
                ToggleSidebarMode,
            ))
            .child(theme_toggle(theme::preference(cx)))
            .child(icon_button("rail-settings", IconName::Settings, "Settings"))
    }

    fn content(&self, p: Palette) -> impl IntoElement {
        let workspace = &self.workspace;
        let branch = workspace.branch();
        let context = format!("{} / {}", workspace.project().name, branch.name);
        div()
            .flex_1()
            .min_h_0()
            .child(match branch.tabs.get(branch.active_tab) {
                Some(tab) => pane::tab(tab, context.into(), p),
                None => div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(p.muted)
                    .child("No tabs on this branch. ⌘T starts a chat.")
                    .into_any_element(),
            })
    }
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let orca = self.mode == SidebarMode::Orca;
        let main = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .when(orca, |main| main.child(orca::tab_strip(self, cx)))
            .child(self.content(p))
            .when(self.bottom_panel_open, |main| main.child(bottom_panel(p)));
        let card = div()
            .flex_1()
            .min_w_0()
            .mr(px(theme::GUTTER))
            .mb(px(theme::GUTTER))
            .rounded(px(theme::CARD_RADIUS))
            .border_1()
            .border_color(p.hairline)
            .bg(p.card)
            .overflow_hidden()
            .flex()
            .when(orca && self.sidebar_open, |card| {
                card.child(orca::sidebar(self, cx))
            })
            .child(main)
            .when(self.right_panel_open, |card| card.child(right_panel(p)));

        div()
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::toggle_sidebar_mode))
            .on_action(cx.listener(Self::toggle_right_panel))
            .on_action(cx.listener(Self::toggle_bottom_panel))
            .on_action(cx.listener(Self::new_chat))
            .size_full()
            .flex()
            .flex_col()
            .bg(p.rail_tint)
            .text_color(p.text)
            .child(self.top_rail())
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.icon_rail(cx))
                    .when(!orca && self.sidebar_open, |body| {
                        body.child(arc::sidebar(self, cx))
                    })
                    .child(card),
            )
    }
}

/// Click cycles System, Light and Dark; right-click picks one from a native menu.
fn theme_toggle(preference: ThemePreference) -> impl IntoElement {
    div()
        .on_mouse_down(
            MouseButton::Right,
            move |event: &MouseDownEvent, window, cx| {
                ThemePreference::ALL
                    .into_iter()
                    .fold(NativeMenu::new(), |menu, option| {
                        menu.menu_with_check(option.label(), option == preference, option.action())
                    })
                    .show(event.position, window, cx);
            },
        )
        .child(action_button(
            "rail-theme",
            preference.icon(),
            format!("Theme: {}", preference.label()),
            CycleTheme,
        ))
}

/// Empty rail space that moves the window, since the app owns titlebar dragging.
fn drag_area(id: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .h_full()
        .on_mouse_down(MouseButton::Left, |event: &MouseDownEvent, window, _| {
            if event.click_count == 2 {
                window.titlebar_double_click();
            } else {
                window.start_window_move();
            }
        })
}

pub fn icon_button(id: &'static str, icon: IconName, tooltip: impl Into<SharedString>) -> Button {
    Button::new(id).ghost().icon(icon).tooltip(tooltip)
}

/// A button that dispatches `action`; its tooltip shows the action's keybinding.
pub fn action_button(
    id: &'static str,
    icon: IconName,
    tooltip: impl Into<SharedString>,
    action: impl Action,
) -> Button {
    Button::new(id)
        .ghost()
        .icon(icon)
        .tooltip_with_action(tooltip, &action, None)
        .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
}

pub fn icon(name: IconName, size: f32) -> Icon {
    Icon::new(name).size(px(size))
}

/// A selectable row: highlighted when selected, tinted on hover otherwise.
pub fn row(id: impl Into<ElementId>, selected: bool, p: Palette) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(theme::ROW_HEIGHT))
        .flex_shrink_0()
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .rounded(px(6.))
        .text_sm()
        .cursor_pointer()
        .when(selected, |row| row.bg(p.selected))
        .when(!selected, |row| row.hover(|style| style.bg(p.hover)))
}

pub fn search_row(p: Palette) -> Div {
    div()
        .h(px(theme::ROW_HEIGHT))
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .text_sm()
        .text_color(p.muted)
        .child(icon(IconName::Search, 14.))
        .child("Search")
}

pub fn section_label(p: Palette) -> Div {
    div()
        .h(px(24.))
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .text_xs()
        .text_color(p.muted)
}

pub fn ellipsized(text: impl IntoElement) -> Div {
    div().min_w_0().truncate().child(text)
}

fn right_panel(p: Palette) -> impl IntoElement {
    let files = [
        (0., "src/"),
        (12., "ledger.rs"),
        (12., "payouts.rs"),
        (0., "Cargo.toml"),
        (0., "README.md"),
    ];
    div()
        .w(px(260.))
        .flex_shrink_0()
        .h_full()
        .p_2()
        .border_l_1()
        .border_color(p.hairline)
        .flex()
        .flex_col()
        .gap_1()
        .child(section_label(p).child("Files"))
        .children(files.iter().map(|(indent, file)| {
            div()
                .px_2()
                .pl(px(8. + indent))
                .h(px(24.))
                .flex()
                .items_center()
                .text_sm()
                .child(*file)
        }))
}

fn bottom_panel(p: Palette) -> impl IntoElement {
    div()
        .h(px(200.))
        .flex_shrink_0()
        .border_t_1()
        .border_color(p.hairline)
        .p_3()
        .font_family(theme::MONO)
        .text_sm()
        .text_color(p.muted)
        .bg(p.sunken)
        .child("Terminal")
}
