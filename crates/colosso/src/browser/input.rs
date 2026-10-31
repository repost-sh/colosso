//! The person's input to a page: the pointer and wheel over it, the keys it owns while it
//! has focus, text from the input method, and the edit commands the app binds. The first
//! of it takes the page for the person (`control`).

use std::ops::Range;

use colosso_browser_ipc::{self as ipc, AppRequest, Cursor, Go, Ime, KeyKind, MouseKind};
use gpui_kit::{
    Bounds, Context, CursorStyle, Div, EntityInputHandler, InteractiveElement as _, KeyDownEvent,
    KeyUpEvent, Keystroke, Modifiers, ModifiersChangedEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, NavigationDirection, Pixels, PlatformInput, Point, ScrollDelta,
    ScrollEvent, Stateful, UTF16Selection, Window, point, px, size,
};

use super::control::Mode;
use super::keys::{characters, key_codes};
use super::view::BrowserView;
use super::{
    Back, Copy, Cut, FocusAddress, Forward, Paste, Redo, Reload, SelectAll, SendBackTab,
    SendEscape, SendTab, ToggleDevTools, Undo,
};

/// Chromium scrolls about this many pixels per line.
const LINE_PIXELS: f32 = 40.;

impl BrowserView {
    /// A press on the page: it takes the keyboard, and the button stays the page's until
    /// it's let go, wherever the pointer goes meanwhile.
    pub(super) fn pressed_at(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The mouse's side buttons step through the page's history, as in any browser;
        // `DevTools` have none worth stepping through.
        if let MouseButton::Navigate(direction) = event.button {
            if self.inspected().is_none() {
                self.go(history_step(direction), cx);
            }
            return;
        }
        let Some(button) = button_of(event.button) else {
            return;
        };
        window.focus(&self.focus, cx);
        self.pressed = Some(button);
        let (at, count, modifiers) = (event.position, event.click_count, event.modifiers);
        self.mouse(MouseKind::Down, at, Some(button), count, modifiers, cx);
    }

    /// A button let go, if it was pressed on the page.
    pub(super) fn released_at(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        let Some(button) = button_of(event.button) else {
            return;
        };
        if self.pressed.take() == Some(button) {
            let (at, count, modifiers) = (event.position, event.click_count, event.modifiers);
            self.mouse(MouseKind::Up, at, Some(button), count, modifiers, cx);
        }
    }

    /// The pointer moved, `over` the page or not: the page follows it while it's over it
    /// or a button pressed on it is held, and is told when it leaves.
    pub(super) fn moved_to(&mut self, event: &MouseMoveEvent, over: bool, cx: &mut Context<Self>) {
        let (at, modifiers) = (event.position, event.modifiers);
        if over || self.pressed.is_some() {
            if !std::mem::replace(&mut self.hovered, true) {
                // Its cursor shows from now on.
                cx.notify();
            }
            let button = event.pressed_button.and_then(button_of);
            self.mouse(MouseKind::Move, at, button, 1, modifiers, cx);
        } else if std::mem::take(&mut self.hovered) {
            cx.notify();
            self.mouse(MouseKind::Leave, at, None, 1, modifiers, cx);
        }
    }

