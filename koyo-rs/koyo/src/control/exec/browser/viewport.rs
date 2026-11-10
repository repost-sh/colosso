//! `browser_viewport`: a page at a chosen size or as a device (`docs/responsive.md` 4.1),
//! under its lease as any act, answered once the page has laid out at it, with an image of
//! it when asked for.

use std::time::Duration;

use gpui_kit::{App, AsyncApp};
use koyo_app_control::ControlError;
use koyo_app_control::ops::browser::{
    BrowserViewportInput, BrowserViewportOutput, DeviceName, ImageFormat, OpenViewport,
    Orientation, ViewportZoom,
};

use super::super::{Caller, Exec, output};
use super::acts::capture;
use super::page::{Page, acted, refused, resolve};
use super::script::evaluate;
use crate::browser::agent::{self, Choice, Viewport, ViewportChange, ViewportRefused, Zoom};

/// How often the page is looked at while it settles, and how many times: 5 seconds.
const POLL: Duration = Duration::from_millis(100);
const POLLS: u32 = 50;

/// What a page opening at `viewport` (`tab_open`, `pane_split`) asks of responsive mode: as
/// `browser_viewport` with those fields would.
pub(crate) fn opening_change(viewport: &OpenViewport) -> Result<ViewportChange, ControlError> {
    change(&BrowserViewportInput {
        device: viewport.device,
        width: viewport.width,
        height: viewport.height,
        orientation: viewport.orientation,
        fill: viewport.fill,
        ..BrowserViewportInput::default()
    })
}

/// What `input` asks of responsive mode, the combinations no page takes refused.
fn change(input: &BrowserViewportInput) -> Result<ViewportChange, ControlError> {
    let device = match input.device {
        None => None,
        Some(DeviceName::Responsive) => Some(Choice::Free),
        Some(name) => Some(Choice::Preset(agent::find_device(name.id()).ok_or_else(
            || ControlError::internal(format!("Koyo has no preset `{}`.", name.id())),
        )?)),
    };
    let zoom = match input.zoom {
        Some(ViewportZoom::Percent(other)) if !matches!(other, 50 | 75 | 100) => {
            return Err(ControlError::invalid_input(format!(
                "`zoom` is `fit`, 50, 75 or 100, not {other}."
            )));
        }
        zoom => zoom.map(Zoom::from),
    };
    Ok(ViewportChange {
        fill: input.fill == Some(true),
        device,
        width: input.width,
        height: input.height,
        scale: input.scale.map(|scale| scale as f32),
        mobile: input.mobile,
        touch: input.touch,
        user_agent: input.user_agent.clone(),
        landscape: (input.orientation).map(|orientation| orientation == Orientation::Landscape),
        zoom,
    })
}

pub(crate) fn viewport(
    caller: &Caller,
    input: &BrowserViewportInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let change = change(input)?;
    let page = resolve(caller, input.pane, cx)?;
    let actor = caller.actor();
    let target = (page
        .view
        .update(cx, |view, cx| view.agent_viewport(&actor, &change, cx)))
    .map_err(|why| match why {
        ViewportRefused::Refused(why) => refused(why),
        ViewportRefused::Invalid(why) => ControlError::invalid_input(why),
    })?;
    acted(caller, &page, cx);
    let screenshot = input.screenshot.unwrap_or(false);
    Ok(Exec::Later(cx.spawn(async move |cx: &mut AsyncApp| {
        let settled = match target {
            Some(target) => settle(&page, target, cx).await,
            None => false,
        };
        let viewport = cx.update(|cx| page.view.read(cx).viewport_facts(cx));
        let image = if screenshot {
            Some(capture(&page, ImageFormat::Png, false, cx).await?)
        } else {
            None
        };
        output(&BrowserViewportOutput {
            viewport,
            settled,
            mime_type: (image.as_ref()).map(|_| ImageFormat::Png.mime_type().to_owned()),
            data: image,
        })
    })))
}

/// Waits, 5 seconds at most, until the page reports the size it lays out at as `target`,
/// having shown a frame of it when it's in sight: whether it did.
async fn settle(page: &Page, target: Viewport, cx: &mut AsyncApp) -> bool {
    for _ in 0..POLLS {
        if cx.update(|cx| page.view.read(cx).ready_to_measure(target)) {
            let inner = evaluate(page, None, "[innerWidth, innerHeight]", false, cx).await;
            let inner = (inner.ok())
                .and_then(|inner| Some((inner.get(0)?.as_f64()?, inner.get(1)?.as_f64()?)));
            let laid_out = inner
                .is_some_and(|inner| cx.update(|cx| page.view.read(cx).laid_out_at(target, inner)));
            if laid_out {
                return true;
            }
        }
        cx.background_executor().timer(POLL).await;
    }
    false
}
