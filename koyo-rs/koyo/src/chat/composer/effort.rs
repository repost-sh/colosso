//! How hard the agent works in a chat (`docs/chat.md` 5.3): a chip in the footer, whose dropdown
//! holds the slider `AppKit` draws over the efforts of the chat's model. The keys stay in what's
//! typed; ←/→ move the slider, heard ahead of it.
//!
//! A chat shows what it's set to, or else what the person's settings choose, fitted to its model,
//! and takes that with its next message, so what shows is what runs.

use gpui_kit::assets::IconName;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Focusable as _, IntoElement, ParentElement as _,
    Render, Styled as _, WeakEntity, Window, div, px, size,
};
use koyo_agent::protocol::methods::{Catalog, Model, UpdateParams};
use koyo_agent::protocol::model::Effort;
use koyo_agent::words::effort_label;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSSlider;

use super::ChatComposer;
use super::footer::chip;
use crate::list;
use crate::menu::{MenuBody, ROW_PADDING};
use crate::native::{self, Control, slider};
use crate::settings;
use crate::settings_view::ticked_slider::ends_row;
use crate::theme;

const PANEL_WIDTH: f32 = 264.;
const PANEL_HEIGHT: f32 = 86.;

/// What the dropdown holds: the slider, over the efforts of the model it opened on. It goes by what
/// it was last moved to, ahead of what the thread says, which catches up.
struct EffortPanel {
    composer: WeakEntity<ChatComposer>,
    efforts: Vec<Effort>,
    at: usize,
}

/// The `AppKit` half of a panel's slider.
struct EffortSlider {
    panel: WeakEntity<EffortPanel>,
    stops: usize,
}

impl Control for EffortSlider {
    type View = NSSlider;
    type Shown = usize;

    fn make(&self, main: MainThreadMarker) -> Retained<NSSlider> {
        let view = slider::over_stops(self.stops, main);
        // What's typed keeps the keys, the thumb moved or not.
        view.setRefusesFirstResponder(true);
        view
    }

    fn show(view: &NSSlider, at: usize) {
        slider::show_stop(view, at);
    }

    fn acted(&self, view: &NSSlider, _: Option<usize>, cx: &mut App) {
        let at = slider::stop(view);
        self.panel
            .update(cx, |panel, cx| panel.slide_to(at, cx))
            .ok();
    }
}

impl MenuBody for EffortPanel {
    fn hears(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        match key {
            "left" => self.step(-1, cx),
            "right" => self.step(1, cx),
            _ => return false,
        }
        true
    }
}

impl EffortPanel {
    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        if let Some(to) = list::stepped(Some(self.at), by, self.efforts.len()) {
            self.slide_to(to, cx);
        }
    }

    /// The slider went to the stop `at`, which goes to the host as it's made.
    fn slide_to(&mut self, at: usize, cx: &mut Context<Self>) {
        let Some(effort) = self.efforts.get(at).copied().filter(|_| at != self.at) else {
            return;
        };
        self.at = at;
        cx.notify();
        self.composer
            .update(cx, |composer, cx| composer.say_effort(effort, cx))
            .ok();
    }
}

impl Render for EffortPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let says = |at: usize| {
            self.efforts
                .get(at)
                .map_or("", |effort| effort_label(*effort))
        };
        let ends = (says(0), says(self.efforts.len().saturating_sub(1)));
        let control = EffortSlider {
            panel: cx.weak_entity(),
            stops: self.efforts.len(),
        };
        let track = native::host("chat-effort-slider", control, self.at, window, cx);
        div()
            .size_full()
            .px(px(ROW_PADDING))
            .flex()
            .flex_col()
            .justify_center()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_sm()
                    .child("Effort")
                    .child(div().text_color(p.muted).child(says(self.at))),
            )
            .children(track.map(|(_, track)| track))
            .child(ends_row(ends, p))
    }
}

impl ChatComposer {
    /// The effort that shows on `model`: what the chat is set to, or else what the person's settings
    /// choose, as near as it gets. None if it has no efforts to choose.
    fn effort_on(&self, model: &Model, cx: &App) -> Option<Effort> {
        (self.settings).effort_on(model, settings::get(cx).chat_effort)
    }

    /// Has the chat set to the effort that shows, which goes ahead of the message about to be sent.
    /// What runs on a model the catalog doesn't list is left to what it was set to.
    pub(super) fn settle_effort(&mut self, cx: &mut Context<Self>) {
        let model = self.settings.model_in(self.catalog.read(cx));
        let on_model = model.map(|model| self.effort_on(model, cx));
        if let Some(effort) = on_model {
            self.change_settings(|settings| settings.effort = effort, cx);
        }
    }

    /// Opens the slider on the effort that shows, or closes it. A model with none has no slider.
    pub(super) fn toggle_effort(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(model) = self.settings.model_in(self.catalog.read(cx)) else {
            return;
        };
        let Some(effort) = self.effort_on(model, cx) else {
            return;
        };
        let efforts = model.efforts.clone();
        let at = efforts.iter().position(|of| *of == effort).unwrap_or(0);
        self.suggest = None;
        let composer = cx.weak_entity();
        self.effort_menu.toggle_panel(
            size(px(PANEL_WIDTH), px(PANEL_HEIGHT)),
            |cx| {
                cx.new(|_| EffortPanel {
                    composer,
                    efforts,
                    at,
                })
            },
            window,
            cx,
        );
        window.focus(&self.focus_handle(cx), cx);
        cx.notify();
    }

    /// Each move goes to the host whatever the thread says by now, as it may not have caught up with
    /// the one before: asked in order, the last one stands.
    fn say_effort(&mut self, effort: Effort, cx: &mut Context<Self>) {
        let Some(id) = self.thread_id(cx) else {
            self.change_settings(|settings| settings.effort = Some(effort), cx);
            return;
        };
        let params = UpdateParams {
            effort: Some(Some(effort)),
            ..UpdateParams::of(id)
        };
        self.update_thread(params, cx);
    }

    /// What the chat's model runs at, for a model with efforts to choose.
    pub(super) fn effort_chip(
        &self,
        catalog: &Catalog,
        p: theme::Palette,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let effort = self.effort_on(self.settings.model_in(catalog)?, cx)?;
        let labelled = self.labels >= 2;
        let label = labelled.then(|| effort_label(effort).into());
        let button = chip("chat-effort", IconName::Gauge, label, p.muted, cx)
            .dropdown_caret(labelled)
            .tooltip("Effort");
        let pressed = cx.listener(|this, _, window, cx| this.toggle_effort(window, cx));
        Some(self.effort_menu.wraps(button, pressed).into_any_element())
    }
}