    /// The pointer over the page's native view, which takes it in GPUI's stead.
    pub(super) fn native_pointer(
        &mut self,
        event: &PlatformInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            PlatformInput::MouseDown(event) => self.pressed_at(event, window, cx),
            PlatformInput::MouseUp(event) => self.released_at(event, cx),
            PlatformInput::MouseMove(event) => self.moved_to(event, true, cx),
            PlatformInput::MouseExited(event) => {
                let moved = MouseMoveEvent {
                    position: event.position,
                    pressed_button: event.pressed_button,
                    modifiers: event.modifiers,
                };
                self.moved_to(&moved, false, cx);
            }
            PlatformInput::Scroll(event) => self.wheel(event, cx),
            _ => {}
        }
    }

    pub(super) fn mouse(
        &mut self,
        kind: MouseKind,
        position: Point<Pixels>,
        button: Option<ipc::MouseButton>,
        click_count: usize,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        if kind == MouseKind::Down {
            self.person_acts(Mode::Implicit, cx);
            self.resolve_menu(cx);
        }
        let at = position - self.origin;
        self.send_to_page(
            AppRequest::Mouse {
                pane: self.pane,
                kind,
                x: at.x.as_f32(),
                y: at.y.as_f32(),
                button,
                click_count: u32::try_from(click_count).unwrap_or(1),
                modifiers: modifiers_of(modifiers),
            },
            cx,
        );
    }

    pub(super) fn wheel(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        let (dx, dy) = match event.delta {
            ScrollDelta::Pixels(delta) => (delta.x.as_f32(), delta.y.as_f32()),
            ScrollDelta::Lines(delta) => (delta.x * LINE_PIXELS, delta.y * LINE_PIXELS),
        };
        let at = event.position - self.origin;
        self.send_to_page(
            AppRequest::Wheel {
                pane: self.pane,
                x: at.x.as_f32(),
                y: at.y.as_f32(),
                dx,
                dy,
                modifiers: modifiers_of(event.modifiers),
            },
            cx,
        );
    }

    fn key(
        &mut self,
        kind: KeyKind,
        keystroke: &Keystroke,
        text: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.person_acts(Mode::Implicit, cx);
        if !self.is_open(cx) {
            return;
        }
        let (key_code, native_key_code) = key_codes(&keystroke.key);
        // A press or release carries the key's character, as a Mac key event does: Chromium
        // reads one without as a modifier key's, every release then a second press.
        let text = match kind {
            KeyKind::Char => text,
            KeyKind::Down | KeyKind::Up => text.or_else(|| characters(keystroke)),
        };
        self.send(
            AppRequest::Key {
                pane: self.pane,
                kind,
                key_code,
                native_key_code,
                text,
                modifiers: modifiers_of(keystroke.modifiers),
            },
            cx,
        );
    }

    fn key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        self.resolve_menu(cx);
        if self.marked.is_some() {
            return;
        }
        let keystroke = &event.keystroke;
        self.key(KeyKind::Down, keystroke, None, cx);
        // Text comes through the input handler; these keys produce none there, but pages
        // expect their characters.
        let character = match keystroke.key.as_str() {
            "enter" if !keystroke.modifiers.platform => Some("\r"),
            _ => None,
        };
        if let Some(character) = character {
            self.key(KeyKind::Char, keystroke, Some(character.into()), cx);
        }
        // Keys that never type text must not reach the input method; keys that do continue
        // to it.
        if keystroke.key_char.is_none() || character.is_some() {
            cx.stop_propagation();
        }
    }

    /// A modifier key pressed or let go, as pages hear it. It doesn't take the page: the
    /// person reaching for a shortcut elsewhere leaves an agent driving it.
    fn modifiers_changed(&mut self, now: Modifiers, cx: &mut Context<Self>) {
        let was = std::mem::replace(&mut self.held, now);
        if !self.is_open(cx) || self.marked.is_some() {
            return;
        }
        let keys = [
            ("shift", was.shift, now.shift),
            ("control", was.control, now.control),
            ("alt", was.alt, now.alt),
            ("platform", was.platform, now.platform),
        ];
        for (key, _, after) in keys
            .into_iter()
            .filter(|(_, before, after)| before != after)
        {
            let kind = if after { KeyKind::Down } else { KeyKind::Up };
            let (key_code, native_key_code) = key_codes(key);
            self.send(
                AppRequest::Key {
                    pane: self.pane,
                    kind,
                    key_code,
                    native_key_code,
                    text: None,
                    modifiers: modifiers_of(now),
                },
                cx,
            );
        }
    }

    fn key_up(&mut self, event: &KeyUpEvent, cx: &mut Context<Self>) {
        if self.marked.is_none() {
            self.key(KeyKind::Up, &event.keystroke, None, cx);
        }
    }

    /// A key the shell binds, pressed on the page as if it had no binding. Its release
    /// reaches the page as any key's does.
    fn press(&mut self, key: &str, shift: bool, character: Option<&str>, cx: &mut Context<Self>) {
        let keystroke = Keystroke {
            modifiers: Modifiers {
                shift,
                ..Modifiers::default()
            },
            key: key.into(),
            key_char: character.map(Into::into),
        };
        self.key(KeyKind::Down, &keystroke, None, cx);
        if let Some(character) = character {
            self.key(KeyKind::Char, &keystroke, Some(character.into()), cx);
        }
    }

    fn edit(&mut self, command: ipc::Edit, cx: &mut Context<Self>) {
        self.person_acts(Mode::Implicit, cx);
        let pane = self.pane;
        self.send_to_page(AppRequest::Edit { pane, command }, cx);
    }

    fn ime(&mut self, action: Ime, cx: &mut Context<Self>) {
        self.person_acts(Mode::Implicit, cx);
        let pane = self.pane;
        self.send_to_page(AppRequest::Ime { pane, action }, cx);
    }
}

