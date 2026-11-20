//! A page's console and its requests, as agents hear them (`browser/agent/heard.rs`): kept from
//! the first call that asks, the domains they come from enabled again by every call, which the
//! engine takes as often as it's told, so a page the engine opened again since still tells.

use gpui_kit::{App, AsyncApp};
use koyo_app_control::ControlError;
use koyo_app_control::ops::browser::{
    BrowserConsoleOutput, BrowserHeardInput, BrowserNetworkOutput,
};
use serde_json::json;

use super::super::{Caller, Exec, output};
use super::acts::on_page;
use super::page::{call, now};
use super::pages::Page;
use crate::browser::agent::Heard;

pub(crate) fn console(
    caller: &Caller,
    input: &BrowserHeardInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let clear = input.clear.unwrap_or(false);
    on_page(caller, input.pane, false, cx, async move |page, _, cx| {
        heard(&page, cx, |heard| {
            heard.console();
        })?;
        for method in ["Runtime.enable", "Log.enable"] {
            call(&page, None, method, json!({}), cx).await?;
        }
        let (messages, dropped) = heard(&page, cx, |heard| heard.console().read(clear))?;
        let at = cx.update(|cx| now(&page, cx))?;
        output(&BrowserConsoleOutput {
            pane: at.pane,
            url: at.url,
            messages,
            dropped,
        })
    })
}

pub(crate) fn network(
    caller: &Caller,
    input: &BrowserHeardInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let clear = input.clear.unwrap_or(false);
    on_page(caller, input.pane, false, cx, async move |page, _, cx| {
        heard(&page, cx, |heard| {
            heard.network();
        })?;
        call(&page, None, "Network.enable", json!({}), cx).await?;
        let (requests, dropped) = heard(&page, cx, |heard| heard.network().read(clear))?;
        let at = cx.update(|cx| now(&page, cx))?;
        output(&BrowserNetworkOutput {
            pane: at.pane,
            url: at.url,
            requests,
            dropped,
        })
    })
}

/// Runs `read` on what agents hear of the page.
pub(super) fn heard<T>(
    page: &Page,
    cx: &mut AsyncApp,
    read: impl FnOnce(&mut Heard) -> T,
) -> Result<T, ControlError> {
    cx.update(|cx| Ok(page.view()?.update(cx, |view, _| read(view.heard()))))
}
