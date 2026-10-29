//! The right panel beside the chat: the selected checkout's changes, or its files, each
//! opened and closed by its own button in the top bar. Each checkout in the sidebar keeps
//! its panels while it's there, so leaving it and coming back finds its open folders, its
//! pick and its scroll as they were. The panel follows the shell after each change to it
//! rather than while it's drawn, so nothing is made or swapped mid-frame.

use std::collections::HashMap;
use std::mem;
use std::path::{Path, PathBuf};

use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::Button;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Div, DragMoveEvent, Empty, Entity, FocusHandle,
    InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _, Render, Styled,
    Subscription, Window, actions, div, px,
};

use crate::changes::ChangesPanel;
use crate::checkout::Checkout;
use crate::files::{FilesPanel, Place};
use crate::go_to_file;
use crate::language;
use crate::navigation::Route;
use crate::settings;
use crate::shell::{Shell, ToggleRightPanel, icon_button};
use crate::sidebar::resize_handle;
use crate::theme::{self, Palette};

actions!(
    right_panel,
    [ShowFiles, ToggleChanges, ToggleFiles, ToggleWide]
);

pub use crate::list::{
    Activate, Collapse, Expand, NextFile, PageDown, PageUp, PreviousFile, SelectFirst, SelectLast,
    SelectNext, SelectPrevious, page,
};

