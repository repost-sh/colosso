//! Go to File: ⌘P opens a palette over the window that finds any file of the selected
//! checkout by fuzzy path, and shows the one picked in the Files panel, as clicking it there does.
//! The search runs off the main thread (`colosso_project::Finder`); each checkout keeps its
//! finder and the files opened from it lately while the app runs, so the palette opens on
//! what it found the last time, with the latest files first.

mod render;
mod view;
#[cfg(feature = "visual-check")]
pub mod visual;

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use colosso_project::Finder;
use gpui_kit::{App, Context, Div, Global, InteractiveElement as _, KeyBinding, Window, actions};

use crate::shell::Shell;

actions!(go_to_file, [GoToFile]);

/// How many of the files opened lately each checkout remembers.
const MOST_RECENT: usize = 50;

/// ⌘P anywhere; and in the palette's field, the keys that move through what it found, which
/// the field would otherwise take.
pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("cmd-p", GoToFile, None)]);
    crate::list::bind_keys("GoToFile > Input", cx);
}

/// What each checkout keeps for Go to File, by its path.
#[derive(Default)]
struct Checkouts(HashMap<PathBuf, Kept>);

impl Global for Checkouts {}

#[derive(Default)]
struct Kept {
    finder: Arc<Finder>,
    /// Relative to the checkout, the latest first.
    recent: VecDeque<Arc<str>>,
}

/// Notes that `path`, absolute or relative to the checkout at `root`, was opened from it, if
/// it's in it.
pub fn opened(root: &Path, path: &Path, cx: &mut App) {
    let path = root.join(path);
    let Some(relative) = (path.strip_prefix(root).ok()).and_then(Path::to_str) else {
        return;
    };
    let kept = (cx.default_global::<Checkouts>().0)
        .entry(root.to_owned())
        .or_default();
    kept.recent.retain(|recent| **recent != *relative);
    kept.recent.push_front(relative.into());
    kept.recent.truncate(MOST_RECENT);
}

impl Shell {
    /// What the window does with Go to File's action.
    pub fn handle_go_to_file(root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(Self::go_to_file))
    }

    /// Opens the palette on the selected checkout, letting go of what's kept for checkouts
    /// gone from the sidebar.
    fn go_to_file(&mut self, _: &GoToFile, window: &mut Window, cx: &mut Context<Self>) {
        let Some(checkout) = self.right_panel.checkout().cloned() else {
            return;
        };
        let root = checkout.read(cx).root().clone();
        let workspace = &self.workspace;
        let checkouts = &mut cx.default_global::<Checkouts>().0;
        checkouts.retain(|path, _| workspace.project_of(path).is_some());
        let kept = checkouts.entry(root).or_default();
        let finder = kept.finder.clone();
        let recent: Vec<Arc<str>> = kept.recent.iter().cloned().collect();
        let shell = cx.entity().downgrade();
        view::open(shell, checkout, finder, &recent, window, cx);
    }
}
