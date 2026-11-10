//! Acting on the agent desktop with the pointer and keys: clicks, moves, drags, scrolls,
//! typing and keys. An act lands on an element of the last snapshot by its `ref`, or at a point
//! in desktop points; the rules here read which, before anything is asked of the desktop.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::MAX_TYPED;
use crate::ops::browser::MouseButton;
use crate::{ControlError, Hints, Operation, Scope};

/// What an act answers: it was done. What wasn't is an error.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DesktopDone {
    /// Always `true`.
    pub done: bool,
}

/// Where an act lands, once its input is read.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// An element of the last snapshot, by its ref.
    Element(String),
    /// A point, in desktop points.
    Point { x: f64, y: f64 },
}

/// `ref`, or `x` with `y`: exactly one.
///
/// # Errors
/// `invalid_input` for neither, both, or half a point.
pub fn target(
    element: Option<&str>,
    x: Option<f64>,
    y: Option<f64>,
) -> Result<Target, ControlError> {
    match (element, x, y) {
        (Some(element), None, None) => Ok(Target::Element(element.to_owned())),
        (None, Some(x), Some(y)) => Ok(Target::Point { x, y }),
        (None, None, None) => Err(ControlError::invalid_input(
            "Say where: `ref` from desktop_snapshot, or `x` and `y` in desktop points.",
        )),
        (Some(_), _, _) => Err(ControlError::invalid_input(
            "Give `ref` or `x` and `y`, not both.",
        )),
        (None, _, _) => Err(ControlError::invalid_input(
            "A point needs both `x` and `y`.",
        )),
    }
}

/// A modifier held through a click.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Modifier {
    Cmd,
    Shift,
    Alt,
    Ctrl,
}

/// What to click: an element by `ref`, or a point.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopClickInput {
    /// An element's ref from `desktop_snapshot`, as `e12`.
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub element: Option<String>,
    /// A point's distance from the desktop's left edge, in points, with `y`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    /// A point's distance from the desktop's top edge, in points, with `x`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    /// The window meant, from `desktop_windows`: raised first when another covers the point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<u32>,
    /// `left` (the default), `middle` or `right`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub button: Option<MouseButton>,
    /// 2 for a double click, 3 for a triple; 1 when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u8>,
    /// Modifiers held: `cmd`, `shift`, `alt`, `ctrl`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modifiers: Option<Vec<Modifier>>,
}

impl DesktopClickInput {
    /// Where the click lands.
    ///
    /// # Errors
    /// `invalid_input` unless it's `ref` or `x` with `y`.
    pub fn target(&self) -> Result<Target, ControlError> {
        target(self.element.as_deref(), self.x, self.y)
    }

    /// How many clicks.
    ///
    /// # Errors
    /// `invalid_input` for anything but 1 to 3.
    pub fn count(&self) -> Result<u8, ControlError> {
        match self.count.unwrap_or(1) {
            count @ 1..=3 => Ok(count),
            other => Err(ControlError::invalid_input(format!(
                "`count` is 1, 2 or 3, not {other}."
            ))),
        }
    }
}

/// `desktop_click`: click an element or a point.
#[derive(Debug)]
pub struct DesktopClick;

impl Operation for DesktopClick {
    const NAME: &'static str = "desktop_click";
    const TITLE: &'static str = "Click on the agent desktop";
    const SUBJECT: &'static [&'static str] = &["ref"];
    const SCOPE: Scope = Scope::Desktop;
    const DESCRIPTION: &'static str = held!(
        "Clicks an element of the agent desktop by `ref` from desktop_snapshot, or a point (`x`, `y`). A plain click on a ref presses it through accessibility, with no event at all."
    );
    const HINTS: Hints = Hints::CHANGE.beyond_koyo();
    type Input = DesktopClickInput;
    type Output = DesktopDone;

    fn summary(_: &DesktopDone) -> String {
        "Clicked on the agent desktop".to_owned()
    }
}

/// Where to move the pointer.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopMoveInput {
    /// An element's ref from `desktop_snapshot`, as `e12`.
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub element: Option<String>,
    /// A point's distance from the desktop's left edge, in points, with `y`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    /// A point's distance from the desktop's top edge, in points, with `x`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    /// The window meant, from `desktop_windows`: raised first when another covers the point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<u32>,
}

impl DesktopMoveInput {
    /// Where the pointer goes.
    ///
    /// # Errors
    /// `invalid_input` unless it's `ref` or `x` with `y`.
    pub fn target(&self) -> Result<Target, ControlError> {
        target(self.element.as_deref(), self.x, self.y)
    }
}

