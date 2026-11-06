//! The terminals the tabs show, and only tabs (`docs/terminals.md` 7.1): the app's one
//! client of the daemon, which runs inside this executable as `koyo termd`, and a view
//! per terminal tab, made the first time the tab is drawn so a tab never looked at never
//! attaches. Closing a tab ends its terminal; quitting the app can leave terminals running,
//! and they come back as tabs when it opens again.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, Focusable as _, Hsla,
    InteractiveElement as _, IntoElement as _, ParentElement as _, Rgba, Styled as _, Subscription,
    Task, Window, div, px,
};
use koyo_term::{LinkTarget, Look, Source, TermClient, TerminalEvent, TerminalView};
use koyo_term_control::{DaemonCommand, Handle, IfExists, OpenRequest, Selector, TermDir, Theme};

use crate::model::{Pane, Workspace};
use crate::settings;
use crate::shell::Shell;
use crate::theme::{self, Palette};

mod beside;
mod header;
mod open;
mod panel;
mod responsive;
mod restore;
pub(crate) mod states;
mod stop;
mod typing;
#[cfg(feature = "visual-check")]
pub mod visual;
mod watch;

pub use responsive::handle_actions;
pub(crate) use stop::close_all;

#[derive(Default)]
pub struct Terminals {
    client: Option<Arc<TermClient>>,
    next: u64,
    /// What each terminal tab shows, until its view is made.
    sources: HashMap<u64, Source>,
    views: HashMap<u64, Made>,
    /// Checkouts whose running terminals were looked for, so they're restored once.
    restored: HashSet<PathBuf>,
    /// The tab whose view takes the keyboard once it's made: the terminal just opened.
    focus_next: Option<u64>,
    /// What the daemon says of every terminal, and the task that keeps it current.
    states: states::States,
    watching: Option<Task<()>>,
    /// The activity panels shown, by terminal tab, and their share of the pane once dragged.
    panels: HashMap<u64, Entity<panel::ActivityPanel>>,
    panel_share: Option<f32>,
    /// The theme's send to the daemon, waiting for the theme to settle.
    theme_send: Option<Task<()>>,
    /// What to type into a terminal once the daemon says it's at its prompt, by its tab.
    typing: HashMap<u64, String>,
}

/// How long the theme stays the same before the daemon is told it.
const THEME_DELAY: Duration = Duration::from_millis(150);

/// A terminal tab's view once it's made, and responsive mode round it, made with it.
struct Made {
    view: Entity<TerminalView>,
    responsive: Entity<responsive::ResponsiveTerminal>,
    /// Whether the terminal was pinned when the view last said its pin changed: the terminal
    /// bar's toggle shows it, so the shell draws again when it flips.
    pinned: bool,
    _events: Subscription,
}

/// A client of the terminal daemon: the daemon is this executable run as `koyo termd`,
/// started when none runs and a request needs it. Tests start no daemon and read none, as
/// the user's terminals aren't theirs.
pub(crate) fn daemon_client() -> Option<Arc<TermClient>> {
    let dir = term_dir().filter(|_| !cfg!(test))?;
    Some(TermClient::new(dir, daemon_command()))
}

/// The terminal daemon's folder in the app's data folder.
pub(crate) fn term_dir() -> Option<TermDir> {
    settings::data_dir().map(|data| TermDir::at(data.join("term")))
}

/// The terminal daemon as this executable runs it: `koyo termd`.
pub(crate) fn daemon_command() -> DaemonCommand {
    DaemonCommand {
        program: std::env::current_exe().unwrap_or_else(|error| {
            log::error!("can't find this executable to run the terminal daemon: {error}");
            "koyo".into()
        }),
        args: vec!["termd".into()],
    }
}

/// Binds the terminals' keys; call once when the app starts.
pub fn init(cx: &mut App) {
    beside::init(cx);
}

