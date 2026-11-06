//! A markdown file as it reads: its text laid out in a column, scrolled on its own, by the
//! wheel or the keys a reader scrolls by, its words selectable and its Mermaid blocks drawn.
//! A table wider than the column scrolls sideways, its columns kept wide enough to read.
//! Its links open as a chat's do: one to the web in the browser, one to a file in a tab.

use std::collections::HashSet;
use std::mem;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::text::{TextView, TextViewState};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FocusHandle, Hsla, InteractiveElement as _, IntoElement,
    KeyBinding, Overflow, ParentElement as _, Render, ScrollHandle, StatefulInteractiveElement as _,
    StyleRefinement, Styled as _, Window, div, point, px,
};

use super::diagram::{self, Card, Key, Look};
use super::mermaid::MermaidPlugin;
use super::toggle;
use crate::files::opens_links;
use crate::list::{PageDown, PageUp};
use crate::theme::{self, Palette};

/// How wide the column of text grows, and the room around it: above, enough to clear the
/// toggle floating over it.
const COLUMN: f32 = 860.;
const SIDE: f32 = 36.;
const TOP: f32 = toggle::BELOW + 14.;
const BOTTOM: f32 = 64.;
/// The size of its body text.
const TEXT: f32 = 15.;
/// How far an arrow key scrolls it; a page keeps as much of the one before in sight.
const LINE: f32 = 40.;

gpui_kit::actions!(markdown_preview, [LineUp, LineDown, Top, Bottom]);

const CONTEXT: &str = "MarkdownPreview";

/// The keys that scroll a preview with the keyboard in it: the arrows, a page at a time as a
/// browser has it, with Space too, and to either end.
pub fn bind_keys(cx: &mut App) {
    let context = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", LineUp, context),
        KeyBinding::new("down", LineDown, context),
        KeyBinding::new("pageup", PageUp, context),
        KeyBinding::new("shift-space", PageUp, context),
        KeyBinding::new("pagedown", PageDown, context),
        KeyBinding::new("space", PageDown, context),
        KeyBinding::new("home", Top, context),
        KeyBinding::new("cmd-up", Top, context),
        KeyBinding::new("end", Bottom, context),
        KeyBinding::new("cmd-down", Bottom, context),
    ]);
}

pub struct MarkdownPreview {
    state: Entity<TextViewState>,
    /// The folder of the file shown, which its relative links start from.
    folder: PathBuf,
    scroll: ScrollHandle,
    /// Its diagrams' card and look, and what they were made from: made again only when the
    /// theme or the accent changes, all a look's colours come from.
    look: Option<(Made, Card, Look)>,
    /// The diagrams it drew as it was last laid out, and those it keeps drawn: each kept while
    /// it shows it, and let go once it doesn't.
    seen: Arc<Mutex<HashSet<Key>>>,
    kept: HashSet<Key>,
}

/// What a diagram's card and look are made from: whether it's dark, the accent and the card.
type Made = (bool, Hsla, Hsla);

impl MarkdownPreview {
    /// A preview of a file in `folder`, showing nothing until it's given its text.
    pub fn new(folder: PathBuf, cx: &mut Context<Self>) -> Self {
        let state = cx.new(|cx| TextViewState::markdown("", cx).selectable(true));
        // Closed, it lets go of the diagrams it showed.
        cx.on_release(|this, cx| {
            let seen = mem::take(&mut *this.seen.lock().unwrap_or_else(PoisonError::into_inner));
            diagram::showing(&this.kept, &seen, None, cx);
            diagram::showing(&seen, &HashSet::new(), None, cx);
        })
        .detach();
        MarkdownPreview {
            state,
            folder,
            scroll: ScrollHandle::new(),
            look: None,
            seen: Arc::default(),
            kept: HashSet::new(),
        }
    }

