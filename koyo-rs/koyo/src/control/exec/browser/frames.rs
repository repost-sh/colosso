//! The frames of other sites a page holds, which run apart from it, each in a session of its
//! own attached under the page's (`Target.setAutoAttach`, as `DevTools` and Puppeteer reach
//! them; `browser/agent/heard.rs` keeps which there are). A snapshot runs in each and puts its
//! tree under the frame's line, its refs and frames marked as the frame is (`f1e3` in the frame
//! marked `f1`); an act on a ref there runs in that frame's session, its point moved by where
//! each frame that holds it starts.

use std::collections::{HashMap, VecDeque};

use gpui_kit::AsyncApp;
use koyo_app_control::ControlError;
use serde_json::{Value, json};

use super::super::Caller;
use super::heard::heard;
use super::page::{CALL_LIMIT, call_within};
use super::pages::Page;
use super::script::{self, FRAME_ATTRIBUTE, ORIGIN, REF_ATTRIBUTE, evaluate_in};

/// The most frames of other sites a snapshot goes into.
const MOST_FRAMES: usize = 20;

/// An element an agent names: what finds it, and the marks of the frames of other sites it's
/// in, outermost first.
pub(super) struct Named<'a> {
    element: Option<&'a str>,
    selector: Option<&'a str>,
    css: String,
    frames: Vec<String>,
}

impl<'a> Named<'a> {
    /// The element a snapshot's ref, `element`, or else `selector` names.
    pub(super) fn new(
        element: Option<&'a str>,
        selector: Option<&'a str>,
    ) -> Result<Self, ControlError> {
        let (css, frames) = match (element, selector) {
            (Some(reference), _) => {
                let frames = frames_of(reference).ok_or_else(|| script::missing(element, None))?;
                (format!("[{REF_ATTRIBUTE}=\"{reference}\"]"), frames)
            }
            (None, Some(selector)) => (selector.to_owned(), Vec::new()),
            (None, None) => {
                return Err(ControlError::invalid_input(
                    "Name the element with `ref` (from browser_snapshot) or `selector`.",
                ));
            }
        };
        Ok(Self {
            element,
            selector,
            css,
            frames,
        })
    }

    /// Why it wasn't found, said to the agent.
    pub(super) fn missing(&self) -> ControlError {
        script::missing(self.element, self.selector)
    }

    /// The session of the frame it's in, and of each frame that holds that one, outermost
    /// first: none for the page's own frames.
    pub(super) fn sessions(
        &self,
        page: &Page,
        cx: &mut AsyncApp,
    ) -> Result<Vec<String>, ControlError> {
        let found = heard(page, cx, |heard| {
            (self.frames.iter())
                .map(|mark| Some(heard.frame(heard.marked_as(mark)?)?.session.clone()))
                .collect::<Option<Vec<_>>>()
        })?;
        found.ok_or_else(|| self.missing())
    }
}

/// The marks of the frames a ref is in, outermost first: `f1f2e3` is in the frame `f1f2` of
/// the frame `f1`. `None` for what's no ref.
fn frames_of(reference: &str) -> Option<Vec<String>> {
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    let (path, element) = reference.rsplit_once('e')?;
    if !digits(element) {
        return None;
    }
    let mut frames = Vec::new();
    let mut rest = path;
    while !rest.is_empty() {
        let after = rest.strip_prefix('f')?;
        let end = after.find('f').unwrap_or(after.len());
        if !digits(&after[..end]) {
            return None;
        }
        rest = &after[end..];
        frames.push(path[..path.len() - rest.len()].to_owned());
    }
    Some(frames)
}

/// Runs `body` on the element `named` names, as `el`, in its frame, after scrolling it into
/// view; refused when there's no such element.
pub(super) async fn on_element(
    page: &Page,
    actor: Option<&Caller>,
    named: &Named<'_>,
    body: &str,
    cx: &mut AsyncApp,
) -> Result<(), ControlError> {
    let sessions = named.sessions(page, cx)?;
    let body = format!("el.scrollIntoView({{ block: 'center' }}); {body} return true;");
    let expression = script::with_element(&named.css, &body);
    let session = sessions.last().map(String::as_str);
    if evaluate_in(page, actor, session, &expression, false, cx).await? == Value::Bool(true) {
        Ok(())
    } else {
        Err(named.missing())
    }
}