impl Terminals {
    /// The daemon's client, made on first use: the daemon is this executable run as
    /// `koyo termd`, started when none runs.
    pub(crate) fn client(&mut self) -> Option<Arc<TermClient>> {
        if self.client.is_none() {
            self.client = daemon_client();
        }
        self.client.clone()
    }

    /// Has the daemon tell every terminal's program the app's theme when it changed: once it
    /// settles, as dragging in the theme picker changes it at every frame.
    pub fn tell_theme(&mut self, cx: &App) {
        let Some(client) = self.client() else {
            return;
        };
        let look = look(theme::palette(cx));
        let theme = Theme {
            foreground: look.foreground.into(),
            background: look.background.into(),
            dark: cx.theme().mode.is_dark(),
        };
        if client.keep_theme(theme) {
            let settled = cx.background_executor().timer(THEME_DELAY);
            // A newer change drops this task, and with it the send it waited for.
            self.theme_send = Some(cx.background_executor().spawn(async move {
                settled.await;
                client.send_theme(theme);
            }));
        }
    }

    /// The terminal tab `id` shows, once it's known.
    fn handle_of(&self, id: u64) -> Option<&Handle> {
        match self.sources.get(&id)? {
            Source::Existing { handle, .. } => Some(handle),
            Source::Open(_) => None,
        }
    }

    /// The tab that shows terminal `handle`, if one does.
    fn tab_of(&self, handle: &Handle) -> Option<u64> {
        (self.sources.iter()).find_map(|(id, source)| match source {
            Source::Existing { handle: shown, .. } if shown == handle => Some(*id),
            _ => None,
        })
    }

    fn add(&mut self, source: Source) -> u64 {
        self.next += 1;
        self.sources.insert(self.next, source);
        self.next
    }

    /// Lets go of the views whose panes closed, which detaches them. A closed tab's terminals
    /// stay known, so ⌘⇧T opens them again.
    pub fn retain(&mut self, workspace: &Workspace) {
        let shown: HashSet<u64> = workspace.terminal_tabs().map(|(_, id)| id).collect();
        let known: HashSet<u64> = (shown.iter().copied())
            .chain(workspace.closed_terminals())
            .collect();
        self.views.retain(|id, _| shown.contains(id));
        self.panels.retain(|id, _| shown.contains(id));
        self.typing.retain(|id, _| shown.contains(id));
        self.sources.retain(|id, _| known.contains(id));
    }
}

/// The count of a checkout's terminals waiting for the person, beside its name.
pub fn waiting_badge(count: usize, p: Palette) -> gpui_kit::Div {
    div()
        .px_1p5()
        .rounded_full()
        .bg(p.modified.opacity(0.2))
        .text_color(p.modified)
        .text_xs()
        .child(count.to_string())
}

/// How terminals look in the theme `p`.
pub fn look(p: Palette) -> Look {
    Look {
        font: gpui_kit::font(p.mono),
        font_size: px(13.),
        line_height: 1.2,
        padding: px(8.),
        foreground: rgb(p.text),
        background: rgb(p.card),
        cursor: p.text,
        selection: p.accent.opacity(0.3),
    }
}

fn rgb(colour: Hsla) -> koyo_term_engine::Rgb {
    let Rgba { r, g, b, .. } = colour.into();
    let byte = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
    koyo_term_engine::Rgb {
        r: byte(r),
        g: byte(g),
        b: byte(b),
    }
}

/// A shell's name in `worktree`, the next free one among `taken`: `zsh`, `zsh-2`, ...
fn free_name(base: &str, taken: &BTreeSet<String>) -> String {
    if !taken.contains(base) {
        return base.to_owned();
    }
    // As many numbers as names taken: one of them is free.
    (2..=taken.len() + 1)
        .map(|n| format!("{base}-{n}"))
        .find(|name| !taken.contains(name))
        .unwrap_or_else(|| base.to_owned())
}

