//! A terminal pane in responsive mode (`docs/responsive.md` 3.3), drawn as a browser page in
//! it is, with the same chrome (`crate::responsive`): the device bar over it with the sizes
//! agent-terminal's TUI QA tries as presets, the terminal centred in the sunken area under it
//! with a 1 px outline, and grips that drag it in whole cells. Its size is the daemon's pin
//! (`colosso_term`'s view asks for it as the grip moves, one at a time); a zoom is the font's size as a
//! share of the person's.

use std::ops::RangeInclusive;
use std::rc::Rc;

use colosso_term::{Scale, TerminalEvent, TerminalView, ToggleResponsive};
use colosso_term_control::{ActorKind, PIN_CELLS};
use gpui_kit::{
    Action, App, ClickEvent, Context, Div, Entity, Focusable as _,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, SharedString,
    Styled as _, Subscription, Window, div, px, size,
};

use crate::responsive::bar::{self, Bar, Fields};
use crate::responsive::{Drag, GUTTER, Resizable, Zoom, grips};
use crate::shell::Shell;
use crate::theme;

mod frame;

/// The presets, each portrait: the sizes agent-terminal's TUI QA tries.
const PRESETS: [(u16, u16); 3] = [(80, 24), (120, 40), (200, 60)];

/// A size picked from the bar's menu for terminal tab `terminal`: a preset by its place, or
/// `None` for a free size.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = terminal_size, no_json)]
pub struct PickSize {
    pub terminal: u64,
    pub preset: Option<usize>,
}

/// A zoom picked from the bar's menu for terminal tab `terminal`.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = terminal_size, no_json)]
pub struct PickZoom {
    pub terminal: u64,
    pub zoom: Zoom,
}

/// Terminal tab `id`'s terminal, and responsive mode round it while it's pinned.
pub struct ResponsiveTerminal {
    id: u64,
    view: Entity<TerminalView>,
    /// The bar's width and height fields, made the first time the bar shows.
    fields: Option<Fields>,
    /// The preset picked, and whether it's turned, while the pin is its size.
    preset: Option<(usize, bool)>,
    zoom: Zoom,
    drag: Option<Drag>,
    /// The size the person asked for last, shown until the daemon's pin is it.
    asked: Option<(u16, u16)>,
    _events: Subscription,
}

impl ResponsiveTerminal {
    pub fn new(id: u64, view: Entity<TerminalView>, cx: &mut Context<Self>) -> Self {
        let events = cx.subscribe(&view, |this, _, event, cx| {
            if *event == TerminalEvent::Responsive {
                this.pin_changed(cx);
            }
        });
        ResponsiveTerminal {
            id,
            view,
            fields: None,
            preset: None,
            zoom: Zoom::Fit,
            drag: None,
            asked: None,
            _events: events,
        }
    }

    /// The size shown: what the person asked for last, or the daemon's pin.
    fn size(&self, cx: &App) -> Option<(u16, u16)> {
        let pinned = self.view.read(cx).pinned()?;
        Some(self.asked.unwrap_or((pinned.cols, pinned.rows)))
    }

    /// The preset shown, while the size is its.
    fn shown_preset(&self, cx: &App) -> Option<(usize, bool)> {
        preset_at(self.preset, self.size(cx)?)
    }

    /// The room the terminal has at its true size, under the bar (`barred`, or as it will
    /// be once the bar shows) and inside the gutters.
    fn room(&self, cx: &App) -> Option<(Pixels, Pixels)> {
        let view = self.view.read(cx);
        let laid = view.laid_out_in()?;
        // Pinned, the terminal is laid out in the room already.
        if view.pinned().is_some() {
            return Some((laid.size.width, laid.size.height));
        }
        let room = crate::responsive::room(size(
            laid.size.width,
            laid.size.height - px(bar::HEIGHT),
        ));
        Some((room.width, room.height))
    }

    /// The cells the room holds at the person's font.
    fn cells_fitting(&self, cx: &App) -> Option<(u16, u16)> {
        let (width, height) = self.room(cx)?;
        self.view.read(cx).cells_in(width, height)
    }

