//! Acting on a browser page: navigating, evaluating, clicking, typing, keys, scrolling, and
//! the page's lease. Each act claims a free page first; a page the person controls refuses
//! with `person_has_control`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::PageOutput;
use crate::ops::shared::PaneId;
use crate::{Hints, Operation, Scope};

/// The rule every acting tool's description ends with.
macro_rules! leased {
    ($text:literal) => {
        concat!(
            $text,
            " Claims a free page first. person_has_control means the person took the page: don't retry or wait, open another page."
        )
    };
}

/// A page to act on.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageInput {
    /// The browser pane. Left out, the page you acted on last, else the shown tab's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
}

/// When a navigation answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WaitUntil {
    /// Once the page has loaded.
    Load,
    /// As soon as the navigation starts.
    None,
}

/// Where to navigate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserNavigateInput {
    /// The browser pane. Left out, the page you acted on last, else the shown tab's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// The address.
    pub url: String,
    /// `load` (the default) answers once the page loaded; `none` right away.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<WaitUntil>,
    /// How long to wait for the load, in milliseconds: 30000 when left out, at most 60000.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

/// `browser_navigate`: a page to an address.
#[derive(Debug)]
pub struct BrowserNavigate;

impl Operation for BrowserNavigate {
    const NAME: &'static str = "browser_navigate";
    const TITLE: &'static str = "Navigate a page";
    const SUBJECT: &'static [&'static str] = &["url"];
    const SCOPE: Scope = Scope::Browser;
    const DESCRIPTION: &'static str =
        leased!("Loads an address in a browser page and, unless `wait` is `none`, waits for it.");
    const HINTS: Hints = Hints::CHANGE;
    type Input = BrowserNavigateInput;
    type Output = PageOutput;

    fn summary(output: &PageOutput) -> String {
        format!("Navigated to {}", output.summary())
    }
}

/// A move through a page's history.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GoAction {
    Back,
    Forward,
    Reload,
}

/// Where to go.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserGoInput {
    /// The browser pane. Left out, the page you acted on last, else the shown tab's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// `back`, `forward` or `reload`.
    pub action: GoAction,
}

/// `browser_go`: back, forward, reload.
#[derive(Debug)]
pub struct BrowserGo;

impl Operation for BrowserGo {
    const NAME: &'static str = "browser_go";
    const TITLE: &'static str = "Go back, forward or reload";
    const SUBJECT: &'static [&'static str] = &["action"];
    const SCOPE: Scope = Scope::Browser;
    const DESCRIPTION: &'static str =
        leased!("Goes back or forward in a browser page's history, or reloads it.");
    const HINTS: Hints = Hints::CHANGE;
    type Input = BrowserGoInput;
    type Output = PageOutput;

    fn summary(output: &PageOutput) -> String {
        format!("Now at {}", output.summary())
    }
}

/// The script to run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserEvalInput {
    /// The browser pane. Left out, the page you acted on last, else the shown tab's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// A JavaScript expression, evaluated in the page.
    pub expression: String,
    /// Wait for the promise the expression gives, and answer what it resolves to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub await_promise: Option<bool>,
}

/// What a script gave.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BrowserEvalOutput {
    /// The expression's value as JSON; null for what JSON can't hold.
    pub value: Value,
}

/// `browser_eval`: JavaScript in a page.
#[derive(Debug)]
pub struct BrowserEval;

impl Operation for BrowserEval {
    const NAME: &'static str = "browser_eval";
    const TITLE: &'static str = "Run JavaScript in a page";
    const SUBJECT: &'static [&'static str] = &["expression"];
    const SCOPE: Scope = Scope::Browser;
    const DESCRIPTION: &'static str = leased!(
        "Evaluates a JavaScript expression in a browser page and answers its value as JSON. The cheapest way to read a few values off a page."
    );
    const HINTS: Hints = Hints::CHANGE;
    type Input = BrowserEvalInput;
    type Output = BrowserEvalOutput;

    fn summary(output: &BrowserEvalOutput) -> String {
        let text = output.value.to_string();
        if text.chars().count() > 80 {
            let cut: String = text.chars().take(79).collect();
            format!("Value: {cut}…")
        } else {
            format!("Value: {text}")
        }
    }
}

/// A mouse button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

/// What to click: an element by `ref` or `selector`, or a point.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserClickInput {
    /// The browser pane. Left out, the page you acted on last, else the shown tab's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// An element's ref from `browser_snapshot`, as `e12`.
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub element: Option<String>,
    /// A CSS selector for the element, when there's no ref.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    /// A point's distance from the page's left edge, in CSS pixels, with `y`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    /// A point's distance from the page's top edge, in CSS pixels, with `x`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    /// `left` (the default), `middle` or `right`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub button: Option<MouseButton>,
    /// 2 for a double click; 1 when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub click_count: Option<u32>,
}

/// `browser_click`: click an element or a point.
#[derive(Debug)]
pub struct BrowserClick;

impl Operation for BrowserClick {
    const NAME: &'static str = "browser_click";
    const TITLE: &'static str = "Click in a page";
    const SUBJECT: &'static [&'static str] = &["ref", "selector"];
    const SCOPE: Scope = Scope::Browser;
    const DESCRIPTION: &'static str = leased!(
        "Clicks an element of a browser page, by `ref` from browser_snapshot or by `selector`, or a point (`x`, `y`)."
    );
    const HINTS: Hints = Hints::CHANGE;
    type Input = BrowserClickInput;
    type Output = PageOutput;

    fn summary(output: &PageOutput) -> String {
        format!("Clicked; now at {}", output.summary())
    }
}

