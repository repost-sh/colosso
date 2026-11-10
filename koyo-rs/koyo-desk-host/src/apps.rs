//! Apps on the agent desktop (`docs/agent-desktop.md` 5): found by name, started as instances
//! of their own (not activated, hidden, as UI elements), and quit again. Only what is started
//! here is ever acted on.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};

use crate::ns::{self, Obj};

/// An app the host started.
pub struct Launched {
    pub pid: i32,
    pub name: String,
    /// Its `NSRunningApplication`.
    pub app: Obj,
}

/// The Finder: the app a desktop with nothing on it has in front, and one never lent.
pub const FINDER: &str = "com.apple.finder";

/// The running app with this pid, if there is one. Any thread may ask.
pub fn by_pid(pid: i32) -> Option<Obj> {
    // SAFETY: a class method answering the running app of a pid, nil when there's none.
    unsafe {
        msg_send![
            class!(NSRunningApplication),
            runningApplicationWithProcessIdentifier: pid
        ]
    }
}

/// A running app as a launch hands it: with its pid and name.
fn launched(app: Obj) -> Launched {
    // SAFETY: a running app answers its pid and its name.
    unsafe {
        let name: Option<Obj> = msg_send![&*app, localizedName];
        Launched {
            pid: msg_send![&*app, processIdentifier],
            name: name.and_then(|name| ns::text(&name)).unwrap_or_default(),
            app,
        }
    }
}

/// The app that runs as `pid`, as a launch would have handed it: for one the host didn't
/// start.
pub fn running_app(pid: i32) -> Option<Launched> {
    by_pid(pid).map(launched)
}

/// A launch under way: Launch Services answers on a queue of its own.
#[derive(Clone)]
pub struct Launch(Arc<Mutex<Option<Result<Launched, String>>>>);

// SAFETY: the one object inside is an `NSRunningApplication`, which is documented as safe to
// use from any thread, and it's only handed from the completion queue to the main thread.
unsafe impl Send for Launched {}

impl Launch {
    /// A launch with no end yet; it stays so for an app that runs already and is only
    /// taken in.
    pub fn none() -> Launch {
        Launch(Arc::default())
    }

