//! The window: translucent rails around a rounded content card, in Orca or Arc layout.

use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, App, AppContext as _, Context, Div, DragMoveEvent, ElementId, Empty, FocusHandle,
    InteractiveElement as _, IntoElement, KeyBinding, MouseButton, MouseDownEvent,
    ParentElement as _, Render, SharedString, Stateful, StatefulInteractiveElement as _,
    Styled as _, Window, actions, div, ease_in_out, px,
};

use crate::menu::{self, MenuItem};
use crate::model::{PaneKind, Workspace};
use crate::theme::{CycleTheme, Palette, ThemePreference};
use crate::{arc, orca, pane, theme};

/// How long the sidebar takes to slide fully in or out.
const SIDEBAR_SLIDE: Duration = Duration::from_millis(180);

actions!(
    colosso,
    [
        ToggleSidebar,
        ToggleSidebarMode,
        ToggleRightPanel,
        ToggleBottomPanel,
        NewChat,
        NewTerminal,
        NewBrowserTab,
        NewEditorTab
    ]
);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-s", ToggleSidebar, None),
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
    /// Where the sidebar was and when it started sliding toward `sidebar_open`.
    sidebar_slide: Option<(f32, Instant)>,
    pub sidebar_width: f32,
    right_panel_open: bool,
    bottom_panel_open: bool,
    /// Horizontal travel of a two-finger swipe in progress over the Arc sidebar.
    pub swipe: Option<f32>,
    pub focus: FocusHandle,
}

impl Shell {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Shell {
            workspace: Workspace::sample(),
            mode: SidebarMode::Orca,
            sidebar_open: true,
            sidebar_slide: None,
            sidebar_width: theme::SIDEBAR_WIDTH,
            right_panel_open: false,
            bottom_panel_open: false,
            swipe: None,
            focus,
        }
    }

    fn toggle_sidebar(&mut self, _: &ToggleSidebar, _: &mut Window, cx: &mut Context<Self>) {
        // Turning around mid-slide starts from wherever the sidebar is now.
        let (reveal, _) = self.sidebar_reveal();
        self.sidebar_open = !self.sidebar_open;
        self.sidebar_slide = Some((reveal, Instant::now()));
        cx.notify();
    }

    /// How far out the sidebar is, from 0 hidden to 1 fully out, and whether it is still sliding.
    fn sidebar_reveal(&self) -> (f32, bool) {
        let target = if self.sidebar_open { 1. } else { 0. };
        let Some((from, start)) = self.sidebar_slide else {
            return (target, false);
        };
        let progress = start.elapsed().as_secs_f32() / SIDEBAR_SLIDE.as_secs_f32();
        if progress >= 1. {
            return (target, false);
        }
        (from + (target - from) * ease_in_out(progress), true)
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

    fn new_tab(&mut self, kind: PaneKind, cx: &mut Context<Self>) {
        self.workspace.new_tab(kind);
        cx.notify();
    }

    fn resize_sidebar(
        &mut self,
        event: &DragMoveEvent<SidebarResize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Both layouts start the sidebar right after the icon rail.
        let width = (event.event.position.x.as_f32() - theme::ICON_RAIL_WIDTH)
            .clamp(theme::SIDEBAR_MIN_WIDTH, theme::SIDEBAR_MAX_WIDTH);
        if width != self.sidebar_width {
            self.sidebar_width = width;
            cx.notify();
        }
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let orca = self.mode == SidebarMode::Orca;
        let (reveal, sliding) = self.sidebar_reveal();
        if sliding {
            window.request_animation_frame();
        } else {
            self.sidebar_slide = None;
        }
        let sidebar_shown = reveal > 0.;
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
            .when(orca && sidebar_shown, |card| {
                card.child(slide(orca::sidebar(self, cx), self.sidebar_width, reveal))
            })
            .child(main)
            .when(self.right_panel_open, |card| card.child(right_panel(p)));

        div()
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::toggle_sidebar_mode))
            .on_action(cx.listener(Self::toggle_right_panel))
            .on_action(cx.listener(Self::toggle_bottom_panel))
            .on_action(cx.listener(|this, _: &NewChat, _, cx| this.new_tab(PaneKind::Chat, cx)))
            .on_action(
                cx.listener(|this, _: &NewTerminal, _, cx| this.new_tab(PaneKind::Terminal, cx)),
            )
            .on_action(
                cx.listener(|this, _: &NewBrowserTab, _, cx| this.new_tab(PaneKind::Browser, cx)),
            )
            .on_action(
                cx.listener(|this, _: &NewEditorTab, _, cx| this.new_tab(PaneKind::Editor, cx)),
            )
            .on_drag_move(cx.listener(Self::resize_sidebar))
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
                    .when(!orca && sidebar_shown, |body| {
                        body.child(slide(arc::sidebar(self, cx), self.sidebar_width, reveal))
                    })
                    .child(card),
            )
    }
}

/// Shows the revealed part of a `width`-wide sidebar, sliding it out from under the left edge
/// so its contents keep their layout while it moves.
fn slide(sidebar: impl IntoElement, width: f32, reveal: f32) -> impl IntoElement {
    div()
        .w(px(width * reveal))
        .flex_shrink_0()
        .h_full()
        .flex()
        .overflow_hidden()
        .child(
            div()
                .flex_shrink_0()
                .h_full()
                .relative()
                .left(px(width * (reveal - 1.)))
                .child(sidebar),
        )
}

/// The drag payload while the sidebar's right border is being dragged.
pub struct SidebarResize;

impl Render for SidebarResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// A grab strip along the sidebar's right border; the sidebar must be `relative`.
pub fn sidebar_resize_handle() -> impl IntoElement {
    div()
        .id("sidebar-resize")
        .absolute()
        .top_0()
        .bottom_0()
        .right_0()
        .w(px(6.))
        .cursor_col_resize()
        .on_drag(SidebarResize, |_, _, _, cx| cx.new(|_| SidebarResize))
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

fn new_tab_action(kind: PaneKind) -> Box<dyn Action> {
    match kind {
        PaneKind::Chat => Box::new(NewChat),
        PaneKind::Terminal => Box::new(NewTerminal),
        PaneKind::Browser => Box::new(NewBrowserTab),
        PaneKind::Editor => Box::new(NewEditorTab),
    }
}

/// Opens a menu under `trigger` offering a tab of each kind for the selected branch unit.
pub fn new_tab_menu(
    id: impl Into<ElementId>,
    trigger: Button,
    target: FocusHandle,
) -> impl IntoElement {
    menu::dropdown(id, trigger, target, || {
        PaneKind::ALL
            .into_iter()
            .map(|kind| MenuItem {
                icon: kind.icon(),
                label: kind.new_tab_label().into(),
                action: new_tab_action(kind),
            })
            .collect()
    })
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

pub fn icon_button(
    id: impl Into<ElementId>,
    icon: IconName,
    tooltip: impl Into<SharedString>,
) -> Button {
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
