//! Delete Space (`docs/arc-sidebar.md` 6): asks first, and what happens to the space's
//! projects: moved to another space, picked from a dropdown, or taken out of Koyo, their files
//! left where they are.

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, App, Context, Div, FocusHandle, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    WeakEntity, Window, div, px,
};

use crate::modal::{self, Button, LINE, SMALL_LINE, Tone, small_line};
use crate::settings;
use crate::settings_view::MenuButton;
use crate::shell::Shell;
use crate::spaces::Leftovers;
use crate::theme::{self, Palette};

const WIDTH: f32 = 400.;
const GAP: f32 = 14.;
/// A choice's row: its line and the small one under it.
const CHOICE: f32 = 30.;

/// Picks the space, by its place in the list, the projects move to.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = arc, no_json)]
struct PickSpace(usize);

/// Asks about deleting the space shown, over `window`, unless it's the only one.
pub fn open(shell: WeakEntity<Shell>, window: &mut Window, cx: &mut App) {
    let settings = settings::get(cx);
    let Some(space) = settings.shown_space().filter(|_| settings.spaces.len() > 1) else {
        return;
    };
    let index = settings.space;
    let title: SharedString = format!("Delete {}?", space.name).into();
    let projects = space.projects.len();
    let heading = modal::heading_height(&title, WIDTH, window);
    let choices = if projects == 0 {
        LINE
    } else {
        LINE + 6. + 2. * CHOICE + 6.
    };
    let height = 2. * modal::PADDING + heading + GAP + choices + GAP + modal::FOOTER;
    modal::open(WIDTH, height, window, cx, move |window, cx| {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        DeleteSpace {
            shell,
            index,
            title,
            projects,
            target: usize::from(index == 0),
            remove: false,
            focus,
        }
    });
}

pub(super) struct DeleteSpace {
    shell: WeakEntity<Shell>,
    /// The space's place in the list when the dialog opened.
    index: usize,
    title: SharedString,
    /// How many projects it has.
    projects: usize,
    /// The space the projects move to, unless they're removed.
    target: usize,
    pub(super) remove: bool,
    /// Held by the dialog so the dropdown's pick reaches it.
    focus: FocusHandle,
}

impl DeleteSpace {
    pub(super) fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let leftovers = if self.remove {
            Leftovers::Remove
        } else {
            Leftovers::MoveTo(self.target)
        };
        let index = self.index;
        // Its loose pages go where its projects do: to the space that takes them, or with it.
        let id = |at: usize| settings::get(cx).spaces.get(at).map(|space| space.id);
        let gone = id(index);
        let takes = match leftovers {
            Leftovers::MoveTo(to) => id(to),
            Leftovers::Remove => None,
        };
        let mut removed = Vec::new();
        settings::update(cx, |settings| {
            removed = settings.delete_space(index, leftovers);
        });
        let shell = self.shell.clone();
        modal::dismiss(window, cx);
        let deleted = shell.update(cx, |shell, cx| {
            if let Some(gone) = gone {
                shell.hand_on_loose(gone, takes, cx);
            }
            shell.remove_projects(&removed, cx);
        });
        deleted.ok();
    }

    /// The dropdown of the spaces the projects may move to.
    fn dropdown(&self, cx: &App) -> MenuButton {
        let settings = settings::get(cx);
        let name =
            (settings.spaces.get(self.target)).map_or_else(String::new, |space| space.name.clone());
        let (deleted, target) = (self.index, self.target);
        MenuButton::new("delete-space-target", name, move |cx| {
            (settings::get(cx).spaces.iter().enumerate())
                .filter(|(index, _)| *index != deleted)
                .fold(NativeMenu::new(), |menu, (index, space)| {
                    menu.menu_with_check(
                        space.name.clone(),
                        index == target,
                        Box::new(PickSpace(index)),
                    )
                })
        })
    }
}

/// A round mark, filled when its choice is picked.
fn radio(picked: bool, p: Palette) -> Div {
    let ring: Hsla = if picked { p.text } else { p.muted };
    div()
        .size(px(14.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .border_1()
        .border_color(ring)
        .when(picked, |mark| {
            mark.child(div().size(px(6.)).rounded_full().bg(p.text))
        })
}

impl Render for DeleteSpace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let count = self.projects;
        let choices = (count > 0).then(|| {
            let said = match count {
                1 => "Its project:".to_owned(),
                count => format!("Its {count} projects:"),
            };
            let choice = |id: &'static str, picked: bool| {
                div()
                    .id(id)
                    .min_h(px(CHOICE))
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .child(radio(picked, p))
            };
            div()
                .flex()
                .flex_col()
                .child(div().h(px(LINE)).text_color(p.muted).child(said))
                .child(
                    choice("delete-space-move", !self.remove)
                        .mt(px(6.))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.remove = false;
                            cx.notify();
                        }))
                        .child("Move them to")
                        .child(self.dropdown(cx)),
                )
                .child(
                    choice("delete-space-remove", self.remove)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.remove = true;
                            cx.notify();
                        }))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child("Remove them from Koyo")
                                .child(
                                    small_line()
                                        .h(px(SMALL_LINE - 4.))
                                        .text_color(p.muted)
                                        .child("Their files stay where they are."),
                                ),
                        ),
                )
        });
        div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, PickSpace(index): &PickSpace, _, cx| {
                this.target = *index;
                this.remove = false;
                cx.notify();
            }))
            .child(modal::heading(self.title.clone()))
            .child(div().mt(px(GAP)).map(|body| {
                match choices {
                    Some(choices) => body.child(choices),
                    None => body
                        .h(px(LINE))
                        .text_color(p.muted)
                        .child("It has no projects."),
                }
            }))
            .child(div().flex_1())
            .child(
                modal::footer(None, p)
                    .child(modal::cancel("delete-space-cancel"))
                    .child(
                        Button::new("delete-space-confirm", "Delete", Tone::Danger)
                            .on_click(cx.listener(|this, _, window, cx| this.delete(window, cx))),
                    ),
            )
    }
}
