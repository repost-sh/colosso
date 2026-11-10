//! Moving a window by its bar (`docs/agent-desktop.md` 7). macOS moves a dragged window
//! itself, after the real pointer: sent to the app, an agent's drag of a title bar moves
//! nothing, and the person's in the tab moves the window by what their own pointer does on
//! their screen, off from where the tab shows it. So a drag that starts on a window's bar is
//! the host's to do: the window is put, through accessibility, where the pointer takes it.

use koyo_desk_wire::Point;
use koyo_desk_wire::link::Button;
use objc2_core_foundation::CGPoint;

use crate::ax::Element;
use crate::host::Host;
use crate::input::Target;
use crate::ns;

/// How far down a window its bar can reach: a title bar with a toolbar in it.
const BAR: f64 = 56.0;
/// How far into a window's tree a press is looked for.
const DEEPEST: usize = 10;
/// What a press on the bar lands on where it drags the window: the bar's own parts, and
/// nothing that takes the press for itself, as a button or a field does.
const GRIPS: [&str; 8] = [
    "AXWindow",
    "AXToolbar",
    "AXStaticText",
    "AXImage",
    "AXGroup",
    "AXSplitGroup",
    "AXLayoutArea",
    "AXUnknown",
];

/// A window on its way, held by the pointer.
pub struct Moving {
    target: Target,
    window: Element,
    /// Where the pointer went down; where the window was then is `target`'s frame.
    from: Point,
    moved: bool,
    /// The press as it came, sent on if it turns out a click.
    press: (Button, u8, u64),
}

impl Host {
    /// The window's own element, if a press at `at` in `target` takes hold of the window.
    fn grip(&mut self, target: Target, at: Point) -> Option<Element> {
        let down = at.y - target.frame.y;
        // A panel another process draws is that process's to drag.
        if target.through.is_some() || !(0.0..BAR).contains(&down) {
            return None;
        }
        let app = self.apps.iter_mut().find(|app| app.pid == target.pid)?;
        let window = app.window(target.window)?;
        let role = under(&window, at).text("AXRole")?;
        GRIPS.contains(&role.as_str()).then_some(window)
    }

    /// The person's button went down: whether it took hold of a window, to move it.
    pub fn holds(&mut self, target: Target, at: Point, press: (Button, u8, u64)) -> bool {
        if press.0 != Button::Left || press.2 != 0 {
            return false;
        }
        let Some(window) = self.grip(target, at) else {
            return false;
        };
        // As a press on a bar does: the window comes in front and takes the keys.
        window.set("AXMain", &ns::yes());
        window.act("AXRaise");
        self.moving = Some(Moving {
            target,
            window,
            from: at,
            moved: false,
            press,
        });
        true
    }

    /// The pointer went on with the button down: whether a window went with it.
    pub fn carries(&mut self, at: Point) -> bool {
        let Some(moving) = &mut self.moving else {
            return false;
        };
        moving.moved = true;
        moving.window.set_point(
            "AXPosition",
            CGPoint {
                x: moving.target.frame.x + at.x - moving.from.x,
                y: moving.target.frame.y + at.y - moving.from.y,
            },
        );
        true
    }

    /// The button came up: whether a window was held. One that never moved was clicked,
    /// and the click is sent on as it was meant.
    pub fn lets_go(&mut self, at: Point) -> bool {
        let Some(moving) = self.moving.take() else {
            return false;
        };
        if !moving.moved {
            let (button, clicks, flags) = moving.press;
            self.aim(moving.target);
            (self.poster).down(moving.target, moving.from, button, clicks, flags);
            self.poster.up(moving.target, at, button, clicks, flags);
        }
        true
    }

    /// An agent's drag from `from` to `to`: whether it was a window's bar, and the window
    /// was moved by as much.
    pub fn moves(&mut self, target: Target, from: Point, to: Point) -> bool {
        let Some(window) = self.grip(target, from) else {
            return false;
        };
        window.set("AXMain", &ns::yes());
        window.act("AXRaise");
        window.set_point(
            "AXPosition",
            CGPoint {
                x: target.frame.x + to.x - from.x,
                y: target.frame.y + to.y - from.y,
            },
        )
    }
}

/// The innermost element of `window` at a point. Found by going down the window's own tree:
/// an app doesn't answer what is under a point while its desktop isn't showing.
fn under(window: &Element, at: Point) -> Element {
    let mut found = window.clone();
    for _ in 0..DEEPEST {
        let inner = (found.children().into_iter())
            .filter_map(|child| Some((child.frame().filter(|frame| frame.contains(at))?, child)))
            .min_by(|(one, _), (other, _)| {
                (one.width * one.height).total_cmp(&(other.width * other.height))
            });
        match inner {
            Some((_, child)) => found = child,
            None => break,
        }
    }
    found
}