/// `page` hearing the keys and the actions bound to the page.
pub(super) fn listen(page: Stateful<Div>, cx: &mut Context<BrowserView>) -> Stateful<Div> {
    page.on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| this.key_down(event, cx)))
        .on_key_up(cx.listener(|this, event: &KeyUpEvent, _, cx| this.key_up(event, cx)))
        .on_modifiers_changed(cx.listener(|this, event: &ModifiersChangedEvent, _, cx| {
            this.modifiers_changed(event.modifiers, cx);
        }))
        .on_action(cx.listener(|this, _: &Copy, _, cx| this.edit(ipc::Edit::Copy, cx)))
        .on_action(cx.listener(|this, _: &Cut, _, cx| this.edit(ipc::Edit::Cut, cx)))
        .on_action(cx.listener(|this, _: &Paste, _, cx| this.edit(ipc::Edit::Paste, cx)))
        .on_action(cx.listener(|this, _: &SelectAll, _, cx| this.edit(ipc::Edit::SelectAll, cx)))
        .on_action(cx.listener(|this, _: &Undo, _, cx| this.edit(ipc::Edit::Undo, cx)))
        .on_action(cx.listener(|this, _: &Redo, _, cx| this.edit(ipc::Edit::Redo, cx)))
        .on_action(cx.listener(|this, _: &SendTab, _, cx| this.press("tab", false, Some("\t"), cx)))
        .on_action(cx.listener(|this, _: &SendBackTab, _, cx| {
            this.press("tab", true, Some("\t"), cx);
        }))
        .on_action(cx.listener(|this, _: &SendEscape, _, cx| this.press("escape", false, None, cx)))
}

/// `view` hearing the keys that steer the page from anywhere in its pane: the page, the
/// address bar and the find bar.
pub(super) fn steer(view: Div, cx: &mut Context<BrowserView>) -> Div {
    view.on_action(cx.listener(|this, _: &Back, _, cx| this.go(Go::Back, cx)))
        .on_action(cx.listener(|this, _: &Forward, _, cx| this.go(Go::Forward, cx)))
        .on_action(cx.listener(|this, _: &Reload, _, cx| this.go(Go::Reload, cx)))
        .on_action(cx.listener(|this, _: &FocusAddress, window, cx| {
            this.focus_address(window, cx);
        }))
        .on_action(cx.listener(|this, _: &ToggleDevTools, _, cx| this.inspect(None, cx)))
}

