//! Delete Space (`docs/sidebar.md` 4): asks first, and what happens to the space's
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

/// Picks the space, by its id, the projects move to.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = spaces, no_json)]
struct PickSpace(u64);

/// Asks about deleting the space with id `space`, over `window`, unless it's the only one; its
/// projects move to the first other space unless another's picked.
pub fn open(shell: WeakEntity<Shell>, space: u64, window: &mut Window, cx: &mut App) {
    let settings = settings::get(cx);
    let Some(index) = settings.space_index(space).filter(|_| settings.spaces.len() > 1) else {
        return;
    };
    let deleted = &settings.spaces[index];
    let title: SharedString = format!("Delete {}?", deleted.name).into();
    let projects = deleted.projects.len();
    let target = settings.spaces[usize::from(index == 0)].id;
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
            space,
            title,
            projects,
            target,
            remove: false,
            focus,
        }
    });
}

pub(in crate::shell) struct DeleteSpace {
    shell: WeakEntity<Shell>,
    /// The id of the space to delete.
    space: u64,
    title: SharedString,
    /// How many projects it has.
    projects: usize,
    /// The id of the space the projects move to, unless they're removed.
    target: u64,
    pub(in crate::shell) remove: bool,
    /// Held by the dialog so the dropdown's pick reaches it.
    focus: FocusHandle,
}

impl DeleteSpace {
    pub(in crate::shell) fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let takes = (!self.remove).then_some(self.target);
        let gone = self.space;
        let shell = self.shell.clone();
        modal::dismiss(window, cx);
        let asked = shell.update(cx, |shell, cx| {
            let pages = match takes {
                None => shell.pages_of_space(gone, cx),
                Some(_) => Vec::new(),
            };
            let delete = move |shell: &mut Shell, cx: &mut Context<Shell>| {
                let listed = |id: u64| settings::get(cx).space_index(id).is_some();
                if !listed(gone) || !takes.is_none_or(listed) {
                    return;
                }
                let leftovers = takes.map_or(Leftovers::Remove, Leftovers::MoveTo);
                let mut removed = Vec::new();
                settings::update(cx, |settings| {
                    removed = settings.delete_space(gone, leftovers);
                });
                shell.hand_on_loose(gone, takes, cx);
                shell.remove_projects(&removed, cx);
            };
            shell.ask_pages_then(pages, delete, cx);
        });
        asked.ok();
    }

    /// The dropdown of the spaces the projects may move to.
    fn dropdown(&self, cx: &App) -> MenuButton {
        let settings = settings::get(cx);
        let name = (settings.space_index(self.target))
            .map_or_else(String::new, |at| settings.spaces[at].name.clone());
        let (deleted, target) = (self.space, self.target);
        MenuButton::new("delete-space-target", name, move |cx| {
            (settings::get(cx).spaces.iter())
                .filter(|space| space.id != deleted)
                .fold(NativeMenu::new(), |menu, space| {
                    menu.menu_with_check(
                        space.name.clone(),
                        space.id == target,
                        Box::new(PickSpace(space.id)),
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
            .on_action(cx.listener(|this, PickSpace(space): &PickSpace, _, cx| {
                this.target = *space;
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
