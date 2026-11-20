//! The sound watcher's renderer side: as a page's main frame makes a script context, a watcher
//! runs in it that hears the page's media elements start and stop playing, and tells the
//! browser process each time the page as a whole starts or stops: an element plays when it's
//! playing, not muted and not at volume 0. It says why it stopped, the event it heard last
//! (`pause`, `waiting` for data, `ended`, `emptied`, `volumechange`, or `pagehide` as the page
//! is left). It runs only on those events, so a page that plays nothing costs nothing.
//!
//! It hears what's in the page's document, by events captured at the window: an element in a
//! shadow root or in no document isn't heard, nor Web Audio.
use super::HEARD;
use cef::{
    CefString, Frame, ImplFrame, ImplListValue, ImplProcessMessage, ImplV8Context, ImplV8Handler,
    ImplV8Value, ProcessId, V8Context, V8Handler, V8Value, WrapV8Handler, process_message_create,
    rc::Rc, v8_value_create_function, wrap_v8_handler,
};
use std::os::raw::c_int;

/// The watcher, a function of the native one it reports through, which stays in its closure
/// so the page never sees it. What it uses it takes as it starts, before the page's own
/// scripts run.
const WATCHER: &str = "(report) => { const Media = HTMLMediaElement, playing = new Set(); \
let said = false; \
const plays = (m) => !m.paused && !m.ended && !m.muted && m.volume > 0; \
const tell = (now, why) => { if (now !== said) { said = now; report(now ? 'plays' : why); } }; \
const heard = (event) => { const m = event.target; if (!(m instanceof Media)) return; \
if ((event.type === 'playing' || event.type === 'volumechange') && plays(m)) playing.add(m); \
else playing.delete(m); tell(playing.size > 0, event.type); }; \
for (const name of ['playing', 'pause', 'waiting', 'ended', 'emptied', 'volumechange']) \
addEventListener(name, heard, { capture: true, passive: true }); \
addEventListener('pagehide', () => { playing.clear(); tell(false, 'pagehide'); }); }";

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
                (said, process_message_create(Some(&CefString::from(HEARD))))
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
    use crate::sight::PLAYS;

    #[test]
    fn the_watcher_says_it_plays_as_the_browser_process_reads_it() {
        let says = format!("report(now ? '{PLAYS}' : why)");
        assert!(WATCHER.contains(&says), "the watcher doesn't say {says}");
    }
}
