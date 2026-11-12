//! A simulator's accessibility tree (`docs/devices.md` 6), read as idb's raw reader reads it:
//! archived `AXPTranslatorRequest`s sent one a message to the service Apple's
//! `CoreSimulatorBridge` runs inside the simulator, `com.apple.CoreSimulator.accessibility`: the
//! frontmost app first, then each element's frame, role, label, value and children in one
//! request. The classes are macOS's own (`AccessibilityPlatformTranslation`), loaded only to
//! archive the requests and unarchive the answers; nothing is installed in the simulator. An
//! app answers only while accessibility is on in the simulator's settings, which `access.rs`
//! switches.

use std::ffi::CStr;
use std::time::{Duration, Instant};

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{ClassType as _, msg_send};
use objc2_foundation::{NSData, NSDictionary, NSError, NSNumber, NSRect, NSString, NSUUID, NSValue};

use super::core::guarded;
use crate::sys;
use crate::xpc::{Connection, Dict};

/// The service, in the simulator's own namespace.
pub const SERVICE: &str = "com.apple.CoreSimulator.accessibility";
const FRAMEWORK: &CStr = c"/System/Library/PrivateFrameworks/AccessibilityPlatformTranslation.framework/AccessibilityPlatformTranslation";

/// Request types, as the translator numbers them (idb's `BridgeAXTranslatorRequestType`).
const FRONTMOST: u64 = 4;
const MULTIPLE: u64 = 5;
/// Attributes (idb's `BridgeAXTranslatorAttribute`).
const ACTIVATION: i64 = 1;
const BUNDLE: i64 = 6;
const CHILDREN: i64 = 8;
const FRAME: i64 = 21;
const IDENTIFIER: i64 = 25;
const ENABLED: i64 = 27;
const LABEL: i64 = 33;
const ROLE: i64 = 45;
const VALUE: i64 = 53;
const REMOTE: i64 = 66;
const ASKED: [i64; 9] = [FRAME, ACTIVATION, ROLE, LABEL, VALUE, IDENTIFIER, ENABLED, REMOTE, CHILDREN];
/// The translator's roles, from 1, as `AppKit` names them without their `AX`.
const ROLES: [&str; 21] = [
    "Application",
    "Button",
    "CheckBox",
    "GenericElement",
    "Group",
    "Heading",
    "Image",
    "Link",
    "RadioButton",
    "ScrollArea",
    "ScrollBar",
    "TabGroup",
    "Slider",
    "StaticText",
    "TextField",
    "WebArea",
    "TextArea",
    "Splitter",
    "PopUpButton",
    "Sheet",
    "Grid",
];

/// One element of the tree.
#[derive(Clone, Debug, Default)]
pub struct Element {
    pub role: String,
    pub label: Option<String>,
    pub value: Option<String>,
    pub identifier: Option<String>,
    pub enabled: bool,
    /// Where it is, as the simulator reports it: x, y, width and height in points.
    pub frame: [f64; 4],
    /// Where a touch activates it, when it says: a switch's row is touched on its switch.
    pub activation: Option<[f64; 2]>,
    /// Whether another process draws what's in it, which the tree doesn't reach.
    pub remote: bool,
    pub children: Vec<Element>,
}

/// How much a read may take.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub elements: usize,
    pub depth: usize,
    /// For one request, and for the whole read.
    pub each: Duration,
    pub whole: Duration,
}

/// How long a read goes on asking while no app is in front: an app just launched isn't in
/// the tree for a moment; and how far apart it asks.
const SETTLING: Duration = Duration::from_secs(5);
const APART: Duration = Duration::from_millis(300);

/// What a read found: the tree, and whether it was cut short.
pub struct Read {
    pub root: Element,
    pub cut: bool,
}

