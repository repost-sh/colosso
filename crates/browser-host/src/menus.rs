//! Context menus. The engine builds the menu; the app shows it with its own
//! menu UI and answers with the picked command. The host keeps CEF's callback
//! until then. Everything here runs on CEF's UI thread.
use crate::host;
use crate::pending::Pending;
use cef::{
    CefString, ContextMenuParams, EventFlags, ImplContextMenuParams, ImplMenuModel,
    ImplRunContextMenuCallback, MenuItemType, MenuModel, RunContextMenuCallback,
};
use colosso_browser_ipc::{HostEvent, MenuEntry, MenuTarget, PaneId};
use std::cell::RefCell;

/// Find and Print need windows the host does not have.
const UNSUPPORTED: [i32; 2] = [130, 131];

/// Chrome's own Inspect rows: they would open a `DevTools` window, which
/// the host cannot render. The app offers its own Inspect instead.
const CHROME_INSPECT: [&std::ffi::CStr; 2] = [
    c"IDC_CONTENT_CONTEXT_INSPECTELEMENT",
    c"IDC_CONTENT_CONTEXT_INSPECTELEMENT_WITH_DEVTOOLS",
];

thread_local! {
    static MENUS: RefCell<Pending<RunContextMenuCallback>> = const { RefCell::new(Pending::new()) };
}

/// Drops the rows the host cannot carry out.
pub(crate) fn prepare(model: &MenuModel) {
    for command in UNSUPPORTED {
        model.remove(command);
    }
    for name in CHROME_INSPECT {
        // SAFETY: `name` is a NUL-terminated string that outlives the call.
        let command = unsafe { cef::sys::cef_id_for_command_id_name(name.as_ptr()) };
        if command > 0 {
            model.remove(command);
        }
    }
}

/// Sends the menu to the app and keeps `callback` for its answer.
pub(crate) fn run(
    pane: PaneId,
    params: &ContextMenuParams,
    model: &MenuModel,
    callback: RunContextMenuCallback,
) {
    let menu = MENUS.with(|menus| menus.borrow_mut().add(pane, callback));
    let text = |value: cef::CefStringUserfree| CefString::from(&value).to_string();
    host::send(&HostEvent::ContextMenu {
        pane,
        menu,
        x: params.xcoord(),
        y: params.ycoord(),
        entries: entries(model),
        target: MenuTarget {
            link: text(params.link_url()),
            media: text(params.source_url()),
            selection: text(params.selection_text()),
            editable: params.is_editable() != 0,
        },
    });
}

/// The person picked `command`, or closed the menu.
pub(crate) fn command(pane: PaneId, menu: u64, command: Option<i32>) {
    let Some(callback) = MENUS.with(|menus| menus.borrow_mut().remove(pane, menu)) else {
        return;
    };
    match command {
        Some(command) => callback.cont(command, EventFlags::default()),
        None => callback.cancel(),
    }
}

/// The pane closed; its menu goes with it.
pub(crate) fn pane_closed(pane: PaneId) {
    cancel(|p| p == pane);
}

/// The host is shutting down; CEF waits for every callback to go.
pub(crate) fn clear() {
    cancel(|_| true);
}

fn cancel(which: impl Fn(PaneId) -> bool) {
    // Taken first: cancelling calls into CEF, which may run a menu again.
    let menus = MENUS.with(|menus| menus.borrow_mut().take(|pane, _| which(pane)));
    for (_, callback) in menus {
        callback.cancel();
    }
}

fn entries(model: &MenuModel) -> Vec<MenuEntry> {
    (0..model.count())
        .filter(|&i| model.is_visible_at(i) != 0)
        .filter_map(|i| {
            let kind = model.type_at(i);
            let label = || mnemonic_free(&CefString::from(&model.label_at(i)).to_string());
            if kind == MenuItemType::SEPARATOR {
                Some(MenuEntry::Separator)
            } else if kind == MenuItemType::SUBMENU {
                let entries = model.sub_menu_at(i).map(|sub| entries(&sub))?;
                Some(MenuEntry::Submenu {
                    label: label(),
                    entries,
                })
            } else if kind == MenuItemType::COMMAND
                || kind == MenuItemType::CHECK
                || kind == MenuItemType::RADIO
            {
                Some(MenuEntry::Command {
                    command: model.command_id_at(i),
                    label: label(),
                    enabled: model.is_enabled_at(i) != 0,
                    checked: (kind != MenuItemType::COMMAND).then(|| model.is_checked_at(i) != 0),
                })
            } else {
                None
            }
        })
        .collect()
}

/// A label without its `&` mnemonic markers; `&&` is a literal `&`.
fn mnemonic_free(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&' {
            if chars.peek() == Some(&'&') {
                out.push('&');
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_lose_their_mnemonics() {
        assert_eq!(mnemonic_free("&Back"), "Back");
        assert_eq!(mnemonic_free("Select &All"), "Select All");
        assert_eq!(mnemonic_free("Tom && Jerry"), "Tom & Jerry");
    }
}
