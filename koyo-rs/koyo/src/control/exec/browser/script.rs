//! JavaScript run in a page: evaluating an expression for its value, finding an element by
//! a selector through open shadow roots and same-origin frames, the page as text, Markdown or
//! HTML, and the snapshot that marks elements with refs, and the frames of other sites, whose
//! own snapshots `frames.rs` fetches in their sessions.

use gpui_kit::AsyncApp;
use koyo_app_control::ControlError;
use serde_json::{Value, json};

use super::super::Caller;
use super::page::{CALL_LIMIT, PROMISE_LIMIT, call_within};
use super::pages::Page;

/// The attribute a snapshot marks elements with, `eN`, or `f1eN` in the frame marked `f1`.
pub(super) const REF_ATTRIBUTE: &str = "data-koyo-ref";
/// The attribute a snapshot marks a frame of another site with, `fN`, or `f1fN` in `f1`.
pub(super) const FRAME_ATTRIBUTE: &str = "data-koyo-frame";

/// `text` as a JavaScript string literal.
pub(super) fn quote(text: &str) -> String {
    Value::String(text.to_owned()).to_string()
}

/// Evaluates `expression` in the page, under its lease for `caller` when it acts: its value
/// as JSON, or the exception it threw as `invalid_input`.
pub(super) async fn evaluate(
    page: &Page,
    caller: Option<&Caller>,
    expression: &str,
    await_promise: bool,
    cx: &mut AsyncApp,
) -> Result<Value, ControlError> {
    evaluate_in(page, caller, None, expression, await_promise, cx).await
}

/// As [`evaluate`], in the frame of another site whose session is `session`.
pub(super) async fn evaluate_in(
    page: &Page,
    caller: Option<&Caller>,
    session: Option<&str>,
    expression: &str,
    await_promise: bool,
    cx: &mut AsyncApp,
) -> Result<Value, ControlError> {
    let params = json!({
        "expression": expression,
        "returnByValue": true,
        "awaitPromise": await_promise,
        "userGesture": caller.is_some(),
    });
    let limit = if await_promise {
        PROMISE_LIMIT
    } else {
        CALL_LIMIT
    };
    let answer = call_within(page, caller, (session, "Runtime.evaluate"), params, limit, cx).await?;
    if let Some(exception) = answer.get("exceptionDetails") {
        let text = (exception.pointer("/exception/description"))
            .or_else(|| exception.get("text"))
            .and_then(Value::as_str)
            .unwrap_or("The expression threw.");
        return Err(ControlError::invalid_input(format!(
            "The page said: {text}"
        )));
    }
    Ok(answer
        .pointer("/result/value")
        .cloned()
        .unwrap_or(Value::Null))
}

/// Finds what a selector matches in the document, its open shadow roots and its same-origin
/// frames, theirs too, the document's first: `deep(selector)`, the first match or `null`.
const DEEP: &str = r"
const frameDoc = (el) => {
  if (el.tagName !== 'IFRAME' && el.tagName !== 'FRAME') return null;
  try { return el.contentDocument; } catch (e) { return null; }
};
const roots = (root, out) => {
  out.push(root);
  for (const el of root.querySelectorAll('*')) {
    if (el.shadowRoot) roots(el.shadowRoot, out);
    const doc = frameDoc(el);
    if (doc) roots(doc, out);
  }
  return out;
};
const deep = (selector) => {
  for (const root of roots(document, [])) { const found = root.querySelector(selector); if (found) return found; }
  return null;
};
const styleOf = (el) => el.ownerDocument.defaultView.getComputedStyle(el);
";

/// An expression for the first element `selector` matches (`DEEP`), or `null`.
pub(super) fn find(selector: &str) -> String {
    format!("(() => {{ {DEEP} return deep({}); }})()", quote(selector))
}

/// An expression running `body` on the first element `selector` matches, as `el`, with
/// `DEEP`'s helpers at hand, if there is one: `null` if not.
pub(super) fn with_element(selector: &str, body: &str) -> String {
    format!(
        "(() => {{ {DEEP} const el = deep({}); if (!el) return null; {body} }})()",
        quote(selector)
    )
}

/// What adds the offset of each same-origin frame `el` is in to a point of its frame's: as far
/// as the frame of another site it may be in, whose own offset `frames.rs` adds.
pub(super) const ORIGIN: &str = r"
const origin = (el) => {
  let x = 0, y = 0, win = el.ownerDocument.defaultView;
  while (win && win.frameElement) {
    const frame = win.frameElement, box = frame.getBoundingClientRect(), style = styleOf(frame);
    x += box.left + frame.clientLeft + parseFloat(style.paddingLeft);
    y += box.top + frame.clientTop + parseFloat(style.paddingTop);
    win = win.parent;
  }
  return [x, y];
};
";

