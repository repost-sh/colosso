//! How the New Worktree dialog looks: the name, what creating would make, the branches to
//! start from, and the buttons.

use colosso_git::{NewWorktree, Ref};
use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::input::Input;
use gpui_kit::component::switch::Switch;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    StatefulInteractiveElement as _, Styled as _, Window, div, px,
};

use super::NewWorktreeDialog;
use crate::modal::{self, Button, Tone};
use crate::projects::display_path;
use crate::shell::{self, ellipsized, icon};
use crate::theme;

impl NewWorktreeDialog {
    /// The branch and folder creating would make, under the name.
    fn preview(&self, p: theme::Palette, cx: &App) -> impl IntoElement {
        let line = |name: IconName, text: String, color| {
            div()
                .h(px(18.))
                .flex()
                .items_center()
                .gap_2()
                .text_xs()
                .text_color(color)
                .child(icon(name, 12.))
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        // Cut from the start, so the end of a long path stays in view.
                        .text_ellipsis_start()
                        .font_family(theme::MONO)
                        .child(text),
                )
        };
        let request = self.request(cx);
        let slug = request.as_ref().map(NewWorktree::slug).unwrap_or_default();
        let (branch, folder) = match &request {
            Some(request) if !slug.is_empty() || self.existing => (
                line(IconName::GitBranch, request.branch(), p.text),
                line(
                    IconName::Folder,
                    display_path(&request.parent.join(&slug)),
                    p.muted,
                ),
            ),
            // Until it has a name, the preview shows where the name goes.
            _ => {
                let prefix = self
                    .project
                    .as_ref()
                    .and_then(|project| project.prefix.as_ref());
                (
                    line(
                        IconName::GitBranch,
                        prefix.map_or("…".to_owned(), |prefix| format!("{prefix}/…")),
                        p.muted,
                    ),
                    line(
                        IconName::Folder,
                        format!("{}/…", display_path(&self.parent)),
                        p.muted,
                    ),
                )
            }
        };
        div().px_1().flex().flex_col().child(branch).child(folder)
    }

    fn branch_list(&self, p: theme::Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let shown = self.shown(cx);
        let default = self
            .project
            .as_ref()
            .and_then(|project| project.default.as_ref());
        let loading = self.project.is_none() && self.error.is_none();
        let empty = match &self.project {
            None if loading => None,
            None => Some("No branches to show."),
            Some(_) if shown.is_empty() => Some("No branch matches."),
            Some(_) => None,
        };
        div()
            .id("new-worktree-branches")
            .flex_1()
            .min_h_0()
            .p_1()
            .flex()
            .flex_col()
            .rounded(px(8.))
            .border_1()
            .border_color(p.hairline)
            .bg(p.raised)
            .overflow_y_scroll()
            .track_scroll(&self.branch_list)
            .relative()
            .when(loading, |list| list.child(modal::spinner(p.muted)))
            .when_some(empty, |list, empty| {
                list.child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(p.muted)
                        .child(empty),
                )
            })
            .children(shown.into_iter().enumerate().map(|(index, branch)| {
                let picked = self.picked.as_ref() == Some(branch);
                let name = match branch {
                    Ref::Local(_) => IconName::GitBranch,
                    Ref::Remote { .. } => IconName::Globe,
                };
                let choice = branch.clone();
                shell::row(("branch", index), picked, p)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.picked = Some(choice.clone());
                        cx.notify();
                    }))
                    .child(div().text_color(p.muted).child(icon(name, 13.)))
                    .child(ellipsized(branch.to_string()).flex_1())
                    .when(default == Some(branch), |row| {
                        row.child(div().text_xs().text_color(p.muted).child("default"))
                    })
            }))
    }
}

impl Render for NewWorktreeDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let label = |text: &'static str| {
            div()
                .h(px(18.))
                .px_1()
                .text_xs()
                .font_weight(gpui_kit::FontWeight::MEDIUM)
                .text_color(p.muted)
                .child(text)
        };
        let can_create = self.ready(cx).is_some();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap_2()
                    .child(modal::heading("New worktree"))
                    .child(
                        div()
                            .text_color(p.muted)
                            .child(format!("in {}", self.project_name)),
                    ),
            )
            .child(div().mt(px(14.)).child(modal::field(
                Input::new(&self.name).disabled(self.creating),
                p,
            )))
            .child(div().mt(px(6.)).child(self.preview(p, cx)))
            .child(div().mt(px(16.)).child(label(if self.existing {
                "Branch"
            } else {
                "Start from"
            })))
            .child(
                div().mt(px(4.)).child(modal::field(
                    Input::new(&self.filter)
                        .prefix(div().text_color(p.muted).child(icon(IconName::Search, 13.)))
                        .disabled(self.creating),
                    p,
                )),
            )
            .child(
                div()
                    .mt(px(6.))
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(self.branch_list(p, cx)),
            )
            .child(
                div()
                    .mt(px(12.))
                    .px_1()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div().flex_1().min_w_0().child("Work on this branch").child(
                            div()
                                .text_xs()
                                .text_color(p.muted)
                                .child("Check it out as it is, instead of branching from it."),
                        ),
                    )
                    .child(
                        Switch::new("new-worktree-existing")
                            .checked(self.existing)
                            .color(p.accent)
                            .disabled(self.creating)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.existing = *checked;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                modal::footer(self.error.clone(), p)
                    .mt(px(16.))
                    .child(modal::cancel("new-worktree-cancel"))
                    .child(
                        Button::new("new-worktree-create", "Create", Tone::Primary)
                            .busy(self.creating)
                            .disabled(!can_create)
                            .on_click(cx.listener(|this, _, window, cx| this.create(window, cx))),
                    ),
            )
    }
}