/// Scrolls the element `named` names into view and answers its centre in the page's
/// viewport: its own frame's point, moved by where each frame that holds it starts.
pub(super) async fn centre(
    page: &Page,
    actor: Option<&Caller>,
    named: &Named<'_>,
    cx: &mut AsyncApp,
) -> Result<(f64, f64), ControlError> {
    let sessions = named.sessions(page, cx)?;
    let body = format!(
        "el.scrollIntoView({{ block: 'center', inline: 'center' }}); {ORIGIN} \
         const box = el.getBoundingClientRect(), [x, y] = origin(el); \
         return [x + box.left + box.width / 2, y + box.top + box.height / 2];"
    );
    let session = sessions.last().map(String::as_str);
    let expression = script::with_element(&named.css, &body);
    let found = evaluate_in(page, actor, session, &expression, false, cx).await?;
    let (mut x, mut y) = pair(&found).ok_or_else(|| named.missing())?;
    let starts = format!(
        "{ORIGIN} const box = el.getBoundingClientRect(), style = styleOf(el), [x, y] = origin(el); \
         return [x + box.left + el.clientLeft + parseFloat(style.paddingLeft), \
         y + box.top + el.clientTop + parseFloat(style.paddingTop)];"
    );
    for (at, mark) in named.frames.iter().enumerate().rev() {
        let holder = at.checked_sub(1).map(|up| sessions[up].as_str());
        let owner = script::with_element(&format!("[{FRAME_ATTRIBUTE}=\"{mark}\"]"), &starts);
        let start = evaluate_in(page, actor, holder, &owner, false, cx).await?;
        let (dx, dy) = pair(&start).ok_or_else(|| named.missing())?;
        (x, y) = (x + dx, y + dy);
    }
    Ok((x, y))
}

/// Sets `files`, absolute paths, on the `<input type=file>` `named` names, in its frame, as if
/// chosen there (`DOM.setFileInputFiles`): its `input` and `change` follow. Refused when
/// there's no such element, or it's no file input.
pub(super) async fn set_files(
    page: &Page,
    actor: Option<&Caller>,
    named: &Named<'_>,
    files: Vec<String>,
    cx: &mut AsyncApp,
) -> Result<(), ControlError> {
    let sessions = named.sessions(page, cx)?;
    let session = sessions.last().map(String::as_str);
    let body = "return el.tagName === 'INPUT' && el.type === 'file' ? el : 'other';";
    let params =
        json!({ "expression": script::with_element(&named.css, body), "returnByValue": false });
    let found = call_within(
        page,
        actor,
        (session, "Runtime.evaluate"),
        params,
        CALL_LIMIT,
        cx,
    )
    .await?;
    let Some(object) = found.pointer("/result/objectId").and_then(Value::as_str) else {
        return Err(match found.pointer("/result/value") {
            Some(Value::String(_)) => ControlError::invalid_input(
                "That element is no `<input type=file>`; browser_upload sets files only on one.",
            ),
            _ => named.missing(),
        });
    };
    let params = json!({ "files": files, "objectId": object });
    call_within(
        page,
        actor,
        (session, "DOM.setFileInputFiles"),
        params,
        CALL_LIMIT,
        cx,
    )
    .await?;
    Ok(())
}

/// `[x, y]` as numbers.
fn pair(value: &Value) -> Option<(f64, f64)> {
    match value.as_array().map(Vec::as_slice) {
        Some([x, y]) => Some((x.as_f64()?, y.as_f64()?)),
        _ => None,
    }
}

/// The page's snapshot (`script::snapshot`), and in it, under each frame of another site's
/// line, that frame's, as far as [`MOST_FRAMES`] frames. A frame that can't be reached keeps
/// only its line. The frames marked are kept for the acts on their refs.
pub(super) async fn snapshot(
    page: &Page,
    interactive: bool,
    cx: &mut AsyncApp,
) -> Result<String, ControlError> {
    attach(page, None, cx).await;
    let root = frame_snapshot(page, None, ("", 0), interactive, cx).await?;
    let mut lines: Vec<String> = root.tree.lines().map(str::to_owned).collect();
    let mut pending: VecDeque<(Option<String>, String)> =
        (root.frames.into_iter()).map(|mark| (None, mark)).collect();
    let mut owners: HashMap<Option<String>, HashMap<String, String>> = HashMap::new();
    let mut marked = HashMap::new();
    while let Some((holder, mark)) = pending.pop_front() {
        if marked.len() >= MOST_FRAMES {
            break;
        }
        if !owners.contains_key(&holder) {
            let found = owned(page, holder.as_deref(), cx).await?;
            owners.insert(holder.clone(), found);
        }
        let Some(target) = owners
            .get(&holder)
            .and_then(|owned| owned.get(&mark))
            .cloned()
        else {
            continue;
        };
        let session = heard(page, cx, |heard| {
            heard.frame(&target).map(|frame| frame.session.clone())
        })?;
        let line = format!("[frame={mark}]");
        let (Some(session), Some(at)) =
            (session, lines.iter().position(|said| said.ends_with(&line)))
        else {
            continue;
        };
        let depth = lines[at].len().saturating_sub(lines[at].trim_start().len()) / 2 + 1;
        attach(page, Some(&session), cx).await;
        let Ok(inner) = frame_snapshot(page, Some(&session), (&mark, depth), interactive, cx).await
        else {
            continue;
        };
        let under = at + 1;
        lines.splice(under..under, inner.tree.lines().map(str::to_owned));
        pending.extend((inner.frames.into_iter()).map(|inner| (Some(session.clone()), inner)));
        marked.insert(mark, target);
    }
    heard(page, cx, |heard| heard.marked(marked))?;
    Ok(lines.join("\n"))
}

