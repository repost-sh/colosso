//! The MCP server page's Agents section (`docs/app-control.md` 9): what agents' calls do when
//! they don't say, for every agent, those in Koyo's terminals and chats too, so it shows
//! whether the server is on or not. Whether the window switches to what they open, and the
//! size a browser page they open starts at: responsive mode at a width × height, 1440 × 900
//! until the person changes it, or filling its pane.

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{Action, App, SharedString, Window, actions};

use super::components::{MenuButton, SettingRow, SettingsSection};
use super::field_dialog::{self, Field};
use super::native_switch::NativeSwitch;
use crate::browser::devices;
use crate::control::{self, PageSize};

/// The window scale whose limits the dialog keeps to: a Retina window's. A denser screen
/// brings a larger page within its own as the page opens.
const SCALE: f32 = 2.;

/// Picks the size pages agents open start at, from the section's menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetPagesOpenAt(pub PageSize);

actions!(settings, [EditPagesOpenAt]);

/// Answers the section's menu, wherever the menu was opened.
pub fn init(cx: &mut App) {
    cx.on_action(|SetPagesOpenAt(size): &SetPagesOpenAt, cx| {
        control::set_pages_open_at(*size, cx);
    });
}

pub fn section(cx: &App) -> SettingsSection {
    let size = control::pages_open_at(cx);
    SettingsSection::new("Agents")
        .row(
            SettingRow::new("Switch to what agents open")
                .description(
                    "When an agent makes a worktree, opens a tab, a file or the changes, splits \
                     a pane, adds a project or starts a chat, the window switches to it. Off, \
                     it's listed in the sidebar and the tab strip and the window stays where \
                     you are. An agent you ask to show you something still does.",
                )
                .control(NativeSwitch::new(
                    "mcp-show-what-agents-open",
                    |settings| settings.external_control.show_what_agents_open,
                    |settings, on| settings.external_control.show_what_agents_open = on,
                )),
        )
        .row(
            SettingRow::new("Pages agents open")
                .description(SharedString::from(describe(size)))
                .control(MenuButton::new("mcp-pages-open-at", label(size), |cx| {
                    menu(control::pages_open_at(cx))
                })),
        )
}

/// What pages agents open do at `size`.
fn describe(size: PageSize) -> String {
    match size.size() {
        Some(_) => format!(
            "A browser page an agent opens starts in responsive mode at {}, scaled down to fit \
             its pane, so it lays out as on that screen. An agent can ask for another size, a \
             device or the whole pane. Pages you open fill their pane.",
            label(size)
        ),
        None => "A browser page an agent opens fills its pane, as pages you open do. An agent \
                 can still ask for a size or a device."
            .to_owned(),
    }
}

/// "1440 × 900", or "Fill the pane".
fn label(size: PageSize) -> String {
    match size.size() {
        Some((width, height)) => format!("{width} × {height}"),
        None => "Fill the pane".to_owned(),
    }
}

/// The default size, the one set when it's another, a size of the person's own, and filling
/// the pane; the one set checked.
fn menu(current: PageSize) -> NativeMenu {
    let item = |menu: NativeMenu, size: PageSize| {
        menu.menu_with_check(label(size), size == current, Box::new(SetPagesOpenAt(size)))
    };
    let mut menu = item(NativeMenu::new(), PageSize::AGENT);
    if current != PageSize::AGENT && current != PageSize::Fill {
        menu = item(menu, current);
    }
    let menu = menu.menu_with_check("Other size…", false, Box::new(EditPagesOpenAt));
    item(menu, PageSize::Fill)
}

/// What's typed as a size: two numbers, each within responsive mode's limits at [`SCALE`],
/// between them any of `×`, `x`, `*`, a comma or spaces.
fn parse(text: &str) -> Result<PageSize, SharedString> {
    let sides: Vec<&str> = text
        .split(|c: char| !c.is_ascii_digit())
        .filter(|side| !side.is_empty())
        .collect();
    let refused = || {
        let (least, most) = (devices::MIN_SIDE, devices::max_side(SCALE));
        format!("A size is a width × height, each from {least} to {most}.").into()
    };
    let [width, height] = sides.as_slice() else {
        return Err(refused());
    };
    let side = |side: &str| {
        side.parse::<u32>()
            .ok()
            .filter(|&side| devices::within(side, SCALE))
    };
    let separators = text
        .chars()
        .all(|c| c.is_ascii_digit() || "×xX*, ".contains(c));
    match (side(width), side(height)) {
        (Some(width), Some(height)) if separators => Ok(PageSize::Size { width, height }),
        _ => Err(refused()),
    }
}

/// Asks for a size of the person's own.
pub fn edit_pages_open_at(window: &mut Window, cx: &mut App) {
    let current = control::pages_open_at(cx);
    let field = Field {
        ids: ["mcp-pages-open-at-cancel", "mcp-pages-open-at-save"],
        title: "Size of pages agents open",
        note: "Width × height in CSS pixels. A page larger than its pane shows scaled down.".into(),
        placeholder: Some("1440 × 900"),
        value: current.size().map(|_| label(current)).unwrap_or_default(),
        save: |value, cx| {
            control::set_pages_open_at(parse(value)?, cx);
            Ok(())
        },
    };
    field_dialog::open(field, window, cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_size_reads_as_a_person_types_it() {
        let size = |width, height| Ok(PageSize::Size { width, height });
        assert_eq!(parse("1920 × 1080"), size(1920, 1080));
        assert_eq!(parse("1440x900"), size(1440, 900));
        assert_eq!(parse(" 1280 * 800 "), size(1280, 800));
        assert_eq!(parse("390, 844"), size(390, 844));
        for wrong in [
            "",
            "1920",
            "1920 × 1080 × 2",
            "20 × 1080",
            "9000 × 1080",
            "1.5 × 2",
            "-5 × 9",
        ] {
            assert!(parse(wrong).is_err(), "{wrong}");
        }
        assert_eq!(label(PageSize::AGENT), "1440 × 900");
        assert_eq!(label(PageSize::Fill), "Fill the pane");
    }
}