    /// Shows `text` in place of what it showed, where it was scrolled to.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| state.set_text(text, cx));
    }

    /// What has the keyboard in it: its text, to select and copy.
    pub fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.state.read(cx).focus_handle().clone()
    }

    /// The text it shows, as it renders it.
    #[cfg(test)]
    pub fn text(&self, cx: &App) -> String {
        self.state.read(cx).rendered_text().as_str().to_owned()
    }

    /// How far down it's scrolled.
    #[cfg(test)]
    pub fn scrolled(&self) -> f32 {
        -self.scroll.offset().y.as_f32()
    }

    /// How tall what it shows is, laid out: as tall as the pane it's in, at the least.
    #[cfg(test)]
    pub fn content_height(&self) -> f32 {
        (self.scroll.max_offset().y + self.scroll.bounds().size.height).as_f32()
    }

    /// Scrolls it `by` down, or up when it's less than nothing, as far as it goes.
    fn scroll_by(&mut self, by: f32, cx: &mut Context<Self>) {
        let offset = self.scroll.offset();
        let end = self.scroll.max_offset().y;
        let y = (offset.y - px(by)).clamp(-end, px(0.));
        if y != offset.y {
            self.scroll.set_offset(point(offset.x, y));
            cx.notify();
        }
    }

    /// A page down, or `-1.` up: what's in sight, less a line kept from the one before.
    fn page(&mut self, pages: f32, cx: &mut Context<Self>) {
        let height = self.scroll.bounds().size.height.as_f32();
        self.scroll_by(pages * (height - LINE).max(LINE), cx);
    }

    /// Its diagrams' card and look in the theme now.
    fn look(&mut self, p: Palette, cx: &App) -> (Card, Look) {
        let dark = cx.theme().mode.is_dark();
        let made = (dark, p.accent, p.card);
        match &self.look {
            Some((was, card, look)) if *was == made => (card.clone(), look.clone()),
            _ => {
                let card = Card::of(cx);
                let look = Look::of(p, card.paper, dark);
                self.look = Some((made, card.clone(), look.clone()));
                (card, look)
            }
        }
    }

    /// Keeps the diagrams it drew as it was last laid out, letting go of the rest.
    fn keep_seen(&mut self, window: &mut Window, cx: &mut App) {
        let seen = mem::take(&mut *self.seen.lock().unwrap_or_else(PoisonError::into_inner));
        diagram::showing(&self.kept, &seen, Some(window), cx);
        self.kept = seen;
    }
}

/// A table's frame as a preview has it: one wider than the column scrolls sideways rather than
/// squeezing its columns until their words break, each column kept about as wide as its text
/// takes on two lines, between 160 and 480 points (the text view's scroll layout).
fn tables() -> StyleRefinement {
    let mut table = StyleRefinement::default();
    table.overflow.x = Some(Overflow::Scroll);
    table
}

impl Render for MarkdownPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.keep_seen(window, cx);
        let p = theme::palette(cx);
        let (card, look) = self.look(p, cx);
        let text = TextView::new(&self.state)
            .selectable(true)
            .table_style(tables())
            .plugin(MermaidPlugin::new(look, card, p, self.seen.clone()))
            .on_link_click(opens_links(self.folder.clone()));
        div()
            .id("markdown-preview")
            .key_context(CONTEXT)
            .on_action(cx.listener(|this, _: &LineUp, _, cx| this.scroll_by(-LINE, cx)))
            .on_action(cx.listener(|this, _: &LineDown, _, cx| this.scroll_by(LINE, cx)))
            .on_action(cx.listener(|this, _: &PageUp, _, cx| this.page(-1., cx)))
            .on_action(cx.listener(|this, _: &PageDown, _, cx| this.page(1., cx)))
            .on_action(cx.listener(|this, _: &Top, _, cx| this.scroll_by(f32::MIN, cx)))
            .on_action(cx.listener(|this, _: &Bottom, _, cx| this.scroll_by(f32::MAX, cx)))
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(
                div()
                    .w_full()
                    .max_w(px(COLUMN + 2. * SIDE))
                    .mx_auto()
                    .px(px(SIDE))
                    .pt(px(TOP))
                    .pb(px(BOTTOM))
                    .text_size(px(TEXT))
                    .text_color(p.text)
                    .child(text),
            )
    }
}
