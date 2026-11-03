//! `Theme`: the app's theme, which every terminal's program is told. Its default colours
//! answer OSC 10 and 11 queries, and `dark` answers CSI ? 996 n and is reported to a program
//! that asked to hear of changes (mode 2031). New shells get it as `COLORFGBG` too.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Theme {
    pub foreground: [u8; 3],
    pub background: [u8; 3],
    pub dark: bool,
}