/// A connection to a simulator's accessibility service.
pub struct Tree {
    connection: Connection,
    /// Whether the connection was interrupted or let go: the service is to be looked up again.
    broken: std::sync::Arc<std::sync::atomic::AtomicBool>,
    token: Retained<NSString>,
    request: &'static AnyClass,
    response: &'static AnyClass,
    object: &'static AnyClass,
}

// SAFETY: the classes are immutable runtime objects, the token an immutable string, the flag
// atomic; the connection is `Send`, and libxpc serializes what is sent on it from any thread.
unsafe impl Send for Tree {}
// SAFETY: as above: reads made at once each send their own requests and wait on their own
// replies.
unsafe impl Sync for Tree {}

fn class(name: &CStr) -> Result<&'static AnyClass, String> {
    AnyClass::get(name).ok_or_else(|| format!("macOS has no {}", name.to_string_lossy()))
}

impl Tree {
    /// A connection to the service whose port `port` is, in the simulator's namespace.
    pub fn connect(port: u32) -> Result<Tree, String> {
        use std::sync::atomic::{AtomicBool, Ordering};
        if !sys::load(FRAMEWORK) {
            return Err("macOS's accessibility translation isn't there".into());
        }
        let broken = std::sync::Arc::new(AtomicBool::new(false));
        let told = std::sync::Arc::clone(&broken);
        let connection = Connection::to_simulator(port, move |event| {
            if matches!(event, crate::xpc::Event::Interrupted | crate::xpc::Event::Invalid) {
                told.store(true, Ordering::Relaxed);
            }
        })?;
        Ok(Tree {
            connection,
            broken,
            token: NSUUID::new().UUIDString(),
            request: class(c"AXPTranslatorRequest")?,
            response: class(c"AXPTranslatorResponse")?,
            object: class(c"AXPTranslationObject")?,
        })
    }

