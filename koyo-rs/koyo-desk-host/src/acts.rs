//! Acts on the desktop's windows (`docs/agent-desktop.md` 7): an agent's, by point or by a
//! snapshot's element, and the person's own from the tab. Through accessibility where an
//! element takes the act, by events to the window's process otherwise; never at the person's
//! pointer or keyboard.

use std::collections::VecDeque;

use koyo_desk_wire::Point;
use koyo_desk_wire::keys;
use koyo_desk_wire::link::{
    Act, Answer, Button, FromHost, Input, Refusal, RefusalCode, RequestId, Spot, Window,
};

use crate::ax::{Done, Element};
use crate::host::{Host, Typing};
use crate::input::{Poster, Target};
use crate::ns;
use crate::windows;

/// The keys text's line break and tab are typed as.
const RETURN: u16 = 36;
const TAB: u16 = 48;

impl Host {
    pub fn target_of(&self, window: &Window) -> Target {
        Target {
            pid: window.pid,
            window: window.id,
            frame: window.frame,
            through: windows::through(&self.hosted, window.id),
        }
    }

    /// The window under a point among the displays.
    fn target_at(&self, at: Point) -> Result<Target, Refusal> {
        windows::under(&self.windows, at)
            .map(|window| self.target_of(window))
            .ok_or_else(|| {
                let at = self.local_point(at);
                let message = format!("no window of the agent's at {:.0},{:.0}", at.x, at.y);
                Refusal::new(RefusalCode::NotFound, message)
            })
    }

    /// The window an act at `at` lands in: the frontmost there, or `window` when one is
    /// named, raised first if another covers the point.
    fn target_for(&mut self, at: Point, window: Option<u32>) -> Result<Target, Refusal> {
        let Some(id) = window else {
            return self.target_at(at);
        };
        for attempt in 0..2 {
            if !self.listed(id)?.frame.contains(at) {
                let message = format!("window {id} isn't at that point");
                return Err(Refusal::new(RefusalCode::Invalid, message));
            }
            let front = self.target_at(at)?;
            if front.window == id {
                return Ok(front);
            }
            if attempt == 0 {
                self.raise(id)?;
            }
        }
        let message = format!("window {id} is covered at that point and didn't come to the front");
        Err(Refusal::new(RefusalCode::Failed, message))
    }

    /// Brings a window in front of the agent's others, through accessibility: the app is
    /// asked, nothing is activated.
    pub fn raise(&mut self, id: u32) -> Result<(), Refusal> {
        let listed = self.listed(id)?.clone();
        self.activate(listed.pid);
        let element = (self.apps.iter_mut())
            .find(|app| app.pid == listed.pid)
            .and_then(|app| app.window(id));
        let raised = element.is_some_and(|element| {
            element.set("AXMain", &ns::yes());
            element.act("AXRaise") != Done::No
        });
        if !raised {
            let message = format!("window {id} doesn't take being raised");
            return Err(Refusal::new(RefusalCode::Failed, message));
        }
        self.focus = Some(id);
        // The window server lists the new order a moment later.
        ns::pump_until(std::time::Duration::from_millis(600), || {
            let order = windows::on_desktop(self);
            order.first().map(|front| front.id) == Some(id)
        });
        self.list();
        Ok(())
    }

    /// A point among the displays as the app counts it, from the desktop's top left.
    pub fn local_point(&self, at: Point) -> Point {
        let origin = self.global(Point::default());
        Point {
            x: at.x - origin.x,
            y: at.y - origin.y,
        }
    }

    /// A point the app gave, counted from the desktop's top left, as one among the displays.
    fn global(&self, at: Point) -> Point {
        self.desktop
            .as_ref()
            .map_or(at, |desktop| desktop.global(at))
    }

