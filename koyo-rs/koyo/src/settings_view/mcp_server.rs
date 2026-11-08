//! The MCP server page (`docs/app-control.md` 9): the switch that serves Koyo's MCP over
//! HTTP on localhost, its URL and port, what agents' calls do when they don't say
//! (`mcp_agents`), one-click setup for the usual clients, and the stdio command agents start.
//! The clients the person allowed are in `mcp_clients`.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use gpui_kit::{App, ClipboardItem, ParentElement as _, SharedString, Styled as _, Window, div};

use super::components::{PillButton, SettingRow, SettingsSection};
use super::field_dialog::{self, Field};
use super::native_switch::NativeSwitch;
use crate::control;
use crate::theme;

/// The name clients list the server under.
const SERVER_NAME: &str = "koyo";

pub fn sections(cx: &App) -> Vec<SettingsSection> {
    let mut sections = vec![server(cx), super::mcp_agents::section(cx)];
    if control::policy(cx).enabled {
        sections.push(clients_setup(cx));
    }
    sections.push(stdio());
    sections.extend(super::mcp_clients::sections(cx));
    sections
}

/// The URL clients reach the server at, served or not.
fn url(cx: &App) -> String {
    control::http_state(cx)
        .url
        .unwrap_or_else(|| koyo_term_mcp::http::url(control::policy(cx).port))
}

fn server(cx: &App) -> SettingsSection {
    let p = theme::palette(cx);
    let policy = control::policy(cx);
    let http = control::http_state(cx);
    let mut section = SettingsSection::new("MCP server").row(
        SettingRow::new("MCP server")
            .description(
                "Lets MCP clients on this Mac (Claude Code, Codex, Cursor, VS Code) see and drive \
                 Koyo: projects, worktrees, tabs, panes, browser pages, terminals and \
                 settings. Each client asks once before it may. Agents in Koyo's terminals \
                 reach it whether this is on or not.",
            )
            .control(NativeSwitch::new(
                "mcp-server",
                |settings| settings.external_control.enabled,
                |settings, on| settings.external_control.enabled = on,
            )),
    );
    if !policy.enabled {
        return section;
    }
    let address = url(cx);
    let (status, colour) = match &http.error {
        Some(error) => (error.clone(), p.danger),
        None => (address.clone(), p.muted),
    };
    section = section.row(
        SettingRow::new("Server URL")
            .description(SharedString::from(status))
            .control(
                div()
                    .text_sm()
                    .text_color(colour)
                    .child(format!("Port {}", policy.port)),
            )
            .control(
                PillButton::new("mcp-copy-url", "Copy").on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(address.clone()));
                }),
            )
            .control(
                PillButton::new("mcp-port", "Change…")
                    .on_click(|_, window, cx| edit_port(window, cx)),
            ),
    );
    section
}

fn clients_setup(cx: &App) -> SettingsSection {
    let address = url(cx);
    let claude = format!("claude mcp add --transport http {SERVER_NAME} {address}");
    let codex = format!("codex mcp add {SERVER_NAME} --url {address}");
    let cursor = cursor_link(&address);
    let vscode = vscode_link(&address);
    SettingsSection::new("Add to a client")
        .row(copy_row("Claude Code", "mcp-add-claude", claude))
        .row(copy_row("Codex", "mcp-add-codex", codex))
        .row(
            SettingRow::new("Cursor")
                .description("Opens Cursor's install prompt for this server.")
                .control(
                    PillButton::new("mcp-add-cursor", "Add").on_click(move |_, _, cx| {
                        cx.open_url(&cursor);
                    }),
                ),
        )
        .row(
            SettingRow::new("VS Code")
                .description("Opens VS Code's install prompt for this server.")
                .control(
                    PillButton::new("mcp-add-vscode", "Add").on_click(move |_, _, cx| {
                        cx.open_url(&vscode);
                    }),
                ),
        )
}

/// A row whose button copies `command`, shown as its description.
fn copy_row(title: &'static str, id: &'static str, command: String) -> SettingRow {
    SettingRow::new(title)
        .description(SharedString::from(command.clone()))
        .control(PillButton::new(id, "Copy").on_click(move |_, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(command.clone()));
        }))
}

