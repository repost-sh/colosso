//! The tone watcher's renderer side: as a page's main frame makes a script context, a watcher
//! runs in it that looks at the page's tone ([`koyo_browser_ipc::Tone`]) as the page first
//! shows and after what could change it: the document changing, a scroll ending (the page's or
//! an element's), a transition or an animation ending, the viewport resized, the system's
//! appearance switched, the page shown again. It looks a moment after the first of them, and
//! at most once a second while they keep coming; never while the page is hidden, which the
//! engine makes it out of sight, and at once when it's shown again. It tells the browser
//! process only what differs from what it said last. A page left alone costs nothing.
//!
//! What it uses it takes as it starts, before the page's own scripts run, so a page that wraps
//! timers or observers (as zone.js does) isn't woken by it.
use super::TOLD;
use cef::{
    CefString, Frame, ImplFrame, ImplListValue, ImplProcessMessage, ImplV8Context, ImplV8Handler,
    ImplV8Value, ProcessId, V8Context, V8Handler, V8Value, WrapV8Handler, process_message_create,
    rc::Rc, v8_value_create_function, wrap_v8_handler,
};
use std::os::raw::c_int;

/// The watcher, a function of the native one it reports through, which stays in its closure
/// so the page never sees it. What it says: the relative luminance of the opaque background
/// at each point of a 12 by 8 grid over the viewport, row by row, comma-separated (where no
/// element has one, the body's, which paints the whole canvas when the root has none, else
/// white); then after a semicolon the canvas's colour (the root's, else the body's, else
/// white) as red, green and blue.
const WATCHER: &str = "(report) => { const C = 12, R = 8, QUIET = 100, EVERY = 1000; \
const later = setTimeout, Watch = MutationObserver, now = performance.now.bind(performance); \
const lum = (c) => { if (!c.startsWith('rgb')) return null; const m = c.match(/[\\d.]+/g); \
if (!m || m.length < 3 || (m.length > 3 && +m[3] < 0.5)) return null; \
const f = (v) => { v /= 255; return v <= 0.04045 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4); }; \
return 0.2126 * f(+m[0]) + 0.7152 * f(+m[1]) + 0.0722 * f(+m[2]); }; \
const bg = (el) => { const c = el ? getComputedStyle(el).backgroundColor : ''; \
const m = c.startsWith('rgb') ? c.match(/[\\d.]+/g) : null; \
return m && m.length >= 3 && !(m.length > 3 && +m[3] < 0.5) ? m.slice(0, 3).map((v) => Math.round(+v)).join(' ') : null; }; \
const tone = () => { const w = innerWidth, h = innerHeight, out = []; \
for (let r = 0; r < R; r++) for (let c = 0; c < C; c++) { let v = null; \
for (const el of document.elementsFromPoint((c + 0.5) * w / C, (r + 0.5) * h / R)) { \
v = lum(getComputedStyle(el).backgroundColor); if (v !== null) break; } \
if (v === null && document.body) v = lum(getComputedStyle(document.body).backgroundColor); \
out.push(v === null ? 1 : Math.round(v * 1000) / 1000); } \
return out.join(',') + ';' + (bg(document.documentElement) || bg(document.body) || '255 255 255'); }; \
let said = null, timer = 0, last = -EVERY; \
const look = () => { timer = 0; \
if (document.visibilityState === 'hidden' || (!document.body && document.readyState === 'loading')) return; \
last = now(); const t = tone(); if (t !== said) { said = t; report(t); } }; \
const soon = () => { if (!timer) timer = later(look, Math.max(QUIET, last + EVERY - now())); }; \
new Watch(soon).observe(document, { subtree: true, childList: true, attributes: true }); \
for (const name of ['DOMContentLoaded', 'load', 'pageshow', 'resize', 'visibilitychange']) addEventListener(name, soon); \
for (const name of ['scrollend', 'transitionend', 'animationend']) addEventListener(name, soon, { capture: true, passive: true }); \
matchMedia('(prefers-color-scheme: dark)').addEventListener('change', soon); soon(); }";

/// Runs the watcher in the context `frame` just made, if it's a page's main frame.
pub(crate) fn context_created(frame: &Frame, context: &mut V8Context) {
    if frame.is_main() != 1 {
        return;
    }
    let mut watcher = None;
    let mut exception = None;
    let ran = context.eval(
        Some(&CefString::from(WATCHER)),
        Some(&CefString::from("")),
        0,
        Some(&mut watcher),
        Some(&mut exception),
    );
    let Some(watcher) = watcher.filter(|f| ran == 1 && f.is_function() == 1) else {
        return;
    };
    let mut handler = Report::new(frame.clone());
    let Some(report) =
        v8_value_create_function(Some(&CefString::from("report")), Some(&mut handler))
    else {
        return;
    };
    let _ = watcher.execute_function_with_context(Some(context), None, Some(&[Some(report)]));
}

wrap_v8_handler! {
    struct Report {
        frame: Frame,
    }

    impl V8Handler {
        fn execute(
            &self,
            _name: Option<&CefString>,
            _object: Option<&mut V8Value>,
            arguments: Option<&[Option<V8Value>]>,
            _retval: Option<&mut Option<V8Value>>,
            _exception: Option<&mut CefString>,
        ) -> c_int {
            let said = arguments
                .and_then(|args| args.first()?.as_ref())
                .filter(|value| value.is_string() == 1)
                .map(|value| CefString::from(&value.string_value()));
            let (Some(said), Some(mut message)) =
                (said, process_message_create(Some(&CefString::from(TOLD))))
            else {
                return 1;
            };
            if let Some(args) = message.argument_list() {
                args.set_string(0, Some(&said));
            }
            if self.frame.is_valid() == 1 {
                self.frame.send_process_message(ProcessId::BROWSER, Some(&mut message));
            }
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WATCHER;
    use koyo_browser_ipc::Tone;

    #[test]
    fn the_watcher_looks_at_the_grid_the_app_reads() {
        let grid = format!("const C = {}, R = {},", Tone::ACROSS, Tone::DOWN);
        assert!(WATCHER.contains(&grid), "the watcher's grid isn't {grid}");
    }
}
