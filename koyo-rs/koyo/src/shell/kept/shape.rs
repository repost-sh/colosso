//! A checkout's tabs as the window keeps them (`docs/browser.md` 2): each tab a pane or a
//! split of two, and each pane a reference to what it showed, which outlives the window: a
//! page's address, a terminal's handle in the daemon, a chat's thread in the host, a file's
//! path, or the checkout's diffs. And both ways between them and the tabs: what a tab is kept
//! as, and the tab a kept one comes back as.

use std::path::PathBuf;

use gpui_kit::{App, Context};
use koyo_agent::protocol::ids::ThreadId;
use koyo_term_control::Handle;
use serde::{Deserialize, Serialize};

use crate::browser;
use crate::model::{Pane, PaneKind, Split, SplitAxis, Tab, Unopened};
use crate::shell::Shell;

/// What a pane showed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum KeptPane {
    /// A page, at the address it showed, titled as it was until it says otherwise. A new
    /// tab's start page has no address.
    Page {
        url: String,
        #[serde(default)]
        title: String,
    },
    /// A terminal the daemon runs, by its handle and its name.
    Terminal { handle: Handle, name: String },
    /// A chat, by its thread, titled as it was until the host says.
    Chat {
        thread: ThreadId,
        #[serde(default)]
        title: String,
    },
    /// A chat with nothing sent yet: its draft is its folder's (`chat/drafts.rs`).
    NewChat,
    File { path: PathBuf },
    /// The checkout's diffs.
    Changes,
    /// A kind another build keeps, which this one brings nothing back for.
    #[serde(other)]
    Unknown,
}

/// A tab: a pane, or a split of two.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum KeptTree {
    Split {
        /// `row` side by side, `column` one over the other.
        axis: SplitAxis,
        /// The first side's share of the room.
        ratio: f32,
        first: Box<KeptTree>,
        second: Box<KeptTree>,
    },
    Pane(KeptPane),
}

impl Shell {
    /// `tab` as it's kept: a split one of whose sides isn't kept is its other side.
    pub(super) fn kept_tree(&self, tab: &Tab, cx: &App) -> Option<KeptTree> {
        match tab {
            Tab::Single(pane) => self.kept_pane(pane, cx).map(KeptTree::Pane),
            Tab::Split(split) => {
                let first = self.kept_tree(&split.first, cx);
                let second = self.kept_tree(&split.second, cx);
                match (first, second) {
                    (Some(first), Some(second)) => Some(KeptTree::Split {
                        axis: split.axis,
                        ratio: split.ratio,
                        first: Box::new(first),
                        second: Box::new(second),
                    }),
                    (side, None) | (None, side) => side,
                }
            }
        }
    }

    /// `pane` as it's kept. Not a page's `DevTools`, which are of a page that isn't made yet
    /// when the window comes back; nor a terminal the daemon doesn't run yet, which has no
    /// handle to find it by.
    fn kept_pane(&self, pane: &Pane, cx: &App) -> Option<KeptPane> {
        match pane.kind {
            PaneKind::Browser if pane.is_inspector(cx) => None,
            PaneKind::Browser => Some(KeptPane::Page {
                url: pane.address(cx)?,
                title: pane.title.to_string(),
            }),
            PaneKind::Terminal => {
                let (handle, name) = self.terminals.kept(pane.terminal?)?;
                Some(KeptPane::Terminal {
                    handle: handle.clone(),
                    name: name.to_owned(),
                })
            }
            PaneKind::Chat => Some(match &pane.chat {
                Some(thread) => KeptPane::Chat {
                    thread: thread.clone(),
                    title: pane.title.to_string(),
                },
                None => KeptPane::NewChat,
            }),
            PaneKind::Editor if pane.shows_changes() => Some(KeptPane::Changes),
            PaneKind::Editor => (pane.file_path(cx)).map(|path| KeptPane::File { path }),
        }
    }

    /// The tab `tree` comes back as, if anything of it does: a split one of whose sides
    /// doesn't come back is its other side.
    pub(super) fn tab_of_kept(&mut self, tree: &KeptTree, cx: &mut Context<Self>) -> Option<Tab> {
        match tree {
            KeptTree::Pane(pane) => self.pane_of_kept(pane, cx).map(Tab::Single),
            KeptTree::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let first = self.tab_of_kept(first, cx);
                let second = self.tab_of_kept(second, cx);
                match (first, second) {
                    (Some(first), Some(second)) => Some(Tab::Split(Box::new(Split {
                        axis: *axis,
                        first,
                        second,
                        ratio: *ratio,
                    }))),
                    (side, None) | (None, side) => side,
                }
            }
        }
    }

    /// The pane `kept` comes back as, showing what it showed once its tab is shown: a page
    /// that opens its address then, a terminal and a chat whose views are made then, a file
    /// and the diffs opened then. Not a file that's gone; a terminal that ended or a chat
    /// deleted goes once the daemon or the host says so.
    fn pane_of_kept(&mut self, kept: &KeptPane, cx: &mut Context<Self>) -> Option<Pane> {
        match kept {
            KeptPane::Page { url, title } => Some(browser::page_brought_back(url, title, cx)),
            KeptPane::Terminal { handle, name } => {
                let mut pane = Pane::new(PaneKind::Terminal);
                pane.terminal = Some(self.terminals.bring_back(handle.clone(), name.clone()));
                pane.title = name.clone().into();
                Some(pane)
            }
            KeptPane::Chat { thread, title } => {
                self.chat_tabs.bring_back(thread);
                let mut pane = Pane::new(PaneKind::Chat);
                pane.chat = Some(thread.clone());
                if !title.is_empty() {
                    pane.title = title.clone().into();
                }
                Some(pane)
            }
            KeptPane::NewChat => Some(Pane::new(PaneKind::Chat)),
            KeptPane::File { path } => path
                .exists()
                .then(|| Pane::unopened(Unopened::File(path.clone()))),
            KeptPane::Changes => Some(Pane::unopened(Unopened::Changes)),
            KeptPane::Unknown => None,
        }
    }
}
