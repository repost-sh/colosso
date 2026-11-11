//! libxpc, as the simulator's input takes it (`docs/devices.md` 4): dictionaries sent on a
//! connection made from a port in the simulator's own bootstrap namespace, with two functions
//! libxpc has for simulators that no header declares.

use std::ffi::{CStr, c_char, c_void};
use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;

use crate::sys::{self, DispatchQueue, Symbol};

pub type Object = *mut c_void;

#[repr(C)]
pub struct Opaque {
    _private: [u8; 0],
}

unsafe extern "C" {
    static _xpc_type_error: Opaque;
    static _xpc_type_dictionary: Opaque;
    static _xpc_type_array: Opaque;
    static _xpc_type_string: Opaque;
    static _xpc_type_bool: Opaque;
    static _xpc_type_data: Opaque;
    fn xpc_data_get_bytes_ptr(data: Object) -> *const c_void;
    fn xpc_data_get_length(data: Object) -> usize;
    fn xpc_dictionary_get_value(dictionary: Object, key: *const c_char) -> Object;
    fn xpc_dictionary_set_int64(dictionary: Object, key: *const c_char, value: i64);
    fn xpc_array_create(values: *const Object, count: usize) -> Object;
    fn xpc_array_append_value(array: Object, value: Object);
    fn xpc_array_get_count(array: Object) -> usize;
    fn xpc_array_get_value(array: Object, index: usize) -> Object;
    fn xpc_string_get_string_ptr(string: Object) -> *const c_char;
    fn xpc_bool_get_value(value: Object) -> bool;
    fn xpc_uint64_create(value: u64) -> Object;
    static _xpc_error_connection_interrupted: Opaque;
    static _xpc_error_connection_invalid: Opaque;
    fn xpc_dictionary_create(
        keys: *const *const c_char,
        values: *const Object,
        count: usize,
    ) -> Object;
    fn xpc_dictionary_set_string(dictionary: Object, key: *const c_char, value: *const c_char);
    fn xpc_dictionary_set_bool(dictionary: Object, key: *const c_char, value: bool);
    fn xpc_dictionary_set_uint64(dictionary: Object, key: *const c_char, value: u64);
    fn xpc_dictionary_set_double(dictionary: Object, key: *const c_char, value: f64);
    fn xpc_dictionary_set_data(
        dictionary: Object,
        key: *const c_char,
        bytes: *const c_void,
        length: usize,
    );
    fn xpc_dictionary_set_value(dictionary: Object, key: *const c_char, value: Object);
    fn xpc_release(object: Object);
    fn xpc_get_type(object: Object) -> *const Opaque;
    fn xpc_copy_description(object: Object) -> *mut c_char;
    fn xpc_connection_create_from_endpoint(endpoint: Object) -> Object;
    fn xpc_connection_set_event_handler(
        connection: Object,
        handler: &block2::Block<dyn Fn(Object)>,
    );
    fn xpc_connection_activate(connection: Object);
    fn xpc_connection_cancel(connection: Object);
    fn xpc_connection_send_message(connection: Object, message: Object);
    fn xpc_connection_send_barrier(connection: Object, barrier: &block2::Block<dyn Fn()>);
    fn xpc_connection_send_message_with_reply(
        connection: Object,
        message: Object,
        queue: DispatchQueue,
        handler: &block2::Block<dyn Fn(Object)>,
    );
}

/// An XPC dictionary this process owns.
pub struct Dict(Object);

impl Dict {
    pub fn new() -> Dict {
        // SAFETY: an empty dictionary; ours to release.
        Dict(unsafe { xpc_dictionary_create(std::ptr::null(), std::ptr::null(), 0) })
    }

    pub fn string(self, key: &CStr, value: &CStr) -> Dict {
        // SAFETY: a live dictionary and two zero-terminated strings, which it copies.
        unsafe { xpc_dictionary_set_string(self.0, key.as_ptr(), value.as_ptr()) };
        self
    }

    pub fn bool(self, key: &CStr, value: bool) -> Dict {
        // SAFETY: as `string`.
        unsafe { xpc_dictionary_set_bool(self.0, key.as_ptr(), value) };
        self
    }

    pub fn u64(self, key: &CStr, value: u64) -> Dict {
        // SAFETY: as `string`.
        unsafe { xpc_dictionary_set_uint64(self.0, key.as_ptr(), value) };
        self
    }

    pub fn f64(self, key: &CStr, value: f64) -> Dict {
        // SAFETY: as `string`.
        unsafe { xpc_dictionary_set_double(self.0, key.as_ptr(), value) };
        self
    }

