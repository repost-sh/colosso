//! The composer's footer (`docs/chat.md` 5.3): what the chat is set to, each a chip that changes
//! it, how full the conversation is, and the send button.
//!
//! What shows is what the thread says, so it's what the agent runs with: a change goes to the
//! host, and shows when the host says so, which is sooner than a person can tell. A chat with no
//! thread yet holds what it will start with.

use std::path::Path;

use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::hover_card::HoverCard;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, Anchor, App, ClickEvent, Context, Div, Focusable as _, Hsla, InteractiveElement as _,
    IntoElement, KeyBinding, MouseButton, ParentElement as _, Pixels, SharedString, Styled as _,
    Svg, Window, actions, div, px, svg,
};
use koyo_agent::client::ClientError;
use koyo_agent::protocol::ids::ThreadId;
use koyo_agent::protocol::methods::{
    Catalog, CreateParams, ThreadParams, ThreadsUpdate, TurnsCompact, UpdateParams,
};
use koyo_agent::protocol::model::{Effort, Mode, Thread};
use koyo_agent::words::{context_line, context_percent, effort_label, mode_label, model_name};

use super::ChatComposer;
use super::model_picker::OpenModelPicker;
use crate::menu::{self, MenuItem};
use crate::shell::glass;
use crate::theme::Palette;

/// The composer's context, where the keys that are its own all the time are bound.
pub(super) const COMPOSER: &str = "ChatComposer";
/// How wide the composer is before each of the chips says what it is, from the first: they're all
/// marks alone in a narrow pane, and the last of them goes first as one narrows.
pub(super) const ROOM_FOR_LABELS: [f32; 3] = [380., 440., 520.];
/// How long a ring's stroke goes all the way around: its radius in `ring`, by 2π.
const RING_LENGTH: f32 = 56.55;
/// How full a conversation is before its ring warns of it, in percent.
const NEARLY_FULL: u8 = 90;
const MODES: [Mode; 4] = [Mode::Ask, Mode::AcceptEdits, Mode::Auto, Mode::FullAccess];

actions!(chat, [TogglePlan, CompactContext]);

/// How much the agent may do without asking is now this.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = chat, no_json)]
pub(super) struct SetPermission(pub(super) Mode);

pub(super) fn bind_keys(cx: &mut App) {
    let context = format!("{COMPOSER} > Input");
    cx.bind_keys([KeyBinding::new("shift-tab", TogglePlan, Some(context.as_str()))]);
}

/// What a chat is set to.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Settings {
    pub(super) model: Option<String>,
    pub(super) effort: Option<Effort>,
    pub(super) fast: bool,
    pub(super) permission: Option<Mode>,
    pub(super) plan: bool,
}

impl Settings {
    pub(super) fn of(thread: &Thread) -> Settings {
        Settings {
            model: thread.model.clone(),
            effort: thread.effort,
            fast: thread.fast,
            permission: thread.permission,
            plan: thread.plan,
        }
    }

    /// A chat in the checkout at `cwd` that starts with these, on the host's first provider.
    pub(super) fn to_create(&self, cwd: &Path) -> CreateParams {
        CreateParams {
            cwd: cwd.to_string_lossy().into_owned(),
            provider: None,
            model: self.model.clone(),
            effort: self.effort,
            fast: self.fast,
            permission: self.permission,
            plan: self.plan,
        }
    }

    /// What thread `id`, set to these, has to hear to be set to `to`, and nothing else.
    fn changes(&self, to: &Settings, id: ThreadId) -> UpdateParams {
        UpdateParams {
            thread_id: id,
            title: None,
            model: (self.model != to.model).then(|| to.model.clone()),
            effort: (self.effort != to.effort).then_some(to.effort),
            fast: (self.fast != to.fast).then_some(to.fast),
            permission: (self.permission != to.permission).then_some(to.permission),
            plan: (self.plan != to.plan).then_some(to.plan),
            tab_open: None,
        }
    }
}

/// How many chips say what they are in a composer `width` wide.
pub(super) fn labels_fitting(width: Pixels) -> usize {
    ROOM_FOR_LABELS.iter().filter(|room| width >= px(**room)).count()
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
    let offered = MODES.into_iter().filter(|mode| *mode != Mode::Auto || can_auto);
    offered
        .map(|mode| MenuItem {
            icon: Some(if mode == current { IconName::Check } else { mode_mark(mode) }),
            label: mode_label(mode).into(),
            action: Box::new(SetPermission(mode)),
            switch: None,
        })
        .collect()
}

