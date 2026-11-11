//! An app the host started, and the way to its windows' accessibility elements
//! (`docs/agent-desktop.md` 7): an app doesn't list its windows while their desktop isn't
//! showing, so they are learnt from its focused element, from the notices of windows it
//! makes, and at last by the numbers it gives its elements.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::apps::Launched;
use crate::ax::{Element, Observer};
use crate::ns::{self, Obj};
use crate::skylight::SpaceId;

/// How long the search by number may take, and how far it counts.
const SEARCH_WITHIN: Duration = Duration::from_millis(250);
const SEARCH_TO: u64 = 3000;

pub struct App {
    pub pid: i32,
    pub name: String,
    /// When its process started, which with its pid names it for good.
    pub started: u64,
    /// Its `NSRunningApplication`.
    pub running: Obj,
    /// Whether it was told it's active, which makes its window key (once is enough).
    pub told_active: bool,
    /// When it was asked to quit.
    pub quitting: Option<Instant>,
    /// The person's own, running before an agent asked for it and lent on their word
    /// (`lending`): given back, never quit.
    pub lent: bool,
    /// The desktop a lent app came from, where it goes back to.
    pub home: Option<SpaceId>,
    /// The host turned on what a screen reader sets on the app ([`ask_for_pages`]): put
    /// back when a lent app goes home.
    asked_pages: bool,
    observer: Option<Observer>,
    /// Windows it said it made, not yet filed under their numbers.
    created: Rc<RefCell<Vec<Element>>>,
    elements: HashMap<u32, Element>,
}

impl App {
    pub fn new(launched: Launched) -> App {
        let mut app = App {
            pid: launched.pid,
            name: launched.name,
            started: crate::desktop::started_at(launched.pid).unwrap_or_default(),
            running: launched.app,
            told_active: false,
            quitting: None,
            lent: false,
            home: None,
            asked_pages: false,
            observer: None,
            created: Rc::default(),
            elements: HashMap::new(),
        };
        app.watch();
        app
    }

    /// The person's own app `running`, taken in as lent from its desktop `home`.
    pub fn lent(running: Launched, home: Option<SpaceId>) -> App {
        App {
            lent: true,
            home,
            ..App::new(running)
        }
    }

    /// Starts watching for the windows it makes; again later if the app wasn't ready to be
    /// watched. Also asks an app that draws its own pages for their tree ([`ask_for_pages`]).
    pub fn watch(&mut self) {
        if self.observer.is_some() {
            return;
        }
        let created = Rc::clone(&self.created);
        self.observer = Observer::new(self.pid, "AXWindowCreated", move |window| {
            created.borrow_mut().push(window);
        });
        if self.observer.is_some()
            && let Some(app) = Element::app(self.pid)
        {
            let chromium = bundle_path(&self.running).is_some_and(|path| chromium(&path));
            self.asked_pages = ask_for_pages(&app, chromium);
        }
    }

    /// Leaves a lent app as it was found: what a screen reader sets on it goes off again,
    /// when it was the host that turned it on.
    pub fn leave_as_found(&mut self) {
        if std::mem::take(&mut self.asked_pages)
            && let Some(app) = Element::app(self.pid)
        {
            app.set(ENHANCED, &ns::no());
        }
    }

    /// Windows the app made since the last call.
    pub fn take_created(&mut self) -> Vec<Element> {
        let created: Vec<Element> = self.created.borrow_mut().drain(..).collect();
        let mut filed = Vec::new();
        for window in created {
            if let Some(id) = window.window_id() {
                self.elements.insert(id, window.clone());
                filed.push(window);
            }
        }
        filed
    }

