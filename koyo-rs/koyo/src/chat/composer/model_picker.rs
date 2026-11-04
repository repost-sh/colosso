//! The model picker (`docs/chat.md` 5.3): a glass popover over the footer, with the catalog's
//! models, the efforts of the one highlighted, and Fast when it can be. The focus stays in what's
//! typed, where its keys are bound while it's open, so closing it has nothing to put back.

use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ClickEvent, Context, Div, Focusable as _, InteractiveElement as _, KeyBinding,
    MouseMoveEvent, ParentElement as _, ScrollHandle, Stateful, StatefulInteractiveElement as _,
    Styled as _, Window, actions, div, px,
};
use koyo_agent::protocol::methods::{Catalog, Model};
use koyo_agent::protocol::model::{Effort, Mode};
use koyo_agent::words::effort_label;

use super::ChatComposer;
use super::footer::{COMPOSER, Settings};
use crate::list::{self, Activate, PageDown, PageUp, SelectNext, SelectPrevious};
use crate::shell::{ellipsized, icon, row};
use crate::theme::{self, Palette};

/// The composer's context while the picker is open.
pub(super) const PICKING: &str = "ChatPicking";
/// Where the picker's keys are heard.
const PICKING_TEXT: &str = "ChatPicking > Input";
/// How many rows a page of the picker is.
const PAGE: isize = 5;
const WIDTH: f32 = 420.;
const EFFORTS_WIDTH: f32 = 120.;
/// The tallest the models get before they scroll.
const MODELS_HEIGHT: f32 = 320.;

actions!(
    chat,
    [
        OpenModelPicker,
        PickerToEfforts,
        PickerToModels,
        PickerClose,
        PickerFast,
    ]
);

pub(super) fn bind_keys(cx: &mut App) {
    let always = format!("{COMPOSER} > Input");
    let picking = Some(PICKING_TEXT);
    list::bind_keys(PICKING_TEXT, cx);
    cx.bind_keys([
        KeyBinding::new("ctrl-m", OpenModelPicker, Some(always.as_str())),
        KeyBinding::new("right", PickerToEfforts, picking),
        KeyBinding::new("left", PickerToModels, picking),
        KeyBinding::new("enter", Activate, picking),
        KeyBinding::new("escape", PickerClose, picking),
        KeyBinding::new("tab", PickerFast, picking),
    ]);
}

pub(super) struct ModelPicker {
    /// The model highlighted, in the catalog's order.
    at: usize,
    /// The row of its efforts highlighted, once a person went there.
    effort: Option<usize>,
    scroll: ScrollHandle,
}

impl ModelPicker {
    /// Where the models are, for what's sure they stay.
    #[cfg(test)]
    pub(super) fn models_bounds(&self) -> gpui_kit::Bounds<gpui_kit::Pixels> {
        self.scroll.bounds()
    }
}

/// The rows of `model`'s efforts, the first leaving it to the model. It may have none to choose.
fn effort_rows(model: &Model) -> impl Iterator<Item = Option<Effort>> {
    let own = model.efforts.iter().copied().map(Some);
    (!model.efforts.is_empty()).then_some(None).into_iter().chain(own)
}

/// What `model` keeps of what's chosen: it can't be set to what it can't do, and a mode it can't
/// run in, even if only the person's settings chose it, has it ask.
fn with_model(settings: &mut Settings, model: &Model, default_mode: Option<Mode>) {
    settings.model = Some(model.id.clone());
    settings.effort = kept_effort(settings, model);
    settings.fast &= model.fast;
    if settings.permission.or(default_mode) == Some(Mode::Auto) && !model.auto {
        settings.permission = Some(Mode::Ask);
    }
}

fn kept_effort(settings: &Settings, model: &Model) -> Option<Effort> {
    settings.effort.filter(|effort| model.efforts.contains(effort))
}