impl EntityInputHandler for BrowserView {
    fn text_for_range(
        &mut self,
        _: Range<usize>,
        _: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let end = self.marked.as_ref().map_or(0, |m| m.encode_utf16().count());
        Some(UTF16Selection {
            range: end..end,
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked
            .as_ref()
            .map(|marked| 0..marked.encode_utf16().count())
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.marked.take() {
            self.ime(Ime::Commit { text }, cx);
        }
    }

    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.marked.take().is_some() {
            self.ime(Ime::Commit { text: text.into() }, cx);
            return;
        }
        if text.is_empty() {
            return;
        }
        let keystroke = Keystroke {
            modifiers: Modifiers::default(),
            key: text.to_lowercase(),
            key_char: Some(text.into()),
        };
        self.key(KeyKind::Char, &keystroke, Some(text.into()), cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        selection: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if text.is_empty() {
            if self.marked.take().is_some() {
                self.ime(Ime::Cancel, cx);
            }
            return;
        }
        let length = u32::try_from(text.encode_utf16().count()).unwrap_or(u32::MAX);
        let selection = selection.map_or((length, length), |range| {
            (
                u32::try_from(range.start).unwrap_or(length),
                u32::try_from(range.end).unwrap_or(length),
            )
        });
        self.marked = Some(text.into());
        self.ime(
            Ime::Compose {
                text: text.into(),
                selection,
            },
            cx,
        );
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // The candidate window sits at the pane's lower left until the host reports the
        // caret.
        Some(Bounds::new(
            point(
                element_bounds.left() + px(12.),
                element_bounds.bottom() - px(28.),
            ),
            size(px(1.), px(20.)),
        ))
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

pub(super) fn modifiers_of(modifiers: Modifiers) -> ipc::Modifiers {
    ipc::Modifiers {
        shift: modifiers.shift,
        control: modifiers.control,
        alt: modifiers.alt,
        platform: modifiers.platform,
    }
}

/// A page's button as the window names it, the other way from [`button_of`].
pub(super) fn platform_button(button: ipc::MouseButton) -> MouseButton {
    match button {
        ipc::MouseButton::Left => MouseButton::Left,
        ipc::MouseButton::Middle => MouseButton::Middle,
        ipc::MouseButton::Right => MouseButton::Right,
    }
}

/// Where a mouse side button takes the page.
pub(super) fn history_step(direction: NavigationDirection) -> Go {
    match direction {
        NavigationDirection::Back => Go::Back,
        NavigationDirection::Forward => Go::Forward,
    }
}

pub(super) fn button_of(button: MouseButton) -> Option<ipc::MouseButton> {
    match button {
        MouseButton::Left => Some(ipc::MouseButton::Left),
        MouseButton::Middle => Some(ipc::MouseButton::Middle),
        MouseButton::Right => Some(ipc::MouseButton::Right),
        MouseButton::Navigate(_) => None,
    }
}

pub(super) fn cursor_style(cursor: Cursor) -> CursorStyle {
    match cursor {
        Cursor::Arrow | Cursor::Wait | Cursor::None => CursorStyle::Arrow,
        Cursor::Pointer => CursorStyle::PointingHand,
        Cursor::Text => CursorStyle::IBeam,
        Cursor::Crosshair => CursorStyle::Crosshair,
        Cursor::NotAllowed => CursorStyle::OperationNotAllowed,
        Cursor::Grab => CursorStyle::OpenHand,
        Cursor::Grabbing => CursorStyle::ClosedHand,
        Cursor::ResizeColumn => CursorStyle::ResizeLeftRight,
        Cursor::ResizeRow => CursorStyle::ResizeUpDown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_buttons_are_not_page_buttons() {
        assert_eq!(button_of(MouseButton::Left), Some(ipc::MouseButton::Left));
        assert_eq!(
            button_of(MouseButton::Navigate(NavigationDirection::Back)),
            None
        );
        // They step through the page's history instead.
        assert_eq!(history_step(NavigationDirection::Back), Go::Back);
        assert_eq!(history_step(NavigationDirection::Forward), Go::Forward);
    }
}
