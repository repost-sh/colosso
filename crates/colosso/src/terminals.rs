//! The terminals the tabs show, and only tabs (`docs/terminals.md` 7.1): the app's one
//! client of the daemon, which runs inside this executable as `colosso termd`, and a view
//! per terminal tab, made the first time the tab is drawn so a tab never looked at never
//! attaches. Closing a tab detaches its view; the terminal keeps running, and comes back as
//! a tab when the app opens again.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use colosso_term::{Look, Source, TermClient, TerminalEvent, TerminalView};
use colosso_term_control::{
    DaemonCommand, IfExists, OpenRequest, RequestBody, ResponseBody, Selector, TermDir,
    TerminalSummary,
};
use gpui_kit::{
    AnyElement, AppContext as _, Context, Entity, Focusable as _, Hsla, IntoElement as _, Rgba,
    Subscription, Window, px,
};

use crate::model::{Pane, Workspace};
use crate::settings;
use crate::shell::Shell;
use crate::theme::{self, Palette};

#[cfg(feature = "visual-check")]
pub mod visual;

#[derive(Default)]
pub struct Terminals {
    client: Option<Arc<TermClient>>,
    next: u64,
    /// What each terminal tab shows, until its view is made.
    sources: HashMap<u64, Source>,
    views: HashMap<u64, (Entity<TerminalView>, Subscription)>,
    /// Checkouts whose running terminals were looked for, so they're restored once.
    restored: HashSet<PathBuf>,
    /// The tab whose view takes the keyboard once it's made: the terminal just opened.
    focus_next: Option<u64>,
}

impl Terminals {
    /// The daemon's client, made on first use: the daemon is this executable run as
    /// `colosso termd`, started when none runs.
    fn client(&mut self) -> Option<Arc<TermClient>> {
        if self.client.is_none() {
            // Tests start no daemon and read none: the user's terminals aren't theirs.
            let dir = settings::data_dir()
                .filter(|_| !cfg!(test))
                .map(|data| TermDir::at(data.join("term")))?;
            let program = match std::env::current_exe() {
                Ok(program) => program,
                Err(error) => {
                    log::error!("can't find this executable to run the terminal daemon: {error}");
                    return None;
                }
            };
            let daemon = DaemonCommand {
                program,
                args: vec!["termd".into()],
            };
            self.client = Some(TermClient::new(dir, daemon));
        }
        self.client.clone()
    }

    fn add(&mut self, source: Source) -> u64 {
        self.next += 1;
        self.sources.insert(self.next, source);
        self.next
    }

    /// Lets go of the views whose panes closed: they detach, and the terminals live on. A
    /// closed tab's terminals stay known, so ⌘⇧T opens them again.
    pub fn retain(&mut self, workspace: &Workspace) {
        let shown: HashSet<u64> = workspace.terminal_tabs().map(|(_, id)| id).collect();
        let known: HashSet<u64> = (shown.iter().copied())
            .chain(workspace.closed_terminals())
            .collect();
        self.views.retain(|id, _| shown.contains(id));
        self.sources.retain(|id, _| known.contains(id));
    }
}

/// How terminals look in the theme `p`.
pub fn look(p: Palette) -> Look {
    Look {
        font: gpui_kit::font(theme::MONO),
        font_size: px(13.),
        line_height: 1.2,
        padding: px(8.),
        foreground: rgb(p.text),
        background: rgb(p.card),
        cursor: p.text,
        selection: p.accent.opacity(0.3),
    }
}