/// `desktop_move`: the pointer over an element or a point.
#[derive(Debug)]
pub struct DesktopMove;

impl Operation for DesktopMove {
    const NAME: &'static str = "desktop_move";
    const TITLE: &'static str = "Move the pointer on the agent desktop";
    const SUBJECT: &'static [&'static str] = &["ref"];
    const SCOPE: Scope = Scope::Desktop;
    const DESCRIPTION: &'static str = held!(
        "Moves the pointer over an element of the agent desktop (`ref`) or a point (`x`, `y`), as a hover."
    );
    const HINTS: Hints = Hints::IDEMPOTENT.beyond_koyo();
    type Input = DesktopMoveInput;
    type Output = DesktopDone;

    fn summary(_: &DesktopDone) -> String {
        "Moved the pointer on the agent desktop".to_owned()
    }
}

/// One end of a drag: an element by `ref`, or a point.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopSpot {
    /// An element's ref from `desktop_snapshot`, as `e12`.
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub element: Option<String>,
    /// A point's distance from the desktop's left edge, in points, with `y`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    /// A point's distance from the desktop's top edge, in points, with `x`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
}

impl DesktopSpot {
    /// Where this end is.
    ///
    /// # Errors
    /// `invalid_input` unless it's `ref` or `x` with `y`.
    pub fn target(&self) -> Result<Target, ControlError> {
        target(self.element.as_deref(), self.x, self.y)
    }
}

/// What to drag, and where to.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopDragInput {
    /// Where the drag starts: `ref`, or `x` and `y`.
    pub from: DesktopSpot,
    /// Where it ends: `ref`, or `x` and `y`.
    pub to: DesktopSpot,
    /// The window meant, from `desktop_windows`: raised first when another covers the start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<u32>,
}

/// `desktop_drag`: press, move and release.
#[derive(Debug)]
pub struct DesktopDrag;

impl Operation for DesktopDrag {
    const NAME: &'static str = "desktop_drag";
    const TITLE: &'static str = "Drag on the agent desktop";
    const SCOPE: Scope = Scope::Desktop;
    const DESCRIPTION: &'static str = held!(
        "Drags with the left button from one element or point (`from`) to another (`to`), each a `ref` or `x` and `y`."
    );
    const HINTS: Hints = Hints::CHANGE.beyond_koyo();
    type Input = DesktopDragInput;
    type Output = DesktopDone;

    fn summary(_: &DesktopDone) -> String {
        "Dragged on the agent desktop".to_owned()
    }
}

/// Where to scroll, and how far.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopScrollInput {
    /// An element's ref from `desktop_snapshot`, as `e12`.
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub element: Option<String>,
    /// A point's distance from the desktop's left edge, in points, with `y`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    /// A point's distance from the desktop's top edge, in points, with `x`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    /// The window meant, from `desktop_windows`: raised first when another covers the point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<u32>,
    /// Points to scroll; a positive `dx` brings what is further right into view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dx: Option<f64>,
    /// Points to scroll; a positive `dy` brings what is further down into view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dy: Option<f64>,
}

impl DesktopScrollInput {
    /// Where the scroll lands.
    ///
    /// # Errors
    /// `invalid_input` unless it's `ref` or `x` with `y`.
    pub fn target(&self) -> Result<Target, ControlError> {
        target(self.element.as_deref(), self.x, self.y)
    }

    /// How far, `(dx, dy)` in points.
    ///
    /// # Errors
    /// `invalid_input` when both are zero or left out.
    pub fn amount(&self) -> Result<(f64, f64), ControlError> {
        let (dx, dy) = (self.dx.unwrap_or(0.), self.dy.unwrap_or(0.));
        if dx == 0. && dy == 0. {
            return Err(ControlError::invalid_input(
                "Say how far to scroll with `dx` or `dy`, in points.",
            ));
        }
        Ok((dx, dy))
    }
}

/// `desktop_scroll`: scroll at an element or a point.
#[derive(Debug)]
pub struct DesktopScroll;

impl Operation for DesktopScroll {
    const NAME: &'static str = "desktop_scroll";
    const TITLE: &'static str = "Scroll on the agent desktop";
    const SUBJECT: &'static [&'static str] = &["ref"];
    const SCOPE: Scope = Scope::Desktop;
    const DESCRIPTION: &'static str = held!(
        "Scrolls by about `dx`/`dy` points (up to a quarter further) at an element (`ref`) or a point (`x`, `y`) of the agent desktop; a positive `dy` brings what is further down into view. Look again after it."
    );
    const HINTS: Hints = Hints::CHANGE.beyond_koyo();
    type Input = DesktopScrollInput;
    type Output = DesktopDone;

    fn summary(_: &DesktopDone) -> String {
        "Scrolled on the agent desktop".to_owned()
    }
}

