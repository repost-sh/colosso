//! The card's project scripts, which run in a terminal tab of the checkout the chat works in.

use std::path::{Path, PathBuf};

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::{
    App, AppContext as _, ClickEvent, Context, Div, Entity, Focusable as _, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, div, px,
};

use super::super::{ChatView, ChatViewEvent};
use super::{clickable, row};
use crate::modal::{self, Button, FIELD, FOOTER, HEADING_LINE, PADDING, Tone};
use crate::pane::close_button;
use crate::settings::{self, ProjectScript};
use crate::shell::ellipsized;
use crate::theme::{self, Palette};

const WIDTH: f32 = 420.;
/// What's over the fields, between them and under them.
const OVER: f32 = 14.;
const BETWEEN: f32 = 10.;
const UNDER: f32 = 16.;
const HEIGHT: f32 = PADDING * 2. + HEADING_LINE + OVER + FIELD + BETWEEN + FIELD + UNDER + FOOTER;
/// The group a script's line is, for its button to show as it's hovered.
const LINE: &str = "chat-details-script";

/// The project's scripts, and the line that adds one.
pub(super) fn section(view: &ChatView, p: Palette, cx: &Context<ChatView>) -> Option<Div> {
    let place = view.details.place.as_ref()?;
    let project = place.project.clone();
    let scripts = settings::get(cx).project_scripts.of(&project).iter().cloned();
    let lines: Vec<_> =
        scripts.enumerate().map(|(at, script)| line(&project, &place.folder, at, script, p, cx)).collect();
    let add = row(IconName::Plus, p).child(ellipsized("Add a script…"));
    let add = clickable(add, "chat-details-add-script", p)
        .on_click(move |_, window, cx| open_adding(project.clone(), window, cx));
    Some(div().flex().flex_col().children(lines).child(add))
}

fn line(
    project: &Path,
    folder: &Path,
    at: usize,
    script: ProjectScript,
    p: Palette,
    cx: &Context<ChatView>,
) -> Stateful<Div> {
    let (project, folder) = (project.to_owned(), folder.to_owned());
    let remove = close_button(("chat-details-remove-script", at), p)
        .opacity(0.)
        .group_hover(LINE, |remove| remove.opacity(1.))
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            settings::update(cx, |settings| settings.project_scripts.remove(&project, at));
        });
    let says = row(IconName::Play, p).group(LINE).child(ellipsized(script.name.clone()).flex_1()).child(remove);
    clickable(says, ("chat-details-script", at), p).on_click(cx.listener(
        move |_, _: &ClickEvent, _, cx| {
            cx.emit(ChatViewEvent::RunScript { script: script.clone(), folder: folder.clone() });
        },
    ))
}

fn open_adding(project: PathBuf, window: &mut Window, cx: &mut App) {
    modal::open(WIDTH, HEIGHT, window, cx, |window, cx| {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Name, like Dev server"));
        let command = cx.new(|cx| InputState::new(window, cx).placeholder("Command, like npm run dev"));
        name.focus_handle(cx).focus(window, cx);
        let _typing = [
            cx.subscribe_in(&name, window, |this: &mut AddScript, _, event, window, cx| match event {
                InputEvent::Change => cx.notify(),
                InputEvent::PressEnter { .. } => this.command.focus_handle(cx).focus(window, cx),
                _ => {}
            }),
            cx.subscribe_in(&command, window, |this: &mut AddScript, _, event, window, cx| match event {
                InputEvent::Change => cx.notify(),
                InputEvent::PressEnter { .. } => this.add(window, cx),
                _ => {}
            }),
        ];
        AddScript { project, name, command, _typing }
    });
}

struct AddScript {
    project: PathBuf,
    name: Entity<InputState>,
    command: Entity<InputState>,
    _typing: [Subscription; 2],
}

impl AddScript {
    /// The script as typed, once it has a name and a command.
    fn script(&self, cx: &App) -> Option<ProjectScript> {
        let name = self.name.read(cx).value().trim().to_owned();
        let command = self.command.read(cx).value().trim().to_owned();
        (!name.is_empty() && !command.is_empty()).then_some(ProjectScript { name, command })
    }

    fn add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(script) = self.script(cx) else {
            return;
        };
        let project = self.project.clone();
        settings::update(cx, |settings| settings.project_scripts.add(project, script));
        modal::dismiss(window, cx);
    }
}

impl Render for AddScript {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let says = |state: &Entity<InputState>| modal::field(Input::new(state), p);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(modal::heading(SharedString::from("Add a script")))
            .child(div().mt(px(OVER)).child(says(&self.name)))
            .child(div().mt(px(BETWEEN)).child(says(&self.command)))
            .child(
                modal::footer(None, p)
                    .mt(px(UNDER))
                    .child(modal::cancel("chat-add-script-cancel"))
                    .child(
                        Button::new("chat-add-script", "Add", Tone::Primary)
                            .disabled(self.script(cx).is_none())
                            .on_click(cx.listener(|this, _, window, cx| this.add(window, cx))),
                    ),
            )
    }
}
