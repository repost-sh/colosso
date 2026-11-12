//! JavaScript run in a page: evaluating an expression for its value, finding an element by
//! a snapshot's ref or a selector, the page as text, Markdown or HTML, and the snapshot that
//! marks elements with refs.

use gpui_kit::AsyncApp;
use koyo_app_control::ControlError;
use serde_json::{Value, json};

use super::super::Caller;
use super::page::{CALL_LIMIT, PROMISE_LIMIT, Page, call_within};

/// The attribute a snapshot marks elements with, `eN`.
const REF_ATTRIBUTE: &str = "data-koyo-ref";

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
    let answer = call_within(page, caller, "Runtime.evaluate", params, limit, cx).await?;
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

/// An expression for the element `element` (a snapshot's ref) or `selector` names, or `null`.
pub(super) fn element(
    element: Option<&str>,
    selector: Option<&str>,
) -> Result<String, ControlError> {
    match (element, selector) {
        (Some(reference), _) => Ok(format!(
            "document.querySelector({})",
            quote(&format!("[{REF_ATTRIBUTE}=\"{reference}\"]"))
        )),
        (None, Some(selector)) => Ok(format!("document.querySelector({})", quote(selector))),
        (None, None) => Err(ControlError::invalid_input(
            "Name the element with `ref` (from browser_snapshot) or `selector`.",
        )),
    }
}

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

/// An expression for the page's tree of meaningful elements as text, each line a role, a
/// name and, for what can be clicked, typed into or scrolled to, `[ref=eN]`; only those with
/// `interactive`. Marks the elements for `element` to find again.
pub(super) fn snapshot(interactive: bool) -> String {
    format!(
        "(() => {{ const ONLY = {interactive}; const ATTR = {attr}; {SNAPSHOT} }})()",
        attr = quote(REF_ATTRIBUTE)
    )
}

const SNAPSHOT: &str = r"
document.querySelectorAll('[' + ATTR + ']').forEach((el) => el.removeAttribute(ATTR));
let next = 0;
const lines = [];
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
      range: 'slider', search: 'searchbox' }[type] || 'textbox';
  }
  if (tag === 'a' && !el.hasAttribute('href')) return null;
  return ROLES[tag] || null;
};
const actionable = (el, role) => ['link', 'button', 'textbox', 'searchbox', 'combobox', 'checkbox', 'radio',
  'slider', 'tab', 'menuitem', 'option', 'switch'].includes(role) || el.isContentEditable
  || el.hasAttribute('onclick') || el.tabIndex >= 0 && el.hasAttribute('tabindex');
const visible = (el) => {
  const box = el.getBoundingClientRect();
  if (box.width === 0 && box.height === 0) return false;
  const style = getComputedStyle(el);
  return style.visibility !== 'hidden' && style.display !== 'none';
};
const nameOf = (el) => {
  const label = el.getAttribute('aria-label') || el.getAttribute('alt') || el.getAttribute('title')
    || (el.labels && el.labels[0] && el.labels[0].innerText) || el.getAttribute('placeholder');
  const text = label || (['INPUT', 'SELECT', 'TEXTAREA'].includes(el.tagName) ? '' : el.innerText) || '';
  return text.replace(/\s+/g, ' ').trim().slice(0, 100);
};
const walk = (el, depth) => {
  if (!(el instanceof Element) || ['SCRIPT', 'STYLE', 'NOSCRIPT', 'TEMPLATE'].includes(el.tagName)) return;
  if (!visible(el) && el !== document.body) return;
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
    if (acts) { const ref = 'e' + (++next); el.setAttribute(ATTR, ref); line += ' [ref=' + ref + ']'; }
    lines.push(line);
    shown = true;
    if (acts && role !== 'list' && !['form', 'dialog'].includes(role)) return;
  }
  for (const child of el.children) walk(child, shown ? depth + 1 : depth);
  if (!ONLY && !role && el.children.length === 0) {
    const text = (el.innerText || '').replace(/\s+/g, ' ').trim();
    if (text) lines.push('  '.repeat(depth) + '- text ' + JSON.stringify(text.slice(0, 200)));
  }
};
walk(document.body, 0);
return lines.join('\n');
";