/// What to type, and where.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopTypeInput {
    /// The text: `\n` presses Return, `\t` Tab.
    pub text: String,
    /// A field's ref from `desktop_snapshot`, focused first. Left out, the keys go to the
    /// window that has them.
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub element: Option<String>,
    /// With `ref`: set the field's whole value to `text` through accessibility, with no keys.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replace: Option<bool>,
}

/// How text goes in, once `desktop_type`'s input is read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Typing {
    /// As keys, into the element named, focused first, or the window that has the keys.
    Keys { text: String, into: Option<String> },
    /// As the element's whole value, through accessibility.
    SetValue { element: String, text: String },
}

impl DesktopTypeInput {
    /// How the text goes in.
    ///
    /// # Errors
    /// `invalid_input` for `replace` without `ref`, or more than [`MAX_TYPED`] characters to
    /// type as keys.
    pub fn typing(&self) -> Result<Typing, ControlError> {
        match (&self.element, self.replace.unwrap_or(false)) {
            (Some(element), true) => Ok(Typing::SetValue {
                element: element.clone(),
                text: self.text.clone(),
            }),
            (None, true) => Err(ControlError::invalid_input(
                "`replace` sets a field's value: name the field with `ref` from desktop_snapshot.",
            )),
            (into, false) => {
                let typed = self.text.chars().count();
                if typed > MAX_TYPED {
                    return Err(ControlError::invalid_input(format!(
                        "{typed} characters is more than the {MAX_TYPED} typed as keys at once: set a field's value with `ref` and `replace: true`, or type it in parts."
                    )));
                }
                Ok(Typing::Keys {
                    text: self.text.clone(),
                    into: into.clone(),
                })
            }
        }
    }
}

/// `desktop_type`: text as keys, or a field's value.
#[derive(Debug)]
pub struct DesktopType;

impl Operation for DesktopType {
    const NAME: &'static str = "desktop_type";
    const TITLE: &'static str = "Type on the agent desktop";
    const SUBJECT: &'static [&'static str] = &["text"];
    const SCOPE: Scope = Scope::Desktop;
    const DESCRIPTION: &'static str = held!(
        "Types text as keys into the window that has the keys (the one last clicked or focused, or the document, panel or sheet its app opened since), or into a field (`ref`), focused first; with `ref` and `replace: true`, sets the field's whole value instead. At most 4000 characters as keys."
    );
    const HINTS: Hints = Hints::CHANGE.beyond_koyo();
    type Input = DesktopTypeInput;
    type Output = DesktopDone;

    fn summary(_: &DesktopDone) -> String {
        "Typed on the agent desktop".to_owned()
    }
}

/// The keys to press.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopKeyInput {
    /// A key with modifiers joined by `+`: `return`, `escape`, `cmd+s`, `shift+cmd+[`.
    pub keys: String,
    /// The window to send them to, from `desktop_windows`: it has the keys from then on. Left
    /// out, the window that has them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<u32>,
}

/// What pressing keys did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DesktopKeyOutput {
    /// Always `true`.
    pub done: bool,
    /// What a copy or cut (`cmd+c`, `cmd+x`) put on the clipboard. The person's clipboard is
    /// put back right after, so this is the only place the copied text is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copied: Option<String>,
}

/// `desktop_key`: a key or a shortcut.
#[derive(Debug)]
pub struct DesktopKey;

impl Operation for DesktopKey {
    const NAME: &'static str = "desktop_key";
    const TITLE: &'static str = "Press keys on the agent desktop";
    const SUBJECT: &'static [&'static str] = &["keys"];
    const SCOPE: Scope = Scope::Desktop;
    const DESCRIPTION: &'static str = held!(
        "Presses a key or a shortcut, such as `cmd+s`, in the window that has the keys (the one last clicked or focused, or the document, panel or sheet its app opened since), or in `window`, which has them from then on. A copy or a cut answers what it copied as `copied`, and leaves the person's clipboard theirs."
    );
    const HINTS: Hints = Hints::CHANGE.beyond_koyo();
    type Input = DesktopKeyInput;
    type Output = DesktopKeyOutput;

    fn summary(output: &DesktopKeyOutput) -> String {
        match &output.copied {
            Some(text) => format!(
                "Pressed the keys on the agent desktop, copying {} characters",
                text.chars().count()
            ),
            None => "Pressed the keys on the agent desktop".to_owned(),
        }
    }
}
