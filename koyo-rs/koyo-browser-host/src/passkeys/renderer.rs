//! The bridge's renderer side, in each renderer process: as a frame's script context is
//! made, the bridge (`bridge.js`) is run in it with a native function to send through, and
//! what it hands back is kept to answer through. The engine adds the interfaces the bridge
//! takes the place of only after that, so it's put in as the frame's document starts to
//! load, before the page's first script ([`load_started`]). Requests go to the browser
//! process as process messages, which CEF delivers with the frame they came from, so the
//! browser process knows the frame's origin from the engine rather than from the page.
//!
//! Each context keeps the numbers of its requests waiting on an answer; a context that goes
//! (the frame navigated or closed) withdraws them, so the app takes their sheets down.
use super::{said, send, text};
use cef::{
    CefString, Frame, ImplFrame, ImplV8Context, ImplV8Handler, ImplV8Value, ProcessId,
    ProcessMessage, V8Context, V8Handler, V8Value, WrapV8Handler, rc::Rc, v8_value_create_double,
    v8_value_create_function, v8_value_create_string, wrap_v8_handler,
};
use std::cell::RefCell;
use std::collections::HashSet;
use std::os::raw::c_int;

/// A context the bridge runs in.
struct Bridged {
    /// The renderer's number for the context.
    key: u64,
    context: V8Context,
    frame: Frame,
    /// The bridge's function the browser process answers through.
    receive: V8Value,
    /// The bridge's function that puts it in the engine's place.
    install: V8Value,
    /// Requests sent and not answered yet.
    waiting: HashSet<u64>,
}

#[derive(Default)]
struct Bridges {
    contexts: Vec<Bridged>,
    /// The last number given to a context or a request; one count for both, so a request's
    /// number is unique in the process.
    last: u64,
}

impl Bridges {
    fn next(&mut self) -> u64 {
        self.last += 1;
        self.last
    }
}

thread_local! {
    // The renderer's main thread, where every V8 callback and process message comes.
    static BRIDGES: RefCell<Bridges> = RefCell::new(Bridges::default());
    /// The bridge, a function of the native one it sends through: made once a renderer, as
    /// every context runs it.
    static BRIDGE: CefString = CefString::from(include_str!("bridge.js"));
}

/// Runs the bridge in the context `frame` just made. Contexts without passkeys (not secure,
/// or no `PublicKeyCredential`) are left as they are.
pub(crate) fn context_created(frame: &Frame, context: &mut V8Context) {
    let mut installer = None;
    let mut exception = None;
    let ran = BRIDGE.with(|code| {
        context.eval(
            Some(code),
            Some(&CefString::from("")),
            0,
            Some(&mut installer),
            Some(&mut exception),
        )
    });
    let Some(installer) = installer.filter(|f| ran == 1 && f.is_function() == 1) else {
        return;
    };
    let Some(key) = BRIDGES.with(|b| b.try_borrow_mut().ok().map(|mut b| b.next())) else {
        return;
    };
    let mut handler = Send::new(key);
    let Some(send) = v8_value_create_function(Some(&CefString::from("send")), Some(&mut handler))
    else {
        return;
    };
    let bridge = installer.execute_function_with_context(Some(context), None, Some(&[Some(send)]));
    let Some(bridge) = bridge.filter(|b| b.is_object() == 1) else {
        return;
    };
    let function = |name: &str| {
        bridge
            .value_bykey(Some(&CefString::from(name)))
            .filter(|f| f.is_function() == 1)
    };
    let (Some(receive), Some(install)) = (function("receive"), function("install")) else {
        return;
    };
    BRIDGES.with(|bridges| {
        if let Ok(mut bridges) = bridges.try_borrow_mut() {
            bridges.contexts.push(Bridged {
                key,
                context: context.clone(),
                frame: frame.clone(),
                receive,
                install,
                waiting: HashSet::new(),
            });
        }
    });
}

/// `frame`'s document started loading: the bridge goes in now, in the frame's context, before
/// the page's first script.
pub(crate) fn load_started(frame: &Frame) {
    let Some(mut context) = frame.v8_context() else {
        return;
    };
    let install = BRIDGES.with(|cell| {
        let bridges = cell.try_borrow().ok()?;
        bridges
            .contexts
            .iter()
            .find(|b| b.context.is_same(Some(&mut context)) == 1)
            .map(|b| b.install.clone())
    });
    if let Some(install) = install {
        let _ = install.execute_function_with_context(Some(&mut context), None, Some(&[]));
    }
}