    /// ⌥⌘R and the terminal bar's button: pins the terminal at what the pane holds at the
    /// person's font, so it shows at 100 %, or lets it fill the pane again.
    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        if self.view.read(cx).pinned().is_some() {
            return self.close(cx);
        }
        let current = self.view.read(cx).cells();
        let Some(cells) = self.cells_fitting(cx).or(current) else {
            return;
        };
        (self.preset, self.zoom) = (None, Zoom::Fit);
        self.set_scale(cx);
        self.ask(cells, cx);
    }

    /// ✕: the terminal fills the pane again, whoever pinned it.
    fn close(&mut self, cx: &mut Context<Self>) {
        (self.drag, self.asked) = (None, None);
        self.view.update(cx, TerminalView::unpin);
        cx.notify();
    }

    /// Asks the daemon for `cells`, shown at once as the view keeps them within its limits.
    fn ask(&mut self, (cols, rows): (u16, u16), cx: &mut Context<Self>) {
        let asked = self.view.update(cx, |view, cx| view.pin(cols, rows, cx));
        self.asked = Some(asked);
        cx.notify();
    }

    /// The daemon's pin changed, or the font the terminal's fitted at.
    fn pin_changed(&mut self, cx: &mut Context<Self>) {
        let Some(pinned) = self.view.read(cx).pinned().cloned() else {
            (self.preset, self.zoom, self.drag, self.asked) = (None, Zoom::Fit, None, None);
            self.set_scale(cx);
            return cx.notify();
        };
        let now = Some((pinned.cols, pinned.rows));
        if pinned.by.kind == ActorKind::Agent {
            // An agent's size is a free one, as an agent's width or height is a page's.
            (self.preset, self.asked) = (None, None);
        } else if self.drag.is_none() && self.asked == now {
            self.asked = None;
        }
        cx.notify();
    }

    /// Picks a preset, or a free size (`None`) at the size shown.
    pub fn pick_preset(&mut self, preset: Option<usize>, cx: &mut Context<Self>) {
        let Some(at) = preset.filter(|at| *at < PRESETS.len()) else {
            self.preset = None;
            return cx.notify();
        };
        self.preset = Some((at, false));
        self.ask(PRESETS[at], cx);
    }

    pub fn pick_zoom(&mut self, zoom: Zoom, cx: &mut Context<Self>) {
        self.zoom = zoom;
        self.set_scale(cx);
        cx.notify();
    }

    /// Rotate: the preset turned the other way.
    fn rotate(&mut self, cx: &mut Context<Self>) {
        let Some((at, turned)) = self.shown_preset(cx) else {
            return;
        };
        self.preset = Some((at, !turned));
        self.ask(turn(PRESETS[at], !turned), cx);
    }

    /// The terminal's font as the zoom has it, or as it is while a grip is in hand.
    fn set_scale(&self, cx: &mut Context<Self>) {
        let scale = match self.zoom {
            Zoom::Fit if self.drag.is_some() => Scale::Held,
            Zoom::Fit => Scale::Fit,
            Zoom::Percent(percent) => Scale::Percent(percent),
        };
        self.view.update(cx, |view, cx| view.set_scale(scale, cx));
    }

    /// Shows the size in the fields, those being edited only when `restore`.
    fn show_size(&self, restore: bool, window: &mut Window, cx: &mut App) {
        if let (Some(fields), Some((cols, rows))) = (&self.fields, self.size(cx)) {
            fields.show(cols.into(), rows.into(), restore, window, cx);
        }
    }

    /// Enter in a field: the terminal takes the size typed, or shows its own again.
    fn size_typed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((cols, rows)) = self.fields.as_ref().and_then(|fields| fields.typed(cx)) {
            self.ask(cells(cols, rows), cx);
        }
        self.show_size(true, window, cx);
    }

    /// The device bar.
    fn bar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Div> {
        if self.fields.is_none() {
            self.fields = Some(Fields::new(Self::size_typed, window, cx));
        }
        self.show_size(false, window, cx);
        let terminal = self.id;
        let pinned = self.view.read(cx).pinned().cloned();
        let agent = pinned
            .filter(|pinned| pinned.by.kind == ActorKind::Agent)
            .map(|pinned| pinned.by.id);
        let me = cx.entity().downgrade();
        let restore = Rc::new(move |window: &mut Window, cx: &mut App| {
            let _ = me.update(cx, |this, cx| {
                this.show_size(true, window, cx);
                window.focus(&this.view.focus_handle(cx), cx);
            });
        });
        let rotate = cx.listener(|this, _: &ClickEvent, _, cx| this.rotate(cx));
        let close = cx.listener(|this, _: &ClickEvent, _, cx| this.close(cx));
        let bar = Bar {
            ids: "terminal",
            agent,
            presets: preset_names(),
            preset: self.shown_preset(cx).map(|(at, _)| at),
            pick_preset: Rc::new(move |preset| Box::new(PickSize { terminal, preset })),
            fields: self.fields.as_ref()?,
            zoom: self.zoom,
            shown_percent: u32::from(self.view.read(cx).font_percent()),
            pick_zoom: Rc::new(move |zoom| Box::new(PickZoom { terminal, zoom })),
            restore,
            rotate: Rc::new(rotate),
            extra: None,
            close: Rc::new(close),
        };
        Some(bar.render(cx))
    }
}

/// The presets as the menu names them: `80 × 24`.
fn preset_names() -> Vec<SharedString> {
    (PRESETS.iter())
        .map(|(cols, rows)| format!("{cols} × {rows}").into())
        .collect()
}

/// `cols` × `rows` as a pin's cells, a side past what a cell count holds at its most.
fn cells(cols: u32, rows: u32) -> (u16, u16) {
    let count = |count: u32| u16::try_from(count).unwrap_or(u16::MAX);
    (count(cols), count(rows))
}