/// Why an element wasn't found, said to the agent.
pub(super) fn missing(element: Option<&str>, selector: Option<&str>) -> ControlError {
    match element {
        Some(reference) => ControlError::not_found(format!(
            "No element has ref {reference}; take a new browser_snapshot, as refs end with a \
             navigation or the next snapshot."
        )),
        None => ControlError::not_found(format!(
            "No element matches {}.",
            selector.unwrap_or_default()
        )),
    }
}

/// An expression for the page, or the element `root` is, as text, Markdown or HTML, cut a
/// little past `limit` characters so a large page isn't sent whole: past twice as many UTF-16
/// units, which hold at least one character more.
pub(super) fn read(root: &str, format: &str, limit: usize) -> String {
    let cut = limit.saturating_mul(2).saturating_add(2);
    format!(
        "(() => {{ const root = {root}; if (!root) return null; {MARKDOWN} \
             const all = {format:?} === 'html' ? root.outerHTML \
             : {format:?} === 'markdown' ? md(root).replace(/\\n{{3,}}/g, '\\n\\n').trim() \
             : root.innerText; \
             return all.slice(0, {cut}); }})()"
    )
}

/// A small converter from the DOM to Markdown: headings, paragraphs, links, emphasis, lists,
/// code, quotes and images; the rest as its text.
const MARKDOWN: &str = r"
const md = (node) => {
  if (node.nodeType === 3) return node.textContent.replace(/\s+/g, ' ');
  if (node.nodeType !== 1) return '';
  const tag = node.tagName.toLowerCase();
  if (['script', 'style', 'noscript', 'template', 'svg'].includes(tag)) return '';
  const style = getComputedStyle(node);
  if (style.display === 'none' || style.visibility === 'hidden') return '';
  const inner = () => Array.from(node.childNodes).map(md).join('');
  switch (tag) {
    case 'h1': case 'h2': case 'h3': case 'h4': case 'h5': case 'h6':
      return '\n\n' + '#'.repeat(+tag[1]) + ' ' + inner().trim() + '\n\n';
    case 'p': case 'div': case 'section': case 'article': case 'main': case 'header': case 'footer': case 'nav':
      return '\n\n' + inner().trim() + '\n\n';
    case 'br': return '\n';
    case 'hr': return '\n\n---\n\n';
    case 'a': { const text = inner().trim(); return node.href ? '[' + text + '](' + node.href + ')' : text; }
    case 'strong': case 'b': return '**' + inner().trim() + '**';
    case 'em': case 'i': return '*' + inner().trim() + '*';
    case 'code': return node.closest('pre') ? node.textContent : '`' + node.textContent + '`';
    case 'pre': return '\n\n```\n' + node.textContent.replace(/\n$/, '') + '\n```\n\n';
    case 'blockquote': return '\n\n' + inner().trim().split('\n').map((line) => '> ' + line).join('\n') + '\n\n';
    case 'img': return node.alt ? '![' + node.alt + '](' + node.src + ')' : '';
    case 'li': {
      const ordered = node.parentElement && node.parentElement.tagName === 'OL';
      const mark = ordered ? (Array.from(node.parentElement.children).indexOf(node) + 1) + '. ' : '- ';
      return '\n' + mark + inner().trim();
    }
    case 'ul': case 'ol': return '\n' + inner() + '\n';
    case 'tr': return '\n| ' + Array.from(node.children).map((cell) => md(cell).trim()).join(' | ') + ' |';
    case 'table': return '\n\n' + inner() + '\n\n';
    default: return inner();
  }
};
";

/// An expression for the tree of meaningful elements of the frame it runs in as text, through
/// open shadow roots and same-origin frames, each line a role, a name and, for what can be
/// clicked, typed into or scrolled to, `[ref=<mark>eN]`; only those with `interactive`. A frame
/// of another site is a line `[frame=<mark>fN]`, its own tree `frames.rs`'s to fetch. The tree
/// starts `depth` deep. Marks the elements and the frames, all others' marks gone, for
/// `find` to find again. Its value: `{ tree, frames }`, the frames' marks.
pub(super) fn snapshot(interactive: bool, mark: &str, depth: usize) -> String {
    format!(
        "(() => {{ const ONLY = {interactive}; const ATTR = {attr}; const FRAME = {frame}; \
         const MARK = {mark}; const DEPTH = {depth}; {DEEP} {SNAPSHOT} }})()",
        attr = quote(REF_ATTRIBUTE),
        frame = quote(FRAME_ATTRIBUTE),
        mark = quote(mark),
    )
}