    /// Readies a window for clicks and keys: its app told it's active, once, and that this
    /// window is its key window, whenever that isn't the window it was last told.
    pub fn aim(&mut self, target: Target) {
        self.front.acted(target.pid);
        self.activate(target.pid);
        if self.keyed != Some(target.window) {
            self.keyed = Some(target.window);
            crate::skylight::make_key(target.pid, target.window);
            ns::pause(std::time::Duration::from_millis(20));
        }
    }

    /// Tells the app it's active, once: its window becomes key and takes first clicks and keys.
    pub fn activate(&mut self, pid: i32) {
        if let Some(app) = self.apps.iter_mut().find(|app| app.pid == pid)
            && !app.told_active
        {
            app.told_active = true;
            Poster::tell_active(pid);
            ns::pause(std::time::Duration::from_millis(30));
        }
    }

    fn element(&self, name: &str) -> Result<Element, Refusal> {
        self.refs.get(name).cloned().ok_or_else(|| {
            let message = format!("no element {name} in the last snapshot: take a new one");
            Refusal::new(RefusalCode::NotFound, message)
        })
    }

    /// Where a spot is among the displays, and its element if it is one.
    fn spot(&self, spot: &Spot) -> Result<(Point, Option<Element>), Refusal> {
        match spot {
            Spot::Point(at) => Ok((self.global(*at), None)),
            Spot::Element(name) => {
                let element = self.element(name)?;
                let frame = element.frame().ok_or_else(|| {
                    let message = format!("element {name} is gone: take a new snapshot");
                    Refusal::new(RefusalCode::NotFound, message)
                })?;
                Ok((frame.center(), Some(element)))
            }
        }
    }

    /// Tells the app where the agent acted, counted from the desktop's top left.
    fn pointer(&mut self, at: Point, click: bool) {
        let at = self.local_point(at);
        self.tell(&FromHost::Pointer { at, click });
    }

    /// A key combo. A copy or a cut would take the person's clipboard: theirs is kept round
    /// it and put back, and what the agent copied is its answer.
    fn keys(&mut self, combo: &str, window: Option<u32>) -> Result<Answer, Refusal> {
        let combo = keys::combo(combo).map_err(|why| Refusal::new(RefusalCode::Invalid, why))?;
        let target = self.keys_to(window)?;
        // The key that types the combo's character on the person's layout, which is how the
        // app will read it; a key named otherwise (`return`, `left`) is where it always is.
        let code = (combo.character)
            .and_then(crate::layout::code_for)
            .unwrap_or(combo.code);
        let copies =
            combo.flags == keys::flags::COMMAND && matches!(combo.character, Some('c' | 'x'));
        let kept = copies.then(crate::clipboard::Kept::take);
        self.poster.press(target, code, combo.flags);
        Ok(match kept.and_then(crate::clipboard::Kept::give_back) {
            Some(text) => Answer::Copied { text },
            None => Answer::Done,
        })
    }

