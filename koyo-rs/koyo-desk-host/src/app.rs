//! An app the host started, and the way to its windows' accessibility elements
//! (`docs/agent-desktop.md` 7): an app doesn't list its windows while their desktop isn't
//! showing, so they are learnt from its focused element, from the notices of windows it
//! makes, and at last by the numbers it gives its elements.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::apps::Launched;
use crate::ax::{Element, Observer};
use crate::ns::{self, Obj};

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
            observer: None,
            created: Rc::default(),
            elements: HashMap::new(),
        };
        app.watch();
        app
    }

    /// Starts watching for the windows it makes; again later if the app wasn't ready to be
    /// watched. Also asks a Chromium or Electron app for its page's tree, which those build
    /// only when asked.
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
            app.set("AXManualAccessibility", &ns::yes());
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

/// An app's own data folder for an app that shares none with a second instance (a Chromium
/// browser, an Electron app): `None` for every other app.
pub fn own_profile(app: &Obj, bundle: Option<&str>, state: &std::path::Path) -> Option<String> {
    // SAFETY: a file URL answers `path`.
    let path: Option<Obj> = unsafe { objc2::msg_send![&**app, path] };
    let path = std::path::PathBuf::from(ns::text(&*path?)?);
    let frameworks = std::fs::read_dir(path.join("Contents/Frameworks")).ok()?;
    let chromium = frameworks.flatten().any(|entry| {
        let name = entry.file_name().to_string_lossy().into_owned();
        name == "Electron Framework.framework"
            || name == "Chromium Embedded Framework.framework"
            || name.ends_with(" Framework.framework")
                && entry
                    .path()
                    .join("Helpers")
                    .join("chrome_crashpad_handler")
                    .exists()
    });
    if !chromium {
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
