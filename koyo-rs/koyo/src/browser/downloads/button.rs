//! The downloads button in the window's top bar, beside the sidebar's toggle, as Safari's is
//! (`docs/browser.md` 16.3): there once there are downloads to list, a ring round its arrow as
//! far as what's coming in has come, the arrow dropping in as each download starts. A press or
//! ⌥⌘L opens the list under it; nothing opens by itself. A view of its own, cached, so a
//! download's progress redraws it and not the shell, which hears only that it came or went.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, AnyView, App, AppContext as _, Context, ElementId,
    Entity, EventEmitter, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    StatefulInteractiveElement as _, StyleRefinement, Styled as _, Subscription, Window, actions,
    div, ease_out_quint, px,
};

use super::panel::{DownloadsPanel, share};
use super::{Downloads, Entry};
use crate::menu::Trigger;
use crate::ring::ring;
use crate::shell::icon;
use crate::theme;

actions!(koyo, [ToggleDownloads]);

/// How long a download's arrival takes to show, and how far above its place the arrow starts.
const ARRIVING: Duration = Duration::from_millis(450);
const DROP: f32 = 7.;
/// How big the button is.
const SIZE: f32 = 24.;

pub struct DownloadsButton {
    trigger: Trigger,
    /// Whether it's there: the list has a download.
    shown: bool,
    _changed: Subscription,
}

/// The button came, with the list's first download, or went, with its last.
pub struct CameOrWent;

impl EventEmitter<CameOrWent> for DownloadsButton {}

impl DownloadsButton {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let downloads = Downloads::global(cx);
        let changed = cx.observe(&downloads, |this: &mut Self, _, cx| {
            let shown = !Downloads::listed(cx).is_empty();
            if shown != this.shown {
                this.shown = shown;
                cx.emit(CameOrWent);
            }
            cx.notify();
        });
        DownloadsButton {
            trigger: Trigger::new("downloads-menu"),
            shown: !Downloads::listed(cx).is_empty(),
            _changed: changed,
        }
    }

    /// `button` as the top bar holds it: nothing while there's nothing to list, else the view,
    /// drawn again only when it changes.
    pub fn in_bar(button: &Entity<Self>, cx: &App) -> Option<AnyElement> {
        let cached = StyleRefinement::default().size(px(SIZE)).flex_shrink_0();
        (button.read(cx).shown).then(|| AnyView::from(button.clone()).cached(cached).into_any_element())
    }

    /// Opens the list under `button`, or closes it; with nothing to list, nothing opens.
    pub fn toggle(button: &Entity<Self>, window: &mut Window, cx: &mut App) {
        let trigger = button.read(cx).trigger.clone();
        toggle(&trigger, window, cx);
    }
}

fn toggle(trigger: &Trigger, window: &mut Window, cx: &mut App) {
    if Downloads::listed(cx).is_empty() {
        return;
    }
    let at = trigger.clone();
    trigger.toggle_panel(|cx| cx.new(|cx| DownloadsPanel::new(at, cx)), window, cx);
}

/// The button's mark: the downloads icon while nothing comes in; while something does, its
/// arrow in a ring as full as what's coming has come, empty while the server hasn't said how
/// much. Its arrow drops in as the `arrivals`th download starts, unless motion is reduced.
fn mark(coming: &[&Entry], arrivals: u64, text: Hsla, accent: Hsla, cx: &App) -> impl IntoElement {
    let arrow = if coming.is_empty() {
        icon(IconName::Download, 16.).text_color(text).into_any_element()
    } else {
        icon(IconName::ArrowDown, 10.).text_color(accent).into_any_element()
    };
    let arrow = div().relative().child(arrow);
    let arrow = if cx.reduce_motion() {
        arrow.into_any_element()
    } else {
        let id = ElementId::NamedInteger("downloads-arrived".into(), arrivals);
        let arriving = Animation::new(ARRIVING).with_easing(ease_out_quint());
        (arrow.with_animation(id, arriving, |arrow, t| arrow.top(px(-DROP * (1. - t))).opacity(t)))
            .into_any_element()
    };
    let percent = share(coming).map_or(0, |share| (share * 100.).round() as u8);
    div()
        .relative()
        .size(px(18.))
        .flex()
        .items_center()
        .justify_center()
        .when(!coming.is_empty(), |mark| {
            mark.child(ring(percent).absolute().size(px(18.)).text_color(accent))
        })
        .child(arrow)
}

impl Render for DownloadsButton {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let listed = Downloads::listed(cx);
        if listed.is_empty() {
            return div().into_any_element();
        }
        let p = theme::palette(cx);
        let coming: Vec<&Entry> = listed.iter().filter(|entry| !entry.stage.ended()).collect();
        let open = self.trigger.is_open(cx);
        let face = div()
            .id("downloads-button")
            .debug_selector(|| "downloads-button".to_owned())
            .size(px(SIZE))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(move |button| button.bg(p.hover))
            .when(open, |button| button.bg(p.selected))
            .tooltip(|window, cx| Tooltip::new("Downloads").action(&ToggleDownloads, None).build(window, cx))
            .child(mark(&coming, Downloads::arrivals(cx), p.text, p.accent, cx));
        #[cfg(feature = "visual-check")]
        let face = gpui_kit::base::ElementExt::on_prepaint(face, crate::visual::noting("downloads-button".into()));
        let trigger = self.trigger.clone();
        (self.trigger)
            .wraps(face, move |_, window, cx| {
                cx.stop_propagation();
                toggle(&trigger, window, cx);
            })
            .into_any_element()
    }
}