    fn learn(&mut self) {
        let Some(app) = Element::app(self.pid) else {
            return;
        };
        let mut found = app.elements("AXWindows");
        found.extend(app.element("AXFocusedWindow"));
        found.extend(app.element("AXMainWindow"));
        found.extend(
            app.element("AXFocusedUIElement")
                .and_then(|focused| focused.element("AXWindow")),
        );
        for window in found {
            if let Some(id) = window.window_id() {
                self.elements.insert(id, window);
            }
        }
        self.take_created();
    }

    /// Looks through the numbers the app gives its elements for its windows.
    fn search(&mut self, wanted: u32) {
        let started = Instant::now();
        for number in 0..SEARCH_TO {
            if started.elapsed() > SEARCH_WITHIN {
                return;
            }
            let Some(element) = Element::numbered(self.pid, number) else {
                return;
            };
            if element.text("AXRole").as_deref() != Some("AXWindow") {
                continue;
            }
            if let Some(id) = element.window_id() {
                self.elements.insert(id, element);
                if id == wanted {
                    return;
                }
            }
        }
    }

    /// The window the app has its keys in, as the app itself says.
    pub fn key_window(&self) -> Option<u32> {
        Element::app(self.pid)?
            .element("AXFocusedWindow")?
            .window_id()
    }

    /// The accessibility element of the app's window `id`.
    pub fn window(&mut self, id: u32) -> Option<Element> {
        let live = |element: &Element| element.text("AXRole").is_some();
        if let Some(known) = self.elements.get(&id).filter(|known| live(known)) {
            return Some(known.clone());
        }
        self.elements.remove(&id);
        self.learn();
        if !self.elements.contains_key(&id) {
            self.search(id);
        }
        self.elements.get(&id).cloned()
    }

    /// Forgets the windows that are gone.
    pub fn keep_only(&mut self, windows: &[u32]) {
        self.elements.retain(|id, _| windows.contains(id));
    }

    pub fn ended(&self) -> bool {
        crate::apps::ended(&self.running)
    }

    /// The app element, for its menu bar.
    pub fn element(&self) -> Option<Element> {
        Element::app(self.pid)
    }
}

/// The attribute a screen reader sets on an app.
const ENHANCED: &str = "AXEnhancedUserInterface";

/// Asks an app that draws its own pages for their tree, which it builds only when asked.
/// An Electron app is asked by an attribute of its own. A Chromium browser hasn't that one
/// and builds its pages' trees for a screen reader, so it gets what a screen reader sets:
/// Chrome says it took neither, and the pages are in its tree from then on (Chrome for
/// Testing 152: toolbar only before, the page after). No other app is set that: it changes
/// how an app behaves for everyone. Whether it was turned on here: not when it was on
/// already, as for a person who uses a screen reader.
fn ask_for_pages(app: &Element, chromium: bool) -> bool {
    if app.set("AXManualAccessibility", &ns::yes()) || !chromium {
        return false;
    }
    let on = (app.get(ENHANCED)).is_some_and(|value| ns::integer(&value) == Some(1));
    if on {
        return false;
    }
    app.set(ENHANCED, &ns::yes());
    true
}

/// Where a running app's bundle is.
fn bundle_path(running: &Obj) -> Option<PathBuf> {
    // SAFETY: a running app answers `bundleURL`, nil for a process with no bundle; a file
    // URL answers `path`.
    let path: Option<Obj> = unsafe {
        let url: Option<Obj> = objc2::msg_send![&**running, bundleURL];
        objc2::msg_send![&*url?, path]
    };
    Some(PathBuf::from(ns::text(&*path?)?))
}

/// Whether the app at `path` is built on Chromium: an Electron app, one on the Chromium
/// Embedded Framework, or a Chromium browser, by the framework and helper they ship.
fn chromium(path: &Path) -> bool {
    let Ok(frameworks) = std::fs::read_dir(path.join("Contents/Frameworks")) else {
        return false;
    };
    frameworks.flatten().any(|entry| {
        let name = entry.file_name().to_string_lossy().into_owned();
        name == "Electron Framework.framework"
            || name == "Chromium Embedded Framework.framework"
            || name.ends_with(" Framework.framework")
                && entry
                    .path()
                    .join("Helpers")
                    .join("chrome_crashpad_handler")
                    .exists()
    })
}