fn shell_name() -> String {
    std::env::var("SHELL")
        .ok()
        .and_then(|shell| {
            Path::new(&shell)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "zsh".into())
}

fn open_request(worktree: &Path, name: String) -> OpenRequest {
    OpenRequest {
        worktree: Some(Selector::Path(worktree.to_path_buf())),
        name,
        command: Vec::new(),
        cwd: None,
        env: BTreeMap::new(),
        size: None,
        labels: BTreeSet::new(),
        if_exists: IfExists::Attach,
        respawn: true,
    }
}

impl Shell {
    /// The view for terminal tab `id`, made now if it hasn't been.
    pub fn terminal_view(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<TerminalView>> {
        let look = look(theme::palette(cx));
        if let Some(made) = self.terminals.views.get(&id) {
            let view = made.view.clone();
            view.update(cx, |view, cx| view.set_look(look, cx));
            return Some(view);
        }
        let source = self.terminals.sources.get(&id)?.clone();
        let client = self.terminals.client()?;
        // A path a program prints relative to the checkout is found wherever its shell is.
        let roots = (self.workspace.terminal_tabs())
            .find(|(_, tab)| *tab == id)
            .map(|(worktree, _)| worktree.to_path_buf());
        let view = cx.new(|cx| {
            let mut view = TerminalView::new(client, source, look, window, cx);
            view.set_link_roots(roots.into_iter().collect());
            view
        });
        let events = cx.subscribe_in(&view, window, move |this, _, event, window, cx| {
            this.terminal_event(id, event, window, cx);
        });
        let responsive = cx.new(|cx| responsive::ResponsiveTerminal::new(id, view.clone(), cx));
        let made = Made {
            view: view.clone(),
            responsive,
            pinned: false,
            _events: events,
        };
        self.terminals.views.insert(id, made);
        if self.terminals.focus_next.take() == Some(id) {
            let focus = view.focus_handle(cx);
            cx.defer_in(window, move |_, window, cx| window.focus(&focus, cx));
        }
        Some(view)
    }

    /// Makes the views of the shown tab's terminal panes, so the panes draw them; the first.
    pub fn make_shown_terminals(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<TerminalView>> {
        let ids: Vec<u64> = (self.workspace.branch())
            .and_then(|branch| branch.tabs.get(branch.active_tab))
            .map(|tab| {
                tab.panes()
                    .iter()
                    .filter_map(|pane| pane.terminal)
                    .collect()
            })
            .unwrap_or_default();
        let mut first = None;
        for id in ids {
            let view = self.terminal_view(id, window, cx);
            if first.is_none() {
                first = view;
            }
        }
        first
    }

    /// What `pane` shows: its chat or its terminal when its view is made, else what
    /// `crate::pane` draws for its kind.
    pub fn pane_content(&self, pane: &Pane, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        if let Some(chat) = self.chat_tabs.view(pane.id) {
            return chat.clone().into_any_element();
        }
        let Some((id, view)) =
            (pane.terminal).and_then(|id| Some((id, &self.terminals.views.get(&id)?.responsive)))
        else {
            return crate::pane::pane(pane, p);
        };
        let view = self.beside_panel(id, view.clone().into_any_element(), p, cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            // ⌘⇧L in the terminal, heard on its way up.
            .on_action(cx.listener(move |this, _: &beside::ToggleActivity, _, cx| {
                this.toggle_activity(id, cx);
            }))
            .children(self.terminal_bar(id, p, cx))
            .child(div().flex_1().min_h_0().child(view))
            .into_any_element()
    }

    /// The shown tab's first terminal pane: its terminal's id and view, if it's made.
    fn shown(&self) -> Option<(u64, Option<Entity<TerminalView>>)> {
        let branch = self.workspace.branch()?;
        let tab = branch.tabs.get(branch.active_tab)?;
        let id = tab.panes().iter().find_map(|pane| pane.terminal)?;
        Some((id, self.terminals.view(id)))
    }

    /// The view of the terminal tab shown, if it's made.
    #[cfg(feature = "visual-check")]
    pub fn shown_terminal(&self) -> Option<Entity<TerminalView>> {
        self.shown()?.1
    }

    /// Takes away the terminal tab shown, as quitting the app does: the terminal stays in the
    /// daemon.
    #[cfg(feature = "visual-check")]
    pub fn let_go_of_shown_terminal(&mut self, cx: &mut Context<Self>) {
        if let Some((id, _)) = self.shown() {
            self.close_terminal_tab(id, cx);
        }
    }

    /// The daemon's client the terminals use.
    #[cfg(feature = "visual-check")]
    pub fn terminal_client(&mut self) -> Option<Arc<TermClient>> {
        self.terminals.client()
    }

    /// Looks for the running terminals again, as the app does when it starts.
    #[cfg(feature = "visual-check")]
    pub fn restore_terminals_again(&mut self, cx: &mut Context<Self>) {
        self.terminals.restored.clear();
        self.restore_terminals(cx);
    }

    fn terminal_event(
        &mut self,
        id: u64,
        event: &TerminalEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = match event {
            TerminalEvent::Title(title) if !title.is_empty() => title.clone(),
            TerminalEvent::Opened(record) => {
                self.terminals.states.upsert(record);
                // Opened again, as ⌘⇧T does, the pane attaches to this very terminal.
                let name = record.name.clone();
                (self.terminals.sources).insert(
                    id,
                    Source::Existing {
                        handle: record.handle.clone(),
                        name: name.clone(),
                    },
                );
                self.type_waiting(cx);
                name
            }
            TerminalEvent::Closed => {
                self.close_terminal_tab(id, cx);
                return;
            }
            TerminalEvent::DaemonReplaced => {
                // The other tabs it turned away try again, with this build's daemon.
                for made in self.terminals.views.values() {
                    made.view.update(cx, TerminalView::try_again);
                }
                return;
            }
            // The terminal bar's toggle shows whether it's pinned; the rest is responsive
            // mode's, which hears it too.
            TerminalEvent::Responsive => {
                if let Some(made) = self.terminals.views.get_mut(&id) {
                    let pinned = made.view.read(cx).pinned().is_some();
                    if made.pinned != pinned {
                        made.pinned = pinned;
                        cx.notify();
                    }
                }
                return;
            }
            TerminalEvent::OpenLink(target) => {
                self.open_link(target, window, cx);
                return;
            }
            TerminalEvent::Title(_)
            | TerminalEvent::Bell
            | TerminalEvent::Exited(_)
            | TerminalEvent::PinRefused(..) => return,
        };
        if self.workspace.retitle_terminal(id, title.into()) {
            cx.notify();
        }
    }

    /// Opens what a link ⌘-clicked in a terminal names: a file in a tab, at its line when
    /// the link gives one, or a page in the browser.
    fn open_link(&mut self, target: &LinkTarget, window: &mut Window, cx: &mut Context<Self>) {
        match target {
            LinkTarget::File { path, line, .. } => {
                let at = line.map(crate::files::At::printed_line);
                self.open_path(path, at, window, cx);
            }
            LinkTarget::Url(url) => cx.open_url(url),
        }
    }

    fn close_terminal_tab(&mut self, id: u64, cx: &mut Context<Self>) {
        self.workspace.close_terminal_pane(id);
        self.terminals.retain(&self.workspace);
        cx.notify();
    }
}

#[cfg(test)]
mod keys;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_count_up_past_the_taken_ones() {
        let taken = |names: &[&str]| {
            names
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<BTreeSet<_>>()
        };
        assert_eq!(free_name("zsh", &taken(&[])), "zsh");
        assert_eq!(free_name("zsh", &taken(&["zsh"])), "zsh-2");
        assert_eq!(
            free_name("zsh", &taken(&["zsh", "zsh-2", "zsh-3"])),
            "zsh-4"
        );
        assert_eq!(free_name("zsh", &taken(&["zsh-2"])), "zsh");
    }
}
