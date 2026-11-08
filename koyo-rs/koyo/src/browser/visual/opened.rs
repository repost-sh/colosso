//! A page an agent opens, as its person sees it (`docs/responsive.md` 9): in responsive mode
//! at the size they set for pages agents open, 1440 × 900 by default, scaled down to fit its
//! pane under the device bar, which names the agent; and the MCP server settings that set it,
//! with no list of what clients did.

use gpui_kit::{Entity, ScrollDelta, ScrollEvent, point, px};

use super::{Check, fits, loaded, shown_page};
use crate::browser::BrowserView;
use crate::browser::agent::{self, SetBy};
use crate::control::{self, PageSize};
use crate::navigation::Route;
use crate::settings;
use crate::settings_view::SettingsPage;
use crate::theme::UseLightTheme;
use crate::visual::Result;

impl Check {
    pub(super) fn opened(&mut self, port: u16) -> Result<()> {
        self.drawn
            .cx
            .update(|cx| cx.dispatch_action(&UseLightTheme));
        self.open_as_agent(port)?;
        self.save("agent-opened.png")
    }

    /// Settings → MCP server with the server on, as the person finds it after agents worked:
    /// the Agents section with the size of their pages, and the clients, nothing more.
    pub(super) fn mcp_settings(&mut self) -> Result<()> {
        let shell = self.drawn.shell.clone();
        self.drawn.cx.update(|cx| {
            settings::update(cx, |settings| settings.external_control.enabled = true);
            control::allow_denied("Claude Code", cx);
            shell.update(cx, |shell, cx| {
                shell.navigate(Route::Settings(SettingsPage::McpServer), cx);
            });
        });
        self.save("settings-mcp.png")?;
        (self.drawn.cx).update(|cx| control::set_pages_open_at(PageSize::Fill, cx));
        self.save("settings-mcp-fill.png")?;
        // The page's end, the clients the person allowed, where Recent activity used to be.
        let wheel = ScrollEvent {
            position: point(px(900.), px(600.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(-3000.))),
            ..ScrollEvent::default()
        };
        (self.drawn.cx).simulate_event(self.drawn.window, wheel);
        self.save("settings-mcp-end.png")?;
        self.drawn.cx.update(|cx| {
            settings::update(cx, |settings| {
                settings.external_control = control::Policy::default();
            });
        });
        Ok(())
    }

    /// A tab opened as app control opens an agent's (`control/exec/open.rs`): its page made
    /// in the mode the person's size for agents' pages says, here shown.
    fn open_as_agent(&mut self, port: u16) -> Result<Entity<BrowserView>> {
        let (shell, window) = (self.drawn.shell.clone(), self.drawn.window);
        let url = format!("127.0.0.1:{port}/agent");
        let opened = (self.drawn.cx)
            .update_window(window, |_, window, cx| {
                shell.update(cx, |shell, cx| {
                    let default = control::pages_open_at(cx).size();
                    let by = SetBy::Agent("Claude Code".into());
                    let mode = agent::opening(None, default, window.scale_factor(), &by)?;
                    (shell.open_browser_tab(Some(url), false, mode, window, cx))
                        .ok_or_else(|| "no checkout to open a page on".to_owned())
                })
            })
            .map_err(|error| error.to_string())?;
        opened?;
        let page = (self.drawn.cx.read(|cx| shown_page(&shell, cx))).ok_or("no agent tab")?;
        // Painted at the size it opened at, not only loaded.
        self.until(&page, "the agent's page at its size", |view| {
            loaded(view, "Agent") && fits(view)
        })?;
        Ok(page)
    }
}
