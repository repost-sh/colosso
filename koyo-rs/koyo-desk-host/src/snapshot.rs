//! Accessibility snapshots (`docs/agent-desktop.md` 7): a window's tree as indented text, a
//! role and a name a line, with `[ref=eN]` on what can be acted on, in the shape a browser
//! page's snapshot has. Each element costs one call to its app, and a snapshot stops at a
//! budget, so a huge or hung app costs a bounded time.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::time::{Duration, Instant};

use koyo_desk_wire::Rect;

use crate::ax::{self, Element};
use crate::ns::{self, Obj};

/// Elements looked at, at most, and how long a snapshot may take.
const MOST: usize = 3000;
const WITHIN: Duration = Duration::from_secs(4);
const DEEPEST: usize = 80;
/// How much of a name or a value a line carries.
const NAME: usize = 120;
const VALUE: usize = 200;

/// What is read of each element, in one call.
const ATTRIBUTES: [&str; 11] = [
    "AXRole",
    "AXSubrole",
    "AXTitle",
    "AXDescription",
    "AXValue",
    "AXEnabled",
    "AXFocused",
    "AXSelected",
    "AXPosition",
    "AXSize",
    "AXChildren",
];

/// A snapshot being written.
pub struct Snapshot {
    pub tree: String,
    pub refs: HashMap<String, Element>,
    pub truncated: bool,
    interactive: bool,
    /// The number of this snapshot's first ref. Refs count on from snapshot to snapshot, so
    /// one from an older snapshot is never taken for another element: it's found gone.
    first: u64,
    /// The desktop among the displays: points are written counted from its top left, and
    /// only for what lies on it (a menu bar's items don't until their menu is open).
    desktop: Rect,
    looked: usize,
    started: Instant,
}

/// The line's role for an accessibility role and subrole, and whether such an element can be
/// acted on. `None` for what is only structure: its children are listed in its place.
fn role(role: &str, subrole: &str) -> Option<(&'static str, bool)> {
    Some(match (role, subrole) {
        (_, "AXSwitch" | "AXToggle") => ("switch", true),
        (_, "AXSearchField") => ("searchbox", true),
        (_, "AXTabButton") => ("tab", true),
        ("AXButton" | "AXMenuButton" | "AXDisclosureTriangle", _) => ("button", true),
        ("AXTextField" | "AXTextArea", _) => ("textbox", true),
        ("AXCheckBox", _) => ("checkbox", true),
        ("AXRadioButton", _) => ("radio", true),
        ("AXPopUpButton" | "AXComboBox", _) => ("combobox", true),
        ("AXLink", _) => ("link", true),
        ("AXSlider", _) => ("slider", true),
        ("AXIncrementor", _) => ("spinbutton", true),
        ("AXMenuItem" | "AXMenuBarItem", _) => ("menuitem", true),
        ("AXRow", _) => ("row", true),
        ("AXCell", _) => ("cell", true),
        ("AXColorWell", _) => ("colorwell", true),
        ("AXStaticText", _) => ("text", false),
        ("AXImage", _) => ("img", false),
        ("AXHeading", _) => ("heading", false),
        ("AXWindow", _) => ("window", false),
        ("AXSheet", _) => ("dialog", false),
        ("AXMenu", _) => ("menu", false),
        ("AXMenuBar", _) => ("menubar", false),
        ("AXToolbar", _) => ("toolbar", false),
        ("AXTabGroup", _) => ("tablist", false),
        ("AXRadioGroup", _) => ("radiogroup", false),
        ("AXTable", _) => ("table", false),
        ("AXOutline", _) => ("tree", false),
        ("AXList", _) => ("list", false),
        ("AXWebArea", _) => ("document", false),
        ("AXGroup" | "AXScrollArea" | "AXSplitGroup", _) => ("group", false),
        _ => return None,
    })
}

/// A line's text for `text`: one line, cut to `most` characters, quoted.
fn quoted(text: &str, most: usize) -> String {
    let mut clean = String::with_capacity(text.len().min(most * 4));
    let mut space = false;
    for character in text.trim().chars().take(most * 2) {
        if character.is_whitespace() {
            space = true;
        } else {
            if space && !clean.is_empty() {
                clean.push(' ');
            }
            space = false;
            match character {
                '"' => clean.push_str("\\\""),
                '\\' => clean.push_str("\\\\"),
                other => clean.push(other),
            }
        }
    }
    let cut: String = clean.chars().take(most).collect();
    let more = if cut.len() < clean.len() { "…" } else { "" };
    format!("\"{cut}{more}\"")
}

fn text_of(value: Option<&Obj>) -> Option<String> {
    ns::text(value?).filter(|text| !text.trim().is_empty())
}

fn truth(value: Option<&Obj>) -> Option<bool> {
    ns::integer(value?).map(|value| value != 0)
}