/// `preset`'s columns and rows, turned to landscape when `turned`.
fn turn(preset: (u16, u16), turned: bool) -> (u16, u16) {
    if turned { (preset.1, preset.0) } else { preset }
}

/// The preset picked, while `size` is its size as it's turned: a size from elsewhere (a
/// drag, a field, an agent) is a free one.
fn preset_at(preset: Option<(usize, bool)>, size: (u16, u16)) -> Option<(usize, bool)> {
    preset.filter(|(at, turned)| PRESETS.get(*at).map(|preset| turn(*preset, *turned)) == Some(size))
}

impl Resizable for ResponsiveTerminal {
    fn drag(&mut self) -> &mut Option<Drag> {
        &mut self.drag
    }

    fn grip_size(&self, cx: &App) -> Option<((u32, u32), (f32, f32))> {
        // A preset is dragged by no grip, as a device isn't.
        if self.shown_preset(cx).is_some() {
            return None;
        }
        let (cols, rows) = self.size(cx)?;
        let (_, cell) = self.view.read(cx).pinned_grid()?;
        Some(((cols.into(), rows.into()), cell))
    }

    fn side_limits(&self, _: &App) -> RangeInclusive<u32> {
        u32::from(*PIN_CELLS.start())..=u32::from(*PIN_CELLS.end())
    }

    fn filling_size(&self, cx: &App) -> Option<(u32, u32)> {
        let (cols, rows) = self.cells_fitting(cx)?;
        Some((cols.into(), rows.into()))
    }

    fn grip_sized(&mut self, width: u32, height: u32, cx: &mut Context<Self>) {
        self.ask(cells(width, height), cx);
    }

    fn grip_held(&mut self, _held: bool, cx: &mut Context<Self>) {
        self.set_scale(cx);
    }
}

impl Render for ResponsiveTerminal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = div()
            .size_full()
            .flex()
            .flex_col()
            .on_action(cx.listener(|this, _: &ToggleResponsive, _, cx| this.toggle(cx)));
        let Some((cols, rows)) = self.size(cx) else {
            return root.child(self.view.clone());
        };
        let p = theme::palette(cx);
        let free = self.shown_preset(cx).is_none();
        let bar = self.bar(window, cx);
        let area = div()
            .flex_1()
            .min_h_0()
            .w_full()
            .relative()
            .overflow_hidden()
            .bg(p.sunken)
            .child(div().size_full().p(px(GUTTER)).child(self.view.clone()))
            .child(frame::Frame {
                host: cx.entity(),
                view: self.view.clone(),
                free,
                palette: p,
            })
            .children(grips::label(self.drag.as_ref(), cols.into(), rows.into(), p));
        root.children(bar).child(area)
    }
}

impl Shell {
    /// Terminal tab `id`'s responsive mode, once its view is made.
    fn responsive_terminal(&self, id: u64) -> Option<Entity<ResponsiveTerminal>> {
        Some(self.terminals.views.get(&id)?.responsive.clone())
    }
}

/// What the window does with the bar's menus' picks: routes them to the terminal they name.
pub fn handle_actions(root: Div, cx: &mut Context<Shell>) -> Div {
    root.on_action(cx.listener(|this, pick: &PickSize, _, cx| {
        if let Some(terminal) = this.responsive_terminal(pick.terminal) {
            terminal.update(cx, |terminal, cx| terminal.pick_preset(pick.preset, cx));
        }
    }))
    .on_action(cx.listener(|this, pick: &PickZoom, _, cx| {
        if let Some(terminal) = this.responsive_terminal(pick.terminal) {
            terminal.update(cx, |terminal, cx| terminal.pick_zoom(pick.zoom, cx));
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_presets_are_agent_terminals_sizes_and_any_other_size_is_free() {
        assert_eq!(preset_names(), ["80 × 24", "120 × 40", "200 × 60"]);
        assert_eq!(preset_at(Some((1, false)), (120, 40)), Some((1, false)));
        // Turned, it's the preset at its landscape size only.
        assert_eq!(turn(PRESETS[0], true), (24, 80));
        assert_eq!(preset_at(Some((0, true)), (24, 80)), Some((0, true)));
        assert_eq!(preset_at(Some((0, true)), (80, 24)), None);
        // Dragged, typed or set by an agent to another size, it's Responsive.
        assert_eq!(preset_at(Some((2, false)), (201, 60)), None);
        assert_eq!(preset_at(None, (80, 24)), None, "picked as a free size");
        assert_eq!(preset_at(Some((9, false)), (80, 24)), None);
    }

    #[test]
    fn a_pick_names_its_terminal_and_preset() {
        let pick = PickSize {
            terminal: 7,
            preset: Some(2),
        };
        assert!(pick.boxed_clone().partial_eq(&pick));
        let zoom = PickZoom {
            terminal: 7,
            zoom: Zoom::Percent(75),
        };
        assert!(!zoom.partial_eq(&PickZoom {
            terminal: 8,
            zoom: Zoom::Percent(75)
        }));
    }
}