/// ⌘⇧E anywhere for the files; and the keys that move through either panel's list, the same
/// in both, while it has focus.
pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("cmd-shift-e", ShowFiles, None)]);
    for context in ["ChangesFiles", "FilesTree"] {
        crate::list::bind_keys(context, cx);
        let list = Some(context);
        cx.bind_keys([
            KeyBinding::new("home", SelectFirst, list),
            KeyBinding::new("end", SelectLast, list),
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

impl View {
    /// How narrow and how wide the panel can be when it shows this, and how wide it's set.
    fn widths(self, settings: &settings::Settings) -> (f32, f32, f32) {
        match self {
            View::Changes => (
                theme::CHANGES_MIN_WIDTH,
                theme::CHANGES_MAX_WIDTH,
                settings.changes_width,
            ),
            View::Files => (
                theme::FILES_MIN_WIDTH,
                theme::FILES_MAX_WIDTH,
                settings.files_panel_width,
            ),
        }
    }
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
    /// Whether the panel has the card to itself, the chat out of sight, whichever it shows.
    wide: bool,
    /// Whether the panel takes the keyboard once it's next shown, as it does when opened.
    focus: bool,
    /// The panels of each checkout in the sidebar, by its path.
    checkouts: HashMap<PathBuf, Panels>,
    /// The checkout selected, as the panels were last told.
    selected: Option<PathBuf>,
    /// The widest the panel could be at the last frame and leave the chat room.
    widest: f32,
    /// The panel's width while its edge is dragged, before it's saved.
    dragged: Option<f32>,
}

impl RightPanel {
    /// Whether the panel is open on `view`.
    pub fn shows(&self, view: View) -> bool {
        self.open && self.view == view
    }

    fn shown(&self) -> Option<&Panels> {
        self.checkouts.get(self.selected.as_deref()?)
    }

    /// The selected checkout.
    pub fn checkout(&self) -> Option<&Entity<Checkout>> {
        Some(&self.shown()?.checkout)
    }

    /// The selected checkout's file tree, once it's been shown.
    #[cfg(any(test, feature = "visual-check"))]
    pub fn files(&self) -> Option<&Entity<FilesPanel>> {
        self.shown()?.files.as_ref()
    }

    /// Drops the Files panel of the checkout at `root`, and the files it showed with it.
    #[cfg(test)]
    pub fn forget_files(&mut self, root: &Path, cx: &mut App) {
        if let Some(panels) = self.checkouts.get_mut(root) {
            panels.files = None;
        }
        cx.refresh_windows();
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
        let wide = self.wide;
        let panel: AnyElement = match self.view {
            View::Changes => panels.changes.clone().map(Entity::into_any_element),
            View::Files => panels.files.clone().map(Entity::into_any_element),
        }
        .unwrap_or_else(|| Empty.into_any_element());
        let width = self.width(cx);
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

    /// The panel's width as set for what it shows, or as its edge is being dragged, within
    /// its limits, unless the window is too narrow for that and the chat both.
    fn width(&self, cx: &App) -> f32 {
        let (min, max, set) = self.view.widths(settings::get(cx));
        self.dragged
            .unwrap_or(set)
            .clamp(min, max)
            .min(self.widest)
            .max(min)
    }

    /// Saves the width the panel's edge was dragged to, once it's let go.
    fn let_go(&mut self, cx: &mut App) {
        if self.dragged.is_none() {
            return;
        }
        let width = self.width(cx);
        self.dragged = None;
        let view = self.view;
        settings::update(cx, |settings| match view {
            View::Changes => settings.changes_width = width,
            View::Files => settings.files_panel_width = width,
        });
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
        .on_action(cx.listener(|this, _: &ToggleWide, _, cx| {
            this.right_panel.wide = !this.right_panel.wide;
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &ShowFiles, _, cx| this.show(View::Files, cx)))
        .on_action(cx.listener(|this, _: &ToggleChanges, _, cx| this.toggle(View::Changes, cx)))
        .on_action(cx.listener(|this, _: &ToggleFiles, _, cx| this.toggle(View::Files, cx)))
        .map(|root| Self::handle_go_to_file(root, cx))
        .on_drag_move(cx.listener(Self::resize_right_panel))
        // The root's everywhere, so it hears the edge let go wherever that is.
        .capture_any_mouse_up(cx.listener(|this, _, _, cx| {
            this.right_panel.let_go(cx);
            cx.notify();
        }))
    }

    /// Shows `view` in the right panel, opening it if it's closed, and gives it the keyboard.
    fn show(&mut self, view: View, cx: &mut Context<Self>) {
        let panel = &mut self.right_panel;
        (panel.open, panel.view, panel.focus) = (true, view, true);
        self.navigate(Route::Workspace, cx);
        cx.notify();
    }

    /// Closes the right panel if it shows `view`, and shows `view` otherwise.
    fn toggle(&mut self, view: View, cx: &mut Context<Self>) {
        if self.right_panel.shows(view) {
            self.right_panel.open = false;
            cx.notify();
        } else {
            self.show(view, cx);
        }
    }

    /// Points the right panel at the selected checkout, and its file tree at the file the
    /// active tab shows; makes what it shows now if it isn't made; and lets go of the panels
    /// of checkouts gone from the sidebar. Runs after each change to the shell.
    pub fn follow_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (panel, workspace) = (&mut self.right_panel, &self.workspace);
        let mut gone = Vec::new();
        panel.checkouts.retain(|path, _| {
            let kept = workspace.project_of(path).is_some();
            if !kept {
                gone.push(path.clone());
            }
            kept
        });
        // Their language servers go with them.
        for root in gone {
            language::forget(&root, cx);
        }
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
        let (open, view, wide) = (panel.open, panel.view, panel.wide);
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
        // What's shown is told whether it has the card to itself as that changes, rather
        // than as it's drawn.
        if open && view == View::Changes {
            let changes = (panels.changes)
                .get_or_insert_with(|| cx.new(|cx| ChangesPanel::new(checkout.clone(), cx)));
            if changes.read(cx).is_wide() != wide {
                changes.update(cx, |changes, cx| changes.set_wide(wide, cx));
            }
            let handle = changes.read(cx).focus_handle().clone();
            focus_once(&mut focus, &handle, window, cx);
        }
        if open && view == View::Files {
            let files = (panels.files)
                .get_or_insert_with(|| cx.new(|cx| FilesPanel::new(checkout.clone(), cx)));
            if files.read(cx).is_wide() != wide {
                files.update(cx, |files, cx| files.set_wide(wide, cx));
            }
            let handle = files.read(cx).focus_handle().clone();
            focus_once(&mut focus, &handle, window, cx);
        }
    }

    /// Shows the selected checkout's file `place` is in, in the Files panel, opening the
    /// panel on its files if it isn't, and puts the cursor where the place says.
    pub fn view_file(&mut self, place: Place, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(checkout) = self.right_panel.checkout().cloned() {
            go_to_file::opened(&checkout.read(cx).root().clone(), &place.path, cx);
        }
        self.show(View::Files, cx);
        // Made now rather than after this change, to be shown the file.
        self.follow_selection(window, cx);
        let Some(files) = self
            .right_panel
            .shown()
            .and_then(|panels| panels.files.clone())
        else {
            return;
        };
        files.update(cx, |files, cx| files.view(place, window, cx));
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
        if self.right_panel.dragged != Some(width) {
            self.right_panel.dragged = Some(width);
            cx.notify();
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

/// The button at the right of either panel's header that gives the panel the whole card, or
/// puts it back beside the chat.
pub fn wide_button(id: &'static str, wide: bool, cx: &App) -> Button {
    let (glyph, tip) = if wide {
        (IconName::Minimize2, "Back beside the chat")
    } else {
        (IconName::Maximize2, "Use the whole card")
    };
    icon_button(id, glyph, tip, cx)
        .xsmall()
        .on_click(|_, window, cx| window.dispatch_action(Box::new(ToggleWide), cx))
}

/// What either panel's header says it shows, at its left.
pub fn title(label: &'static str, p: Palette) -> Div {
    div().text_sm().text_color(p.text).child(label)
}

/// Either panel's header: a strip as tall as the tabs', its title at its left.
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

mod buttons;
pub mod split;
#[cfg(test)]
mod tests;
