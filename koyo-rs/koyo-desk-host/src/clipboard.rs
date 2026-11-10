//! The person's clipboard, kept theirs (`docs/agent-desktop.md` 2): there is one clipboard,
//! and an agent's copy in its app would take it. So around an agent's copy or cut, everything
//! on the clipboard is kept, what the agent copied is read off it, and the person's own is
//! put back: theirs but for the moment between, a few milliseconds as measured.

use std::time::Duration;

use objc2::{class, msg_send};

use crate::ns::{self, Obj};

/// How long a copy gets to reach the clipboard.
const COPIES_WITHIN: Duration = Duration::from_millis(500);
const TEXT: &str = "public.utf8-plain-text";

fn general() -> Obj {
    // SAFETY: the system's one general pasteboard.
    unsafe { msg_send![class!(NSPasteboard), generalPasteboard] }
}

fn count(pasteboard: &Obj) -> isize {
    // SAFETY: a pasteboard answers its change count.
    unsafe { msg_send![&**pasteboard, changeCount] }
}

/// Everything on the clipboard: each item's data under each of its types.
pub struct Kept {
    items: Vec<Vec<(Obj, Obj)>>,
    count: isize,
}

impl Kept {
    /// Keeps what the clipboard holds now.
    pub fn take() -> Kept {
        let pasteboard = general();
        // SAFETY: a pasteboard's items, each answering its types and the data of each.
        let items = unsafe {
            let items: Option<Obj> = msg_send![&*pasteboard, pasteboardItems];
            (items.map(|items| ns::items(&items)).unwrap_or_default())
                .iter()
                .map(|item| {
                    let types: Obj = msg_send![&**item, types];
                    ns::items(&types)
                        .into_iter()
                        // A promise is kept by its owner until asked for: asking would make
                        // the owner write the file it promises. It's left out, and so gone
                        // from what is put back.
                        .filter(|kind| {
                            ns::text(kind)
                                .is_none_or(|kind| !kind.to_lowercase().contains("promise"))
                        })
                        .filter_map(|kind| {
                            let data: Option<Obj> = msg_send![&**item, dataForType: &*kind];
                            Some((kind, data?))
                        })
                        .collect()
                })
                .collect()
        };
        Kept {
            items,
            count: count(&pasteboard),
        }
    }

    /// Waits for a copy made since [`Kept::take`] to reach the clipboard, reads its text and
    /// puts back what was kept. `None` when nothing was copied: the clipboard is then left
    /// as it is.
    pub fn give_back(self) -> Option<String> {
        let pasteboard = general();
        if !ns::pump_until(COPIES_WITHIN, || count(&pasteboard) != self.count) {
            return None;
        }
        // SAFETY: the pasteboard's text, nil when it holds none; then its contents replaced
        // by items made here from the data kept.
        unsafe {
            let copied: Option<Obj> = msg_send![&*pasteboard, stringForType: &*ns::string(TEXT)];
            let copied = copied.and_then(|text| ns::text(&text)).unwrap_or_default();
            let _: isize = msg_send![&*pasteboard, clearContents];
            let items: Vec<Obj> = (self.items.iter())
                .map(|kept| {
                    let item: Obj = msg_send![class!(NSPasteboardItem), new];
                    for (kind, data) in kept {
                        let _: bool = msg_send![&*item, setData: &**data, forType: &**kind];
                    }
                    item
                })
                .collect();
            if !items.is_empty() {
                let _: bool = msg_send![&*pasteboard, writeObjects: &*ns::array(&items)];
            }
            Some(copied)
        }
    }
}