fn stdio() -> SettingsSection {
    let program = std::env::current_exe()
        .map_or_else(|_| "koyo".to_owned(), |path| path.display().to_string());
    let command = format!("claude mcp add {SERVER_NAME} -- \"{program}\" mcp");
    SettingsSection::new("For agents").row(
        copy_row("Standard input and output", "mcp-copy-stdio", command).description(format!(
            "The same server for clients that start it themselves: claude mcp add \
                 {SERVER_NAME} -- \"{program}\" mcp. It starts Koyo with app_launch when it \
                 isn't running."
        )),
    )
}

/// Cursor's install link: the server's entry, as its `mcp.json` writes one, in base64.
fn cursor_link(url: &str) -> String {
    let config = serde_json::json!({ "url": url }).to_string();
    format!(
        "cursor://anysphere.cursor-deeplink/mcp/install?name={SERVER_NAME}&config={}",
        STANDARD.encode(config)
    )
}

/// VS Code's install link: the server's entry as JSON, encoded as `encodeURIComponent` does.
fn vscode_link(url: &str) -> String {
    let config = serde_json::json!({ "name": SERVER_NAME, "type": "http", "url": url }).to_string();
    format!(
        "vscode:mcp/install?{}",
        utf8_percent_encode(&config, URI_COMPONENT)
    )
}

/// What JavaScript's `encodeURIComponent` leaves as it is: letters, digits and `-_.!~*'()`.
const URI_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

/// Asks for the port the server listens on.
fn edit_port(window: &mut Window, cx: &mut App) {
    let field = Field {
        ids: ["mcp-port-cancel", "mcp-port-save"],
        title: "MCP server port",
        note: format!(
            "The server listens on 127.0.0.1 at this port. {} is the default.",
            control::DEFAULT_PORT
        )
        .into(),
        placeholder: None,
        value: control::policy(cx).port.to_string(),
        save: |value, cx| match value.parse::<u16>() {
            Ok(port) if port >= 1024 => {
                control::set_port(port, cx);
                Ok(())
            }
            _ => Err("A port is a number from 1024 to 65535.".into()),
        },
    };
    field_dialog::open(field, window, cx);
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;

    use super::super::SettingsPage;
    use super::*;
    use crate::settings;

    #[gpui_kit::test]
    fn the_page_lists_no_activity_and_offers_the_size_of_agents_pages(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let _requests = cx.update(control::start_for_test);
        cx.update(|cx| {
            settings::update(cx, |settings| settings.external_control.enabled = true);
            // Something for a list of activity to show, were there one.
            control::allow_denied("Windsurf", cx);
        });
        let matches = |query: &str, cx: &mut TestAppContext| {
            cx.read(|cx| SettingsPage::McpServer.matches(query, cx))
        };
        assert!(matches("windsurf", cx), "the clients are listed");
        assert!(!matches("recent activity", cx));
        assert!(!matches("asked to control koyo", cx));
        assert!(matches("pages agents open", cx));
        assert!(matches("1440 × 900", cx));
        cx.update(|cx| control::set_pages_open_at(control::PageSize::Fill, cx));
        assert!(matches("fills its pane", cx) && !matches("1440 × 900", cx));
        // The Agents section shows with the server off, as it holds for Koyo's own agents.
        cx.update(|cx| {
            settings::update(cx, |settings| settings.external_control.enabled = false);
        });
        assert!(matches("pages agents open", cx) && !matches("windsurf", cx));
    }

    #[test]
    fn install_links_carry_the_servers_entry_as_each_client_reads_it() {
        assert_eq!(
            cursor_link("http://127.0.0.1:6272/mcp"),
            "cursor://anysphere.cursor-deeplink/mcp/install?name=koyo&config=\
             eyJ1cmwiOiJodHRwOi8vMTI3LjAuMC4xOjYyNzIvbWNwIn0="
        );
        assert_eq!(
            vscode_link("http://127.0.0.1:6272/mcp"),
            "vscode:mcp/install?%7B%22name%22%3A%22koyo%22%2C%22type%22%3A%22http%22%2C\
             %22url%22%3A%22http%3A%2F%2F127.0.0.1%3A6272%2Fmcp%22%7D"
        );
    }
}
