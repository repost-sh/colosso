//! What reads and acts on a page over the Chrome `DevTools` protocol: its content, its
//! snapshot, its image, an expression's value, and clicks, typing, keys and scrolling, which
//! go in as the person's input would.

use gpui_kit::{App, AsyncApp};
use koyo_app_control::ControlError;
use koyo_app_control::ops::browser::{
    BrowserClickInput, BrowserEvalInput, BrowserEvalOutput, BrowserPressKeyInput, BrowserReadInput,
    BrowserReadOutput, BrowserScreenshotInput, BrowserScreenshotOutput, BrowserScrollInput,
    BrowserSnapshotInput, BrowserSnapshotOutput, BrowserTypeInput, ImageFormat, MouseButton,
    ReadFormat,
};
use serde_json::{Value, json};

use super::super::{Caller, Exec, output};
use super::frames::{self, Named, centre, on_element};
use super::keys::Key;
use super::page::{acted, call, later, now, refused, resolve};
use super::pages::Page;
use super::script::{self, evaluate};
use crate::browser::BrowserView;

/// The most characters `browser_read` answers unless told otherwise.
const READ_LIMIT: usize = 50_000;

/// Finds the page, then carries out `act` on it from a task, its answer settled as every
/// call's on the person's page is (`page::later`); `acts` claims it for `caller`.
pub(super) fn on_page<F>(
    caller: &Caller,
    named: Option<u64>,
    acts: bool,
    cx: &mut App,
    act: F,
) -> Result<Exec, ControlError>
where
    F: AsyncFnOnce(Page, Option<Caller>, &mut AsyncApp) -> Result<Value, ControlError> + 'static,
{
    let found = resolve(caller, named, cx)?;
    acted(caller, found.pane(), cx);
    let actor = acts.then(|| caller.clone());
    Ok(later(found, cx, async move |page, cx| {
        act(page.clone(), actor, cx).await
    }))
}

pub(crate) fn read(
    caller: &Caller,
    input: &BrowserReadInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let format = match input.format.unwrap_or(ReadFormat::Text) {
        ReadFormat::Text => "text",
        ReadFormat::Markdown => "markdown",
        ReadFormat::Html => "html",
    };
    let selector = input.selector.clone();
    let root = (selector.as_deref()).map_or_else(|| "document.body".to_owned(), script::find);
    let limit = input.limit.map_or(READ_LIMIT, |limit| limit as usize);
    on_page(caller, input.pane, false, cx, async move |page, _, cx| {
        let expression = script::read(&root, format, limit);
        let value = evaluate(&page, None, &expression, false, cx).await?;
        let Value::String(content) = value else {
            return Err(script::missing(None, selector.as_deref()));
        };
        let truncated = content.chars().count() > limit;
        let content = if truncated {
            content.chars().take(limit).collect()
        } else {
            content
        };
        let at = cx.update(|cx| now(&page, cx))?;
        output(&BrowserReadOutput {
            pane: at.pane,
            url: at.url,
            title: at.title,
            content,
            truncated,
        })
    })
}

pub(crate) fn snapshot(
    caller: &Caller,
    input: &BrowserSnapshotInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let interactive = input.interactive.unwrap_or(false);
    on_page(caller, input.pane, false, cx, async move |page, _, cx| {
        let tree = frames::snapshot(&page, interactive, cx).await?;
        let at = cx.update(|cx| now(&page, cx))?;
        output(&BrowserSnapshotOutput {
            pane: at.pane,
            url: at.url,
            title: at.title,
            tree,
        })
    })
}

/// An image of the page as the pane shows it, or the whole page: the image, base64-encoded.
pub(super) async fn capture(
    page: &Page,
    format: ImageFormat,
    full_page: bool,
    cx: &mut AsyncApp,
) -> Result<String, ControlError> {
    let mut params = json!({
        "format": match format { ImageFormat::Png => "png", ImageFormat::Jpeg => "jpeg" },
    });
    if full_page {
        let metrics = call(page, None, "Page.getLayoutMetrics", json!({}), cx).await?;
        let size = metrics.get("cssContentSize").cloned().unwrap_or_default();
        params["captureBeyondViewport"] = json!(true);
        params["clip"] = json!({
            "x": 0, "y": 0,
            "width": size.get("width").and_then(Value::as_f64).unwrap_or(1280.),
            "height": size.get("height").and_then(Value::as_f64).unwrap_or(800.),
            "scale": 1,
        });
    }
    let mut shot = call(page, None, "Page.captureScreenshot", params, cx).await?;
    let Value::String(data) = shot["data"].take() else {
        return Err(ControlError::browser_unavailable("The page gave no image."));
    };
    Ok(data)
}

pub(crate) fn screenshot(
    caller: &Caller,
    input: &BrowserScreenshotInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let format = input.format.unwrap_or(ImageFormat::Png);
    let full_page = input.full_page.unwrap_or(false);
    on_page(caller, input.pane, false, cx, async move |page, _, cx| {
        let data = capture(&page, format, full_page, cx).await?;
        let at = cx.update(|cx| now(&page, cx))?;
        output(&BrowserScreenshotOutput {
            pane: at.pane,
            url: at.url,
            mime_type: format.mime_type().to_owned(),
            data,
        })
    })
}

