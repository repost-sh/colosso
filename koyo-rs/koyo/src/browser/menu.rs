//! The page's context menu: the engine's rows, shown as the app's native menu, led by the
//! app's own rows for links, images and selected text. Picks come back as actions on the
//! window, which the shell routes to the page.

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{Action, App, ClipboardItem, Context, Div, InteractiveElement as _, Window};
use koyo_browser_ipc::{self as ipc, AppRequest, MenuEntry, MenuTarget, PaneId};

use super::control::{Leased as _, Mode};
use super::host::Browsers;
use super::search::SearchEngine;
use super::view::{BrowserEvent, BrowserView};
use crate::native::menus::{self, Refusal};

/// What a picked row does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pick {
    /// The engine carries out its command.
    Engine(i32),
    /// Open an address in a new tab beside this one.
    OpenTab(String),
    /// Put text on the clipboard.
    Copy(String),
    /// Show the element under the pointer in the page's `DevTools`.
    Inspect,
    /// Show the page's `DevTools`, or hide them.
    DevTools,
    /// Turn responsive mode on or off.
    Responsive,
    /// Save the page as a PDF where the person picks.
    SavePdf,
}

/// What a page's own rows at the menu's end say: whether it's in responsive mode, and
/// whether its `DevTools` are up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct PageRows {
    responsive: bool,
    inspecting: bool,
}

/// A row picked from the context menu `menu` of the page `pane`.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = browser, no_json)]
pub struct MenuPick {
    pub pane: PaneId,
    pub menu: u64,
    pub pick: Pick,
}

/// A context menu the host sent, waiting for a render to show it.
pub(super) struct MenuRequest {
    id: u64,
    /// Where it opens, in the page's own coordinates.
    page_at: ipc::Point,
    entries: Vec<MenuEntry>,
    target: MenuTarget,
}

/// A row of the menu as the app lays it out, before it's a native menu.
#[derive(Debug, PartialEq, Eq)]
enum Row {
    Separator,
    Item {
        label: String,
        disabled: bool,
        checked: Option<bool>,
        pick: Pick,
    },
    Submenu {
        label: String,
        rows: Vec<Row>,
    },
}

/// Rows the app adds for what was under the pointer: selected text, a search with `engine`.
fn own_rows(target: &MenuTarget, engine: SearchEngine) -> Vec<Row> {
    let item = |label: String, pick: Pick| Row::Item {
        label,
        disabled: false,
        checked: None,
        pick,
    };
    let mut rows = Vec::new();
    for (what, url) in [("Link", &target.link), ("Image", &target.media)] {
        if is_address(url) {
            rows.push(item(
                format!("Open {what} in New Tab"),
                Pick::OpenTab(url.clone()),
            ));
            rows.push(item(
                format!("Copy {what} Address"),
                Pick::Copy(url.clone()),
            ));
            rows.push(Row::Separator);
        }
    }
    let query = search_text(&target.selection);
    if !target.editable && !query.is_empty() {
        let search = Pick::OpenTab(engine.results(&target.selection));
        rows.push(item(format!("Search {} for “{query}”", engine.label()), search));
        rows.push(Row::Separator);
    }
    rows
}

fn command(entry: &MenuEntry) -> Option<Row> {
    let MenuEntry::Command {
        command,
        label,
        enabled,
        checked,
    } = entry
    else {
        return None;
    };
    Some(Row::Item {
        label: label.clone(),
        disabled: !enabled,
        checked: *checked,
        pick: Pick::Engine(*command),
    })
}

/// Every row of the menu, with no leading, trailing or doubled rules. A page (`Some`)
/// ends with Save as PDF, beside Chrome's own Print further up (`docs/browser.md` 16), then
/// Responsive mode, Inspect and Show or Hide `DevTools`, as Chrome's menus end with Inspect:
/// the page's toolbar had the switches for them, and with the address bar in the sidebar it
/// has no toolbar (`docs/layout.md`). `DevTools` (`None`) have none of them.
fn rows(
    entries: &[MenuEntry],
    target: &MenuTarget,
    page: Option<PageRows>,
    engine: SearchEngine,
) -> Vec<Row> {
    let mut rows = own_rows(target, engine);
    rows.extend(entries.iter().map(|entry| match entry {
        MenuEntry::Separator => Row::Separator,
        MenuEntry::Command { .. } => command(entry).unwrap_or(Row::Separator),
        MenuEntry::Submenu { label, entries } => Row::Submenu {
            label: label.clone(),
            rows: entries.iter().filter_map(command).collect(),
        },
    }));
    if let Some(page) = page {
        rows.push(Row::Separator);
        rows.push(Row::Item {
            label: "Save as PDF…".into(),
            disabled: false,
            checked: None,
            pick: Pick::SavePdf,
        });
        rows.push(Row::Separator);
        rows.push(Row::Item {
            label: "Responsive mode".into(),
            disabled: false,
            checked: Some(page.responsive),
            pick: Pick::Responsive,
        });
        rows.push(Row::Item {
            label: "Inspect".into(),
            disabled: false,
            checked: None,
            pick: Pick::Inspect,
        });
        rows.push(Row::Item {
            label: if page.inspecting {
                "Hide DevTools"
            } else {
                "Show DevTools"
            }
            .into(),
            disabled: false,
            checked: None,
            pick: Pick::DevTools,
        });
    }
    let mut tidy: Vec<Row> = Vec::with_capacity(rows.len());
    for row in rows {
        let rule = row == Row::Separator;
        if rule && tidy.last().is_none_or(|last| *last == Row::Separator) {
            continue;
        }
        tidy.push(row);
    }
    while tidy.last() == Some(&Row::Separator) {
        tidy.pop();
    }
    tidy
}