/// An app's own data folder for an app that shares none with a second instance (a Chromium
/// browser, an Electron app): `None` for every other app.
pub fn own_profile(app: &Obj, bundle: Option<&str>, state: &Path) -> Option<String> {
    // SAFETY: a file URL answers `path`.
    let path: Option<Obj> = unsafe { objc2::msg_send![&**app, path] };
    if !chromium(&PathBuf::from(ns::text(&*path?)?)) {
        return None;
    }
    // Short: the app keeps a socket under it, whose path has a limit of 103 bytes.
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bundle.unwrap_or("app").bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    let folder = state.join("p").join(format!("{:08x}", hash & 0xffff_ffff));
    std::fs::create_dir_all(&folder).ok()?;
    Some(folder.to_string_lossy().into_owned())
}

/// Forgets the windows the agent's instance of a Chromium browser had open when it last
/// quit, kept in each of its profiles. Started by the host it brings them all back, on top
/// of what it is started for: one window more with every start (Chrome for Testing 152
/// had 28 after as many, each drawing, 5 GB between them). Its other data stays.
pub fn forget_windows(profile: &Path) {
    let Ok(folders) = std::fs::read_dir(profile) else {
        return;
    };
    for folder in folders.flatten() {
        let _ = std::fs::remove_dir_all(folder.path().join("Sessions"));
    }
}

#[cfg(test)]
mod tests {
    use super::{chromium, forget_windows};

    /// What a Chromium browser keeps of its last run's windows goes, in every profile; its
    /// other data, and a folder that isn't one of its own, stay.
    #[test]
    fn a_browsers_saved_windows_are_forgotten_and_nothing_else() {
        let root = std::env::temp_dir().join(format!("koyo-sessions-{}", std::process::id()));
        for kept in ["Default/Sessions", "Profile 1/Sessions", "Default/Cache"] {
            std::fs::create_dir_all(root.join(kept)).unwrap();
        }
        std::fs::write(root.join("Default/Sessions/Session_1"), "windows").unwrap();
        std::fs::write(root.join("Default/Cookies"), "cookies").unwrap();
        std::fs::write(root.join("Local State"), "{}").unwrap();
        forget_windows(&root);
        assert!(!root.join("Default/Sessions").exists());
        assert!(!root.join("Profile 1/Sessions").exists());
        for stays in ["Default/Cookies", "Default/Cache", "Profile 1", "Local State"] {
            assert!(root.join(stays).exists(), "{stays}");
        }
        forget_windows(&root.join("missing"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// An app is taken for one built on Chromium by what it ships: Electron's framework,
    /// the embedded framework, or a browser's own framework with Chromium's crash helper.
    /// A framework that only ends like a browser's isn't enough.
    #[test]
    fn an_app_is_known_as_chromium_by_the_framework_it_ships() {
        let root = std::env::temp_dir().join(format!("koyo-chromium-{}", std::process::id()));
        let app = |name: &str, inside: &[&str]| {
            let app = root.join(name);
            for folder in inside {
                std::fs::create_dir_all(app.join("Contents/Frameworks").join(folder)).unwrap();
            }
            app
        };
        let helper = "Google Chrome Framework.framework/Helpers/chrome_crashpad_handler";
        assert!(chromium(&app("Code.app", &["Electron Framework.framework"])));
        assert!(chromium(&app(
            "Embeds.app",
            &["Chromium Embedded Framework.framework"]
        )));
        assert!(chromium(&app("Chrome.app", &[helper])));
        assert!(!chromium(&app("Other.app", &["Some Framework.framework"])));
        assert!(!chromium(&app("Plain.app", &[])));
        assert!(!chromium(&root.join("Missing.app")));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