const SNAPSHOT: &str = r"
for (const root of roots(document, [])) {
  root.querySelectorAll('[' + ATTR + '],[' + FRAME + ']').forEach((el) => { el.removeAttribute(ATTR); el.removeAttribute(FRAME); });
}
let next = 0;
const lines = [], frames = [];
const ROLES = { a: 'link', button: 'button', select: 'combobox', textarea: 'textbox', h1: 'heading', h2: 'heading',
  h3: 'heading', h4: 'heading', h5: 'heading', h6: 'heading', img: 'img', nav: 'navigation', main: 'main',
  header: 'banner', footer: 'contentinfo', form: 'form', ul: 'list', ol: 'list', li: 'listitem', table: 'table',
  tr: 'row', td: 'cell', th: 'columnheader', dialog: 'dialog', label: 'label', summary: 'button', p: 'paragraph' };
const roleOf = (el) => {
  const explicit = el.getAttribute('role');
  if (explicit) return explicit;
  const tag = el.tagName.toLowerCase();
  if (tag === 'input') {
    const type = (el.getAttribute('type') || 'text').toLowerCase();
    return { checkbox: 'checkbox', radio: 'radio', button: 'button', submit: 'button', reset: 'button',
      file: 'button', range: 'slider', search: 'searchbox' }[type] || 'textbox';
  }
  if (tag === 'a' && !el.hasAttribute('href')) return null;
  return ROLES[tag] || null;
};
const actionable = (el, role) => ['link', 'button', 'textbox', 'searchbox', 'combobox', 'checkbox', 'radio',
  'slider', 'tab', 'menuitem', 'option', 'switch'].includes(role) || el.isContentEditable
  || el.hasAttribute('onclick') || el.tabIndex >= 0 && el.hasAttribute('tabindex');
const visible = (el) => {
  const style = styleOf(el);
  if (style.display === 'contents') return true;
  const box = el.getBoundingClientRect();
  if (box.width === 0 && box.height === 0) return false;
  return style.visibility !== 'hidden' && style.display !== 'none';
};
const kids = (el) => {
  if (el.shadowRoot) return Array.from(el.shadowRoot.children);
  if (el.tagName === 'SLOT') { const slotted = el.assignedElements(); if (slotted.length) return slotted; }
  return Array.from(el.children);
};
const frame = (el, depth) => {
  const title = el.getAttribute('title') || el.getAttribute('name') || '';
  const line = '  '.repeat(depth) + '- iframe' + (title ? ' ' + JSON.stringify(title.slice(0, 100)) : '');
  const doc = frameDoc(el);
  if (doc) {
    lines.push(line);
    if (doc.body) walk(doc.body, depth + 1);
    return;
  }
  const mark = MARK + 'f' + (frames.length + 1);
  el.setAttribute(FRAME, mark);
  frames.push(mark);
  lines.push(line + ' [frame=' + mark + ']');
};
const nameOf = (el) => {
  const label = el.getAttribute('aria-label') || el.getAttribute('alt') || el.getAttribute('title')
    || (el.labels && el.labels[0] && el.labels[0].innerText) || el.getAttribute('placeholder');
  const text = label || (['INPUT', 'SELECT', 'TEXTAREA'].includes(el.tagName) ? '' : el.innerText) || '';
  return text.replace(/\s+/g, ' ').trim().slice(0, 100);
};
const walk = (el, depth) => {
  if (el.nodeType !== 1 || ['SCRIPT', 'STYLE', 'NOSCRIPT', 'TEMPLATE'].includes(el.tagName)) return;
  if (!visible(el) && el !== el.ownerDocument.body) return;
  if (el.tagName === 'IFRAME' || el.tagName === 'FRAME') return frame(el, depth);
  const role = roleOf(el);
  const acts = role ? actionable(el, role) : false;
  let shown = false;
  if (role && (!ONLY || acts)) {
    let line = '  '.repeat(depth) + '- ' + role;
    const name = nameOf(el);
    if (name) line += ' ' + JSON.stringify(name);
    if (role === 'heading') line += ' [level=' + el.tagName[1] + ']';
    if ('value' in el && el.value && role !== 'button') line += ' value=' + JSON.stringify(String(el.value).slice(0, 100));
    if (el.checked) line += ' [checked]';
    if (el.disabled) line += ' [disabled]';
    if (acts) { const ref = MARK + 'e' + (++next); el.setAttribute(ATTR, ref); line += ' [ref=' + ref + ']'; }
    lines.push(line);
    shown = true;
    if (acts && role !== 'list' && !['form', 'dialog'].includes(role)) return;
  }
  const children = kids(el);
  for (const child of children) walk(child, shown ? depth + 1 : depth);
  if (!ONLY && !role && children.length === 0) {
    const text = (el.innerText || '').replace(/\s+/g, ' ').trim();
    if (text) lines.push('  '.repeat(depth) + '- text ' + JSON.stringify(text.slice(0, 200)));
  }
};
if (document.body) walk(document.body, DEPTH);
return { tree: lines.join('\n'), frames };
";
