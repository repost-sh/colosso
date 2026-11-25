//! The rails round the islands: the top rail, with room for the traffic lights and the window's
//! buttons, across the window while the tabs are on it, over the sidebar on the glass and the
//! islands, after them the pinned pages, the space's loose pages and the selected checkout's tabs
//! (`top_rail::pages`, `top_rail::tabs`), and the selected worktree's Changes and Files buttons at
//! its end; with the tabs in the sidebar, the sidebar's top row over the sidebar alone, where the
//! islands rise beside it to the gutter along the window's top, as they stop at the one along its
//! bottom; and the status bar along the bottom. Down the left there's only the gutter, as on the
//! right: settings are in the app's menu, ⌘, opens them. ⌘B folds them away with the sidebar into
//! compact mode, as the Arc browser's: every edge then as thin as the gutter right of the islands,
//! and the traffic lights gone; but for the top rail, which holds the tabs and stays. They move on
//! springs, all at once, and turn round mid-way, keeping their speed, when ⌘B is pressed again;
//! with Reduce Motion on in System Settings they fold and come back at once (gpui-base follows it).
//! The status bar is only tall while the sidebar is out and it has language servers to show.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::base::motion::{Spring, spring};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnimationPhase, AnyElement, App, Context, Div, InteractiveElement as _, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div, px,
};

use super::{Shell, ToggleSidebar, action_button, drag_area, sidebar_menu, with_menu};
use crate::browser::DownloadsButton;
use crate::settings;
use crate::theme;

/// How the rails move: without overshooting, as a rail can be neither thinner than the gutter
/// nor wider than itself, and quickly, felt over about 0.12 s.
const MOTION: Spring = Spring::new(Duration::from_millis(120));

/// How far out the top rail is once it's too thin to hold the traffic lights: where they're
/// gone.
const LIGHTS_GONE: f32 = (theme::TRAFFIC_LIGHTS_TOP + theme::TRAFFIC_LIGHT_SIZE - theme::GUTTER)
    / (theme::TITLE_BAR_HEIGHT - theme::GUTTER);

/// How big the glyphs of the top rail's Changes and Files buttons are.
const RAIL_GLYPH: f32 = 16.;

/// The least of the top rail left empty after its tabs, to move the window by and double-click: the
/// tabs narrow before it does.
const EMPTY_RAIL: f32 = 96.;

/// How strong the traffic lights are while the pointer isn't on them, as Arc dims its own, and how
/// long they take to come up or settle back.
const LIGHTS_RESTING: f32 = 0.4;
const LIGHTS_EASE: Duration = Duration::from_millis(160);

/// The room the traffic lights sit in: empty rail that moves the window, which notes whether the
/// pointer's on it.
fn lights_area(hover: impl Fn(&bool, &mut Window, &mut App) + 'static) -> gpui_kit::Stateful<Div> {
    drag_area("traffic-lights")
        .w(px(theme::TRAFFIC_LIGHTS_WIDTH))
        .on_hover(hover)
}

/// What the rails follow besides the sidebar.
#[derive(Default)]
pub(super) struct Rails {
    /// Whether the selected checkout has language servers to show, which the status bar is
    /// tall for.
    pub servers: bool,
}

/// Where the rails are this frame, each from as thin as the gutter (0) to its full size (1), and
/// where the tabs have them run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Shown {
    /// How far out the sidebar is.
    pub open: AnimationPhase,
    /// What runs along the window's top as the tabs have it (`Shell::frame`): the top rail,
    /// always out, as it holds the tabs, or the sidebar's top row, out with the sidebar.
    pub top: AnimationPhase,
    /// The status bar.
    pub status: AnimationPhase,
    /// Whether the tabs are in the sidebar, which leaves no top rail: the sidebar's top row runs
    /// along the window's top instead (`Settings::tabs_in_sidebar`).
    pub in_sidebar: bool,
}

/// A window edge a rail runs along.
#[derive(Clone, Copy)]
enum Edge {
    Top,
    Bottom,
}

