//! The browser operations, on the app's browser panes (`docs/app-control.md` 5, Browser):
//! the page an agent means, listing pages, the page's lease, moving it, and the Chrome
//! `DevTools` protocol commands that read and act on it. Reading needs no lease; acting
//! claims a free page, and the person's page is refused with `person_has_control`.

mod acts;
mod keys;
mod page;
mod script;

pub(crate) use acts::{click, eval, press_key, read, screenshot, scroll, snapshot, type_text};
pub(crate) use page::{claim, go, list, navigate, release, wait};