    /// Whether the connection broke: it is made again for the next read.
    pub fn broken(&self) -> bool {
        self.broken.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The frontmost app's tree, as far as `limits` let it go, asked again a few times while no
    /// app is in front, or nothing but the status bar is: an app just launched isn't in the tree
    /// until it has drawn, and `SpringBoard`'s status bar stands in for it meanwhile.
    pub fn read_settled(&self, limits: Limits) -> Result<Read, String> {
        let started = Instant::now();
        loop {
            let late = started.elapsed() >= SETTLING;
            match self.read(limits) {
                Ok(read) if settled(&read.root) || late => return Ok(read),
                Err(why) if late || self.broken() => return Err(why),
                _ => std::thread::park_timeout(APART),
            }
        }
    }

    /// The bundle id of the app in front, as its tree's root says (a system alert's is
    /// `SpringBoard`'s); `None` when it names none.
    pub fn in_front(&self, each: Duration) -> Result<Option<String>, String> {
        autoreleasepool(|_| {
            let Some(root) = self.frontmost(each)? else {
                return Ok(None);
            };
            let values = self.attributes(&root, &[BUNDLE], each)?;
            Ok(values.and_then(|values| value_of(&values, BUNDLE)).and_then(|bundle| text(&bundle)))
        })
    }

    /// The frontmost app's tree, as far as `limits` let it go.
    pub fn read(&self, limits: Limits) -> Result<Read, String> {
        autoreleasepool(|_| {
            let started = Instant::now();
            let root = self.frontmost(limits.each)?.ok_or("the simulator named no app in front")?;
            let mut walk = Walk {
                limits,
                started,
                elements: 0,
                cut: false,
            };
            let root = self.element(&root, 0, &mut walk)?;
            Ok(Read { root, cut: walk.cut })
        })
    }

    /// The translation of the app in front, or `None` when the simulator names none.
    fn frontmost(&self, each: Duration) -> Result<Option<Retained<AnyObject>>, String> {
        // SAFETY: a blank translation object, as the translator sends for the frontmost app.
        let blank: Retained<AnyObject> = guarded("making a translation", || unsafe { msg_send![self.object, new] })?;
        let display = NSNumber::new_u32(0);
        let answer = self.send(FRONTMOST, Some(&blank), &parameters(&[("displayId", &display)]), each)?;
        Ok(result(&answer))
    }

    /// The values of `asked` of the element `translation`, by attribute; `None` when it has none.
    fn attributes(&self, translation: &AnyObject, asked: &[i64], each: Duration) -> Result<Option<Retained<AnyObject>>, String> {
        // SAFETY: a translation object's token, a string it keeps.
        guarded("tagging a translation", || unsafe {
            let _: () = msg_send![translation, setBridgeDelegateToken: &*self.token];
        })?;
        let asked: Vec<Retained<NSNumber>> = asked.iter().map(|attribute| NSNumber::new_i64(*attribute)).collect();
        let asked = objc2_foundation::NSArray::from_retained_slice(&asked);
        let answer = self.send(MULTIPLE, Some(translation), &parameters(&[("attributes", &asked)]), each)?;
        Ok(result(&answer))
    }

    fn element(&self, translation: &AnyObject, depth: usize, walk: &mut Walk) -> Result<Element, String> {
        walk.elements += 1;
        let values = self.attributes(translation, &ASKED, walk.limits.each)?;
        let get = |attribute: i64| values.as_ref().and_then(|values| value_of(values, attribute));
        let mut element = Element {
            role: get(ROLE).and_then(|role| number(&role)).map_or_else(
                || "Element".to_owned(),
                |role| {
                    (role.cast_unsigned().checked_sub(1))
                        .and_then(|at| ROLES.get(usize::try_from(at).ok()?))
                        .map_or_else(|| format!("Role{role}"), |name| (*name).to_owned())
                },
            ),
            label: get(LABEL).and_then(|label| text(&label)).filter(|label| !label.is_empty()),
            value: get(VALUE).and_then(|value| text(&value)).filter(|value| !value.is_empty()),
            identifier: get(IDENTIFIER).and_then(|id| text(&id)).filter(|id| !id.is_empty()),
            enabled: get(ENABLED).and_then(|enabled| number(&enabled)).is_none_or(|enabled| enabled != 0),
            frame: get(FRAME).and_then(|frame| rect(&frame)).unwrap_or_default(),
            activation: get(ACTIVATION).and_then(|point| point_of(&point)),
            remote: get(REMOTE).and_then(|remote| number(&remote)).is_some_and(|remote| remote != 0),
            children: Vec::new(),
        };
        for child in get(CHILDREN).map(|children| items(&children)).unwrap_or_default() {
            if walk.elements >= walk.limits.elements
                || depth + 1 >= walk.limits.depth
                || walk.started.elapsed() >= walk.limits.whole
            {
                walk.cut = true;
                break;
            }
            element.children.push(self.element(&child, depth + 1, walk)?);
        }
        Ok(element)
    }

    /// One request, archived and sent, and its answer unarchived.
    fn send(
        &self,
        kind: u64,
        translation: Option<&AnyObject>,
        parameters: &AnyObject,
        limit: Duration,
    ) -> Result<Retained<AnyObject>, String> {
        let archived = guarded("archiving a request", || -> Option<Vec<u8>> {
            // SAFETY: the request class's own constructor and setters, with the types its
            // header gives; archived with secure coding, as the service expects.
            unsafe {
                let request: Option<Retained<AnyObject>> =
                    msg_send![self.request, requestWithTranslation: translation];
                let request = request?;
                let _: () = msg_send![&request, setRequestType: kind];
                let _: () = msg_send![&request, setClientType: 0_u64];
                let _: () = msg_send![&request, setParameters: parameters];
                let mut error: *mut NSError = std::ptr::null_mut();
                let data: Option<Retained<NSData>> = msg_send![
                    objc2_foundation::NSKeyedArchiver::class(),
                    archivedDataWithRootObject: &*request,
                    requiringSecureCoding: true,
                    error: &mut error
                ];
                Some(data?.to_vec())
            }
        })?
        .ok_or("a request couldn't be archived")?;
        let message = Dict::new()
            .string(c"SimAccessibility_PayloadClassName", c"AXPTranslatorRequest")
            .dict(
                c"SimAccessibility_Payload",
                &Dict::new().data(c"translatorRequest", &archived),
            );
        let reply = self.connection.ask_with(&message, limit, |read| {
            read.get(c"SimAccessibility_Payload")
                .and_then(|payload| payload.get(c"translatorResponse"))
                .and_then(|data| data.data())
        });
        let bytes = reply
            .ok_or("the simulator's accessibility didn't answer in time")?
            .ok_or("the simulator's accessibility answered without a response")?;
        let data = NSData::with_bytes(&bytes);
        let response = guarded("unarchiving an answer", || -> Option<Retained<AnyObject>> {
            let mut error: *mut NSError = std::ptr::null_mut();
            // SAFETY: unarchived as the response class, whose own allowed classes decode it.
            unsafe {
                msg_send![
                    objc2_foundation::NSKeyedUnarchiver::class(),
                    unarchivedObjectOfClass: self.response,
                    fromData: &*data,
                    error: &mut error
                ]
            }
        })?;
        response.ok_or_else(|| "an answer couldn't be unarchived".into())
    }
}

/// The most a status bar reaches down, in points (an iPhone 17 Pro's is 62 high with its
/// island): a tree with nothing below it isn't an app's.
const STATUS_BAR: f64 = 70.;

/// Whether the tree has something of an app's: an element below the status bar.
fn settled(root: &Element) -> bool {
    root.children.iter().any(|child| child.frame[1] + child.frame[3] > STATUS_BAR || settled(child))
}

struct Walk {
    limits: Limits,
    started: Instant,
    elements: usize,
    cut: bool,
}

/// A request's parameters: string keys to objects.
fn parameters(pairs: &[(&str, &AnyObject)]) -> Retained<AnyObject> {
    let keys: Vec<Retained<NSString>> = pairs.iter().map(|(key, _)| NSString::from_str(key)).collect();
    let keys: Vec<&NSString> = keys.iter().map(|key| &**key).collect();
    let values: Vec<&AnyObject> = pairs.iter().map(|(_, value)| *value).collect();
    let dictionary = NSDictionary::<NSString, AnyObject>::from_slices(&keys, &values);
    Retained::into_super(Retained::into_super(dictionary))
}

/// An answer's `resultData`.
fn result(answer: &AnyObject) -> Option<Retained<AnyObject>> {
    // SAFETY: the response's own property.
    guarded("reading an answer", || unsafe { msg_send![answer, resultData] })
        .ok()
        .flatten()
}

fn is(object: &AnyObject, class: &AnyClass) -> bool {
    // SAFETY: every object answers `isKindOfClass:`.
    unsafe { msg_send![object, isKindOfClass: class] }
}

/// `values[attribute]`, when `values` is a dictionary that has it.
fn value_of(values: &AnyObject, attribute: i64) -> Option<Retained<AnyObject>> {
    if !is(values, NSDictionary::<AnyObject, AnyObject>::class()) {
        return None;
    }
    let key = NSNumber::new_i64(attribute);
    // SAFETY: a dictionary's value by a key it may not have.
    unsafe { msg_send![values, objectForKey: &*key] }
}

fn number(object: &AnyObject) -> Option<i64> {
    // SAFETY: a number's value, asked only of a number.
    is(object, NSNumber::class()).then(|| unsafe { msg_send![object, longLongValue] })
}

/// A string, an attributed string's text, or a number's, as it reads.
fn text(object: &AnyObject) -> Option<String> {
    if is(object, NSString::class()) {
        // SAFETY: a string, checked.
        let string: &NSString = unsafe { &*std::ptr::from_ref(object).cast::<NSString>() };
        return Some(string.to_string());
    }
    if is(object, NSNumber::class()) {
        // SAFETY: a number's own description.
        let string: Retained<NSString> = unsafe { msg_send![object, stringValue] };
        return Some(string.to_string());
    }
    if is(object, objc2_foundation::NSAttributedString::class()) {
        // SAFETY: an attributed string's text.
        let string: Retained<NSString> = unsafe { msg_send![object, string] };
        return Some(string.to_string());
    }
    None
}

fn rect(object: &AnyObject) -> Option<[f64; 4]> {
    if !is(object, NSValue::class()) {
        return None;
    }
    // SAFETY: a value holding a rectangle, as the translator answers a frame.
    let rect: NSRect = guarded("reading a frame", || unsafe { msg_send![object, rectValue] }).ok()?;
    Some([rect.origin.x, rect.origin.y, rect.size.width, rect.size.height])
}

fn point_of(object: &AnyObject) -> Option<[f64; 2]> {
    if !is(object, NSValue::class()) {
        return None;
    }
    // SAFETY: a value holding a point, as the translator answers an activation point.
    let point: objc2_foundation::NSPoint = guarded("reading a point", || unsafe { msg_send![object, pointValue] }).ok()?;
    (point.x.is_finite() && point.y.is_finite() && point.x >= 0. && point.y >= 0.).then_some([point.x, point.y])
}

/// An array's items.
fn items(object: &AnyObject) -> Vec<Retained<AnyObject>> {
    if !is(object, objc2_foundation::NSArray::<AnyObject>::class()) {
        return Vec::new();
    }
    // SAFETY: an array's count and items.
    let count: usize = unsafe { msg_send![object, count] };
    (0..count)
        .filter_map(|at| unsafe { msg_send![object, objectAtIndex: at] })
        .collect()
}

/// A snapshot's lines, its refs and whether more were left out.
pub type Lines = (String, Vec<(String, koyo_device_wire::Point)>, bool);

/// The tree as a snapshot's lines, refs numbered from `first`: each element on a line indented by
/// its depth, its role, label and value, and, when it has a size, its ref and the middle of its
/// frame. At most `cap` refs; whether more were left out.
pub fn lines(root: &Element, first: u32, cap: usize) -> Lines {
    let mut out = (String::new(), Vec::new(), false);
    line(root, 0, first, cap, &mut out);
    out
}

fn line(element: &Element, depth: usize, first: u32, cap: usize, out: &mut Lines) {
    use std::fmt::Write as _;
    if out.1.len() >= cap {
        out.2 = true;
        return;
    }
    let lines = &mut out.0;
    let [x, y, width, height] = element.frame;
    let _ = write!(lines, "{:indent$}{}", "", element.role, indent = depth * 2);
    if let Some(label) = &element.label {
        let _ = write!(lines, " {label:?}");
    }
    if let Some(value) = &element.value {
        let _ = write!(lines, " value={value:?}");
    }
    if let Some(identifier) = &element.identifier {
        let _ = write!(lines, " id={identifier:?}");
    }
    if !element.enabled {
        lines.push_str(" disabled");
    }
    if element.remote && element.children.is_empty() {
        lines.push_str(" remote");
    }
    if width > 0. && height > 0. {
        // Where a touch activates it, inside it; else its middle.
        let inside = |[px, py]: [f64; 2]| px >= x && px <= x + width && py >= y && py <= y + height;
        let [mx, my] = (element.activation)
            .filter(|point| inside(*point))
            .unwrap_or([x + width / 2., y + height / 2.]);
        let middle = koyo_device_wire::Point { x: mx, y: my };
        let name = format!("e{}", first + u32::try_from(out.1.len()).unwrap_or(u32::MAX));
        let _ = write!(lines, " [ref={name}] @{:.0},{:.0}", middle.x, middle.y);
        out.1.push((name, middle));
    }
    out.0.push('\n');
    for child in &element.children {
        line(child, depth + 1, first, cap, out);
    }
}

#[cfg(test)]
mod tests;
