//! The scroll watcher's renderer side: as a page's main frame makes a script context, a
//! watcher runs in it that tells the browser process where the page is each time a scroll of
//! it ends, and once it's shown (`pageshow`, after Blink has put a page gone back to where it
//! was), so the app keeps it with the window. Only the page's own scroll, as browsers
//! bring back: what a page scrolls inside it is the page's to keep.
use super::SCROLLED;
use cef::{
    CefString, Frame, ImplFrame, ImplListValue, ImplProcessMessage, ImplV8Context, ImplV8Handler,
    ImplV8Value, ProcessId, V8Context, V8Handler, V8Value, WrapV8Handler, process_message_create,
    rc::Rc, v8_value_create_function, wrap_v8_handler,
};
use std::os::raw::c_int;

/// The watcher, a function of the native one it reports through, which stays in its closure
/// so the page never sees it.
const WATCHER: &str = "(report) => { const said = () => report(scrollX, scrollY); \
    addEventListener('scrollend', said, { passive: true }); \
    addEventListener('pageshow', () => setTimeout(said, 0)); }";

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

/// A number argument, in whole pixels; 0 for anything else.
fn pixels(arguments: Option<&[Option<V8Value>]>, at: usize) -> c_int {
    let value = arguments
        .and_then(|args| args.get(at)?.as_ref())
        .filter(|value| value.is_double() == 1 || value.is_int() == 1)
        .map_or(0., ImplV8Value::double_value);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "clamped to the range of an i32 first"
    )]
    let value = value
        .round()
        .clamp(f64::from(c_int::MIN), f64::from(c_int::MAX)) as c_int;
    value
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
            let Some(mut message) = process_message_create(Some(&CefString::from(SCROLLED)))
            else {
                return 1;
            };
            if let Some(args) = message.argument_list() {
                args.set_int(0, pixels(arguments, 0));
                args.set_int(1, pixels(arguments, 1));
            }
            if self.frame.is_valid() == 1 {
                self.frame.send_process_message(ProcessId::BROWSER, Some(&mut message));
            }
            1
        }
    }
}