impl Snapshot {
    pub fn new(interactive: bool, desktop: Rect, first: u64) -> Snapshot {
        Snapshot {
            first,
            desktop,
            tree: String::new(),
            refs: HashMap::new(),
            truncated: false,
            interactive,
            looked: 0,
            started: Instant::now(),
        }
    }

    /// How many elements got a ref.
    pub fn elements(&self) -> u32 {
        u32::try_from(self.refs.len()).unwrap_or(u32::MAX)
    }

    /// A heading line of the caller's, as a window's or an app's.
    pub fn heading(&mut self, line: &str) {
        let _ = writeln!(self.tree, "{line}");
    }

    /// Writes `element` and what is under it, `depth` deep in the text.
    pub fn walk(&mut self, element: &Element, depth: usize, level: usize) {
        if self.looked >= MOST || self.started.elapsed() > WITHIN {
            self.truncated = true;
            return;
        }
        self.looked += 1;
        let read = element.many(&ATTRIBUTES);
        let at = |index: usize| read.get(index).and_then(Option::as_ref);
        let kind = text_of(at(0)).unwrap_or_default();
        let subrole = text_of(at(1)).unwrap_or_default();
        if kind == "AXScrollBar" || kind == "AXGrowArea" {
            return;
        }
        let title = text_of(at(2)).or_else(|| text_of(at(3)));
        let value = at(4);
        let named = role(&kind, &subrole);
        let frame = at(8)
            .and_then(ax::as_point)
            .zip(at(9).and_then(ax::as_size));
        let mut shown = false;
        if let Some((name, acts)) = named {
            // A text's words are its value; where it has a name too (a display's "Result"),
            // the words follow it as a value.
            let labelled = matches!(name, "text" | "heading") && title.is_some();
            let text = match name {
                "text" | "heading" => title.or_else(|| text_of(value)),
                _ => title,
            };
            // Structure with no name says nothing; text with no text says nothing.
            let says = acts || text.is_some() && name != "group" || name == "window";
            if says && (!self.interactive || acts) {
                let mut line = format!("{}- {name}", "  ".repeat(depth));
                if let Some(text) = &text {
                    let _ = write!(line, " {}", quoted(text, NAME));
                }
                let valued = labelled
                    || matches!(
                        name,
                        "textbox" | "searchbox" | "combobox" | "slider" | "spinbutton"
                    );
                if valued && subrole != "AXSecureTextField" {
                    let said = text_of(value).or_else(|| {
                        value
                            .and_then(|value| ns::float(value))
                            .map(|n| n.to_string())
                    });
                    if let Some(said) = said {
                        let _ = write!(line, " value={}", quoted(&said, VALUE));
                    }
                }
                if matches!(name, "checkbox" | "radio" | "switch" | "tab")
                    && truth(value) == Some(true)
                {
                    line.push_str(" [checked]");
                }
                if truth(at(7)) == Some(true) {
                    line.push_str(" [selected]");
                }
                if truth(at(6)) == Some(true) {
                    line.push_str(" [focused]");
                }
                if truth(at(5)) == Some(false) {
                    line.push_str(" [disabled]");
                }
                if acts {
                    let name = format!("e{}", self.first + self.refs.len() as u64);
                    let _ = write!(line, " [ref={name}]");
                    let middle = frame.map(|(at, size)| {
                        Rect {
                            x: at.x,
                            y: at.y,
                            width: size.width,
                            height: size.height,
                        }
                        .center()
                    });
                    if let Some(middle) = middle.filter(|middle| self.desktop.contains(*middle)) {
                        let (x, y) = (middle.x - self.desktop.x, middle.y - self.desktop.y);
                        let _ = write!(line, " @{x:.0},{y:.0}");
                    }
                    self.refs.insert(name, element.clone());
                }
                let _ = writeln!(self.tree, "{line}");
                shown = true;
            }
        }
        if level >= DEEPEST {
            self.truncated = true;
            return;
        }
        let children = at(10).map(|list| ns::items(list)).unwrap_or_default();
        for child in children {
            self.walk(&Element::from(child), depth + usize::from(shown), level + 1);
            if self.truncated {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_quotes_one_line_of_text() {
        assert_eq!(quoted("  Save   as…\n", 20), "\"Save as…\"");
        assert_eq!(quoted("say \"hi\"", 20), "\"say \\\"hi\\\"\"");
        assert_eq!(quoted("abcdefgh", 4), "\"abcd…\"");
    }

    #[test]
    fn roles_read_as_a_page_s_do() {
        assert_eq!(role("AXButton", ""), Some(("button", true)));
        assert_eq!(role("AXCheckBox", "AXSwitch"), Some(("switch", true)));
        assert_eq!(
            role("AXTextField", "AXSearchField"),
            Some(("searchbox", true))
        );
        assert_eq!(role("AXStaticText", ""), Some(("text", false)));
        assert_eq!(role("AXLayoutArea", ""), None);
    }
}
