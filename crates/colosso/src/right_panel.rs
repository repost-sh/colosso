//! The right panel beside the chat: the selected checkout's changes, or its files, with a
//! switch between the two at the left of either's header. Each checkout in the sidebar keeps
//! its panels while it's there, so leaving it and coming back finds its open folders, its
//! pick and its scroll as they were. The panel follows the shell after each change to it
//! rather than while it's drawn, so nothing is made or swapped mid-frame.

use std::collections::HashMap;
use std::mem;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::component::{Selectable as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Div, DragMoveEvent, Empty, Entity, FocusHandle,
    InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _, Render, Styled,
    Subscription, UniformListScrollHandle, Window, actions, div, px,
};

use crate::changes::{ChangesPanel, ToggleChangesWidth};
use crate::checkout::Checkout;
use crate::files::{FileEditor, FilesPanel, OpenFile};
use crate::model::FileTab;
use crate::navigation::Route;
use crate::settings;
use crate::shell::{Shell, ToggleRightPanel, glass};
use crate::sidebar::resize_handle;
use crate::theme::{self, Palette};

actions!(right_panel, [ShowChanges, ShowFiles]);

actions!(
    list,
    [
        SelectPrevious,
        SelectNext,
        SelectFirst,
        SelectLast,
        PageUp,
        PageDown,
        PreviousFile,
        NextFile,
        Collapse,
        Expand,
        Activate
    ]
);

/// ⌘⇧E anywhere for the files; and the keys that move through either panel's list, the same
/// in both, while it has focus.
pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("cmd-shift-e", ShowFiles, None)]);
    for list in ["ChangesFiles", "FilesTree"] {
        let list = Some(list);
        cx.bind_keys([
            KeyBinding::new("up", SelectPrevious, list),
            KeyBinding::new("down", SelectNext, list),
            KeyBinding::new("home", SelectFirst, list),
            KeyBinding::new("end", SelectLast, list),
            KeyBinding::new("pageup", PageUp, list),
            KeyBinding::new("pagedown", PageDown, list),
            KeyBinding::new("k", PreviousFile, list),
            KeyBinding::new("j", NextFile, list),
            KeyBinding::new("left", Collapse, list),
            KeyBinding::new("right", Expand, list),
            KeyBinding::new("enter", Activate, list),
            KeyBinding::new("space", Activate, list),
        ]);
    }
}

/// What the right panel shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum View {
    #[default]
    Changes,
    Files,
}

/// The drag payload while the right panel's left border is being dragged.
#[derive(Clone)]
pub struct RightPanelResize;

impl Render for RightPanelResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// One checkout's panels, each made the first time it's shown.
struct Panels {
    checkout: Entity<Checkout>,
    changes: Option<Entity<ChangesPanel>>,
    files: Option<Entity<FilesPanel>>,
    _subscriptions: Vec<Subscription>,
}

#[derive(Default)]
pub struct RightPanel {
    open: bool,
    view: View,
    /// Whether the changes have the card to themselves.
    wide: bool,
    /// Whether the panel takes the keyboard once it's next shown, as it does when opened.
    focus: bool,
    /// The panels of each checkout in the sidebar, by its path.
    checkouts: HashMap<PathBuf, Panels>,
    /// The checkout selected, and the file its active tab shows, as the panels were last told.
    selected: Option<PathBuf>,
    active: Option<PathBuf>,
    /// The widest the panel could be at the last frame and leave the chat room.
    widest: f32,
}

impl RightPanel {
    pub fn is_open(&self) -> bool {
        self.open
    }

    fn shown(&self) -> Option<&Panels> {
        self.checkouts.get(self.selected.as_deref()?)
    }

    /// The selected checkout.
    pub fn checkout(&self) -> Option<&Entity<Checkout>> {
        Some(&self.shown()?.checkout)
    }

    /// The selected checkout's file tree, once it's been shown.
    #[cfg(feature = "visual-check")]
    pub fn files(&self) -> Option<&Entity<FilesPanel>> {
        self.shown()?.files.as_ref()
    }

    /// The lines the selected checkout's changes add and remove, once they've been read.
    pub fn totals(&self, cx: &App) -> (u32, u32) {
        (self.checkout()).map_or((0, 0), |checkout| checkout.read(cx).totals())
    }

