//! The right panel beside the chat: the selected checkout's changed files, or its file tree,
//! each opened and closed by its own button, on the branch's row in the sidebar or at the top
//! rail's end, where Changes and Files places them (`buttons`);
//! a file picked in either opens in the workspace's tabs, its diff in the checkout's Changes tab
//! or the file in a tab of its own. Each checkout in the sidebar keeps its panels while it's
//! there, and whether its panel's open and on which (`side`), so leaving it and coming back
//! finds its panel, its open folders, its pick and its scroll as they were. The panel follows the
//! shell after each change to it rather than while it's drawn, so nothing is made or swapped
//! mid-frame.

use std::collections::HashMap;
use std::mem;
use std::path::{Path, PathBuf};

use gpui_kit::{
    AnyElement, AnyWindowHandle, App, AppContext as _, Context, Div, DragMoveEvent, Empty, Entity,
    FocusHandle, IntoElement, KeyBinding, ParentElement as _, Styled, Subscription, WeakEntity,
    Window, actions, div, px,
};

use crate::changes::ChangesPanel;
use crate::checkout::{Checkout, HeadMoved, Seen, TotalsChanged};
use crate::files::{FilesPanel, Place};
use crate::language;
use crate::settings;
use crate::shell::Shell;
use crate::theme::{self, Palette};

actions!(right_panel, [ShowFiles, ToggleChanges, ToggleFiles]);

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
                theme::CHANGES_FILES_MIN_WIDTH,
                theme::CHANGES_FILES_MAX_WIDTH,
                settings.changes_files_width,
            ),
            View::Files => (
                theme::FILES_TREE_MIN_WIDTH,
                theme::FILES_TREE_MAX_WIDTH,
                settings.files_tree_width,
            ),
        }
    }
}

/// The drag payload while the right panel's left edge is being dragged.
#[derive(Clone)]
pub struct RightPanelResize;

/// One checkout's panels, each made the first time it's shown.
struct Panels {
    checkout: Entity<Checkout>,
    /// The lines its changes add and remove, for the changes button: the shell reads these, not
    /// the checkout, which would draw it at each of the checkout's changes.
    totals: (u32, u32),
    changes: Option<Entity<ChangesPanel>>,
    files: Option<Entity<FilesPanel>>,
    _subscriptions: Vec<Subscription>,
}

impl Panels {
    /// The changes panel, made now if it isn't: a file picked in its list shows the diffs.
    fn changes(&mut self, window: &mut Window, cx: &mut Context<Shell>) -> Entity<ChangesPanel> {
        if let Some(changes) = &self.changes {
            return changes.clone();
        }
        let checkout = self.checkout.clone();
        let changes = cx.new(|cx| ChangesPanel::new(checkout, cx));
        (self._subscriptions).push(cx.subscribe_in(&changes, window, Shell::changes_event));
        self.changes = Some(changes.clone());
        changes
    }
}

#[derive(Default)]
pub struct RightPanel {
    /// What each checkout's panel shows, by its path: closed for one that isn't here.
    sides: HashMap<PathBuf, side::Side>,
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
    fn shown(&self) -> Option<&Panels> {
        self.checkouts.get(self.selected.as_deref()?)
    }

    /// The selected checkout.
    pub fn checkout(&self) -> Option<&Entity<Checkout>> {
        Some(&self.shown()?.checkout)
    }

    /// The selected checkout, if it's the one at `root`.
    pub fn checkout_at(&self, root: &Path) -> Option<&Entity<Checkout>> {
        self.checkout().filter(|_| self.is_selected(root))
    }

    fn is_selected(&self, root: &Path) -> bool {
        self.selected.as_deref() == Some(root)
    }

    /// The selected checkout's changed files, once they've been shown.
    #[cfg(test)]
    pub fn changes(&self) -> Option<&Entity<ChangesPanel>> {
        self.shown()?.changes.as_ref()
    }

    /// The selected checkout's file tree, once it's been shown.
    pub fn files(&self) -> Option<&Entity<FilesPanel>> {
        self.shown()?.files.as_ref()
    }

    /// The lines the changes of the checkout at `root` add and remove, while it's the selected
    /// one and they've been read.
    pub fn totals_in(&self, root: &Path) -> (u32, u32) {
        let shown = self.shown().filter(|_| self.is_selected(root));
        shown.map_or((0, 0), |panels| panels.totals)
    }

