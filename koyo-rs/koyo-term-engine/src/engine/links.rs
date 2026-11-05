//! The links a program makes of its text (OSC 8), read off a row as it's copied into a frame.

use ghostty_vt::Terminal;
use ghostty_vt::terminal::{Point, PointCoordinate};

use crate::frame::{Cell, RowLink, Width};

/// The links in the viewport's row `y`, read from the cells at columns `linked`, the ones
/// that have one: neighbours with the same URI make one link, the tail of a wide character
/// included. `uri` is where a URI is read into, grown to the longest seen. A link that can't
/// be read is left out.
pub(super) fn read(
    term: &Terminal,
    y: i32,
    linked: &[u16],
    cells: &[Cell],
    uri: &mut Vec<u8>,
) -> Vec<RowLink> {
    let Ok(y) = u32::try_from(y) else {
        return Vec::new();
    };
    let tail = |col: u16| {
        (cells.get(usize::from(col))).is_some_and(|cell| cell.width == Width::SpacerTail)
    };
    let mut links: Vec<RowLink> = Vec::new();
    for &x in linked {
        let Ok(at) = term.grid_ref(Point::Viewport(PointCoordinate { x, y })) else {
            continue;
        };
        let len = match at.hyperlink_uri(uri) {
            Err(crate::Error::OutOfSpace { required }) => {
                uri.resize(required, 0);
                at.hyperlink_uri(uri)
            }
            read => read,
        };
        let Some(bytes) = (len.ok())
            .and_then(|len| uri.get(..len))
            .filter(|bytes| !bytes.is_empty())
        else {
            continue;
        };
        let after_tail = |end: u16| end + 1 == x && tail(end);
        match links.last_mut() {
            Some(last)
                if last.uri.as_bytes() == bytes
                    && (last.cols.end == x || after_tail(last.cols.end)) =>
            {
                last.cols.end = x + 1;
            }
            _ => links.push(RowLink {
                cols: x..x + 1,
                uri: String::from_utf8_lossy(bytes).into(),
            }),
        }
    }
    for link in &mut links {
        if tail(link.cols.end) {
            link.cols.end += 1;
        }
    }
    links
}