/// One thing the chat is set to, and what changes it.
fn chip(
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
fn ring(percent: u8) -> Svg {
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
        let params = self.settings.changes(&changed, thread.read(cx).id().clone());
        #[cfg(test)]
        self.updated.push(params.clone());
        let updating = self.chats.read(cx).ordered::<ThreadsUpdate>(params);
        self.when_said(updating, cx);
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
        let Some(thread) = &self.thread else {
            return;
        };
        let params = ThreadParams {
            thread_id: thread.read(cx).id().clone(),
        };
        let compacting = self.chats.read(cx).ordered::<TurnsCompact>(params);
        self.when_said(compacting, cx);
    }

    /// The mode the chat runs in: what it chose, or else what the person's settings choose, which
    /// isn't known until the host says.
    fn mode(&self, catalog: &Catalog) -> Option<Mode> {
        match self.settings.permission {
            Some(mode) => Some(mode),
            None if catalog.models.is_empty() => None,
            None => Some(catalog.default_mode.unwrap_or(Mode::Ask)),
        }
    }

    /// "Opus 5.5 · High": the name the catalog has for it, or its id if it no longer lists it.
    fn model_label(&self, catalog: &Catalog) -> SharedString {
        let settings = &self.settings;
        let name = match (catalog.model(settings.model.as_deref()), &settings.model) {
            (Some(model), _) => model_name(model),
            (None, Some(id)) => id,
            (None, None) => "Default",
        };
        match settings.effort {
            Some(effort) => format!("{name} · {}", effort_label(effort)).into(),
            None => name.to_owned().into(),
        }
    }

    fn attach_chip(&self, p: Palette, cx: &Context<Self>) -> Button {
        chip("chat-attach", IconName::Plus, None, p.muted, cx)
            .tooltip("Attach files or images")
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.choose_files(cx)))
    }

    fn model_chip(&self, catalog: &Catalog, p: Palette, cx: &Context<Self>) -> Button {
        let mark = if self.settings.fast { IconName::Zap } else { IconName::Sparkles };
        let labelled = self.labels >= 1;
        chip("chat-model", mark, labelled.then(|| self.model_label(catalog)), p.muted, cx)
            .dropdown_caret(labelled)
            .tooltip_with_action("Model and effort", &OpenModelPicker, None)
            // A press outside the picker closes it first, so on this chip it mustn't open it again.
            .when(self.picker.is_none(), |chip| {
                chip.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.toggle_picker(window, cx)),
                )
            })
    }

    fn permission_chip(
        &self,
        catalog: &Catalog,
        p: Palette,
        cx: &App,
    ) -> Option<impl IntoElement + use<>> {
        let mode = self.mode(catalog)?;
        let color = if mode == catalog.default_mode.unwrap_or(Mode::Ask) { p.muted } else { p.text };
        let labelled = self.labels >= 2;
        let can_auto = catalog.model(self.settings.model.as_deref()).is_none_or(|model| model.auto);
        let trigger = chip("chat-permission", mode_mark(mode), labelled.then(|| mode_label(mode).into()), color, cx)
            .dropdown_caret(labelled)
            .tooltip("What the agent may do without asking");
        Some(menu::dropdown(
            "chat-permission-menu",
            trigger,
            self.focus_handle(cx),
            move || permission_rows(mode, can_auto),
        ))
    }

    fn plan_chip(&self, p: Palette, cx: &Context<Self>) -> Button {
        let color = if self.settings.plan { p.text } else { p.muted };
        chip("chat-plan-toggle", IconName::ListChecks, (self.labels >= 3).then(|| "Plan".into()), color, cx)
            .toggled(self.settings.plan)
            .tooltip_with_action("Plan first", &TogglePlan, None)
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_plan(cx)))
    }

    pub(super) fn toggle_plan(&mut self, cx: &mut Context<Self>) {
        self.change_settings(|settings| settings.plan = !settings.plan, cx);
    }

    /// How full the conversation is, once a run has said: more, and compacting it, on hover.
    fn context_ring(&self, p: Palette, cx: &App) -> Option<HoverCard> {
        let (held, room) = self.context?;
        let percent = context_percent(held, room);
        let color = if percent >= NEARLY_FULL { p.danger } else { p.muted };
        let composer = self.focus_handle(cx);
        let card = HoverCard::new("chat-context-card")
            .anchor(Anchor::BottomRight)
            .trigger(div().flex_shrink_0().child(ring(percent).text_color(color)))
            .content(move |_, _, cx| {
                let composer = composer.clone();
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .text_xs()
                    .whitespace_nowrap()
                    .child(context_line(held, room))
                    .child(
                        glass("chat-compact", p.text, cx)
                            .small()
                            .label("Compact now")
                            .on_click(move |_, window, cx| {
                                composer.dispatch_action(&CompactContext, window, cx);
                            }),
                    )
            });
        Some(card)
    }

    pub(super) fn footer(&self, typed: &str, p: Palette, cx: &Context<Self>) -> Div {
        let catalog = self.catalog.read(cx);
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(self.attach_chip(p, cx))
            .child(self.model_chip(catalog, p, cx))
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
            .on_action(cx.listener(|this, _: &CompactContext, _, cx| this.compact(cx)))
            .on_action(cx.listener(|this, permission: &SetPermission, _: &mut Window, cx| {
                let mode = permission.0;
                this.change_settings(|settings| settings.permission = Some(mode), cx);
            }))
    }
}
