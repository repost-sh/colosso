//! The window: translucent rails around a rounded content card, in Orca or Arc layout. The
//! sidebar slot and the card show whichever route the navigation history is on: the
//! workspace, or a settings page.

use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::component::{Disableable as _, Icon, Selectable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, AnyElement, App, AppContext as _, Context, Div, DragMoveEvent, ElementId, Entity,
    FocusHandle, InteractiveElement as _, IntoElement, KeyBinding, MouseButton, MouseDownEvent,
    ParentElement as _, Pixels, Point, Render, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, actions, div,
    ease_in_out, px,
};

use crate::menu::{self, MenuItem};
use crate::model::{PaneKind, Workspace};
use crate::navigation::{
    CloseSettings, GoBack, GoForward, Navigate, NavigationHistory, OpenSettings, Route,
};
use crate::settings::{self, Layout};
use crate::settings_view::{self, SettingsSidebar};
use crate::sidebar::{SidebarFrame, SidebarResize, slide_out};
use crate::theme::{CycleTheme, Palette, ThemePreference};
use crate::{arc, backdrop, gradient, orca, pane, picker, theme};

/// How long the sidebar takes to slide fully in or out.
const SIDEBAR_SLIDE: Duration = Duration::from_millis(180);

actions!(
    colosso,
    [
        ToggleSidebar,
        ToggleSidebarMode,
        UseOrcaLayout,
        UseArcLayout,
        ToggleRightPanel,
        ToggleBottomPanel,
        NewChat,
        NewTerminal,
        NewBrowserTab,
        NewEditorTab,
        EditTheme
    ]
);

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-s", ToggleSidebar, None),
        KeyBinding::new("cmd-alt-b", ToggleRightPanel, None),
        KeyBinding::new("cmd-j", ToggleBottomPanel, None),
        KeyBinding::new("cmd-t", NewChat, None),
        KeyBinding::new("cmd-shift-m", ToggleSidebarMode, None),
    ]);
    // The layout is a setting, so these work from any window or menu.
    cx.on_action(|_: &UseOrcaLayout, cx| set_layout(Layout::Orca, cx));
    cx.on_action(|_: &UseArcLayout, cx| set_layout(Layout::Arc, cx));
    cx.on_action(|_: &ToggleSidebarMode, cx| {
        set_layout(
            match settings::get(cx).layout {
                Layout::Orca => Layout::Arc,
                Layout::Arc => Layout::Orca,
            },
            cx,
        )
    });
}

fn set_layout(layout: Layout, cx: &mut App) {
    settings::update(cx, |settings| settings.layout = layout);
}

impl Layout {
    /// Switches to this layout from a menu.
    pub fn action(self) -> Box<dyn Action> {
        match self {
            Layout::Orca => Box::new(UseOrcaLayout),
            Layout::Arc => Box::new(UseArcLayout),
        }
    }
}

pub struct Shell {
    pub workspace: Workspace,
    history: Entity<NavigationHistory>,
    settings_sidebar: Entity<SettingsSidebar>,
    sidebar_open: bool,
    /// Where the sidebar was and when it started sliding toward `sidebar_open`.
    sidebar_slide: Option<(f32, Instant)>,
    right_panel_open: bool,
    bottom_panel_open: bool,
    /// Horizontal travel of a two-finger swipe in progress over the Arc sidebar.
    pub swipe: Option<f32>,
    pub focus: FocusHandle,
    /// Where the rails were last right-clicked, for the theme picker their menu opens.
    rails_menu_at: Point<Pixels>,
    _subscriptions: [Subscription; 2],
}