    /// The right panel beside the tab shown, if it's open while it's `shown`, `beside` being
    /// how wide what's left of the chat is.
    pub fn render(
        &mut self,
        shown: bool,
        beside: f32,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Div> {
        self.widest = widest(beside, window);
        let side = self.selected_side();
        let panels = self.shown().filter(|_| shown && side.open)?;
        let panel: AnyElement = match side.view {
            View::Changes => panels.changes.clone().map(Entity::into_any_element),
            View::Files => panels.files.clone().map(Entity::into_any_element),
        }
        .unwrap_or_else(|| Empty.into_any_element());
        let width = self.width(cx);
        let p = theme::palette(cx);
        Some(
            div()
                .h_full()
                .flex_shrink_0()
                .flex()
                .child(theme::card::seam("right-panel-resize", true, RightPanelResize))
                .child(
                    theme::card::island(p)
                        .h_full()
                        .w(px(width))
                        .flex_shrink_0()
                        .children(crate::chat_backdrop::backdrop(
                            settings::Surface::Explorer,
                            "explorer-backdrop",
                            cx,
                        ))
                        .child(panel),
                ),
        )
    }

    /// The panel's width as set for what it shows, or as its edge is being dragged, within
    /// its limits, unless the window is too narrow for that and the chat both.
    fn width(&self, cx: &App) -> f32 {
        let (min, max, set) = self.selected_side().view.widths(settings::get(cx));
        self.dragged
            .unwrap_or(set)
            .clamp(min, max)
            .min(self.widest)
            .max(min)
    }

    /// Whether the panel's edge is being dragged.
    pub(crate) fn is_dragged(&self) -> bool {
        self.dragged.is_some()
    }

    /// Saves the width the panel's edge was dragged to, once it's let go.
    pub(crate) fn let_go(&mut self, cx: &mut App) {
        if self.dragged.is_none() {
            return;
        }
        let width = self.width(cx);
        self.dragged = None;
        let view = self.selected_side().view;
        settings::update(cx, |settings| match view {
            View::Changes => settings.changes_files_width = width,
            View::Files => settings.files_tree_width = width,
        });
    }

    /// Makes the panels of the checkout at `path`, unless it has them, and tells the language
    /// servers whether it's seen, now and as that changes. The shell going with its window
    /// takes it out of sight, unless it left the sidebar first, which lets go of its servers.
    fn make_panels(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Shell>) {
        if !self.checkouts.contains_key(path) {
            let checkout = cx.new(|cx| Checkout::new(path.to_owned(), window, cx));
            let root = path.to_owned();
            let counted = cx.subscribe(&checkout, move |shell, _, TotalsChanged(totals), cx| {
                if let Some(panels) = shell.right_panel.checkouts.get_mut(&root) {
                    panels.totals = *totals;
                }
                if shell.right_panel.is_selected(&root) {
                    cx.notify();
                }
            });
            let worktree = path.to_owned();
            let moved = cx.subscribe(&checkout, move |shell, _, _: &HeadMoved, cx| {
                let project = shell.workspace.project_of(&worktree);
                if let Some(project) = project.map(|project| project.path.clone()) {
                    shell.reload_project(project, cx);
                }
            });
            let root = path.to_owned();
            let seen = cx.subscribe(&checkout, move |_, _, Seen(seen), cx| {
                language::seen(&root, *seen, cx);
            });
            let root = path.to_owned();
            let gone = cx.on_release(move |_, cx| language::seen(&root, false, cx));
            language::seen(path, checkout.read(cx).is_seen(), cx);
            let panels = Panels {
                totals: checkout.read(cx).totals(),
                checkout,
                changes: None,
                files: None,
                _subscriptions: vec![counted, moved, seen, gone],
            };
            self.checkouts.insert(path.to_owned(), panels);
        }
    }
}

impl Shell {
    /// Lets go of the panels of checkouts gone from the sidebar, and of their language servers.
    /// Says how many.
    pub(crate) fn let_go_of_checkouts_gone(&mut self, cx: &mut Context<Self>) -> usize {
        let (panel, workspace) = (&mut self.right_panel, &self.workspace);
        let mut gone = Vec::new();
        panel.checkouts.retain(|path, _| {
            let kept = workspace.project_of(path).is_some();
            if !kept {
                gone.push(path.clone());
            }
            kept
        });
        (panel.sides).retain(|path, _| workspace.project_of(path).is_some());
        for root in &gone {
            language::forget(root, cx);
        }
        gone.len()
    }