/// `rows` as the native menu of the page `pane`'s menu `menu`.
fn native(rows: Vec<Row>, pane: PaneId, menu: u64) -> NativeMenu {
    rows.into_iter()
        .fold(NativeMenu::new(), |built, row| match row {
            Row::Separator => built.separator(),
            Row::Item {
                label,
                disabled,
                checked,
                pick,
            } => {
                let action = Box::new(MenuPick { pane, menu, pick });
                match checked {
                    Some(checked) if !disabled => built.menu_with_check(label, checked, action),
                    _ => built.menu_with_disabled(label, disabled, action),
                }
            }
            Row::Submenu { label, rows } => built.submenu(label, native(rows, pane, menu)),
        })
}

/// Addresses worth opening or copying; `data:` images are left to the page.
fn is_address(url: &str) -> bool {
    super::LINK_SCHEMES
        .iter()
        .any(|scheme| url.starts_with(scheme))
}

/// The selection as the search row names it: one line, at most 32 characters.
fn search_text(selection: &str) -> String {
    let line = selection.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() > 32 {
        let short: String = line.chars().take(31).collect();
        let mut short = short.trim_end().to_owned();
        short.push('…');
        short
    } else {
        line
    }
}

impl BrowserView {
    /// The page asks for its context menu at `(x, y)` in the pane.
    pub(super) fn context_menu(
        &mut self,
        id: u64,
        (x, y): (i32, i32),
        entries: Vec<MenuEntry>,
        target: MenuTarget,
        cx: &mut Context<Self>,
    ) {
        self.resolve_menu(cx);
        if let Some(old) = self.menu_request.replace(MenuRequest {
            id,
            page_at: ipc::Point {
                x: x as f32,
                y: y as f32,
            },
            entries,
            target,
        }) {
            self.menu_command(old.id, None, cx);
        }
        cx.notify();
    }

    /// Shows the menu asked for; the pick comes back as a [`MenuPick`] on the window.
    pub(super) fn show_menu(
        &mut self,
        request: &MenuRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let page = (!self.is_inspector()).then_some(PageRows {
            responsive: self.responsive.mode.is_some(),
            inspecting: self.devtools.is_some(),
        });
        let engine = crate::settings::get(cx).search_engine;
        let rows = rows(&request.entries, &request.target, page, engine);
        if rows.is_empty() {
            self.menu_command(request.id, None, cx);
            return;
        }
        let at = self.window_point(request.page_at);
        self.open_menu = Some((request.id, request.page_at));
        if cfg!(feature = "visual-check") {
            return;
        }
        if let Err(refusal) = self.menu_gate(window) {
            if refusal == Refusal::Agent {
                log::warn!(target: "koyo::menus", "refused a page's menu: {}", refusal.why());
            }
            self.open_menu = None;
            self.menu_command(request.id, None, cx);
            return;
        }
        log::debug!("showing a page's menu at {at:?}");
        native(rows, self.pane, request.id).show(at, window, cx);
    }

    /// Whether the menu the page asked for may show now: only for the person's own press or
    /// key, in a window on their screen (`docs/browser.md` 14). Never while an agent drives
    /// the page, as its act asked for it; the person's press in the page takes it back first.
    pub(super) fn menu_gate(&self, window: &Window) -> Result<(), Refusal> {
        match self.lease.control.agent() {
            Some(_) => Err(Refusal::Agent),
            None => menus::may_show(window, "a page's menu"),
        }
    }

    /// A menu left without a pick is over for the page: the next press or key says so.
    pub(super) fn resolve_menu(&mut self, cx: &mut Context<Self>) {
        if let Some((id, _)) = self.open_menu.take() {
            self.menu_command(id, None, cx);
        }
    }