impl ChatComposer {
    /// Opens the picker on the model the chat is set to, or closes it.
    pub(super) fn toggle_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.notify();
        if self.picker.take().is_some() {
            return;
        }
        self.suggest = None;
        let models = &self.catalog.read(cx).models;
        if models.is_empty() {
            return;
        }
        let shown = self.settings.model.as_deref();
        let at = models.iter().position(|model| Some(model.id.as_str()) == shown).unwrap_or(0);
        let scroll = ScrollHandle::new();
        scroll.scroll_to_item(at);
        self.picker = Some(ModelPicker {
            at,
            effort: None,
            scroll,
        });
        window.focus(&self.focus_handle(cx), cx);
    }

    fn close_picker(&mut self, cx: &mut Context<Self>) {
        self.picker = None;
        cx.notify();
    }

    /// `by` rows up or down the column the picker is in, as far as there are any.
    fn move_in_picker(&mut self, by: isize, cx: &mut Context<Self>) {
        let Some(picker) = &mut self.picker else {
            return;
        };
        let models = &self.catalog.read(cx).models;
        if let Some(effort) = &mut picker.effort {
            let efforts = models.get(picker.at).map_or(0, |model| effort_rows(model).count());
            *effort = list::stepped(Some(*effort), by, efforts).unwrap_or(0);
        } else {
            picker.at = list::stepped(Some(picker.at), by, models.len()).unwrap_or(0);
            picker.scroll.scroll_to_item(picker.at);
        }
        cx.notify();
    }

    /// Over to the efforts of the model highlighted, if it has any, at the one it would keep.
    fn over_to_efforts(&mut self, cx: &mut Context<Self>) {
        let Some(picker) = &mut self.picker else {
            return;
        };
        let Some(model) = self.catalog.read(cx).models.get(picker.at) else {
            return;
        };
        let kept = kept_effort(&self.settings, model);
        picker.effort = effort_rows(model).position(|row| row == kept);
        cx.notify();
    }

    fn back_to_models(&mut self, cx: &mut Context<Self>) {
        if let Some(picker) = &mut self.picker {
            picker.effort = None;
            cx.notify();
        }
    }

    /// The pointer moved over the model at `at`: its efforts show. Moved, as a row that only came
    /// under a pointer at rest, the keys having scrolled it there, mustn't take the highlight.
    fn point_at_model(&mut self, at: usize, cx: &mut Context<Self>) {
        if let Some(picker) = &mut self.picker
            && (picker.at, picker.effort) != (at, None)
        {
            (picker.at, picker.effort) = (at, None);
            cx.notify();
        }
    }

    fn point_at_effort(&mut self, at: usize, cx: &mut Context<Self>) {
        if let Some(picker) = &mut self.picker
            && picker.effort != Some(at)
        {
            picker.effort = Some(at);
            cx.notify();
        }
    }

    /// The chat is set to what's highlighted, and the picker closes.
    fn pick(&mut self, cx: &mut Context<Self>) {
        let Some(picker) = self.picker.take() else {
            return;
        };
        cx.notify();
        let catalog = self.catalog.read(cx);
        let default_mode = catalog.default_mode;
        let Some(model) = catalog.models.get(picker.at).cloned() else {
            return;
        };
        let effort = picker.effort.and_then(|row| effort_rows(&model).nth(row));
        self.change_settings(
            |settings| {
                with_model(settings, &model, default_mode);
                if let Some(effort) = effort {
                    settings.effort = effort;
                }
            },
            cx,
        );
    }

    /// Turns Fast on or off for the model highlighted, which that picks; the picker stays.
    fn toggle_fast(&mut self, cx: &mut Context<Self>) {
        let Some(picker) = &self.picker else {
            return;
        };
        let catalog = self.catalog.read(cx);
        let default_mode = catalog.default_mode;
        let Some(model) = catalog.models.get(picker.at).filter(|model| model.fast).cloned() else {
            return;
        };
        self.change_settings(
            |settings| {
                with_model(settings, &model, default_mode);
                settings.fast = !settings.fast;
            },
            cx,
        );
    }

    /// `column`, which hears the picker's keys.
    pub(super) fn hears_picker(&self, column: Div, cx: &Context<Self>) -> Div {
        column
            .on_action(cx.listener(|this, _: &OpenModelPicker, window, cx| {
                this.toggle_picker(window, cx);
            }))
            .when(self.picker.is_some(), |column| {
                column
                    .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| {
                        this.move_in_picker(-1, cx);
                    }))
                    .on_action(cx.listener(|this, _: &SelectNext, _, cx| {
                        this.move_in_picker(1, cx);
                    }))
                    .on_action(cx.listener(|this, _: &PageUp, _, cx| this.move_in_picker(-PAGE, cx)))
                    .on_action(cx.listener(|this, _: &PageDown, _, cx| this.move_in_picker(PAGE, cx)))
                    .on_action(cx.listener(|this, _: &PickerToEfforts, _, cx| {
                        this.over_to_efforts(cx);
                    }))
                    .on_action(cx.listener(|this, _: &PickerToModels, _, cx| {
                        this.back_to_models(cx);
                    }))
                    .on_action(cx.listener(|this, _: &Activate, _, cx| this.pick(cx)))
                    .on_action(cx.listener(|this, _: &PickerClose, _, cx| this.close_picker(cx)))
                    .on_action(cx.listener(|this, _: &PickerFast, _, cx| this.toggle_fast(cx)))
            })
    }

    /// The popover, while it's open.
    pub(super) fn model_picker(
        &self,
        p: Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let picker = self.picker.as_ref()?;
        let popover = Self::popover("chat-model-picker", p, window, cx)
            .w(px(WIDTH))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_picker(cx)));
        let catalog = self.catalog.read(cx);
        let highlighted = catalog.models.get(picker.at)?;
        // It's the same size whichever model is highlighted, having grown from the footer: anything
        // less would move the rows from under a pointer on its way to one.
        let most_efforts = catalog.models.iter().map(|model| effort_rows(model).count()).max();
        let columns = div()
            .min_h(px(theme::ROW_HEIGHT * most_efforts.unwrap_or(0) as f32))
            .flex()
            .items_start()
            .gap_2()
            .child(self.models_column(catalog, picker, p, cx))
            .child(self.efforts_column(highlighted, picker, p, cx));
        let some_go_fast = catalog.models.iter().any(|model| model.fast);
        Some(
            popover
                .child(columns)
                .when(some_go_fast, |popover| popover.child(self.fast_row(highlighted.fast, p, cx))),
        )
    }

    fn models_column(
        &self,
        catalog: &Catalog,
        picker: &ModelPicker,
        p: Palette,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let current = catalog.model(self.settings.model.as_deref());
        let rows = (catalog.models.iter().enumerate()).map(|(at, model)| {
            let chosen = current.is_some_and(|current| current.id == model.id);
            Self::model_row(at, model, at == picker.at, chosen, p, cx)
        });
        div()
            .id("chat-model-rows")
            .flex_1()
            .min_w_0()
            .max_h(px(MODELS_HEIGHT))
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .track_scroll(&picker.scroll)
            .children(rows)
    }

    fn model_row(
        at: usize,
        model: &Model,
        highlighted: bool,
        chosen: bool,
        p: Palette,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let words = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(ellipsized(model.name.clone()))
            .when(!model.description.is_empty(), |words| {
                words.child(
                    div().text_xs().text_color(p.muted).child(ellipsized(model.description.clone())),
                )
            });
        row(("chat-model-row", at), highlighted, p)
            .h_auto()
            .py_1()
            .on_mouse_move(cx.listener(move |this, _: &MouseMoveEvent, _, cx| {
                this.point_at_model(at, cx);
            }))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.point_at_model(at, cx);
                this.pick(cx);
            }))
            .child(words)
            .when(chosen, |row| row.child(icon(IconName::Check, 14.)))
    }

    /// The efforts of the model highlighted, with a check on the one it would run at.
    fn efforts_column(
        &self,
        model: &Model,
        picker: &ModelPicker,
        p: Palette,
        cx: &Context<Self>,
    ) -> Div {
        let kept = kept_effort(&self.settings, model);
        let rows = effort_rows(model).enumerate().map(|(at, effort)| {
            row(("chat-effort-row", at), picker.effort == Some(at), p)
                .on_mouse_move(cx.listener(move |this, _: &MouseMoveEvent, _, cx| {
                    this.point_at_effort(at, cx);
                }))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.point_at_effort(at, cx);
                    this.pick(cx);
                }))
                .child(div().flex_1().child(effort.map_or("Default", effort_label)))
                .when(effort == kept, |row| row.child(icon(IconName::Check, 14.)))
        });
        div().w(px(EFFORTS_WIDTH)).flex_shrink_0().flex().flex_col().children(rows)
    }

    /// Fast, for the model highlighted, if it `can` go fast.
    fn fast_row(&self, can: bool, p: Palette, cx: &Context<Self>) -> Div {
        let switch = Switch::new("chat-fast")
            .checked(can && self.settings.fast)
            .disabled(!can)
            .color(p.accent)
            .on_click(cx.listener(|this, _: &bool, _, cx| this.toggle_fast(cx)));
        div()
            .flex()
            .items_center()
            .gap_2()
            .mt_1()
            .px_2()
            .pt_2()
            .border_t_1()
            .border_color(p.hairline)
            .text_sm()
            .when(!can, |row| row.text_color(p.muted))
            .child(icon(IconName::Zap, 13.))
            .child(div().flex_1().child("Fast"))
            .child(switch)
    }
}