    pub fn data(self, key: &CStr, bytes: &[u8]) -> Dict {
        // SAFETY: as `string`; the bytes are copied.
        unsafe {
            xpc_dictionary_set_data(self.0, key.as_ptr(), bytes.as_ptr().cast(), bytes.len());
        }
        self
    }

    pub fn i64(self, key: &CStr, value: i64) -> Dict {
        // SAFETY: as `string`.
        unsafe { xpc_dictionary_set_int64(self.0, key.as_ptr(), value) };
        self
    }

    /// An array of unsigned numbers put under `key`.
    pub fn u64s(self, key: &CStr, values: &[u64]) -> Dict {
        // SAFETY: a new array, each value appended (it retains them) and released; the
        // dictionary retains the array, which is then released.
        unsafe {
            let array = xpc_array_create(std::ptr::null(), 0);
            for &value in values {
                let number = xpc_uint64_create(value);
                xpc_array_append_value(array, number);
                xpc_release(number);
            }
            xpc_dictionary_set_value(self.0, key.as_ptr(), array);
            xpc_release(array);
        }
        self
    }

    /// `value` put under `key`, which retains it.
    pub fn dict(self, key: &CStr, value: &Dict) -> Dict {
        // SAFETY: two live dictionaries; the outer one retains the inner.
        unsafe { xpc_dictionary_set_value(self.0, key.as_ptr(), value.0) };
        self
    }
}

impl Drop for Dict {
    fn drop(&mut self) {
        // SAFETY: the one reference this wrapper holds.
        unsafe { xpc_release(self.0) };
    }
}

/// Whether an event or a reply is an error, and which.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Interrupted,
    Invalid,
    OtherError,
    Message,
}

/// What `object`, as an event handler or a reply handler is given it, is.
pub fn event(object: Object) -> Event {
    // SAFETY: comparing the object with libxpc's own constants, and asking its type.
    unsafe {
        if object.cast_const() == (&raw const _xpc_error_connection_interrupted).cast() {
            Event::Interrupted
        } else if object.cast_const() == (&raw const _xpc_error_connection_invalid).cast() {
            Event::Invalid
        } else if xpc_get_type(object) == &raw const _xpc_type_error {
            Event::OtherError
        } else {
            Event::Message
        }
    }
}

/// Reading a reply, in the handler that is given it: valid only for as long as the handler runs.
#[derive(Clone, Copy)]
pub struct Read(Object);

impl Read {
    /// How it reads, for the log.
    pub fn describe(&self) -> String {
        describe(self.0)
    }

    fn is(&self, kind: &Opaque) -> bool {
        // SAFETY: asking an object's type, which a null one hasn't.
        !self.0.is_null() && unsafe { xpc_get_type(self.0) } == std::ptr::from_ref(kind)
    }

    /// `self[key]`, when it is a dictionary that has it.
    pub fn get(&self, key: &CStr) -> Option<Read> {
        // SAFETY: a dictionary's value by a zero-terminated key; borrowed from the dictionary.
        let is = self.is(unsafe { &_xpc_type_dictionary });
        is.then(|| unsafe { xpc_dictionary_get_value(self.0, key.as_ptr()) })
            .filter(|value| !value.is_null())
            .map(Read)
    }

    /// The items, when it is an array.
    pub fn items(&self) -> Vec<Read> {
        // SAFETY: an array's count and items, borrowed from it.
        if !self.is(unsafe { &_xpc_type_array }) {
            return Vec::new();
        }
        let count = unsafe { xpc_array_get_count(self.0) };
        (0..count)
            .map(|index| Read(unsafe { xpc_array_get_value(self.0, index) }))
            .collect()
    }

    pub fn string(&self) -> Option<String> {
        // SAFETY: a string's own bytes, zero-terminated, copied out.
        self.is(unsafe { &_xpc_type_string }).then(|| unsafe {
            CStr::from_ptr(xpc_string_get_string_ptr(self.0))
                .to_string_lossy()
                .into_owned()
        })
    }

    /// The bytes, copied out, when it is data.
    pub fn data(&self) -> Option<Vec<u8>> {
        // SAFETY: a data object's own bytes and length, borrowed from it and copied.
        self.is(unsafe { &_xpc_type_data }).then(|| unsafe {
            let length = xpc_data_get_length(self.0);
            let bytes = xpc_data_get_bytes_ptr(self.0).cast::<u8>();
            if bytes.is_null() || length == 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(bytes, length).to_vec()
            }
        })
    }

    pub fn bool(&self) -> Option<bool> {
        // SAFETY: a boolean's value.
        self.is(unsafe { &_xpc_type_bool })
            .then(|| unsafe { xpc_bool_get_value(self.0) })
    }
}