    /// The person picked a row of the menu `menu`.
    pub(super) fn picked(&mut self, menu: u64, pick: Pick, cx: &mut Context<Self>) {
        let page_at = match self.open_menu.take() {
            Some((open, page_at)) if open == menu => page_at,
            Some(other) => {
                self.menu_command(other.0, None, cx);
                ipc::Point::default()
            }
            None => ipc::Point::default(),
        };
        self.person_acts(Mode::Implicit, cx);
        let command = match pick {
            Pick::Engine(command) => Some(command),
            _ => None,
        };
        self.menu_command(menu, command, cx);
        match pick {
            Pick::Engine(_) => {}
            Pick::OpenTab(url) => cx.emit(BrowserEvent::OpenTab(url)),
            Pick::Copy(text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
            Pick::Inspect => self.inspect(Some(page_at), cx),
            Pick::DevTools => self.inspect(None, cx),
            Pick::Responsive => self.toggle_responsive(cx),
            Pick::SavePdf => self.save_as_pdf(cx),
        }
        cx.notify();
    }

    fn menu_command(&self, menu: u64, command: Option<i32>, cx: &mut Context<Self>) {
        let pane = self.pane;
        self.send_to_page(
            AppRequest::MenuCommand {
                pane,
                menu,
                command,
            },
            cx,
        );
    }
}

/// What the window does with the menus' picks, history lists' included: routes them to the
/// page they name, wherever the keyboard is.
pub fn handle_actions(root: Div) -> Div {
    let root = super::responsive::bar::handle_actions(root);
    let root = super::profiles::handle_actions(root);
    root.on_action(|pick: &MenuPick, _, cx| {
        with_view(pick.pane, cx, |view, cx| {
            view.picked(pick.menu, pick.pick.clone(), cx);
        });
    })
    .on_action(|pick: &super::history::HistoryPick, _, cx| {
        with_view(pick.pane, cx, |view, cx| view.go_to_entry(pick.entry, cx));
    })
    .on_action(|pick: &super::extensions::menu::ExtensionPick, _, cx| {
        with_view(pick.pane, cx, |view, cx| {
            view.extension_picked(&pick.extension, pick.pick, cx);
        });
    })
}

pub(super) fn with_view(
    pane: PaneId,
    cx: &mut App,
    update: impl FnOnce(&mut BrowserView, &mut Context<BrowserView>),
) {
    let browsers = Browsers::global(cx);
    if let Some(view) = browsers.read(cx).view(pane) {
        view.update(cx, update);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picks(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|row| match row {
                Row::Separator => "-".into(),
                Row::Item { pick, .. } => format!("{pick:?}"),
                Row::Submenu { label, .. } => format!("submenu:{label}"),
            })
            .collect()
    }

    #[test]
    fn a_link_menu_leads_with_the_apps_rows_and_keeps_rules_tidy() {
        let entries = vec![
            MenuEntry::Separator,
            MenuEntry::Command {
                command: 100,
                label: "Back".into(),
                enabled: false,
                checked: None,
            },
            MenuEntry::Separator,
            MenuEntry::Separator,
            MenuEntry::Command {
                command: 132,
                label: "View Source".into(),
                enabled: true,
                checked: None,
            },
            MenuEntry::Separator,
        ];
        let target = MenuTarget {
            link: "https://example.com/a".into(),
            ..MenuTarget::default()
        };
        assert_eq!(
            picks(&rows(&entries, &target, Some(PageRows::default()), SearchEngine::Google)),
            [
                "OpenTab(\"https://example.com/a\")",
                "Copy(\"https://example.com/a\")",
                "-",
                "Engine(100)",
                "-",
                "Engine(132)",
                "-",
                "SavePdf",
                "-",
                "Responsive",
                "Inspect",
                "DevTools"
            ]
        );
        assert_eq!(
            picks(&rows(&entries, &target, None, SearchEngine::Google))
                .last()
                .map(String::as_str),
            Some("Engine(132)"),
            "DevTools have no inspector of their own, nor responsive mode"
        );
        let on = rows(
            &entries,
            &target,
            Some(PageRows {
                responsive: true,
                inspecting: true,
            }),
            SearchEngine::Google,
        );
        assert!(
            on.iter().any(|row| matches!(
                row,
                Row::Item {
                    pick: Pick::Responsive,
                    checked: Some(true),
                    ..
                }
            )),
            "the row is checked while the mode is on"
        );
        assert!(
            on.iter().any(|row| matches!(
                row,
                Row::Item { pick: Pick::DevTools, label, .. } if label == "Hide DevTools"
            )),
            "with DevTools up, the row hides them"
        );
    }

    #[test]
    fn selected_text_offers_a_search_outside_text_fields() {
        let target = MenuTarget {
            selection: "  the quick\nbrown fox jumps over the lazy dog again ".into(),
            ..MenuTarget::default()
        };
        let menu = rows(&[], &target, None, SearchEngine::DuckDuckGo);
        let Some(Row::Item { label, pick, .. }) = menu.first() else {
            panic!("a search row");
        };
        assert_eq!(label, "Search DuckDuckGo for “the quick brown fox jumps over…”");
        let Pick::OpenTab(url) = pick else {
            panic!("it opens the results in a tab: {pick:?}");
        };
        assert!(url.starts_with("https://duckduckgo.com/?q="), "{url}");
        assert!(url.contains("lazy+dog+again"), "all of it, not the row's: {url}");
        let editable = MenuTarget {
            editable: true,
            ..target
        };
        assert!(rows(&[], &editable, None, SearchEngine::Google).is_empty());
    }
}
