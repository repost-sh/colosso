//! General: how the sidebar opens, which layout holds it, and how wide it is; where new
//! worktrees go and what their branches are named under.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::component::switch::Switch;
use gpui_kit::{
    App, AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement as _, Render,
    Styled as _, Subscription, Window, actions, div, px,
};

use super::components::{MenuButton, PillButton, SettingRow, SettingsSection};
use crate::modal::{self, Button, Tone};
use crate::projects::{display_path, pick_folder};
use crate::settings::{self, BranchPrefix, SetBranchPrefix};
use crate::theme;

actions!(settings, [EditBranchPrefix]);

pub fn sections(cx: &App) -> Vec<SettingsSection> {
    vec![sidebar(cx), worktrees(cx)]
}

fn sidebar(cx: &App) -> SettingsSection {
    let p = theme::palette(cx);
    let current = settings::get(cx);
    let layout = current.layout;
    let width = current.sidebar_width().round();
    SettingsSection::new("Sidebar")
        .row(
            SettingRow::new("Show sidebar on launch")
                .description("Open the window with the sidebar out. ⌘S slides it in and out.")
                .control(
                    Switch::new("sidebar-open-on-launch")
                        .checked(current.sidebar_open_on_launch)
                        .color(p.accent)
                        .on_click(|open, _, cx| {
                            settings::update(cx, |settings| settings.sidebar_open_on_launch = *open)
                        }),
                ),
        )
        .row(
            SettingRow::new("Layout")
                .description(
                    "Orca keeps the sidebar and tabs inside the card. Arc lists tabs on the glass.",
                )
                .control(MenuButton::new("layout", layout.label(), |cx| {
                    settings::get(cx).layout.menu()
                })),
        )
        .row(
            SettingRow::new("Sidebar width")
                .description("Drag the sidebar's right edge to resize it.")
                .control(
                    div()
                        .text_sm()
                        .text_color(p.muted)
                        .child(format!("{width} pt")),
                )
                .control(
                    PillButton::new("reset-sidebar-width", "Reset")
                        .disabled(width == theme::SIDEBAR_WIDTH)
                        .on_click(|_, _, cx| {
                            settings::update(cx, |settings| {
                                settings.sidebar_width = theme::SIDEBAR_WIDTH
                            })
                        }),
                ),
        )
}

fn worktrees(cx: &App) -> SettingsSection {
    let p = theme::palette(cx);
    let current = settings::get(cx);
    let location = current.worktrees_root();
    SettingsSection::new("Worktrees")
        .row(
            SettingRow::new("Location")
                .description("New worktrees go here, in a folder for each project.")
                .control(
                    div()
                        .max_w(px(220.))
                        .truncate()
                        .text_sm()
                        .text_color(p.muted)
                        .child(display_path(&location)),
                )
                .control(
                    PillButton::new("choose-worktrees-dir", "Choose…").on_click(|_, _, cx| {
                        let picked = pick_folder("Use Folder", cx);
                        cx.spawn(async move |cx| {
                            if let Some(folder) = picked.await {
                                cx.update(|cx| {
                                    settings::update(cx, |settings| {
                                        settings.worktrees_dir = Some(folder)
                                    })
                                });
                            }
                        })
                        .detach();
                    }),
                )
                .control(
                    PillButton::new("reset-worktrees-dir", "Reset")
                        .disabled(current.worktrees_dir.is_none())
                        .on_click(|_, _, cx| {
                            settings::update(cx, |settings| settings.worktrees_dir = None)
                        }),
                ),
        )
        .row(
            SettingRow::new("Branch prefix")
                .description(
                    "New branches are named <prefix>/<name>. Your GitHub username comes from \
                     git's config or the GitHub CLI.",
                )
                .control(MenuButton::new(
                    "branch-prefix",
                    prefix_label(&current.branch_prefix),
                    |cx| prefix_menu(&settings::get(cx).branch_prefix),
                )),
        )
}

fn prefix_label(prefix: &BranchPrefix) -> String {
    match prefix {
        BranchPrefix::GitHubUser => "GitHub username".into(),
        BranchPrefix::Custom(prefix) => prefix.clone(),
        BranchPrefix::None => "None".into(),
    }
}

fn prefix_menu(current: &BranchPrefix) -> NativeMenu {
    NativeMenu::new()
        .menu_with_check(
            "GitHub username",
            *current == BranchPrefix::GitHubUser,
            Box::new(SetBranchPrefix(BranchPrefix::GitHubUser)),
        )
        .menu_with_check(
            "Custom…",
            matches!(current, BranchPrefix::Custom(_)),
            Box::new(EditBranchPrefix),
        )
        .menu_with_check(
            "None",
            *current == BranchPrefix::None,
            Box::new(SetBranchPrefix(BranchPrefix::None)),
        )
}

/// Asks for a prefix of the user's own. Saving it empty means no prefix.
pub fn edit_branch_prefix(window: &mut Window, cx: &mut App) {
    modal::open(380., 212., window, cx, BranchPrefixDialog::new);
}

struct BranchPrefixDialog {
    input: Entity<InputState>,
    _subscription: Subscription,
}

impl BranchPrefixDialog {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let current = match &settings::get(cx).branch_prefix {
            BranchPrefix::Custom(prefix) => prefix.clone(),
            _ => String::new(),
        };
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("e.g. your initials")
                .default_value(current)
        });
        input.focus_handle(cx).focus(window, cx);
        let _subscription = cx.subscribe_in(&input, window, |this, _, event, window, cx| {
            if let InputEvent::PressEnter { .. } = event {
                this.save(window, cx);
            }
        });
        BranchPrefixDialog {
            input,
            _subscription,
        }
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prefix = self
            .input
            .read(cx)
            .value()
            .trim()
            .trim_matches('/')
            .to_owned();
        settings::update(cx, |settings| {
            settings.branch_prefix = match prefix.is_empty() {
                true => BranchPrefix::None,
                false => BranchPrefix::Custom(prefix),
            }
        });
        modal::dismiss(window, cx);
    }
}

impl Render for BranchPrefixDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(modal::heading("Branch prefix"))
            .child(div().mt(px(4.)).text_xs().text_color(p.muted).child(
                "New branches go under it, as in prefix/fix-login. Leave it empty for none.",
            ))
            .child(
                div()
                    .mt(px(14.))
                    .child(modal::field(Input::new(&self.input), p)),
            )
            .child(div().flex_1())
            .child(
                modal::footer(None, p)
                    .child(modal::cancel("branch-prefix-cancel"))
                    .child(
                        Button::new("branch-prefix-save", "Save", Tone::Primary)
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    ),
            )
    }
}