/// How much of the traffic lights shows with the top rail `open` this far: they fade as it
/// folds, gone once it's too thin to hold them.
fn lights(open: AnimationPhase) -> f32 {
    open.interpolate_between_clamped(LIGHTS_GONE..=1., 0., 1.)
}

/// How thick a rail of `full` thickness is `shown` this far out, from the gutter's thickness.
fn thickness(full: f32, shown: AnimationPhase) -> f32 {
    shown.interpolate(theme::GUTTER, full)
}

/// The gutter down the window's left, beside the sidebar or the card.
fn gutter() -> Div {
    div().w(px(theme::GUTTER)).flex_shrink_0()
}

/// How wide the left gutter and the `sidebar`-wide sidebar, left of the chat, are together once
/// they've settled with the sidebar `open`.
pub(super) fn left_of_chat(open: bool, sidebar: f32) -> f32 {
    let shown = AnimationPhase::from(open);
    theme::GUTTER + shown.interpolate(0., sidebar)
}

/// A rail along `edge`, `shown` this far from the gutter's thickness to `full`, holding what
/// `content` makes laid out at `full`. The content keeps to the card's side of the rail, so as
/// the rail thins it slides out of the window, fading as it goes; once thin, the rail is bare.
fn rail(edge: Edge, full: f32, shown: AnimationPhase, content: impl FnOnce() -> Div) -> Div {
    let thick = thickness(full, shown);
    let rail = div()
        .flex_shrink_0()
        .overflow_hidden()
        .flex()
        .flex_col()
        .h(px(thick));
    if shown.0 <= 0. {
        return rail;
    }
    let past = px(thick - full);
    let content = content().flex_shrink_0().relative().opacity(shown.0);
    rail.child(match edge {
        Edge::Top => content.h(px(full)).top(past),
        Edge::Bottom => content.h(px(full)),
    })
}

impl Shell {
    /// The window between the status bar and its top, with the `sidebar` and the `card` beside
    /// it, as the tabs have it (`docs/layout.md`, Address bar). On the top rail, the rail runs
    /// across the window over the sidebar on the glass and the card beside it. In the sidebar
    /// there's no top rail: the sidebar's top row is over the sidebar alone, as wide as the gutter
    /// and the sidebar out as far as it's `shown`, and the card is beside them from the gutter
    /// along the window's top, which moves the window as the row does.
    pub(super) fn frame(
        &self,
        shown: Shown,
        (sidebar, card): (Option<AnyElement>, Div),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let top = self.top_rail(shown, window, cx);
        let body = div().flex_1().min_h_0().flex().child(gutter()).children(sidebar);
        if shown.in_sidebar {
            let left = theme::GUTTER + self.sidebar_width(cx) * shown.open.0;
            let side = div()
                .w(px(left))
                .flex_shrink_0()
                .flex()
                .flex_col()
                .child(top)
                .child(body);
            let over_card = drag_area("card-top").w_full().h(px(theme::GUTTER)).flex_shrink_0();
            let beside = div().flex_1().min_w_0().flex().flex_col().child(over_card).child(card);
            div().flex_1().min_h_0().flex().child(side).child(beside)
        } else {
            (div().flex_1().min_h_0().flex().flex_col())
                .child(top)
                .child(body.child(card))
        }
    }

    /// Where the rails are this frame: out with the sidebar, the status bar only with servers
    /// to show too, and the top rail always, the sidebar's top row with the sidebar.
    pub(super) fn move_rails(&self, window: &mut Window, cx: &mut App) -> Shown {
        let open = self.sidebar_open;
        let sidebar = spring("rails-open", open, MOTION, window, cx);
        let in_sidebar = settings::get(cx).tabs_in_sidebar();
        Shown {
            open: sidebar,
            top: if in_sidebar {
                sidebar
            } else {
                AnimationPhase::from(true)
            },
            status: spring(
                "rails-status",
                open && self.rails.servers,
                MOTION,
                window,
                cx,
            ),
            in_sidebar,
        }
    }

