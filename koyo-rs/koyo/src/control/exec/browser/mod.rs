//! The browser operations, on the app's browser panes (`docs/app-control.md` 5, Browser):
//! the pages there are and the one an agent means, a page's lease, moving it, and the Chrome
//! `DevTools` protocol commands that read and act on it. Reading needs no lease; acting
//! claims a free page, and the person's page is refused with `person_has_control`. The
//! person's own pages, pinned and loose, are listed, and every tool reaches one only through
//! the gate of `pages`, with their consent, which `browser_claim` asks for. Its viewport, a
//! chosen size or a device, is set in `viewport`.

mod acts;
mod asker;
mod asking;
mod frames;
mod heard;
mod keys;
mod page;
mod pages;
mod refusals;
mod root;
mod script;
mod upload;
mod viewport;

pub(crate) use acts::{click, eval, press_key, read, screenshot, scroll, snapshot, type_text};
pub(crate) use asking::{claim, duplicate, release};
pub(crate) use heard::{console, network};
pub(crate) use upload::upload;
pub(crate) use page::{acted, dialog, go, navigate, wait};
pub(crate) use root::opening;
#[cfg(test)]
pub(crate) use pages::{DECLINED_FOR, IDLE, OTHERS_WAIT, declines_kept};
pub(crate) use pages::{joined, left, list, prune, rescope};
pub(crate) use viewport::{opening_change, viewport};