impl Shell {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let history = cx.new(|_| NavigationHistory::new());
        let settings_sidebar = cx.new(|cx| SettingsSidebar::new(history.clone(), window, cx));
        let _subscriptions = [
            settings::observe(cx),
            cx.observe(&history, |_, _, cx| cx.notify()),
        ];
        Shell {
            workspace: Workspace::sample(),
            history,
            settings_sidebar,
            sidebar_open: settings::get(cx).sidebar_open_on_launch,
            sidebar_slide: None,
            right_panel_open: false,
            bottom_panel_open: false,
            swipe: None,
            focus,
            rails_menu_at: Point::default(),
            _subscriptions,
        }
    }

    fn layout(cx: &App) -> Layout {
        settings::get(cx).layout
    }

    fn sidebar_width(cx: &App) -> f32 {
        settings::get(cx)
            .sidebar_width
            .clamp(theme::SIDEBAR_MIN_WIDTH, theme::SIDEBAR_MAX_WIDTH)
    }

    fn route(&self, cx: &App) -> Route {
        self.history.read(cx).current()
    }

    fn navigate(&mut self, route: Route, cx: &mut Context<Self>) {
        self.history.update(cx, |history, cx| {
            if history.push(route) {
                cx.notify();
            }
        });
    }

    fn open_settings(&mut self, _: &OpenSettings, _: &mut Window, cx: &mut Context<Self>) {
        let page = self.history.read(cx).last_settings_page();
        self.navigate(Route::Settings(page), cx);
    }

    fn go_back(&mut self, _: &GoBack, _: &mut Window, cx: &mut Context<Self>) {
        self.history.update(cx, |history, cx| {
            if history.go_back() {
                cx.notify();
            }
        });
    }

    fn go_forward(&mut self, _: &GoForward, _: &mut Window, cx: &mut Context<Self>) {
        self.history.update(cx, |history, cx| {
            if history.go_forward() {
                cx.notify();
            }
        });
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
        // A new tab is something to look at, so leave settings for it.
        self.navigate(Route::Workspace, cx);
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
        settings::update(cx, |settings| settings.sidebar_width = width);
    }

    fn show_rails_menu(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        picker::close(cx);
        self.rails_menu_at = event.position;
        let other_layout = match Self::layout(cx) {
            Layout::Orca => "Switch to Arc Layout",
            Layout::Arc => "Switch to Orca Layout",
        };
        NativeMenu::new()
            .menu("Edit Theme…", Box::new(EditTheme))
            .submenu("Appearance", appearance_menu(theme::preference(cx)))
            .separator()
            .menu("Toggle Sidebar", Box::new(ToggleSidebar))
            .menu(other_layout, Box::new(ToggleSidebarMode))
            .separator()
            .menu("Settings…", Box::new(OpenSettings))
            .show(event.position, window, cx);
    }

    fn top_rail(&self, cx: &App) -> impl IntoElement {
        let history = self.history.read(cx);
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
            .child(
                action_button("back", IconName::ArrowLeft, "Back", GoBack)
                    .disabled(!history.can_go_back()),
            )
            .child(
                action_button("forward", IconName::ArrowRight, "Forward", GoForward)
                    .disabled(!history.can_go_forward()),
            )
            .child(drag_area("title").flex_1())
            .child(action_button(
                "toggle-right",
                IconName::PanelRight,
                "Toggle right panel",
                ToggleRightPanel,
            ))
    }

    fn icon_rail(&self, cx: &App) -> impl IntoElement {
        let in_settings = matches!(self.route(cx), Route::Settings(_));
        let other_layout = match Self::layout(cx) {
            Layout::Orca => "Switch to Arc layout",
            Layout::Arc => "Switch to Orca layout",
        };
        div()
            .w(px(theme::ICON_RAIL_WIDTH))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .pb(px(theme::GUTTER))
            .child(
                icon_button("rail-chats", IconName::MessagesSquare, "Chats")
                    .selected(!in_settings)
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(Navigate(Route::Workspace)), cx)
                    }),
            )
            .child(icon_button(
                "rail-changes",
                IconName::GitPullRequest,
                "Changes",
            ))
            .child(div().flex_1())
            .child(action_button(
                "rail-mode",
                IconName::PanelsTopLeft,
                other_layout,
                ToggleSidebarMode,
            ))
            .child(theme_toggle(theme::preference(cx)))
            .child(
                action_button("rail-settings", IconName::Settings, "Settings", OpenSettings)
                    .selected(in_settings),
            )
    }

    /// What the sidebar slot holds on the current route.
    fn sidebar(&self, route: Route, layout: Layout, cx: &mut Context<Self>) -> AnyElement {
        let frame = SidebarFrame::new(layout, Self::sidebar_width(cx));
        match (route, layout) {
            (Route::Settings(_), _) => frame.child(self.settings_sidebar.clone()),
            (Route::Workspace, Layout::Orca) => frame.child(orca::sidebar(self, cx)),
            (Route::Workspace, Layout::Arc) => frame.child(arc::sidebar(self, cx)),
        }
        .into_any_element()
    }

    /// What the card shows beside the sidebar on the current route.
    fn main(
        &self,
        route: Route,
        layout: Layout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = theme::palette(cx);
        let column = div().flex_1().min_w_0().flex().flex_col();
        match route {
            Route::Settings(page) => column
                .child(settings_view::page(page, window, cx))
                .into_any_element(),
            Route::Workspace => column
                .when(layout == Layout::Orca, |main| main.child(orca::tab_strip(self, cx)))
                .child(self.workspace_content(p))
                .when(self.bottom_panel_open, |main| main.child(bottom_panel(p)))
                .into_any_element(),
        }
    }

    fn workspace_content(&self, p: Palette) -> impl IntoElement {
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
        let route = self.route(cx);
        let layout = Self::layout(cx);
        let width = Self::sidebar_width(cx);
        let (reveal, sliding) = self.sidebar_reveal();
        if sliding {
            window.request_animation_frame();
        } else {
            self.sidebar_slide = None;
        }
        let sidebar = (reveal > 0.).then(|| slide_out(self.sidebar(route, layout, cx), width, reveal));
        // Orca keeps the sidebar inside the card, Arc on the glass beside it.
        let (card_sidebar, glass_sidebar) = match layout {
            Layout::Orca => (sidebar, None),
            Layout::Arc => (None, sidebar),
        };
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
            .children(card_sidebar)
            .child(self.main(route, layout, window, cx))
            .when(route == Route::Workspace && self.right_panel_open, |card| {
                card.child(right_panel(p))
            })
            // The rails' menu belongs to the rails, not the card.
            .on_mouse_down(MouseButton::Right, |_, _, cx| {
                picker::close(cx);
                cx.stop_propagation();
            });

        div()
            .track_focus(&self.focus)
            .when(matches!(route, Route::Settings(_)), |root| {
                root.key_context("Settings")
            })
            .on_action(cx.listener(|this, Navigate(route), _, cx| this.navigate(*route, cx)))
            .on_action(cx.listener(Self::open_settings))
            .on_action(
                cx.listener(|this, _: &CloseSettings, _, cx| this.navigate(Route::Workspace, cx)),
            )
            .on_action(cx.listener(Self::go_back))
            .on_action(cx.listener(Self::go_forward))
            .on_action(cx.listener(Self::toggle_sidebar))
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
            .on_mouse_down(MouseButton::Left, |_, _, cx| picker::close(cx))
            .on_action(cx.listener(|this, _: &EditTheme, window, cx| {
                picker::open(this.rails_menu_at, window, cx)
            }))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::show_rails_menu))
            .size_full()
            .relative()
            .flex()
            .flex_col()
            // A gradient theme paints the glass instead of the plain tint.
            .when(gradient::current(cx).dots.is_empty(), |root| root.bg(p.rail_tint))
            .text_color(p.text)
            .child(backdrop::backdrop(cx))
            .child(self.top_rail(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.icon_rail(cx))
                    .children(glass_sidebar)
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
                // This menu stands in for the one the rest of the rails open.
                cx.stop_propagation();
                appearance_menu(preference).show(event.position, window, cx);
            },
        )
        .child(action_button(
            "rail-theme",
            preference.icon(),
            format!("Theme: {}", preference.label()),
            CycleTheme,
        ))
}

/// System, Light and Dark, with `preference` checked.
fn appearance_menu(preference: ThemePreference) -> NativeMenu {
    ThemePreference::ALL
        .into_iter()
        .fold(NativeMenu::new(), |menu, option| {
            menu.menu_with_check(option.label(), option == preference, option.action())
        })
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

/// The × that closes the tab at `(project, branch, tab)` without selecting it.
pub fn tab_close_button(
    id: impl Into<ElementId>,
    (project, branch, tab): (usize, usize, usize),
    p: Palette,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(18.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .hover(|style| style.bg(p.hover))
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.workspace.close_tab(project, branch, tab);
            cx.notify();
        }))
        .child(icon(IconName::X, 12.))
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