    /// Points the right panel at the selected checkout, and its file tree at the file the
    /// shown tab shows; makes what it shows now if it isn't made; colours the checkout's
    /// diffs while its Changes tab is shown. Runs after each change to the shell.
    pub fn follow_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (panel, workspace) = (&mut self.right_panel, &self.workspace);
        let selected = workspace
            .branch()
            .map(|branch| branch.worktree.path.as_path());
        let located = (workspace.project().zip(selected))
            .map(|(project, root)| (root, project.path.as_path()));
        self.status_bar.follow(located, cx);
        let reselected = panel.selected.as_deref() != selected;
        if reselected {
            if let Some(left) = panel.shown() {
                Checkout::show(&left.checkout, false, false, cx);
            }
            panel.selected = selected.map(Path::to_path_buf);
        }
        let Some(root) = selected else {
            return;
        };
        let side::Side { open, view } = panel.side(root);
        let diffs_shown = (workspace.tab_shown())
            .is_some_and(|tab| tab.any_pane(&|pane| pane.changes().is_some()));
        let mut focus = mem::take(&mut panel.focus);
        panel.make_panels(root, window, cx);
        let Some(panels) = panel.checkouts.get_mut(root) else {
            return;
        };
        let checkout = panels.checkout.clone();
        Checkout::show(&checkout, true, diffs_shown, cx);
        if open && view == View::Changes {
            let changes = panels.changes(window, cx);
            let handle = changes.read(cx).focus_handle().clone();
            focus_once(&mut focus, &handle, window, cx);
        }
        if open && view == View::Files && panels.files.is_none() {
            let files = cx.new(|cx| FilesPanel::new(checkout.clone(), cx));
            (panels._subscriptions).push(cx.subscribe_in(&files, window, Self::files_event));
            panels.files = Some(files);
        }
        if let Some(files) = panels.files.clone() {
            let shown = self.shown_file(window, cx);
            let active = shown.and_then(|file| file.read(cx).relative.clone());
            FilesPanel::set_active(&files, active, cx);
            if open && view == View::Files {
                let handle = files.read(cx).focus_handle().clone();
                focus_once(&mut focus, &handle, window, cx);
            }
        }
    }

    /// The selected checkout's changes panel, made now if it isn't, as the panel would be on
    /// it.
    pub(crate) fn changes_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<ChangesPanel>> {
        let root = self.right_panel.selected.clone()?;
        let panels = self.right_panel.checkouts.get_mut(&root)?;
        Some(panels.changes(window, cx))
    }

    /// Shows the file `place` is in from a dialog over `behind`, in a tab: as
    /// [`Self::view_file`] does, in that window.
    pub fn view_behind(
        shell: &WeakEntity<Shell>,
        behind: AnyWindowHandle,
        place: Place,
        cx: &mut App,
    ) {
        let shown = behind.update(cx, |_, window, cx| {
            shell.update(cx, |shell, cx| shell.view_file(place, window, cx))
        });
        if let Err(error) = shown.and_then(|shown| shown) {
            log::warn!("couldn't show the file picked: {error}");
        }
    }

    fn resize_right_panel(
        &mut self,
        event: &DragMoveEvent<RightPanelResize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let right = window.viewport_size().width.as_f32() - theme::GUTTER;
        let width = (right - event.event.position.x.as_f32()).min(self.right_panel.widest);
        if self.right_panel.dragged != Some(width) {
            self.right_panel.dragged = Some(width);
            cx.notify();
        }
    }
}

/// The widest the right panel can be and leave the chat room, `beside` being how wide what's
/// left of the chat is: the left gutter and the sidebar, or in compact mode the gutter alone.
fn widest(beside: f32, window: &Window) -> f32 {
    window.viewport_size().width.as_f32() - theme::GUTTER - beside - theme::CHAT_MIN_WIDTH
}

/// Gives `handle` the keyboard if it's `pending`, and then not again.
fn focus_once(pending: &mut bool, handle: &FocusHandle, window: &mut Window, cx: &mut App) {
    if mem::take(pending) {
        window.focus(handle, cx);
    }
}

/// What either panel's header says it shows, at its left.
pub fn title(label: &'static str, p: Palette) -> Div {
    div().text_sm().text_color(p.text).child(label)
}

/// Either panel's header, its title at its left.
pub fn panel_header(p: Palette) -> Div {
    div()
        .h(px(theme::PANEL_HEADER_HEIGHT))
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
mod side;
#[cfg(test)]
mod tests;