/// How an object reads, for the log.
pub fn describe(object: Object) -> String {
    // SAFETY: the description is a malloc'd C string, ours to free.
    unsafe {
        let text = xpc_copy_description(object);
        if text.is_null() {
            return String::new();
        }
        let said = CStr::from_ptr(text).to_string_lossy().into_owned();
        libc::free(text.cast());
        said
    }
}

type EndpointFromPort = unsafe extern "C" fn(u32, u64, u64) -> Object;
type EnableSim2Host = unsafe extern "C" fn(Object);
static ENDPOINT_FROM_PORT: Symbol = Symbol::loaded(c"xpc_endpoint_create_mach_port_4sim");
static ENABLE_SIM2HOST: Symbol = Symbol::loaded(c"xpc_connection_enable_sim2host_4sim");

/// A connection to a service in a simulator.
pub struct Connection(Object);

// SAFETY: an XPC connection may be used from any thread; libxpc serializes what it does.
unsafe impl Send for Connection {}

impl Connection {
    /// A connection to the service whose port `port` is in the simulator's namespace, with
    /// `events` told of what happens to it.
    pub fn to_simulator(port: u32, events: impl Fn(Event) + Send + 'static) -> Result<Connection, String> {
        // SAFETY: the two functions' types as libxpc has them (idb's declarations).
        let (endpoint_from_port, enable) = unsafe {
            (
                ENDPOINT_FROM_PORT.function::<EndpointFromPort>(),
                ENABLE_SIM2HOST.function::<EnableSim2Host>(),
            )
        };
        let (Some(endpoint_from_port), Some(enable)) = (endpoint_from_port, enable) else {
            return Err("libxpc's simulator functions aren't there".into());
        };
        // SAFETY: an endpoint made from a send right we hold, a connection from it, and a
        // handler block kept alive by libxpc for as long as the connection is.
        unsafe {
            let endpoint = endpoint_from_port(port, 0, 0);
            if endpoint.is_null() {
                return Err("no endpoint for the service's port".into());
            }
            let connection = xpc_connection_create_from_endpoint(endpoint);
            xpc_release(endpoint);
            if connection.is_null() {
                return Err("no connection from the endpoint".into());
            }
            enable(connection);
            let handler = RcBlock::new(move |object: Object| events(event(object)));
            xpc_connection_set_event_handler(connection, &handler);
            xpc_connection_activate(connection);
            Ok(Connection(connection))
        }
    }

    pub fn send(&self, message: &Dict) {
        // SAFETY: a live connection and dictionary; the message is copied.
        unsafe { xpc_connection_send_message(self.0, message.0) };
    }

    /// Waits up to `limit` until everything sent before has gone; whether it did.
    pub fn flush(&self, limit: Duration) -> bool {
        let (done, flushed) = mpsc::channel();
        let barrier = RcBlock::new(move || {
            let _ = done.send(());
        });
        // SAFETY: a live connection; libxpc copies the block.
        unsafe { xpc_connection_send_barrier(self.0, &barrier) };
        flushed.recv_timeout(limit).is_ok()
    }

    /// Sends `message`; what the reply is comes on the receiver.
    pub fn ask_later(&self, message: &Dict) -> mpsc::Receiver<Event> {
        let (answer, answered) = mpsc::channel();
        let handler = RcBlock::new(move |reply: Object| {
            let _ = answer.send(event(reply));
        });
        // SAFETY: a live connection and message; libxpc copies the block and calls it once.
        unsafe {
            xpc_connection_send_message_with_reply(
                self.0,
                message.0,
                sys::global_queue(),
                &handler,
            );
        }
        answered
    }

    /// Sends `message` and waits up to `limit` for the reply, read by `read` in its handler.
    pub fn ask_with<T: Send + 'static>(
        &self,
        message: &Dict,
        limit: Duration,
        read: impl Fn(Read) -> T + Send + 'static,
    ) -> Option<T> {
        let (answer, answered) = mpsc::channel();
        let handler = RcBlock::new(move |reply: Object| {
            let _ = answer.send(read(Read(reply)));
        });
        // SAFETY: a live connection and message; libxpc copies the block and calls it once.
        unsafe {
            xpc_connection_send_message_with_reply(
                self.0,
                message.0,
                sys::global_queue(),
                &handler,
            );
        }
        answered.recv_timeout(limit).ok()
    }

    /// Sends `message` and waits up to `limit` for the reply: what it is, and its description.
    pub fn ask(&self, message: &Dict, limit: Duration) -> Option<(Event, String)> {
        self.ask_with(message, limit, |reply| (event(reply.0), describe(reply.0)))
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // SAFETY: the connection is cancelled, then our reference let go.
        unsafe {
            xpc_connection_cancel(self.0);
            xpc_release(self.0);
        }
    }
}