    /// Does an agent's act.
    pub fn act(&mut self, act: Act) -> Result<Answer, Refusal> {
        match act {
            Act::Raise { window } => self.raise(window)?,
            Act::Click {
                spot,
                window,
                button,
                count,
                flags,
            } => {
                let (at, element) = self.spot(&spot)?;
                let plain = button == Button::Left && count <= 1 && flags == 0;
                if let (Some(element), true) = (&element, plain) {
                    // A menu bar's title pressed opens the menu itself, which takes the
                    // person's keyboard for as long as it's up: an item is pressed instead.
                    if element.text("AXRole").as_deref() == Some("AXMenuBarItem") {
                        let message = "a menu's title isn't pressed: press the item you want in \
                                       it by its ref, which needs no menu open";
                        return Err(Refusal::new(RefusalCode::Invalid, message));
                    }
                    match element.act("AXPress") {
                        Done::Yes | Done::NoAnswer => {
                            // An element of a panel another process draws names that
                            // process's window: the keys are in the agent's.
                            self.focus = (element.window_id())
                                .map(|id| windows::listed_for(&self.hosted, id))
                                .or(self.focus);
                            self.pointer(at, true);
                            return Ok(Answer::Done);
                        }
                        Done::Gone => {
                            let message = "that element is gone: take a new snapshot";
                            return Err(Refusal::new(RefusalCode::NotFound, message));
                        }
                        Done::No => {}
                    }
                }
                let target = self.target_for(at, window)?;
                self.aim(target);
                self.focus = Some(target.window);
                self.pointer(at, true);
                self.poster.click(target, at, button, count, flags);
            }
            Act::Move { spot, window } => {
                let (at, _) = self.spot(&spot)?;
                if let Ok(target) = self.target_for(at, window) {
                    self.poster.move_to(target, at);
                }
                self.pointer(at, false);
            }
            Act::Drag { from, to, window } => {
                let ((from, _), (to, _)) = (self.spot(&from)?, self.spot(&to)?);
                let target = self.target_for(from, window)?;
                self.aim(target);
                self.focus = Some(target.window);
                self.pointer(from, true);
                if !self.moves(target, from, to) {
                    self.poster.drag(target, from, to);
                }
                self.pointer(to, false);
            }
            Act::Scroll {
                spot,
                window,
                dx,
                dy,
            } => {
                let (at, _) = self.spot(&spot)?;
                let target = self.target_for(at, window)?;
                self.pointer(at, false);
                // The wheel's own sign is the other way round: a positive delta shows what
                // is further up.
                self.poster.scroll(target, at, -dx, -dy);
            }
            // Typing answers later, turn by turn ([`Host::begin_typing`]).
            Act::Type { text, into } => {
                let window = self.typing_window(into)?;
                let target = self.keys_to(window)?;
                self.type_text(target, &text);
            }
            Act::SetValue { element, text } => {
                let element = self.element(&element)?;
                if !element.set("AXValue", &ns::string(&text)) {
                    let message = "that element's value can't be set: click it and type instead";
                    return Err(Refusal::new(RefusalCode::Failed, message));
                }
            }
            Act::Key { combo, window } => return self.keys(&combo, window),
        }
        Ok(Answer::Done)
    }

    /// The window text typed `into` an element goes to, the element focused first: by
    /// asking, or by a click where it doesn't take the focus by asking.
    fn typing_window(&mut self, into: Option<String>) -> Result<Option<u32>, Refusal> {
        let Some(name) = into else {
            return Ok(None);
        };
        let element = self.element(&name)?;
        let focused = element.set("AXFocused", &ns::yes());
        if let (false, Some(frame)) = (focused, element.frame()) {
            let at = frame.center();
            let target = self.target_at(at)?;
            self.aim(target);
            self.pointer(at, true);
            self.poster.click(target, at, Button::Left, 1, 0);
        }
        Ok(element.window_id())
    }

    /// Starts typing `text` for `request`: the answer comes once it's all typed
    /// ([`Host::type_along`]), or a refusal when the person acts meanwhile.
    pub fn begin_typing(
        &mut self,
        request: RequestId,
        text: &str,
        into: Option<String>,
    ) -> Result<(), Refusal> {
        let window = self.typing_window(into)?;
        let target = self.keys_to(window)?;
        self.typings.push_back(Typing {
            request,
            target,
            rest: text.chars().collect(),
        });
        Ok(())
    }

    /// Types the next few characters of the text under way, and answers once it's typed.
    pub fn type_along(&mut self) {
        /// Characters a turn: about a fifth of a second of typing between two looks at
        /// what the app says.
        const A_TURN: usize = 12;
        let Some(mut typing) = self.typings.pop_front() else {
            return;
        };
        if let Err(gone) = self.listed(typing.target.window).map(|_| ()) {
            self.answer(typing.request, Err(gone));
            return;
        }
        let turn: String = (0..A_TURN).map_while(|_| typing.rest.pop_front()).collect();
        self.front.acted(typing.target.pid);
        self.type_text(typing.target, &turn);
        if typing.rest.is_empty() {
            self.answer(typing.request, Ok(Answer::Done));
        } else {
            self.typings.push_front(typing);
        }
    }