    /// The launch's end, once: the app, or why not.
    pub fn take(&self) -> Option<Result<Launched, String>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

fn workspace() -> Obj {
    // SAFETY: the shared workspace.
    unsafe { msg_send![class!(NSWorkspace), sharedWorkspace] }
}

fn file_url(path: &str) -> Obj {
    // SAFETY: a URL for a path string.
    unsafe { msg_send![class!(NSURL), fileURLWithPath: &*ns::string(path)] }
}

/// Where the app named `app` is: a path to a bundle, a bundle identifier, or its name in the
/// places apps live.
pub fn find(app: &str) -> Option<Obj> {
    if app.contains('/') {
        return Path::new(app).exists().then(|| file_url(app));
    }
    // SAFETY: the workspace's lookup by identifier; nil when there's none.
    let by_identifier: Option<Obj> = unsafe {
        msg_send![&*workspace(), URLForApplicationWithBundleIdentifier: &*ns::string(app)]
    };
    if by_identifier.is_some() {
        return by_identifier;
    }
    let name = app.strip_suffix(".app").unwrap_or(app);
    let home = std::env::var("HOME").unwrap_or_default();
    let listed = [
        "/Applications".to_owned(),
        "/System/Applications".to_owned(),
        "/System/Applications/Utilities".to_owned(),
        "/Applications/Utilities".to_owned(),
        format!("{home}/Applications"),
        "/System/Library/CoreServices".to_owned(),
        "/System/Library/CoreServices/Applications".to_owned(),
    ]
    .iter()
    .map(|folder| format!("{folder}/{name}.app"))
    .find(|path| Path::new(path).exists());
    // Anywhere else, Launch Services knows apps by name.
    let known = || {
        // SAFETY: the workspace's lookup by name; nil when it knows none.
        let path: Option<Obj> =
            unsafe { msg_send![&*workspace(), fullPathForApplication: &*ns::string(name)] };
        path.and_then(|path| ns::text(&path))
    };
    listed.or_else(known).map(|path| file_url(&path))
}

/// The bundle identifier of the app at `url`.
pub fn bundle_id(url: &Obj) -> Option<String> {
    // SAFETY: a bundle for a URL, nil when it isn't one; its identifier, nil when it has none.
    unsafe {
        let bundle: Option<Obj> = msg_send![class!(NSBundle), bundleWithURL: &**url];
        let identifier: Option<Obj> = msg_send![&*bundle?, bundleIdentifier];
        ns::text(&*identifier?)
    }
}

/// The pids of the running apps with this bundle identifier.
pub fn running(bundle: &str) -> Vec<i32> {
    // SAFETY: a class method returning an array of running apps, each answering its pid.
    unsafe {
        let apps: Obj = msg_send![
            class!(NSRunningApplication),
            runningApplicationsWithBundleIdentifier: &*ns::string(bundle)
        ];
        ns::items(&apps)
            .iter()
            .map(|app| msg_send![&**app, processIdentifier])
            .collect()
    }
}

/// The first running app with this bundle identifier, by its pid.
pub fn first_running(bundle: &str) -> Option<i32> {
    running(bundle).first().copied()
}

/// Brings the app forward, as Launch Services does it: its windows come with it, and
/// macOS goes to the desktop they're on.
pub fn activate(app: &Obj) -> bool {
    // SAFETY: a running app answers `activateWithOptions:`.
    unsafe { msg_send![&**app, activateWithOptions: 0usize] }
}

/// Starts the app at `url` with `document` (a path or a URL) and `arguments`: an instance of
/// its own when `fresh`, else the one that runs if one does.
pub fn start(url: &Obj, document: Option<&str>, arguments: &[String], fresh: bool) -> Launch {
    // SAFETY: a configuration built here, with values of the kinds its setters take.
    let configuration = unsafe {
        let configuration = quiet();
        let _: () = msg_send![&*configuration, setCreatesNewApplicationInstance: fresh];
        let _: () = msg_send![&*configuration, setHides: true];
        if !arguments.is_empty() {
            let arguments: Vec<Obj> = arguments.iter().map(|text| ns::string(text)).collect();
            let _: () = msg_send![&*configuration, setArguments: &*ns::array(&arguments)];
        }
        launch_options(&configuration);
        configuration
    };
    open(url, document, &configuration)
}

/// Asks the instance of the app that runs, the person's own that is lent, to open
/// `document`, or to open again as a click on its Dock icon asks. Nothing is started, and
/// nothing about the app is changed: it isn't hidden, as a start of the agent's own is.
pub fn ask_running(url: &Obj, document: Option<&str>) {
    drop(open(url, document, &quiet()));
}

/// A launch that takes nothing from the person: the app isn't made the front one, and
/// isn't put among their recent items.
fn quiet() -> Obj {
    // SAFETY: the configuration's own constructor and setters.
    unsafe {
        let configuration: Obj = msg_send![class!(NSWorkspaceOpenConfiguration), configuration];
        let _: () = msg_send![&*configuration, setActivates: false];
        let _: () = msg_send![&*configuration, setAddsToRecentItems: false];
        configuration
    }
}

/// Asks the workspace for the app at `url`, with `document` when there's one.
fn open(url: &Obj, document: Option<&str>, configuration: &Obj) -> Launch {
    let launch = Launch::none();
    let told = launch.clone();
    let done = RcBlock::new(move |app: *mut AnyObject, error: *mut AnyObject| {
        // SAFETY: Launch Services hands a running app or an error, either may be nil.
        let result = unsafe {
            if let Some(app) = Retained::retain(app) {
                Ok(launched(app))
            } else {
                let said: Option<Obj> = match error.as_ref() {
                    Some(error) => msg_send![error, localizedDescription],
                    None => None,
                };
                Err(said
                    .and_then(|said| ns::text(&said))
                    .unwrap_or_else(|| "the app didn't start".to_owned()))
            }
        };
        *told.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
        ns::wake();
    });
    // SAFETY: the workspace's launch calls, with a configuration built by the caller and a
    // block that is copied by the callee.
    unsafe {
        match document {
            Some(document) => {
                let document: Option<Obj> = if document.contains("://") {
                    msg_send![class!(NSURL), URLWithString: &*ns::string(document)]
                } else {
                    Some(file_url(document))
                };
                let documents = ns::array(&document.into_iter().collect::<Vec<_>>());
                let _: () = msg_send![
                    &*workspace(),
                    openURLs: &*documents,
                    withApplicationAtURL: &**url,
                    configuration: &**configuration,
                    completionHandler: &*done
                ];
            }
            None => {
                let _: () = msg_send![
                    &*workspace(),
                    openApplicationAtURL: &**url,
                    configuration: &**configuration,
                    completionHandler: &*done
                ];
            }
        }
    }
    launch
}

/// `'rsto'` and `'no  '`: the open event's parameter that tells an app not to restore the
/// windows of its last run, as `open --fresh` sends it.
const RESTORE: i32 = 0x7273_746f;
const NO: u32 = 0x6e6f_2020;

/// One of Launch Services' own option names, read from the constant it exports; the
/// constant's name itself when it isn't found, which is what its value has been.
fn option(name: &std::ffi::CStr) -> Obj {
    crate::sys::loaded_symbol(name)
        // SAFETY: the symbol is a constant holding a string, which lives as long as its library.
        .and_then(|address| ns::kept(unsafe { *address.cast::<crate::sys::Ref>() }))
        .unwrap_or_else(|| ns::string(&name.to_string_lossy()))
}

/// Asks Launch Services, through the configuration's private options, for what its public
/// ones don't offer: an app with no Dock icon and no place in the app switcher, started
/// fresh, so the agent's instance doesn't reopen the windows the person's last had. Left as
/// it is on a macOS that hasn't the options.
fn launch_options(configuration: &Obj) {
    let set = std::panic::AssertUnwindSafe(|| {
        // SAFETY: a dictionary built here; key-value coding on the configuration, where an
        // unknown key raises, caught below.
        unsafe {
            let options: Obj = msg_send![class!(NSMutableDictionary), dictionary];
            let yes = ns::yes();
            let restore: Obj = msg_send![class!(NSNumber), numberWithInt: RESTORE];
            let no: Obj = msg_send![class!(NSAppleEventDescriptor), descriptorWithEnumCode: NO];
            for (value, key) in [
                (&yes, c"_kLSOpenOptionUIElementLaunchKey"),
                (&restore, c"_kLSOpenOptionAEParamKeyKey"),
                (&no, c"_kLSOpenOptionAEParamDescKey"),
            ] {
                let _: () = msg_send![&*options, setObject: &**value, forKey: &*option(key)];
            }
            let _: () = msg_send![
                &**configuration,
                setValue: &*options,
                forKey: &*ns::string("_additionalLSOpenOptions")
            ];
        }
    });
    let _ = objc2::exception::catch(set);
}

/// The name and bundle identifier of a running app.
pub fn named(app: &Obj) -> Option<(String, String)> {
    // SAFETY: a running app answers its name and its bundle's identifier, nil for none.
    unsafe {
        let name: Option<Obj> = msg_send![&**app, localizedName];
        let bundle: Option<Obj> = msg_send![&**app, bundleIdentifier];
        Some((ns::text(&*name?)?, ns::text(&*bundle?)?))
    }
}

/// Starts an instance of the app `bundle` for the person, as their own click would have:
/// a plain launch, in front. Only ever on the person's own word (a button in Koyo).
pub fn start_for_person(bundle: &str) {
    // SAFETY: the workspace's lookup and launch calls, with a plain configuration and no
    // completion handler.
    unsafe {
        let url: Option<Obj> = msg_send![
            &*workspace(),
            URLForApplicationWithBundleIdentifier: &*ns::string(bundle)
        ];
        let Some(url) = url else { return };
        let configuration: Obj = msg_send![class!(NSWorkspaceOpenConfiguration), configuration];
        let _: () = msg_send![&*configuration, setCreatesNewApplicationInstance: true];
        let _: () = msg_send![
            &*workspace(),
            openApplicationAtURL: &*url,
            configuration: &*configuration,
            completionHandler: ns::no_handler::<dyn Fn(*mut AnyObject, *mut AnyObject)>()
        ];
    }
}

/// Shows a hidden app's windows.
pub fn unhide(app: &Obj) {
    // SAFETY: a running app answers `unhide`.
    let _: bool = unsafe { msg_send![&**app, unhide] };
}

/// Whether the app is hidden.
pub fn hidden(app: &Obj) -> bool {
    // SAFETY: a running app answers `isHidden`.
    unsafe { msg_send![&**app, isHidden] }
}

/// Whether the app's process has ended.
pub fn ended(app: &Obj) -> bool {
    // SAFETY: a running app answers `isTerminated`.
    unsafe { msg_send![&**app, isTerminated] }
}

/// Asks the app to quit; `force` kills it.
pub fn quit(app: &Obj, force: bool) {
    // SAFETY: a running app answers both.
    let _: bool = unsafe {
        if force {
            msg_send![&**app, forceTerminate]
        } else {
            msg_send![&**app, terminate]
        }
    };
}