/// One frame's snapshot: its tree, and the marks of its frames of other sites.
#[derive(Default)]
struct FrameSnapshot {
    tree: String,
    frames: Vec<String>,
}

/// The snapshot of the frame in `session` (none: the page's own), marked `mark`, its tree
/// starting `depth` deep.
async fn frame_snapshot(
    page: &Page,
    session: Option<&str>,
    (mark, depth): (&str, usize),
    interactive: bool,
    cx: &mut AsyncApp,
) -> Result<FrameSnapshot, ControlError> {
    let expression = script::snapshot(interactive, mark, depth);
    let value = evaluate_in(page, None, session, &expression, false, cx).await?;
    Ok(FrameSnapshot {
        tree: value["tree"].as_str().unwrap_or_default().to_owned(),
        frames: (value["frames"].as_array().into_iter().flatten())
            .filter_map(|mark| mark.as_str().map(str::to_owned))
            .collect(),
    })
}

/// Has the frames of other sites under `session` (none: the page's own) attached, each in a
/// session of its own, which the engine says as it does (`Target.attachedToTarget`), before
/// it answers. One that can't be is no frame a snapshot reaches.
async fn attach(page: &Page, session: Option<&str>, cx: &mut AsyncApp) {
    let params = json!({
        "autoAttach": true,
        "waitForDebuggerOnStart": false,
        "flatten": true,
        "filter": [{ "type": "iframe" }],
    });
    if let Err(why) = call_within(
        page,
        None,
        (session, "Target.setAutoAttach"),
        params,
        CALL_LIMIT,
        cx,
    )
    .await
    {
        log::debug!("the frames of another site aren't reached: {}", why.message);
    }
}

/// The marks of the frames attached under `session` (none: the page's own), as the last
/// snapshot there put them on the frame elements that hold them: each one's target.
async fn owned(
    page: &Page,
    session: Option<&str>,
    cx: &mut AsyncApp,
) -> Result<HashMap<String, String>, ControlError> {
    let targets = heard(page, cx, |heard| heard.frames_under(session))?;
    let mut marks = HashMap::new();
    for target in targets {
        let asked = ("DOM.getFrameOwner", json!({ "frameId": target }));
        let Ok(owner) = call_within(page, None, (session, asked.0), asked.1, CALL_LIMIT, cx).await
        else {
            continue;
        };
        let described = json!({ "backendNodeId": owner["backendNodeId"] });
        let Ok(node) = call_within(
            page,
            None,
            (session, "DOM.describeNode"),
            described,
            CALL_LIMIT,
            cx,
        )
        .await
        else {
            continue;
        };
        let attributes = node.pointer("/node/attributes").and_then(Value::as_array);
        let mut pairs = attributes.into_iter().flatten().filter_map(Value::as_str);
        while let Some(name) = pairs.next() {
            let value = pairs.next();
            if name == FRAME_ATTRIBUTE
                && let Some(mark) = value
            {
                marks.insert(mark.to_owned(), target.clone());
            }
        }
    }
    Ok(marks)
}

#[cfg(test)]
mod tests {
    use super::frames_of;

    #[test]
    fn a_ref_says_the_frames_it_is_in_outermost_first() {
        assert_eq!(frames_of("e12"), Some(vec![]));
        assert_eq!(frames_of("f1e3"), Some(vec!["f1".to_owned()]));
        assert_eq!(
            frames_of("f1f2e3"),
            Some(vec!["f1".to_owned(), "f1f2".to_owned()])
        );
        for not in ["", "e", "x1", "f1", "fe3", "f1xe3", "e3f1", "f12e", "1e2"] {
            assert_eq!(frames_of(not), None, "{not}");
        }
    }
}