fn rgb(colour: Hsla) -> colosso_term_engine::Rgb {
    let Rgba { r, g, b, .. } = colour.into();
    let byte = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
    colosso_term_engine::Rgb {
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
    /// Opens a shell in the selected checkout, in a new tab, named after the shell and the
    /// next free number.
    pub fn new_terminal(&mut self, cx: &mut Context<Self>) {
        let Some(worktree) = self
            .workspace
            .branch()
            .map(|unit| unit.worktree.path.clone())
        else {
            return;
        };
        let Some(client) = self.terminals.client() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let listed = cx
                .background_spawn({
                    let worktree = worktree.clone();
                    async move {
                        client.request(RequestBody::ListTerminals {
                            worktree: Some(Selector::Path(worktree)),
                            status: None,
                        })
                    }
                })
                .await;
            let taken: BTreeSet<String> = match listed {
                Ok(ResponseBody::Terminals { terminals }) => terminals
                    .into_iter()
                    .map(|summary| summary.record.name)
                    .collect(),
                Ok(_) => BTreeSet::new(),
                Err(error) => {
                    log::warn!(
                        "couldn't list the terminals of {}: {error}",
                        worktree.display()
                    );
                    BTreeSet::new()
                }
            };
            this.update(cx, |this, cx| {
                // Names the open tabs hold count too: their terminals may not be open yet.
                let mut taken = taken;
                taken.extend(
                    this.terminals
                        .views
                        .values()
                        .map(|(view, _)| view.read(cx).name().to_owned()),
                );
                let name = free_name(&shell_name(), &taken);
                let request = open_request(&worktree, name.clone());
                let id = this.terminals.add(Source::Open(Box::new(request)));
                this.terminals.focus_next = Some(id);
                this.workspace
                    .add_terminal_tab(&worktree, id, name.into(), true);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Brings back as tabs the terminals the daemon keeps for checkouts listed for the first
    /// time.
    pub fn restore_terminals(&mut self, cx: &mut Context<Self>) {
        let fresh: Vec<PathBuf> = (self.workspace.projects.iter())
            .flat_map(|project| {
                project
                    .branches
                    .iter()
                    .map(|unit| unit.worktree.path.clone())
            })
            .filter(|path| !self.terminals.restored.contains(path))
            .collect();
        if fresh.is_empty() {
            return;
        }
        let Some(client) = self.terminals.client() else {
            return;
        };
        self.terminals.restored.extend(fresh.iter().cloned());
        cx.spawn(async move |this, cx| {
            let listed = cx
                .background_spawn(async move {
                    client.request_if_running(RequestBody::ListTerminals {
                        worktree: None,
                        status: None,
                    })
                })
                .await;
            let terminals: Vec<TerminalSummary> = match listed {
                Ok(Some(ResponseBody::Terminals { terminals })) => terminals,
                Ok(_) => return,
                Err(error) => return log::warn!("couldn't list the running terminals: {error}"),
            };
            this.update(cx, |this, cx| {
                for summary in terminals {
                    let record = summary.record;
                    let Some(worktree) = record
                        .worktree
                        .as_ref()
                        .map(|worktree| worktree.path.clone())
                    else {
                        continue;
                    };
                    if !fresh.contains(&worktree) || record.exit.is_some() {
                        continue;
                    }
                    let name = record.name.clone();
                    let id = this.terminals.add(Source::Existing {
                        handle: record.handle,
                        name: name.clone(),
                    });
                    this.workspace
                        .add_terminal_tab(&worktree, id, name.into(), false);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The view for terminal tab `id`, made now if it hasn't been.
    pub fn terminal_view(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<TerminalView>> {
        let look = look(theme::palette(cx));
        if let Some((view, _)) = self.terminals.views.get(&id) {
            let view = view.clone();
            view.update(cx, |view, cx| view.set_look(look, cx));
            return Some(view);
        }
        let source = self.terminals.sources.get(&id)?.clone();
        let client = self.terminals.client()?;
        let view = cx.new(|cx| TerminalView::new(client, source, look, window, cx));
        let subscription = cx.subscribe(&view, move |this, _, event, cx| {
            this.terminal_event(id, event, cx);
        });
        self.terminals
            .views
            .insert(id, (view.clone(), subscription));
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
            .map(|tab| tab.panes().iter().filter_map(|pane| pane.terminal).collect())
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

    /// What `pane` shows: its terminal when it's a terminal pane whose view is made, else what
    /// `crate::pane` draws for its kind.
    pub fn pane_content(&self, pane: &Pane, context: &str, p: Palette) -> AnyElement {
        match pane.terminal.and_then(|id| self.terminals.views.get(&id)) {
            Some((view, _)) => view.clone().into_any_element(),
            None => crate::pane::pane(pane, context, p),
        }
    }

    /// The shown tab's first terminal pane: its terminal's id and view, if it's made.
    #[cfg(feature = "visual-check")]
    fn shown(&self) -> Option<(u64, Option<Entity<TerminalView>>)> {
        let branch = self.workspace.branch()?;
        let tab = branch.tabs.get(branch.active_tab)?;
        let id = tab.panes().iter().find_map(|pane| pane.terminal)?;
        Some((id, self.terminals.views.get(&id).map(|(view, _)| view.clone())))
    }

    /// The view of the terminal tab shown, if it's made.
    #[cfg(feature = "visual-check")]
    pub fn shown_terminal(&self) -> Option<Entity<TerminalView>> {
        self.shown()?.1
    }

    /// Closes the terminal tab shown, as its × does: the terminal stays in the daemon.
    #[cfg(feature = "visual-check")]
    pub fn close_shown_terminal(&mut self, cx: &mut Context<Self>) {
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

    fn terminal_event(&mut self, id: u64, event: &TerminalEvent, cx: &mut Context<Self>) {
        let title = match event {
            TerminalEvent::Title(title) if !title.is_empty() => title.clone(),
            TerminalEvent::Opened(record) => {
                // Opened again, as ⌘⇧T does, the pane attaches to this very terminal.
                let name = record.name.clone();
                (self.terminals.sources).insert(
                    id,
                    Source::Existing {
                        handle: record.handle.clone(),
                        name: name.clone(),
                    },
                );
                name
            }
            TerminalEvent::Closed => {
                self.close_terminal_tab(id, cx);
                return;
            }
            TerminalEvent::Title(_) | TerminalEvent::Bell | TerminalEvent::Exited(_) => return,
        };
        if self.workspace.retitle_terminal(id, title.into()) {
            cx.notify();
        }
    }

    fn close_terminal_tab(&mut self, id: u64, cx: &mut Context<Self>) {
        self.workspace.close_terminal_pane(id);
        self.terminals.retain(&self.workspace);
        cx.notify();
    }
}

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