    /// The right panel as the card shows it, if it's open while it's `shown`; and whether the
    /// changes have the card to themselves.
    pub fn render(
        &mut self,
        shown: bool,
        sidebar_open: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> (Option<Div>, bool) {
        self.widest = widest(sidebar_open, window, cx);
        let Some(panels) = self.shown().filter(|_| shown && self.open) else {
            return (None, false);
        };
        let wide = self.view == View::Changes && self.wide;
        let (panel, width): (AnyElement, f32) = match self.view {
            View::Changes => {
                let Some(changes) = panels.changes.clone() else {
                    return (None, false);
                };
                let width = self.panel_width(
                    theme::CHANGES_MIN_WIDTH,
                    theme::CHANGES_MAX_WIDTH,
                    settings::get(cx).changes_width(),
                );
                changes.update(cx, |changes, cx| {
                    changes.set_width((!wide).then_some(width), cx);
                });
                (changes.into_any_element(), width)
            }
            View::Files => {
                let Some(files) = panels.files.clone() else {
                    return (None, false);
                };
                let width = self.panel_width(
                    theme::FILES_MIN_WIDTH,
                    theme::FILES_MAX_WIDTH,
                    settings::get(cx).files_width(),
                );
                (files.into_any_element(), width)
            }
        };
        let p = theme::palette(cx);
        let panel = div()
            .h_full()
            .min_w_0()
            .when(wide, Styled::flex_1)
            .when(!wide, |panel| {
                panel
                    .relative()
                    .w(px(width))
                    .flex_shrink_0()
                    .border_l_1()
                    .border_color(p.hairline)
            })
            .child(panel)
            // Over the panel's edge, so it's grabbed before what's under it.
            .when(!wide, |panel| {
                let handle = resize_handle("right-panel-resize", RightPanelResize);
                panel.child(handle.left_0())
            });
        (Some(panel), wide)
    }

    /// The panel's width from its `setting`, between `min` and `max`, unless the window is
    /// too narrow for that and the chat both.
    fn panel_width(&self, min: f32, max: f32, setting: f32) -> f32 {
        setting.clamp(min, max).min(self.widest).max(min)
    }

    /// Makes the panels of the checkout at `path`, unless it has them.
    fn make_panels(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Shell>) {
        if !self.checkouts.contains_key(path) {
            let checkout = cx.new(|cx| Checkout::new(path.to_owned(), window, cx));
            // The top rail counts its changes.
            let observing = cx.observe(&checkout, |_, _, cx| cx.notify());
            let panels = Panels {
                checkout,
                changes: None,
                files: None,
                _subscriptions: vec![observing],
            };
            self.checkouts.insert(path.to_owned(), panels);
        }
    }
}

impl Shell {
    /// What the window does with the right panel's actions and drags.
    pub fn handle_right_panel(root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &ToggleRightPanel, _, cx| {
            let panel = &mut this.right_panel;
            panel.open = !panel.open;
            panel.focus = panel.open;
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &ToggleChangesWidth, _, cx| {
            this.right_panel.wide = !this.right_panel.wide;
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &ShowFiles, _, cx| this.show(View::Files, cx)))
        .on_action(cx.listener(|this, _: &ShowChanges, _, cx| this.show(View::Changes, cx)))
        .on_drag_move(cx.listener(Self::resize_right_panel))
    }

    /// Shows `view` in the right panel, opening it if it's closed, and gives it the keyboard.
    fn show(&mut self, view: View, cx: &mut Context<Self>) {
        let panel = &mut self.right_panel;
        (panel.open, panel.view, panel.focus) = (true, view, true);
        self.navigate(Route::Workspace, cx);
        cx.notify();
    }

    /// Points the right panel at the selected checkout, and its file tree at the file the
    /// active tab shows; makes what it shows now if it isn't made; and lets go of the panels
    /// of checkouts gone from the sidebar. Runs after each change to the shell.
    pub fn follow_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (panel, workspace) = (&mut self.right_panel, &self.workspace);
        panel
            .checkouts
            .retain(|path, _| workspace.project_of(path).is_some());
        let selected = workspace
            .branch()
            .map(|branch| branch.worktree.path.as_path());
        let reselected = panel.selected.as_deref() != selected;
        if reselected {
            if let Some(left) = panel.shown() {
                left.checkout.update(cx, |checkout, cx| {
                    checkout.set_selected(false, cx);
                    checkout.set_coloured(false, cx);
                });
            }
            panel.selected = selected.map(Path::to_path_buf);
        }
        let Some(root) = selected else {
            return;
        };
        let (open, view) = (panel.open, panel.view);
        let mut focus = mem::take(&mut panel.focus);
        panel.make_panels(root, window, cx);
        let Some(panels) = panel.checkouts.get_mut(root) else {
            return;
        };
        let checkout = panels.checkout.clone();
        checkout.update(cx, |checkout, cx| {
            checkout.set_selected(true, cx);
            checkout.set_coloured(open && view == View::Changes, cx);
        });
        let mut made_files = false;
        if open && view == View::Changes {
            let changes = (panels.changes)
                .get_or_insert_with(|| cx.new(|cx| ChangesPanel::new(checkout.clone(), cx)));
            let handle = changes.read(cx).focus_handle().clone();
            focus_once(&mut focus, &handle, window, cx);
        }
        if open && view == View::Files {
            if panels.files.is_none() {
                let files = cx.new(|cx| FilesPanel::new(checkout.clone(), cx));
                let opening =
                    cx.subscribe_in(&files, window, |this, _, OpenFile(path), window, cx| {
                        this.open_file(path, window, cx);
                    });
                panels._subscriptions.push(opening);
                panels.files = Some(files);
                made_files = true;
            }
            if let Some(files) = &panels.files {
                let handle = files.read(cx).focus_handle().clone();
                focus_once(&mut focus, &handle, window, cx);
            }
        }
        let files = panels.files.clone();
        // The tree picks the file the active tab shows, once it changes.
        let active = workspace.active_file();
        if reselected || made_files || panel.active.as_deref() != active {
            panel.active = active.map(Path::to_path_buf);
            let relative = (active.and_then(|path| path.strip_prefix(root).ok()))
                .and_then(Path::to_str)
                .map(Arc::from);
            if let Some(files) = files {
                files.update(cx, |files, cx| files.set_active(relative, cx));
            }
        }
    }