    /// What runs along the window's top as the tabs have it (`Shown::in_sidebar`): the top rail
    /// across the window, or with the tabs in the sidebar its top row, over it and sliding out
    /// with it, right-clicked the sidebar's menu.
    /// Back, forward and reload are in it while the address bar is in the sidebar, at the row's
    /// end or after the rail's sidebar toggle; with the address bar in the page, each page's
    /// toolbar has them instead (`docs/layout.md`, Address bar). The traffic lights fade with it
    /// and, as Arc's, rest dimmed into the glass, coming up in full while the pointer's on them,
    /// easing either way. They're set each frame, as `AppKit` makes them anew when the window
    /// leaves full screen; `fade` touches only what differs.
    fn top_rail(&self, shown: Shown, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let (pointed, hover) = theme::hover::eased("rails-lights", LIGHTS_EASE, window, cx);
        let strength = LIGHTS_RESTING + (1. - LIGHTS_RESTING) * pointed;
        #[cfg(target_os = "macos")]
        crate::native::traffic_lights::fade(window, lights(shown.top) * strength);
        let controls = settings::get(cx).address_in_sidebar().then(|| self.sidebar_top.controls());
        rail(Edge::Top, theme::TITLE_BAR_HEIGHT, shown.top, || {
            let toggle = action_button(
                "toggle-sidebar",
                IconName::PanelLeft,
                "Toggle sidebar",
                ToggleSidebar,
                cx,
            )
            .debug_selector(|| "toggle-sidebar".to_owned());
            let row = div().flex().items_center().gap_1().pr(px(theme::GUTTER));
            if shown.in_sidebar {
                let sidebar = self.sidebar_width(cx);
                let row = (row.debug_selector(|| "sidebar-top".to_owned()))
                    .w(px(theme::GUTTER + sidebar))
                    .left(px(sidebar * (shown.open.0 - 1.)))
                    .h_full()
                    .flex_shrink_0();
                with_menu(row, |_, cx| sidebar_menu(cx))
                    .child(lights_area(hover))
                    .child(toggle)
                    .children(DownloadsButton::in_bar(&self.downloads, cx))
                    .child(drag_area("sidebar-lights-title").flex_1())
                    .children(controls)
            } else {
                row.child(lights_area(hover))
                    .child(toggle)
                    .children(DownloadsButton::in_bar(&self.downloads, cx))
                    .children(controls)
                    .child(super::top_rail::pages(self, cx))
                    .child(super::top_rail::tabs(self, cx))
                    .child(
                        drag_area("title")
                            .debug_selector(|| "rail-title".to_owned())
                            .flex_1()
                            .min_w(px(EMPTY_RAIL)),
                    )
                    .children(self.panel_buttons_in_rail(cx))
            }
        })
        .when(shown.top.0 <= 0., |rail| {
            rail.child(drag_area("compact-title").w_full())
        })
    }

    /// The selected worktree's Changes and Files buttons at the end of the top rail while
    /// they're placed there (`right_panel::buttons`), the changes' with how many lines they add
    /// and remove, moving with the sidebar's space on a switch, as its tabs do; none with no
    /// worktree selected, nor while they're on the branch's row
    /// (`Settings::panel_buttons_on_rail`).
    fn panel_buttons_in_rail(&self, cx: &mut Context<Self>) -> Option<Div> {
        if !settings::get(cx).panel_buttons_on_rail() {
            return None;
        }
        let workspace = &self.workspace;
        let project = workspace.project()?;
        let at = (workspace.active_project()?, project.active_branch);
        let root = project.branch()?.worktree.path.as_path();
        let lit = theme::palette(cx).selected;
        let buttons = self.panel_buttons((at, root), (RAIL_GLYPH, lit), true, cx);
        let row = self.carry(div(), cx).flex().items_center().gap_1();
        let row = row.children(buttons.into_iter().map(|(b, _)| b));
        Some(super::top_rail::sliding(div().flex_shrink_0(), row))
    }

    pub(super) fn status_rail(&self, shown: Shown, cx: &App) -> Div {
        let p = theme::palette(cx);
        rail(Edge::Bottom, theme::STATUS_BAR_HEIGHT, shown.status, || {
            self.status_bar.render(p)
        })
    }
}

#[cfg(test)]
mod tests;
