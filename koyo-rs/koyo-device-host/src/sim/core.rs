//! `CoreSimulator`'s objects, reached by message (`docs/devices.md` 4): the device in Koyo's own
//! set, its main display's surface and screen callbacks, and the ports of the services inside
//! it. Every private call can raise an Objective-C exception; each is caught and becomes an
//! error in words.

use std::panic::AssertUnwindSafe;
use std::ptr::NonNull;

use std::ffi::CStr;

use block2::{ManualBlockEncoding, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{msg_send, sel};
use objc2_core_foundation::CFRetained;
use objc2_foundation::{NSArray, NSError, NSString, NSUUID};
use objc2_io_surface::IOSurfaceRef;

use crate::sim::proxy;
use crate::sys;

const CORE_SIMULATOR: &CStr =
    c"/Library/Developer/PrivateFrameworks/CoreSimulator.framework/CoreSimulator";
/// From this `CoreSimulator` on (Xcode 27), input goes to `dtuhidd` in the simulator.
pub const NEEDED: &str = "1155.4";

/// `SimDeviceState`.
pub const BOOTED: u64 = 3;

/// Runs `call`, an Objective-C call that may raise; what it raised becomes an error.
pub fn guarded<R>(what: &str, call: impl FnOnce() -> R) -> Result<R, String> {
    objc2::exception::catch(AssertUnwindSafe(call)).map_err(|raised| {
        let said = raised.map_or_else(
            || "nothing".to_owned(),
            |exception| format!("{exception:?}"),
        );
        format!("{what} raised {said}")
    })
}

/// `CoreSimulator`'s version, as its bundle says: `1174.21`.
pub fn version() -> Option<String> {
    bundle_version("/Library/Developer/PrivateFrameworks/CoreSimulator.framework")
}

/// The `CFBundleVersion` of the bundle at `path`, as `NSBundle` reads it (a binary or a text
/// property list alike).
pub fn bundle_version(path: &str) -> Option<String> {
    let path = NSString::from_str(path);
    let key = NSString::from_str("CFBundleVersion");
    guarded("reading a bundle's version", || {
        // SAFETY: `NSBundle`'s public methods; the value is a string for this key.
        unsafe {
            let bundle: Option<Retained<AnyObject>> =
                msg_send![objc2::class!(NSBundle), bundleWithPath: &*path];
            let value: Option<Retained<NSString>> =
                msg_send![&bundle?, objectForInfoDictionaryKey: &*key];
            value.map(|value| value.to_string())
        }
    })
    .ok()
    .flatten()
}

/// Whether `version` is at least `wanted`, number by number.
pub fn at_least(version: &str, wanted: &str) -> bool {
    let numbers =
        |text: &str| -> Vec<u32> { text.split('.').filter_map(|n| n.parse().ok()).collect() };
    numbers(version) >= numbers(wanted)
}

/// A simulator of a device set.
pub struct Device(Retained<AnyObject>);

impl Device {
    /// The device `udid` in the set at `set`, through the Xcode in `developer`.
    pub fn find(developer: &str, set: &str, udid: &str) -> Result<Device, String> {
        if !sys::load(CORE_SIMULATOR) {
            return Err("CoreSimulator didn't load: is Xcode 27 installed?".into());
        }
        let class = AnyClass::get(c"SimServiceContext").ok_or("no SimServiceContext")?;
        let developer = NSString::from_str(developer);
        let set = NSString::from_str(set);
        guarded("finding the simulator", || {
            // SAFETY: CoreSimulator's own methods, with the argument types its headers give.
            unsafe {
                let context: Result<Retained<AnyObject>, Retained<NSError>> =
                    msg_send![class, sharedServiceContextForDeveloperDir: &*developer, error: _];
                let context = context.map_err(|error| format!("no simulator service: {error}"))?;
                let devices: Result<Retained<AnyObject>, Retained<NSError>> =
                    msg_send![&context, deviceSetWithPath: &*set, error: _];
                let devices = devices.map_err(|error| format!("no device set: {error}"))?;
                let all: Retained<NSArray<AnyObject>> = msg_send![&devices, devices];
                for device in &all {
                    let id: Retained<NSUUID> = msg_send![&device, UDID];
                    if id.UUIDString().to_string().eq_ignore_ascii_case(udid) {
                        return Ok(Device(device));
                    }
                }
                Err(format!("no simulator {udid} in Koyo's set"))
            }
        })?
    }

    /// `SimDeviceState`: 3 when booted.
    pub fn state(&self) -> u64 {
        // SAFETY: a property CoreSimulator declares as `unsigned long long`.
        guarded("reading its state", || unsafe { msg_send![&self.0, state] }).unwrap_or(0)
    }

    /// Whether it has ever booted: a first boot takes about a minute.
    pub fn booted_before(&self) -> bool {
        guarded("reading when it last booted", || {
            // SAFETY: a property CoreSimulator declares, asked only where it answers.
            unsafe {
                let answers: bool = msg_send![&self.0, respondsToSelector: sel!(lastBootedAt)];
                let at: Option<Retained<AnyObject>> = if answers {
                    msg_send![&self.0, lastBootedAt]
                } else {
                    None
                };
                at.is_some()
            }
        })
        .unwrap_or(false)
    }

    /// Pixels a point: 3 for an iPhone 17 Pro, 2 for an iPad.
    pub fn scale(&self) -> f64 {
        guarded("reading its scale", || {
            // SAFETY: `deviceType` and its `mainScreenScale`, a `float`.
            unsafe {
                let kind: Retained<AnyObject> = msg_send![&self.0, deviceType];
                let scale: f32 = msg_send![&kind, mainScreenScale];
                f64::from(scale)
            }
        })
        .ok()
        .filter(|scale| *scale >= 1.)
        .unwrap_or(2.)
    }

    /// A send right to the service `name` in the simulator's own bootstrap namespace.
    pub fn lookup(&self, name: &str) -> Result<u32, String> {
        let name_ = NSString::from_str(name);
        let mut error: *mut NSError = std::ptr::null_mut();
        // SAFETY: `lookup:error:` returns a port name and fills the error when it fails.
        let port: u32 = guarded("looking up a service", || unsafe {
            msg_send![&self.0, lookup: &*name_, error: &mut error]
        })?;
        if port == 0 {
            // SAFETY: a failed lookup leaves an autoreleased error, or none.
            let why = unsafe { error.as_ref() }
                .map_or_else(|| "no answer".to_owned(), ToString::to_string);
            return Err(format!("no service {name} in the simulator: {why}"));
        }
        Ok(port)
    }

    /// The main display: the last of the display ports whose class is 0 (the only one on an
    /// ordinary iPhone or iPad). The ports and their descriptors are remote proxies
    /// (`proxy.rs`).
    pub fn display(&self) -> Result<Display, String> {
        guarded("finding its display", || {
            // SAFETY: the device's IO object and its ports, as CoreSimulator has them; each port
            // asked only for what it says it answers, with the types its headers give.
            unsafe {
                let io: Retained<AnyObject> = msg_send![&self.0, io];
                let ports: Retained<NSArray<AnyObject>> = msg_send![&io, ioPorts];
                let mut main = None;
                for port in &ports {
                    let has: bool = msg_send![&port, respondsToSelector: sel!(descriptor)];
                    if !has {
                        continue;
                    }
                    let port = Retained::as_ptr(&port).cast_mut();
                    let Some(descriptor) = Retained::retain(proxy::object(port, sel!(descriptor)))
                    else {
                        continue;
                    };
                    let screen: bool =
                        msg_send![&descriptor, respondsToSelector: sel!(framebufferSurface)];
                    let stated: bool = msg_send![&descriptor, respondsToSelector: sel!(state)];
                    if !screen || !stated {
                        continue;
                    }
                    let state =
                        proxy::object(Retained::as_ptr(&descriptor).cast_mut(), sel!(state));
                    if !state.is_null() && proxy::short(state, sel!(displayClass)) == 0 {
                        main = Some(descriptor);
                    }
                }
                main.map(|descriptor| Display {
                    descriptor,
                    token: NSUUID::new(),
                })
                .ok_or_else(|| "the simulator has no display: is it booted?".to_owned())
            }
        })?
    }
}

/// The frame callback's type, `void (^)(void)`.
struct FrameBlock;
// SAFETY: the encoding of a block that takes nothing and returns nothing.
unsafe impl ManualBlockEncoding for FrameBlock {
    type Arguments = ();
    type Return = ();
    const ENCODING_CSTR: &'static CStr = c"v8@?0";
}

/// The surfaces callback's type, `void (^)(id, id)`.
struct SurfacesBlock;
// SAFETY: the encoding of a block that takes two objects and returns nothing.
unsafe impl ManualBlockEncoding for SurfacesBlock {
    type Arguments = (*mut AnyObject, *mut AnyObject);
    type Return = ();
    const ENCODING_CSTR: &'static CStr = c"v24@?0@8@16";
}

/// The properties callback's type, `void (^)(id)`.
struct PropertiesBlock;
// SAFETY: the encoding of a block that takes one object and returns nothing.
unsafe impl ManualBlockEncoding for PropertiesBlock {
    type Arguments = (*mut AnyObject,);
    type Return = ();
    const ENCODING_CSTR: &'static CStr = c"v16@?0@8";
}

/// A simulator's main display.
pub struct Display {
    descriptor: Retained<AnyObject>,
    token: Retained<NSUUID>,
}

/// The surface behind an object `CoreSimulator` hands over: the same object, toll-free.
fn surface_of(object: *mut AnyObject) -> Option<CFRetained<IOSurfaceRef>> {
    let surface = NonNull::new(object.cast::<IOSurfaceRef>())?;
    // SAFETY: CoreSimulator's framebuffer objects are `IOSurface`s, which are `IOSurfaceRef`s;
    // retained here for as long as the ring keeps them.
    Some(unsafe { CFRetained::retain(surface) })
}

impl Display {
    fn raw(&self) -> *mut AnyObject {
        Retained::as_ptr(&self.descriptor).cast_mut()
    }

    fn token(&self) -> *mut AnyObject {
        Retained::as_ptr(&self.token).cast::<AnyObject>().cast_mut()
    }

    /// The surface it draws into now.
    pub fn surface(&self) -> Option<CFRetained<IOSurfaceRef>> {
        guarded("reading its surface", || {
            // SAFETY: the descriptor's `framebufferSurface`, an object or nil.
            let object = unsafe { proxy::object(self.raw(), sel!(framebufferSurface)) };
            surface_of(object)
        })
        .ok()
        .flatten()
    }

    /// `frame` on every presented frame and `surface` when the surface is replaced, on `queue`.
    pub fn watch(
        &self,
        queue: sys::DispatchQueue,
        frame: impl Fn() + 'static,
        surface: impl Fn(CFRetained<IOSurfaceRef>) + 'static,
    ) -> Result<(), String> {
        // The screen forwards its callbacks to the simulator's service, which needs each block's
        // type: "Block is missing signature field" otherwise.
        let frame = RcBlock::with_encoding::<_, _, _, FrameBlock>(frame);
        let surfaces = RcBlock::with_encoding::<_, _, _, SurfacesBlock>(
            move |framebuffer: *mut AnyObject, _masked: *mut AnyObject| {
                if let Some(replaced) = surface_of(framebuffer) {
                    surface(replaced);
                }
            },
        );
        let properties = RcBlock::with_encoding::<_, _, _, PropertiesBlock>(|_: *mut AnyObject| {});
        let blocks = [
            RcBlock::as_ptr(&frame).cast_const().cast(),
            RcBlock::as_ptr(&surfaces).cast_const().cast(),
            RcBlock::as_ptr(&properties).cast_const().cast(),
        ];
        guarded("registering for its frames", || {
            // SAFETY: the method `SimScreen` declares, with blocks of its types, which it copies.
            unsafe { proxy::register(self.raw(), self.token(), queue, blocks) };
        })
    }

    pub fn unwatch(&self) {
        let _ = guarded("unregistering", || {
            // SAFETY: the pair of `watch`'s method, with the same token.
            unsafe {
                proxy::with(
                    self.raw(),
                    sel!(unregisterScreenCallbacksWithUUID:),
                    self.token(),
                );
            };
        });
    }
}