    /// Opens the selected checkout's file at `path`, relative to it, in a tab of its own, or
    /// selects the tab it's open in.
    pub fn open_file(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(checkout) = self.right_panel.checkout().cloned() else {
            return;
        };
        let path = checkout.read(cx).root().join(path);
        if !self.workspace.select_file(&path) {
            let document = checkout.update(cx, |checkout, cx| checkout.document(&path, cx));
            let editor = cx.new(|cx| FileEditor::new(document, window, cx));
            self.workspace.open_file(FileTab { path, editor });
        }
        self.navigate(Route::Workspace, cx);
        cx.notify();
    }

    fn resize_right_panel(
        &mut self,
        event: &DragMoveEvent<RightPanelResize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The panel ends at the card's right edge, a gutter in from the window's.
        let right = window.viewport_size().width.as_f32() - theme::GUTTER;
        let width = (right - event.event.position.x.as_f32()).min(self.right_panel.widest);
        match self.right_panel.view {
            View::Changes => settings::update(cx, |settings| {
                settings.changes_width =
                    width.clamp(theme::CHANGES_MIN_WIDTH, theme::CHANGES_MAX_WIDTH);
            }),
            View::Files => settings::update(cx, |settings| {
                settings.files_width = width.clamp(theme::FILES_MIN_WIDTH, theme::FILES_MAX_WIDTH);
            }),
        }
    }
}

/// The widest the right panel can be and leave the chat room beside the rail and the sidebar.
fn widest(sidebar_open: bool, window: &Window, cx: &App) -> f32 {
    let sidebar = if sidebar_open {
        settings::get(cx).sidebar_width()
    } else {
        0.
    };
    window.viewport_size().width.as_f32()
        - theme::GUTTER
        - theme::ICON_RAIL_WIDTH
        - sidebar
        - theme::CHAT_MIN_WIDTH
}

/// Gives `handle` the keyboard if it's `pending`, and then not again.
fn focus_once(pending: &mut bool, handle: &FocusHandle, window: &mut Window, cx: &mut App) {
    if mem::take(pending) {
        window.focus(handle, cx);
    }
}

/// The switch between the changes and the files, at the left of either's header, `view`
/// being the one shown.
pub fn switch(view: View, cx: &App) -> impl IntoElement + use<> {
    let p = theme::palette(cx);
    let choice = |id: &'static str, label: &'static str, this: View| {
        let shown = view == this;
        glass(id, if shown { p.text } else { p.muted }, cx)
            .label(label)
            .small()
            .selected(shown)
    };
    div()
        .flex()
        .items_center()
        .gap_0p5()
        .child(
            choice("right-panel-changes", "Changes", View::Changes).on_click(|_, window, cx| {
                window.dispatch_action(Box::new(ShowChanges), cx);
            }),
        )
        .child(
            choice("right-panel-files", "Files", View::Files).on_click(|_, window, cx| {
                window.dispatch_action(Box::new(ShowFiles), cx);
            }),
        )
}

/// Either panel's header: a strip as tall as the tabs', the switch at its left.
pub fn panel_header(p: Palette) -> Div {
    div()
        .h(px(theme::TAB_STRIP_HEIGHT))
        .flex_shrink_0()
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .border_b_1()
        .border_color(p.hairline)
}

/// What either panel says in place of its list: nothing changed, or why there's nothing.
pub fn note(p: Palette) -> Div {
    div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .px_4()
        .text_sm()
        .text_color(p.muted)
}

/// How many of a list's rows, `row` high, fit in it: a page's step.
pub fn page(scroll: &UniformListScrollHandle, row: f32) -> isize {
    let height = scroll.0.borrow().base_handle.bounds().size.height.as_f32();
    (height / row).floor().max(1.) as isize
}

#[cfg(test)]
mod tests;