pub(crate) fn eval(
    caller: &Caller,
    input: BrowserEvalInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let await_promise = input.await_promise.unwrap_or(true);
    on_page(
        caller,
        input.pane,
        true,
        cx,
        async move |page, actor, cx| {
            let value =
                evaluate(&page, actor.as_ref(), &input.expression, await_promise, cx).await?;
            output(&BrowserEvalOutput { value })
        },
    )
}

pub(crate) fn click(
    caller: &Caller,
    input: BrowserClickInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    on_page(
        caller,
        input.pane,
        true,
        cx,
        async move |page, actor, cx| {
            let actor = actor.as_ref();
            let (x, y) = if let (Some(x), Some(y)) = (input.x, input.y) {
                (x, y)
            } else {
                let named = Named::new(input.element.as_deref(), input.selector.as_deref())?;
                centre(&page, actor, &named, cx).await?
            };
            let button = match input.button.unwrap_or(MouseButton::Left) {
                MouseButton::Left => "left",
                MouseButton::Middle => "middle",
                MouseButton::Right => "right",
            };
            let count = input.click_count.unwrap_or(1).max(1);
            if let Some(caller) = actor {
                let actor = caller.actor();
                let travel = cx.update(|cx| {
                    page.still(cx)?;
                    (page.view()?.update(cx, |view, cx| view.agent_points(&actor, (x, y), cx)))
                        .map_err(refused)
                });
                let travel = travel?;
                if !travel.is_zero() {
                    cx.background_executor().timer(travel).await;
                }
            }
            let scale = cx.update(|cx| Ok::<_, ControlError>(page.view()?.read(cx).input_scale()))?;
            let (x, y) = (x * scale, y * scale);
            mouse(
                &page,
                actor,
                json!({ "type": "mouseMoved", "x": x, "y": y }),
                cx,
            )
            .await?;
            for kind in ["mousePressed", "mouseReleased"] {
                let event =
                    json!({ "type": kind, "x": x, "y": y, "button": button, "clickCount": count });
                mouse(&page, actor, event, cx).await?;
            }
            cx.update(|cx| (page.view()).map(|view| view.update(cx, BrowserView::agent_clicked)))?;
            at_page(&page, cx)
        },
    )
}

/// Where the page is now, as an acting operation answers.
fn at_page(page: &Page, cx: &mut AsyncApp) -> Result<Value, ControlError> {
    output(&cx.update(|cx| now(page, cx))?)
}

async fn mouse(
    page: &Page,
    actor: Option<&Caller>,
    event: Value,
    cx: &mut AsyncApp,
) -> Result<(), ControlError> {
    call(page, actor, "Input.dispatchMouseEvent", event, cx)
        .await
        .map(drop)
}

async fn keys(
    page: &Page,
    actor: Option<&Caller>,
    key: &Key,
    cx: &mut AsyncApp,
) -> Result<(), ControlError> {
    for event in key.events() {
        call(page, actor, "Input.dispatchKeyEvent", event, cx).await?;
    }
    Ok(())
}

pub(crate) fn type_text(
    caller: &Caller,
    input: BrowserTypeInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    on_page(
        caller,
        input.pane,
        true,
        cx,
        async move |page, actor, cx| {
            let actor = actor.as_ref();
            if input.element.is_some() || input.selector.is_some() {
                let named = Named::new(input.element.as_deref(), input.selector.as_deref())?;
                let clear = input.clear.unwrap_or(false);
                let body = format!(
                    "el.focus(); if ({clear}) {{ if ('value' in el) {{ el.value = ''; \
                 el.dispatchEvent(new Event('input', {{ bubbles: true }})); }} \
                 else if (el.isContentEditable) {{ el.textContent = ''; }} }}"
                );
                on_element(&page, actor, &named, &body, cx).await?;
            }
            call(
                &page,
                actor,
                "Input.insertText",
                json!({ "text": input.text }),
                cx,
            )
            .await?;
            if input.submit.unwrap_or(false) {
                keys(&page, actor, &Key::parse("Enter")?, cx).await?;
            }
            at_page(&page, cx)
        },
    )
}

pub(crate) fn press_key(
    caller: &Caller,
    input: &BrowserPressKeyInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let key = Key::parse(&input.key)?;
    on_page(
        caller,
        input.pane,
        true,
        cx,
        async move |page, actor, cx| {
            keys(&page, actor.as_ref(), &key, cx).await?;
            at_page(&page, cx)
        },
    )
}

pub(crate) fn scroll(
    caller: &Caller,
    input: BrowserScrollInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    on_page(
        caller,
        input.pane,
        true,
        cx,
        async move |page, actor, cx| {
            let actor = actor.as_ref();
            if input.element.is_none() && input.selector.is_none() {
                let (dx, dy) = (input.dx.unwrap_or(0.), input.dy.unwrap_or(0.));
                if dx == 0. && dy == 0. {
                    return Err(ControlError::invalid_input(
                        "Say how far to scroll with `dx`/`dy`, or what to scroll to with `ref` or `selector`.",
                    ));
                }
                let by = format!("window.scrollBy({dx}, {dy})");
                evaluate(&page, actor, &by, false, cx).await?;
            } else {
                let named = Named::new(input.element.as_deref(), input.selector.as_deref())?;
                on_element(&page, actor, &named, "", cx).await?;
            }
            at_page(&page, cx)
        },
    )
}
