//! The Editor settings page: how the Files panel opens files and what ⌘S saves, and what
//! the code shows over its text.

use gpui_kit::App;

use super::components::{MenuButton, SettingRow, SettingsSection};
use super::native_switch::NativeSwitch;
use crate::settings;

pub fn sections(cx: &App) -> Vec<SettingsSection> {
    vec![files(cx), code()]
}

fn files(cx: &App) -> SettingsSection {
    let scope = settings::get(cx).save_scope;
    SettingsSection::new("Files")
        .row(
            SettingRow::new("Preview files")
                .description(
                    "A file picked in the Files panel, by a click or the arrow keys, opens in \
                     a preview tab the next one takes over. A double click keeps it open.",
                )
                .control(NativeSwitch::new(
                    "preview-files",
                    |settings| settings.preview_files,
                    |settings, on| settings.preview_files = on,
                )),
        )
        .row(
            SettingRow::new("⌘S saves")
                .description(
                    "The file shown, as most editors do; or every file with unsaved edits open \
                     in the window, as Save All does.",
                )
                .control(MenuButton::new("save-scope", scope.label(), |cx| {
                    settings::get(cx).save_scope.menu()
                })),
        )
}

fn code() -> SettingsSection {
    SettingsSection::new("Code")
        .row(
            SettingRow::new("Inlay hints")
                .description(
                    "The types and parameter names the language server infers, shown greyed \
                     in the code where they'd go. They aren't in the file.",
                )
                .control(NativeSwitch::new(
                    "inlay-hints",
                    |settings| settings.inlay_hints,
                    |settings, on| settings.inlay_hints = on,
                )),
        )
        .row(
            SettingRow::new("Semantic colouring")
                .description(
                    "The language server's own reading of the code colours it over the \
                     grammar's: methods, fields, parameters, types in macros, doc comments.",
                )
                .control(NativeSwitch::new(
                    "semantic-colouring",
                    |settings| settings.semantic_colouring,
                    |settings, on| settings.semantic_colouring = on,
                )),
        )
}
