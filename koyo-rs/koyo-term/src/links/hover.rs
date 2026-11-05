//! The view's side of links: following the pointer while ⌘ is held, for the element to
//! underline the link under it, and finding the link a ⌘-click opens.

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::Context;
use koyo_term_engine::GridPoint;

use super::LinkTarget;
use crate::view::TerminalView;

impl TerminalView {
    /// The folders a relative path printed in the terminal is looked for in when it isn't in
    /// the shell's own: the checkout the terminal belongs to.
    pub fn set_link_roots(&mut self, roots: Vec<PathBuf>) {
        self.links.roots = roots;
        self.links.looked = None;
    }

    /// Follows the pointer: over the grid's cell `at` with ⌘ held, or `None` once it's left
    /// or ⌘ is up, which takes the underline away. A press being dragged shows no link.
    pub(crate) fn point_at_link(&mut self, at: Option<GridPoint>, cx: &mut Context<Self>) {
        let at = at.filter(|_| self.drag.is_none());
        if at.is_none() && self.links.pointer.is_none() && self.links.hovered.is_none() {
            return;
        }
        self.links.pointer = at;
        self.look_for_link(cx);
    }

    /// Looks again under the pointer, for a frame that came in while ⌘ is held.
    pub(crate) fn frame_under_pointer(&mut self, cx: &mut Context<Self>) {
        if self.links.pointer.is_some() {
            self.look_for_link(cx);
        }
    }

    fn look_for_link(&mut self, cx: &mut Context<Self>) {
        let found = if let Some((at, frame)) = self.links.pointer.zip(self.frame.clone()) {
            if self.links.looked == Some((at, frame.sequence)) {
                return;
            }
            self.links.looked = Some((at, frame.sequence));
            self.links.find(&frame, at).map(Rc::new)
        } else {
            self.links.looked = None;
            None
        };
        if self.links.hovered != found {
            self.links.hovered = found;
            cx.notify();
        }
    }

    /// What a ⌘-click on cell `at` opens, if it's on a link.
    pub(crate) fn link_at(&mut self, at: GridPoint) -> Option<LinkTarget> {
        let frame = self.frame.clone()?;
        self.links.find(&frame, at).map(|found| found.target)
    }
}
