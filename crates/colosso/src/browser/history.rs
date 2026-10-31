//! The page's history behind the back and forward buttons: held a moment, or pressed with
//! the right button, one lists the pages that way in a menu, nearest first, and a pick goes
//! straight to that page. A plain click still goes one step.

use std::time::Duration;

use colosso_browser_ipc::PaneId;
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{
    Action, Context, Div, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent,
    ParentElement as _, Pixels, Point, Window, div, point, px,
};
use serde::Deserialize;

use super::control::Mode;
use super::view::BrowserView;

/// How long a press on back or forward is held before its list shows instead.
const HOLD: Duration = Duration::from_millis(400);
/// The most pages a list shows.
const LISTED: usize = 20;
/// The most characters of a page's title a row shows.
const TITLE_CHARS: usize = 60;

/// A page picked from a history list of the page `pane`, by its entry in that history.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = browser, no_json)]
pub struct HistoryPick {
    pub pane: PaneId,
    pub entry: i64,
}

/// A press on back or forward, until it's told apart as a click or a hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Hold {
    /// Which press this is, so a timer of an earlier one does nothing.
    serial: u64,
    /// Whether its list is up, which the click that ends the press then leaves alone.
    listed: bool,
}

/// The page's history, as `Page.getNavigationHistory` answers.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct History {
    current_index: usize,
    entries: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
struct Entry {
    id: i64,
    url: String,
    #[serde(default)]
    title: String,
}

impl History {
    /// The entries one way from the page shown, nearest first.
    fn side(&self, back: bool) -> Vec<&Entry> {
        let current = self.current_index.min(self.entries.len());
        let listed: Vec<&Entry> = if back {
            self.entries[..current].iter().rev().collect()
        } else {
            self.entries.iter().skip(current + 1).collect()
        };
        listed.into_iter().take(LISTED).collect()
    }
}

/// How a row names a page: its title, else its address, cut short.
fn label(entry: &Entry) -> String {
    let name = if entry.title.trim().is_empty() {
        &entry.url
    } else {
        entry.title.trim()
    };
    if name.chars().count() > TITLE_CHARS {
        format!(
            "{}…",
            name.chars().take(TITLE_CHARS - 1).collect::<String>()
        )
    } else {
        name.to_owned()
    }
}

/// Back or forward in the toolbar, `back` saying which, while it can go that way: a click
/// goes one step, a hold or a right press lists the pages that way.
pub(super) fn button(
    button: impl IntoElement,
    back: bool,
    enabled: bool,
    cx: &mut Context<BrowserView>,
) -> Div {
    // Heard on the way down, before the button's own click at the release.
    div()
        .child(button)
        .capture_any_mouse_down(
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                if !enabled {
                    return;
                }
                match event.button {
                    MouseButton::Left => this.hold_history(back, event.position, window, cx),
                    MouseButton::Right => this.list_history(back, event.position, window, cx),
                    _ => {}
                }
            }),
        )
        .capture_any_mouse_up(cx.listener(|this, _, _, _| this.let_go_history()))
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(|this, _, _, _| this.history_hold = None),
        )
}

impl BrowserView {
    /// A press on back or forward: held long enough, it lists the pages that way.
    fn hold_history(
        &mut self,
        back: bool,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let serial = self.history_hold.map_or(0, |hold| hold.serial) + 1;
        self.history_hold = Some(Hold {
            serial,
            listed: false,
        });
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(HOLD).await;
            let _ = this.update_in(cx, |this, window, cx| {
                let held = this.history_hold;
                if held.is_some_and(|hold| hold.serial == serial && !hold.listed) {
                    this.list_history(back, at, window, cx);
                }
            });
        })
        .detach();
    }

    /// The press on back or forward ends over the button: unless its list is up, the click
    /// that follows goes a step.
    fn let_go_history(&mut self) {
        if self.history_hold.is_some_and(|hold| !hold.listed) {
            self.history_hold = None;
        }
    }

    /// Whether the click ending a press on back or forward is the end of a hold that listed
    /// the pages, and so goes nowhere.
    pub(super) fn ends_history_hold(&mut self) -> bool {
        self.history_hold.take().is_some_and(|hold| hold.listed)
    }

    /// Lists the pages back or forward of the one shown under the press at `at`.
    fn list_history(
        &mut self,
        back: bool,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(hold) = &mut self.history_hold {
            hold.listed = true;
        }
        let pane = self.pane;
        let answer = self.browsers.update(cx, |browsers, _| {
            browsers.call(pane, "Page.getNavigationHistory", serde_json::json!({}))
        });
        // Just under the button, as a browser's list drops from it.
        let at = point(at.x - px(8.), at.y + px(14.));
        cx.spawn_in(window, async move |_, cx| {
            let Ok(Ok(history)) = answer.await else {
                return;
            };
            let Ok(history) = serde_json::from_value::<History>(history) else {
                return;
            };
            let menu = (history.side(back).into_iter()).fold(NativeMenu::new(), |menu, entry| {
                let pick = HistoryPick {
                    pane,
                    entry: entry.id,
                };
                menu.menu(label(entry), Box::new(pick))
            });
            // The visual check runs unattended, where the menu's tracking would wait.
            if menu.is_empty() || cfg!(feature = "visual-check") {
                return;
            }
            let _ = cx.update(|window, cx| menu.show(at, window, cx));
        })
        .detach();
    }

    /// Goes straight to `entry` of the page's history, picked from a list.
    pub(super) fn go_to_entry(&mut self, entry: i64, cx: &mut Context<Self>) {
        self.person_acts(Mode::Implicit, cx);
        let pane = self.pane;
        // A page that's gone has nothing to answer; its own events tell the rest.
        drop(self.browsers.update(cx, |browsers, _| {
            browsers.call(
                pane,
                "Page.navigateToHistoryEntry",
                serde_json::json!({ "entryId": entry }),
            )
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history(current: usize, titles: &[&str]) -> History {
        let entries = (titles.iter().enumerate())
            .map(|(id, title)| Entry {
                id: id as i64 + 10,
                url: format!("https://example.com/{id}"),
                title: (*title).to_owned(),
            })
            .collect();
        History {
            current_index: current,
            entries,
        }
    }

    #[test]
    fn each_side_lists_its_pages_nearest_first() {
        let shown = history(2, &["a", "b", "c", "d", "e"]);
        let ids = |back| shown.side(back).iter().map(|e| e.id).collect::<Vec<_>>();
        assert_eq!(ids(true), [11, 10]);
        assert_eq!(ids(false), [13, 14]);
        assert!(history(0, &["a"]).side(true).is_empty());
        assert!(history(0, &["a"]).side(false).is_empty());
    }

    #[test]
    fn a_long_history_lists_its_nearest_pages() {
        let titles = vec!["page"; LISTED + 5];
        assert_eq!(history(LISTED + 4, &titles).side(true).len(), LISTED);
    }

    #[test]
    fn a_row_names_its_page_by_title_else_address() {
        let shown = history(0, &["  Docs  ", ""]);
        assert_eq!(label(&shown.entries[0]), "Docs");
        assert_eq!(label(&shown.entries[1]), "https://example.com/1");
        let long = history(0, &[&"x".repeat(80)]);
        let cut = label(&long.entries[0]);
        assert_eq!(cut.chars().count(), TITLE_CHARS);
        assert!(cut.ends_with('…'));
    }
}