/// The context went: its waiting requests are withdrawn.
pub(crate) fn context_released(context: &mut V8Context) {
    let gone = BRIDGES.with(|bridges| {
        let mut bridges = bridges.try_borrow_mut().ok()?;
        let at = bridges
            .contexts
            .iter()
            .position(|b| b.context.is_same(Some(context)) == 1)?;
        Some(bridges.contexts.swap_remove(at))
    });
    if let Some(gone) = gone {
        for id in gone.waiting {
            let cancel = serde_json::json!({"id": id, "op": "cancel"});
            send(&gone.frame, ProcessId::BROWSER, &cancel);
        }
    }
}

/// An answer from the browser process for one of this process's requests, handed to the
/// bridge that sent it. `true` when the message was the bridge's.
pub(crate) fn answered(message: &ProcessMessage) -> bool {
    /// Whose an answer is; the rest of it is the bridge's to read.
    #[derive(serde::Deserialize)]
    struct Answer {
        id: u64,
    }
    let Some(answer) = said(message) else {
        return false;
    };
    let Ok(Answer { id }) = serde_json::from_str(&answer) else {
        return true;
    };
    // Taken out before calling into the page, whose scripts may send again meanwhile.
    let to = BRIDGES.with(|cell| {
        let mut bridges = cell.try_borrow_mut().ok()?;
        let asked = bridges
            .contexts
            .iter_mut()
            .find(|b| b.waiting.contains(&id))?;
        asked.waiting.remove(&id);
        Some((asked.context.clone(), asked.receive.clone()))
    });
    if let Some((mut context, receive)) = to
        && context.is_valid() == 1
    {
        let answer = v8_value_create_string(Some(&CefString::from(answer.as_str())));
        let _ = receive.execute_function_with_context(Some(&mut context), None, Some(&[answer]));
    }
    true
}

/// A string argument, or empty.
fn string_at(arguments: Option<&[Option<V8Value>]>, at: usize) -> String {
    arguments
        .and_then(|args| args.get(at)?.as_ref())
        .filter(|value| value.is_string() == 1)
        .map(|value| text(&value.string_value()))
        .unwrap_or_default()
}

/// A number argument, or 0.
fn number_at(arguments: Option<&[Option<V8Value>]>, at: usize) -> u64 {
    let number = arguments
        .and_then(|args| args.get(at)?.as_ref())
        .filter(|value| value.is_double() == 1 || value.is_int() == 1 || value.is_uint() == 1)
        .map_or(0., ImplV8Value::double_value);
    // The bridge passes back a number it was given; anything else saturates to one no request
    // has.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "only ever looked up among the requests' numbers"
    )]
    let number = number as u64;
    number
}

/// What the bridge's `send(op, json, id)` does for the context `key`: a request goes to the
/// browser process under a new number, which it returns; `cancel` withdraws request `id`.
fn sent(key: u64, op: &str, options: &str, cancel: u64) -> u64 {
    let request = BRIDGES.with(|cell| {
        let mut bridges = cell.try_borrow_mut().ok()?;
        let id = match op {
            "cancel" => cancel,
            "create" | "get" | "available" | "capabilities" => bridges.next(),
            _ => return None,
        };
        let sender = bridges.contexts.iter_mut().find(|b| b.key == key)?;
        // A cancel goes only for a request still waiting.
        let goes = if op == "cancel" {
            sender.waiting.remove(&id)
        } else {
            sender.waiting.insert(id)
        };
        goes.then(|| (id, sender.frame.clone()))
    });
    let Some((id, frame)) = request else {
        return 0;
    };
    let options = serde_json::from_str(options).unwrap_or(serde_json::Value::Null);
    let message = serde_json::json!({"id": id, "op": op, "options": options});
    send(&frame, ProcessId::BROWSER, &message);
    id
}

wrap_v8_handler! {
    struct Send {
        key: u64,
    }

    impl V8Handler {
        fn execute(
            &self,
            _name: Option<&CefString>,
            _object: Option<&mut V8Value>,
            arguments: Option<&[Option<V8Value>]>,
            retval: Option<&mut Option<V8Value>>,
            _exception: Option<&mut CefString>,
        ) -> c_int {
            let op = string_at(arguments, 0);
            let options = string_at(arguments, 1);
            let id = sent(self.key, &op, &options, number_at(arguments, 2));
            if let Some(retval) = retval {
                #[expect(clippy::cast_precision_loss, reason = "numbers stay far below 2^53")]
                let id = id as f64;
                *retval = v8_value_create_double(id);
            }
            1
        }
    }
}