/// What to type, and where.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserTypeInput {
    /// The browser pane. Left out, the page you acted on last, else the shown tab's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// The field's ref from `browser_snapshot`; the focused element when neither this nor
    /// `selector` is given.
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub element: Option<String>,
    /// A CSS selector for the field, when there's no ref.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    /// The text to type.
    pub text: String,
    /// Empty the field first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clear: Option<bool>,
    /// Press Enter after typing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submit: Option<bool>,
}

/// `browser_type`: text into a field.
#[derive(Debug)]
pub struct BrowserType;

impl Operation for BrowserType {
    const NAME: &'static str = "browser_type";
    const TITLE: &'static str = "Type in a page";
    const SUBJECT: &'static [&'static str] = &["text"];
    const SCOPE: Scope = Scope::Browser;
    const DESCRIPTION: &'static str = leased!(
        "Types text into a field of a browser page (by `ref` or `selector`, else the focused one), optionally clearing it first and pressing Enter after."
    );
    const HINTS: Hints = Hints::CHANGE;
    type Input = BrowserTypeInput;
    type Output = PageOutput;

    fn summary(output: &PageOutput) -> String {
        format!("Typed into {}", output.summary())
    }
}

/// The key to press.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserPressKeyInput {
    /// The browser pane. Left out, the page you acted on last, else the shown tab's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// A key, with modifiers joined by `+`: `Enter`, `Tab`, `Escape`, `ArrowDown`, `Meta+A`.
    pub key: String,
}

/// `browser_press_key`: a key in a page.
#[derive(Debug)]
pub struct BrowserPressKey;

impl Operation for BrowserPressKey {
    const NAME: &'static str = "browser_press_key";
    const TITLE: &'static str = "Press a key in a page";
    const SUBJECT: &'static [&'static str] = &["key"];
    const SCOPE: Scope = Scope::Browser;
    const DESCRIPTION: &'static str =
        leased!("Presses a key, with modifiers, in a browser page's focused element.");
    const HINTS: Hints = Hints::CHANGE;
    type Input = BrowserPressKeyInput;
    type Output = PageOutput;

    fn summary(output: &PageOutput) -> String {
        format!("Pressed the key in {}", output.summary())
    }
}

/// How far to scroll, or what into view.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserScrollInput {
    /// The browser pane. Left out, the page you acted on last, else the shown tab's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// CSS pixels to scroll right; negative scrolls left.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dx: Option<f64>,
    /// CSS pixels to scroll down; negative scrolls up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dy: Option<f64>,
    /// An element's ref from `browser_snapshot` to scroll into view.
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub element: Option<String>,
    /// A CSS selector for the element to scroll into view, when there's no ref.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
}

/// `browser_scroll`: by an amount or to an element.
#[derive(Debug)]
pub struct BrowserScroll;

impl Operation for BrowserScroll {
    const NAME: &'static str = "browser_scroll";
    const TITLE: &'static str = "Scroll a page";
    const SUBJECT: &'static [&'static str] = &["ref", "selector"];
    const SCOPE: Scope = Scope::Browser;
    const DESCRIPTION: &'static str = leased!(
        "Scrolls a browser page by `dx`/`dy` pixels, or brings an element (`ref` or `selector`) into view."
    );
    const HINTS: Hints = Hints::CHANGE;
    type Input = BrowserScrollInput;
    type Output = PageOutput;

    fn summary(output: &PageOutput) -> String {
        format!("Scrolled {}", output.summary())
    }
}

/// The page to claim.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserClaimInput {
    /// The browser pane. Left out, the page you acted on last, else the shown tab's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// Take the page from another agent. Never takes it from the person.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub takeover: Option<bool>,
    /// Why, shown to whoever loses the page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Who holds a page's lease after a claim or a release.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Lease {
    /// The browser pane.
    pub pane: PaneId,
    /// Whether you hold it now.
    pub held: bool,
}

/// `browser_claim`: take a page's lease.
#[derive(Debug)]
pub struct BrowserClaim;

impl Operation for BrowserClaim {
    const NAME: &'static str = "browser_claim";
    const TITLE: &'static str = "Take control of a page";
    const SCOPE: Scope = Scope::Browser;
    const DESCRIPTION: &'static str = "Takes a browser page's lease so only you act on it. `takeover` displaces another agent, never the person: person_has_control means leave the page alone and open another.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = BrowserClaimInput;
    type Output = Lease;

    fn summary(output: &Lease) -> String {
        lease_summary(output)
    }
}

/// `browser_release`: give a page's lease up.
#[derive(Debug)]
pub struct BrowserRelease;

impl Operation for BrowserRelease {
    const NAME: &'static str = "browser_release";
    const TITLE: &'static str = "Give up control of a page";
    const SCOPE: Scope = Scope::Browser;
    const DESCRIPTION: &'static str =
        "Gives a browser page's lease up, so the page is free for others.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = PageInput;
    type Output = Lease;

    fn summary(output: &Lease) -> String {
        lease_summary(output)
    }
}

fn lease_summary(output: &Lease) -> String {
    if output.held {
        format!("You control pane {}", output.pane)
    } else {
        format!("Pane {} is free of you", output.pane)
    }
}
