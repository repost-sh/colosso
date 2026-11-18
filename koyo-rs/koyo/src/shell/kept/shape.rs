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
use crate::browser::profiles::By;
use crate::model::{Content, Editor, Pane, PaneKind, Split, SplitAxis, Tab};
use crate::shell::Shell;

/// What a pane showed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum KeptPane {
    /// A page, at the address it showed, titled as it was until it says otherwise, and
    /// whether it's an agent's (`Pane::by`), said only when it is: one kept before that was
    /// kept is the person's. A new tab's start page has no address.
    Page {
        url: String,
        #[serde(default)]
        title: String,
        #[serde(
            default,
            rename = "byAgent",
            skip_serializing_if = "std::ops::Not::not"
        )]
        by_agent: bool,
    },
    /// A terminal the daemon runs, by its handle and its name.
    Terminal {
        handle: Handle,
        name: String,
    },
    /// A chat, by its thread, titled as it was until the host says.
    Chat {
        thread: ThreadId,
        #[serde(default)]
        title: String,
    },
    /// A chat with nothing sent yet: its draft is its folder's (`chat/drafts.rs`).
    NewChat,
    File {
        path: PathBuf,
    },
    /// The checkout's diffs.
    Changes,
    /// A device, by its id: brought back not running, a simulator being gigabytes.
    Device {
        udid: String,
        name: String,
        #[serde(default)]
        ipad: bool,
        /// A real device by cable; else a simulator of Koyo's set.
        #[serde(default)]
        phone: bool,
    },
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
        match &pane.content {
            Content::Browser(browser) if browser.is_inspector() => None,
            Content::Browser(browser) => Some(KeptPane::Page {
                url: browser.address()?.to_owned(),
                title: pane.title.to_string(),
                by_agent: browser.by == By::Agent,
            }),
            Content::Terminal(id) => {
                let (handle, name) = self.terminals.kept((*id)?)?;
                Some(KeptPane::Terminal {
                    handle: handle.clone(),
                    name: name.to_owned(),
                })
            }
            Content::Chat(thread) => Some(match thread {
                Some(thread) => KeptPane::Chat {
                    thread: thread.clone(),
                    title: pane.title.to_string(),
                },
                None => KeptPane::NewChat,
            }),
            Content::Editor(Editor::Changes(_)) => Some(KeptPane::Changes),
            Content::Editor(Editor::File(file)) => Some(KeptPane::File { path: file.path(cx) }),
            Content::Editor(Editor::Untitled) | Content::Desktop(_) => None,
            Content::Device(device) => {
                let chosen = device.chosen(cx)?;
                Some(KeptPane::Device {
                    udid: chosen.udid,
                    name: chosen.name,
                    ipad: chosen.ipad,
                    phone: chosen.phone,
                })
            }
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
            KeptPane::Page {
                url,
                title,
                by_agent,
            } => {
                let by = if *by_agent { By::Agent } else { By::Person };
                Some(browser::page_brought_back(url, title, by, cx))
            }
            KeptPane::Terminal { handle, name } => {
                let id = self.terminals.bring_back(handle.clone(), name.clone());
                Some(Pane {
                    title: name.clone().into(),
                    ..Pane::of(Content::Terminal(Some(id)))
                })
            }
            KeptPane::Chat { thread, title } => {
                self.chat_tabs.bring_back(thread);
                let mut pane = Pane::of(Content::Chat(Some(thread.clone())));
                if !title.is_empty() {
                    pane.title = title.clone().into();
                }
                Some(pane)
            }
            KeptPane::NewChat => Some(Pane::new(PaneKind::Chat)),
            KeptPane::File { path } => path.exists().then(|| Pane::of_path(path.clone())),
            KeptPane::Changes => Some(Pane::of_changes(None)),
            KeptPane::Device { udid, name, ipad, phone } => {
                let chosen = crate::devices::Chosen {
                    udid: udid.clone(),
                    name: name.clone(),
                    ipad: *ipad,
                    phone: *phone,
                };
                let view = self.device_view(Some(chosen), cx);
                Some(Pane::device(view, cx))
            }
            KeptPane::Unknown => None,
        }
    }
}
