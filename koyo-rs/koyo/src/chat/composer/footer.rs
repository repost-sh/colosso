//! The composer's footer (`docs/chat.md` 5.3): what the chat is set to, each a chip that changes
//! it, how full the conversation is, and the send button.
//!
//! What shows is what the thread says, so it's what the agent runs with: a change goes to the
//! host, and shows when the host says so, which is sooner than a person can tell. A chat with no
//! thread yet holds what it will start with.

use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::Button;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, AnyElement, App, ClickEvent, Context, Div, Focusable as _, Hsla,
    InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _, Pixels, SharedString,
    Styled as _, Svg, Window, actions, div, px, svg,
};
use koyo_agent::client::ClientError;
use koyo_agent::protocol::methods::{
    Catalog, ThreadParams, ThreadsUpdate, TurnsCompact, UpdateParams,
};
use koyo_agent::protocol::model::Mode;
use koyo_agent::words::{mode_label, model_name};

use super::ChatComposer;
use super::model_menu::OpenModelMenu;
use crate::chat::chosen::Settings;
use crate::menu::{self, MenuItem};
use crate::shell::glass;
use crate::theme::Palette;

/// The composer's context, where the keys that are its own all the time are bound.
pub(super) const COMPOSER: &str = "ChatComposer";
/// How wide the composer is before each of the chips says what it is, from the first: they're all
/// marks alone in a narrow pane, and the last of them goes first as one narrows.
pub(super) const ROOM_FOR_LABELS: [f32; 4] = [340., 420., 480., 560.];
/// How long a ring's stroke goes all the way around: its radius in `ring`, by 2π.
const RING_LENGTH: f32 = 56.55;

actions!(chat, [TogglePlan]);

/// How much the agent may do without asking is now this.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = chat, no_json)]
pub(super) struct SetPermission(pub(super) Mode);

pub(super) fn bind_keys(cx: &mut App) {
    let context = format!("{COMPOSER} > Input");
    cx.bind_keys([KeyBinding::new(
        "shift-tab",
        TogglePlan,
        Some(context.as_str()),
    )]);
}

/// How many chips say what they are in a composer `width` wide.
pub(super) fn labels_fitting(width: Pixels) -> usize {
    ROOM_FOR_LABELS
        .iter()
        .filter(|room| width >= px(**room))
        .count()
}

fn mode_mark(mode: Mode) -> IconName {
    match mode {
        Mode::Ask => IconName::Hand,
        Mode::AcceptEdits => IconName::FilePen,
        Mode::Auto => IconName::WandSparkles,
        Mode::FullAccess => IconName::ShieldAlert,
    }
}

/// The modes on offer, with a check in place of the mark of the one the chat is in.
fn permission_rows(current: Mode, can_auto: bool) -> Vec<MenuItem> {
    let offered = Mode::ALL
        .into_iter()
        .filter(|mode| *mode != Mode::Auto || can_auto);
    offered
        .map(|mode| {
            let icon = if mode == current {
                IconName::Check
            } else {
                mode_mark(mode)
            };
            MenuItem::new(Some(icon), mode_label(mode), Box::new(SetPermission(mode)))
        })
        .collect()
}

/// One thing the chat is set to, and what changes it.
pub(super) fn chip(
    id: &'static str,
    mark: IconName,
    label: Option<SharedString>,
    color: Hsla,
    cx: &App,
) -> Button {
    glass(id, color, cx)
        .small()
        .tab_stop(false)
        .icon(mark)
        .when_some(label, Button::label)
}

/// A ring `percent` full, in the color it's given. A sprite, the same one until that changes: the
/// kit's ring is a path, and a path in view has every frame of a streaming answer draw the window
/// over to antialias it.
pub(super) fn ring(percent: u8) -> Svg {
    let filled = RING_LENGTH * f32::from(percent) / 100.;
    let drawn = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="3"><circle cx="12" cy="12" r="9" opacity="0.25"/><circle cx="12" cy="12" r="9" stroke-dasharray="{filled} {RING_LENGTH}" transform="rotate(-90 12 12)"/></svg>"#
    );
    svg().data(drawn.as_bytes()).size(px(20.)).flex_shrink_0()
}

impl ChatComposer {
    /// Changes what the chat is set to.
    pub(super) fn change_settings(
        &mut self,
        change: impl FnOnce(&mut Settings),
        cx: &mut Context<Self>,
    ) {
        let mut changed = self.settings.clone();
        change(&mut changed);
        if changed == self.settings {
            return;
        }
        let Some(thread) = &self.thread else {
            self.settings = changed;
            cx.notify();
            return;
        };
        let params = self
            .settings
            .changes(&changed, thread.read(cx).id().clone());
        self.update_thread(params, cx);
    }

    /// Asks the host to change the thread, after anything else asked of it.
    pub(super) fn update_thread(&mut self, params: UpdateParams, cx: &mut Context<Self>) {
        #[cfg(test)]
        self.updated.push(params.clone());
        self.ask::<ThreadsUpdate>(params, cx);
    }

