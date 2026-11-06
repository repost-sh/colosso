//! The model menu (`docs/chat.md` 5.3): the dropdown of its chip, with the catalog's models, and
//! below them Fast's switch when the chat's model can go fast. It's the chip's, or a key's or
//! `/model`'s to open.

use gpui_kit::assets::IconName;
use gpui_kit::{
    Action, App, Context, Div, Focusable as _, InteractiveElement as _, KeyBinding, SharedString,
    Window, actions,
};
use koyo_agent::protocol::methods::{Catalog, Model};
use koyo_agent::protocol::model::Mode;
use koyo_agent::words::model_name;

use super::ChatComposer;
use super::footer::{COMPOSER, Settings};
use crate::menu::MenuItem;

actions!(chat, [OpenModelMenu, ToggleFast]);

/// The chat is now on this model.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = chat, no_json)]
struct SetModel(String);

pub(super) fn bind_keys(cx: &mut App) {
    let always = format!("{COMPOSER} > Input");
    cx.bind_keys([KeyBinding::new(
        "ctrl-m",
        OpenModelMenu,
        Some(always.as_str()),
    )]);
}

/// What `model` keeps of what's chosen: it can't be set to what it can't do, and a mode it can't
/// run in, even if only the person's settings chose it, has it ask.
fn with_model(settings: &mut Settings, model: &Model, default_mode: Option<Mode>) {
    settings.model = Some(model.id.clone());
    settings.effort = settings.effort.and_then(|effort| model.fitted(effort));
    settings.fast &= model.fast;
    if settings.permission.or(default_mode) == Some(Mode::Auto) && !model.auto {
        settings.permission = Some(Mode::Ask);
    }
}

/// What a model's row says: its name, and what it runs if that isn't in its name.
fn row_label(model: &Model) -> SharedString {
    if model.name.contains(model_name(model)) {
        model.name.clone().into()
    } else {
        format!("{} · {}", model.name, model_name(model)).into()
    }
}

/// The models, with a check on the one the chat is on, and Fast's switch if it can.
fn rows(catalog: &Catalog, settings: &Settings) -> Vec<MenuItem> {
    let current = settings.model_in(catalog);
    let models = catalog.models.iter().map(|model| {
        let chosen = current.is_some_and(|current| current.id == model.id);
        let icon = chosen.then_some(IconName::Check);
        MenuItem::new(icon, row_label(model), Box::new(SetModel(model.id.clone())))
    });
    let fast = current.filter(|model| model.fast).map(|_| {
        MenuItem::new(Some(IconName::Zap), "Fast", Box::new(ToggleFast))
            .switch(settings.fast)
            .apart(true)
    });
    models.chain(fast).collect()
}

impl ChatComposer {
    /// Opens the menu on the model the chat is on, or closes it.
    pub(super) fn toggle_model_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let catalog = self.catalog.read(cx);
        if catalog.models.is_empty() {
            return;
        }
        let items = rows(catalog, &self.settings);
        let shown = self.settings.model.as_deref();
        let at = catalog
            .models
            .iter()
            .position(|model| Some(model.id.as_str()) == shown);
        self.suggest = None;
        let target = self.focus_handle(cx);
        self.model_menu
            .toggle(|_| items, Some(at.unwrap_or(0)), &target, window, cx);
        window.focus(&target, cx);
        cx.notify();
    }

    fn set_model(&mut self, id: &str, cx: &mut Context<Self>) {
        let catalog = self.catalog.read(cx);
        let default_mode = catalog.default_mode;
        if let Some(model) = catalog.model(Some(id)).cloned() {
            self.change_settings(|settings| with_model(settings, &model, default_mode), cx);
        }
    }

    fn toggle_fast(&mut self, cx: &mut Context<Self>) {
        let model = self.settings.model_in(self.catalog.read(cx));
        if model.is_some_and(|model| model.fast) {
            self.change_settings(|settings| settings.fast = !settings.fast, cx);
        }
    }

    /// `column`, which hears what the menu and its key say.
    pub(super) fn hears_model_menu(column: Div, cx: &mut Context<Self>) -> Div {
        column
            .on_action(cx.listener(|this, _: &OpenModelMenu, window, cx| {
                this.toggle_model_menu(window, cx);
            }))
            .on_action(cx.listener(|this, set: &SetModel, _, cx| this.set_model(&set.0, cx)))
            .on_action(cx.listener(|this, _: &ToggleFast, _, cx| this.toggle_fast(cx)))
    }
}
