//! The page's context menu: the engine's rows, shown as the app's native menu, led by the
//! app's own rows for links, images and selected text. Picks come back as actions on the
//! window, which the shell routes to the page.

use colosso_browser_ipc::{self as ipc, AppRequest, MenuEntry, MenuTarget, MouseKind, PaneId};
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{
    Action, App, ClipboardItem, Context, Div, InteractiveElement as _, MouseUpEvent, Pixels,
    PlatformInput, Point, Window, point, px,
};

use super::control::Mode;
use super::host::Browsers;
use super::view::{BrowserEvent, BrowserView};

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
}

/// A row picked from the context menu `menu` of the page `pane`.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = browser, no_json)]
pub struct MenuPick {
    pub pane: PaneId,
    pub menu: u64,
    pub pick: Pick,
}

/// The person takes the page `pane`, or gives it back.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = browser, no_json)]
pub struct Lease {
    pub pane: PaneId,
    pub take: bool,
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

/// Rows the app adds for what was under the pointer.
fn own_rows(target: &MenuTarget) -> Vec<Row> {
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
        let search = Pick::OpenTab(super::search_url(&target.selection));
        rows.push(item(format!("Search for “{query}”"), search));
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

/// Every row of the menu, with no leading, trailing or doubled rules. Pages that can be
/// `inspectable` end with Inspect, as Chrome's menus do.
fn rows(entries: &[MenuEntry], target: &MenuTarget, inspectable: bool) -> Vec<Row> {
    let mut rows = own_rows(target);
    rows.extend(entries.iter().map(|entry| match entry {
        MenuEntry::Separator => Row::Separator,
        MenuEntry::Command { .. } => command(entry).unwrap_or(Row::Separator),
        MenuEntry::Submenu { label, entries } => Row::Submenu {
            label: label.clone(),
            rows: entries.iter().filter_map(command).collect(),
        },
    }));
    if inspectable {
        rows.push(Row::Separator);
        rows.push(Row::Item {
            label: "Inspect".into(),
            disabled: false,
            checked: None,
            pick: Pick::Inspect,
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
        // A newer menu replaces one not shown or not picked from yet.
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
        let rows = rows(&request.entries, &request.target, !self.is_inspector());
        if rows.is_empty() {
            self.menu_command(request.id, None, cx);
            return;
        }
        let at: Point<Pixels> = self.origin + point(px(request.page_at.x), px(request.page_at.y));
        self.open_menu = Some((request.id, request.page_at));
        self.end_press(window, cx);
        // The visual check runs unattended, where the menu's tracking would wait for a pick.
        if cfg!(feature = "visual-check") {
            return;
        }
        native(rows, self.pane, request.id).show(at, window, cx);
    }

    /// The menu's tracking takes the release of a press still held on the page, and until
    /// that press is let go the window gives every press to the page, the app's own buttons
    /// left dead. So the press ends here, for the page and for the window, before it shows.
    fn end_press(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(button) = self.pressed.take() else {
            return;
        };
        let (position, modifiers) = (window.mouse_position(), window.modifiers());
        self.mouse(MouseKind::Up, position, Some(button), 1, modifiers, cx);
        let release = MouseUpEvent {
            button: super::input::platform_button(button),
            position,
            modifiers,
            click_count: 1,
        };
        window.defer(cx, move |window, cx| {
            window.dispatch_event(PlatformInput::MouseUp(release), cx);
        });
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
                // Not the menu on screen: it's over, and so is the one picked from.
                self.menu_command(other.0, None, cx);
                ipc::Point::default()
            }
            None => ipc::Point::default(),
        };
        self.person_acts(Mode::Implicit, cx);
        // The engine carries out its own rows; the app's rows close the menu with no command.
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

/// What the window does with the menus' picks, history lists' included, and the lease
/// chip's choices: routes them to the page they name, wherever the keyboard is.
pub fn handle_actions(root: Div) -> Div {
    root.on_action(|pick: &MenuPick, _, cx| {
        with_view(pick.pane, cx, |view, cx| {
            view.picked(pick.menu, pick.pick.clone(), cx);
        });
    })
    .on_action(|pick: &super::history::HistoryPick, _, cx| {
        with_view(pick.pane, cx, |view, cx| view.go_to_entry(pick.entry, cx));
    })
    .on_action(|lease: &Lease, _, cx| {
        with_view(lease.pane, cx, |view, cx| {
            if lease.take {
                view.person_acts(Mode::Explicit, cx);
            } else {
                view.give_back(cx);
            }
        });
    })
}

fn with_view(
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
            picks(&rows(&entries, &target, true)),
            [
                "OpenTab(\"https://example.com/a\")",
                "Copy(\"https://example.com/a\")",
                "-",
                "Engine(100)",
                "-",
                "Engine(132)",
                "-",
                "Inspect"
            ]
        );
        assert_eq!(
            picks(&rows(&entries, &target, false))
                .last()
                .map(String::as_str),
            Some("Engine(132)"),
            "DevTools have no inspector of their own"
        );
    }

    #[test]
    fn selected_text_offers_a_search_outside_text_fields() {
        let target = MenuTarget {
            selection: "  the quick\nbrown fox jumps over the lazy dog again ".into(),
            ..MenuTarget::default()
        };
        let menu = rows(&[], &target, false);
        let Some(Row::Item { label, .. }) = menu.first() else {
            panic!("a search row");
        };
        assert_eq!(label, "Search for “the quick brown fox jumps over…”");
        let editable = MenuTarget {
            editable: true,
            ..target
        };
        assert!(rows(&[], &editable, false).is_empty());
    }
}