    /// Whatever `asked` said back is no one's business, but why it didn't happen is a person's.
    pub(super) fn when_said<T: 'static>(
        &mut self,
        asked: impl Future<Output = Result<T, ClientError>> + 'static,
        cx: &mut Context<Self>,
    ) {
        if self.error.take().is_some() {
            cx.notify();
        }
        cx.spawn(async move |this, cx| {
            if let Err(why) = asked.await {
                log::warn!("what was asked of a chat: {why}");
                this.update(cx, |this, cx| {
                    this.error = Some(why.to_string().into());
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Asks the agent to summarize the conversation so far.
    pub(super) fn compact(&mut self, cx: &mut Context<Self>) {
        if let Some(thread_id) = self.thread_id(cx) {
            self.ask::<TurnsCompact>(ThreadParams { thread_id }, cx);
        }
    }

    /// What the model goes by, or its id if the catalog doesn't list it: the chat's own, or else
    /// the one the agent's settings run.
    pub(super) fn model_label(&self, catalog: &Catalog) -> SharedString {
        let model = self.settings.model.as_deref();
        match (self.settings.model_in(catalog), catalog.model_id(model)) {
            (Some(model), _) => model_name(model).to_owned().into(),
            (None, Some(id)) => id.to_owned().into(),
            (None, None) => "Model".into(),
        }
    }

    fn attach_chip(&self, p: Palette, cx: &Context<Self>) -> Button {
        chip("chat-attach", IconName::Plus, None, p.muted, cx)
            .tooltip("Attach files or images")
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.choose_files(cx)))
    }

    fn model_chip(&self, catalog: &Catalog, p: Palette, cx: &Context<Self>) -> AnyElement {
        let mark = if self.settings.fast {
            IconName::Zap
        } else {
            IconName::Sparkles
        };
        let labelled = self.labels >= 1;
        let button = chip(
            "chat-model",
            mark,
            labelled.then(|| self.model_label(catalog)),
            p.muted,
            cx,
        )
        .dropdown_caret(labelled)
        .tooltip_with_action("Model", &OpenModelMenu, None);
        let pressed = cx.listener(|this, _, window, cx| this.toggle_model_menu(window, cx));
        self.model_menu.wraps(button, pressed).into_any_element()
    }

    fn permission_chip(
        &self,
        catalog: &Catalog,
        p: Palette,
        cx: &App,
    ) -> Option<impl IntoElement + use<>> {
        let mode = self.settings.mode(catalog)?;
        let color = if mode == catalog.default_mode.unwrap_or(Mode::Ask) {
            p.muted
        } else {
            p.text
        };
        let labelled = self.labels >= 3;
        let can_auto = self.settings.can_auto(catalog);
        let trigger = chip(
            "chat-permission",
            mode_mark(mode),
            labelled.then(|| mode_label(mode).into()),
            color,
            cx,
        )
        .dropdown_caret(labelled)
        .tooltip("What the agent may do without asking");
        Some(menu::dropdown(
            "chat-permission-menu",
            trigger,
            self.focus_handle(cx),
            move |_| permission_rows(mode, can_auto),
        ))
    }

    fn plan_chip(&self, p: Palette, cx: &Context<Self>) -> Button {
        let color = if self.settings.plan { p.text } else { p.muted };
        chip(
            "chat-plan-toggle",
            IconName::ListChecks,
            (self.labels >= 4).then(|| "Plan".into()),
            color,
            cx,
        )
        .toggled(self.settings.plan)
        .tooltip_with_action("Plan first", &TogglePlan, None)
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_plan(cx)))
    }

    pub(super) fn toggle_plan(&mut self, cx: &mut Context<Self>) {
        self.change_settings(|settings| settings.plan = !settings.plan, cx);
    }

    pub(super) fn footer(&self, typed: &str, p: Palette, cx: &Context<Self>) -> Div {
        let catalog = self.catalog.read(cx);
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(self.attach_chip(p, cx))
            .child(self.model_chip(catalog, p, cx))
            .children(self.effort_chip(catalog, p, cx))
            .children(self.permission_chip(catalog, p, cx))
            .child(self.plan_chip(p, cx))
            .child(div().flex_1())
            .children(self.context_ring(p, cx))
            .child(self.send_or_stop(typed, p, cx))
    }

    /// `column`, which hears what the footer's keys, chips and menus say.
    pub(super) fn hears_footer(column: Div, cx: &mut Context<Self>) -> Div {
        column
            .on_action(cx.listener(|this, _: &TogglePlan, _, cx| this.toggle_plan(cx)))
            .on_action(
                cx.listener(|this, permission: &SetPermission, _: &mut Window, cx| {
                    let mode = permission.0;
                    this.change_settings(|settings| settings.permission = Some(mode), cx);
                }),
            )
    }
}