    /// The person acted: what was being typed stops where it is.
    pub fn stop_typing(&mut self) {
        let message = "the person took over the agent desktop while this was being typed: \
                       part of the text is in, look before going on";
        self.stop_typing_where(|_| true, &Refusal::new(RefusalCode::PersonActing, message));
    }

    /// Stops the texts being typed that `cut` picks, each where it is, and answers them
    /// with `refusal`.
    pub fn stop_typing_where(&mut self, cut: impl Fn(&Typing) -> bool, refusal: &Refusal) {
        let (stopped, kept): (VecDeque<Typing>, VecDeque<Typing>) =
            (std::mem::take(&mut self.typings).into_iter()).partition(cut);
        self.typings = kept;
        for typing in stopped {
            self.answer(typing.request, Err(refusal.clone()));
        }
    }

    /// Text as keys: its characters as they are, a line break as Return, a tab as Tab.
    fn type_text(&self, target: Target, text: &str) {
        let mut run = String::new();
        for character in text.chars() {
            let key = match character {
                '\n' => Some(RETURN),
                '\t' => Some(TAB),
                '\r' => continue,
                _ => None,
            };
            match key {
                Some(code) => {
                    self.poster.type_text(target, &run);
                    run.clear();
                    self.poster.press(target, code, 0);
                    ns::pause(std::time::Duration::from_millis(8));
                }
                None => run.push(character),
            }
        }
        self.poster.type_text(target, &run);
    }

    /// The person's own input from the tab, passed on to the window under it.
    pub fn person(&mut self, input: &Input) {
        match *input {
            Input::Move { at } => {
                let at = self.global(at);
                if let Ok(target) = self.target_at(at) {
                    self.poster.move_to(target, at);
                }
            }
            Input::Down {
                at,
                button,
                clicks,
                flags,
            } => {
                let at = self.global(at);
                let Ok(target) = self.target_at(at) else {
                    return;
                };
                self.focus = Some(target.window);
                self.lively = Some(std::time::Instant::now());
                if self.holds(target, at, (button, clicks, flags)) {
                    return;
                }
                self.aim(target);
                self.grab = Some(target);
                self.poster.down(target, at, button, clicks, flags);
            }
            Input::Up {
                at,
                button,
                clicks,
                flags,
            } => {
                self.lively = Some(std::time::Instant::now());
                let at = self.global(at);
                if !self.lets_go(at)
                    && let Some(target) = self.grab.take()
                {
                    self.poster.up(target, at, button, clicks, flags);
                }
            }
            Input::Drag { at, button } => {
                self.lively = Some(std::time::Instant::now());
                let at = self.global(at);
                if !self.carries(at)
                    && let Some(target) = self.grab
                {
                    self.poster.dragged(target, at, button);
                }
            }
            Input::Scroll {
                at,
                dx,
                dy,
                phase,
                momentum,
            } => {
                let at = self.global(at);
                let Ok(target) = self.target_at(at) else {
                    return;
                };
                let scroll = |dx, dy, phase: u8| {
                    self.poster.scroll_event(
                        target,
                        at,
                        dx,
                        dy,
                        i64::from(phase),
                        i64::from(momentum),
                    );
                };
                if phase == 0 && momentum == 0 {
                    // A wheel's click has no phases, and a window that isn't showing takes
                    // only scrolling that has them: a gesture of one step.
                    scroll(0.0, 0.0, 1);
                    scroll(dx, dy, 2);
                    scroll(0.0, 0.0, 4);
                } else {
                    scroll(dx, dy, phase);
                }
            }
            Input::Text { ref text } => {
                if let Ok(target) = self.keys_to(None) {
                    self.type_text(target, text);
                }
            }
            Input::Key { down, code, flags } => {
                if let Ok(target) = self.keys_to(None) {
                    let code = crate::layout::placed(code);
                    self.poster.key(target, code, down, flags);
                }
            }
        }
    }
}
