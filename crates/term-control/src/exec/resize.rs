//! Sizing a terminal for every viewer (`docs/responsive.md` 3.1, 4.2): `terminal_resize` pins
//! its cells or unpins it, then, as agent-terminal's `resize` does, waits for the program to
//! redraw and reads the screen; and the size `terminal_open` was given, pinned for the agent
//! that asked for it.

use serde_json::{Value, json};

use super::{Done, Executor, invalid, shape, unexpected};
use crate::catalog::args::{CellsArg, ReadSpec, ResizeArgs, Then, WaitFor, WaitSpec};
use crate::{
    ErrorCode, Handle, Pinned, ProtocolError, RequestBody, ResponseBody, Size, TerminalRecord,
    pin_cells_error,
};

/// How long the screen holds still before a resize's answer reads it, and the most it waits:
/// a program redraws after `SIGWINCH`, so the screen right after the resize is the old one.
const SETTLE_MS: u64 = 300;
const SETTLE_TIMEOUT_MS: u64 = 25_000;

impl Executor {
    pub(super) fn resize(&self, args: ResizeArgs) -> Result<Done, ProtocolError> {
        let cells = cells(&args)?;
        let terminal = self.terminal(args.terminal.as_deref())?;
        let handle = terminal.record.handle.clone();
        let free = terminal.record.control.holder.is_none();
        let (size, pinned) = self.pin(&handle, cells, free)?;
        self.remember(&handle);
        let then = args.then.unwrap_or_else(settle);
        let waited = (then.wait.as_ref())
            .map(|spec| self.wait_for(&handle, spec, None))
            .transpose()?;
        let read = (then.read.as_ref())
            .map(|spec| self.read_spec(&handle, spec))
            .transpose()?;
        let name = &terminal.record.name;
        let mut summary = match &pinned {
            Some(pinned) => format!("pinned {name} at {}x{}", pinned.cols, pinned.rows),
            None => format!("{name} fills its views again, {}x{}", size.cols, size.rows),
        };
        if let Some(waited) = &waited {
            summary = format!("{summary}; {}", waited.words());
        }
        let mut structured = json!({
            "size": { "cols": size.cols, "rows": size.rows },
            "pinned": pinned.as_ref().map(shape::pinned),
        });
        if let Some(waited) = &waited {
            structured["wait"] = waited.json();
        }
        if let Some((read, _)) = read {
            structured["read"] = read;
        }
        Ok(Done {
            structured,
            summary,
        })
    }

    /// Pins `handle` at `cells`, or unpins it with `None`; a terminal no one holds is claimed
    /// first when `free`, as an agent takes one to write to it.
    fn pin(
        &self,
        handle: &Handle,
        cells: Option<(u16, u16)>,
        free: bool,
    ) -> Result<(Size, Option<Pinned>), ProtocolError> {
        let pin = || RequestBody::Pin {
            handle: handle.clone(),
            size: cells.map(|(cols, rows)| Size::cells(cols, rows)),
        };
        let answer = match self.request(pin()) {
            Err(error) if error.code == ErrorCode::ControlRequired && free => {
                let claim = RequestBody::Claim {
                    handle: handle.clone(),
                    takeover: false,
                    reason: None,
                };
                self.request(claim)?;
                self.request(pin())?
            }
            answer => answer?,
        };
        match answer {
            ResponseBody::Pinned { size, pinned } => Ok((size, pinned)),
            other => Err(unexpected(&other)),
        }
    }

    /// Pins the size `terminal_open` was given, so the first view attaching doesn't size it
    /// away. What refused it doesn't undo the open, which stands: the answer says it.
    pub(super) fn pin_opened(
        &self,
        record: &mut TerminalRecord,
        size: CellsArg,
    ) -> Result<(), ProtocolError> {
        within(size.cols, size.rows)?;
        let (now, pinned) = self.pin(&record.handle, Some((size.cols, size.rows)), false)?;
        record.size = now;
        record.pinned = pinned;
        Ok(())
    }
}

/// What `terminal_open`'s answer says of the size it pinned, after its summary: `pinned` with
/// the pin, or `pinned: null` and `pinRefused` with what refused it.
pub(super) fn opened(
    structured: &mut Value,
    record: &TerminalRecord,
    pinned: Result<(), ProtocolError>,
) -> String {
    let size = record.size;
    match pinned {
        Ok(()) => {
            structured["pinned"] = record.pinned.as_ref().map_or(Value::Null, shape::pinned);
            format!("; pinned at {}x{}", size.cols, size.rows)
        }
        Err(error) => {
            structured["pinned"] = Value::Null;
            structured["pinRefused"] = json!({ "code": error.code, "message": error.message });
            format!("; not pinned: {}", error.message)
        }
    }
}

/// The cells `args` ask for, `None` to unpin.
fn cells(args: &ResizeArgs) -> Result<Option<(u16, u16)>, ProtocolError> {
    match (args.cols, args.rows, args.fill) {
        (None, None, Some(true)) => Ok(None),
        (_, _, Some(true)) => Err(invalid("fill: true takes no cols or rows")),
        (Some(cols), Some(rows), _) => within(cols, rows).map(|()| Some((cols, rows))),
        _ => Err(invalid("give cols and rows, or fill: true")),
    }
}

fn within(cols: u16, rows: u16) -> Result<(), ProtocolError> {
    pin_cells_error(cols, rows).map_or(Ok(()), |message| Err(invalid(message)))
}

/// What a resize does after without `then`: waits for the screen to hold still, and reads it.
fn settle() -> Then {
    Then {
        wait: Some(WaitSpec {
            until: WaitFor::Stable,
            pattern: None,
            rows: None,
            stable_ms: Some(SETTLE_MS),
            timeout_ms: Some(SETTLE_TIMEOUT_MS),
        }),
        read: Some(ReadSpec::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(value: Value) -> ResizeArgs {
        serde_json::from_value(value).unwrap_or_default()
    }

    #[test]
    fn a_resize_asks_for_cells_within_the_limits_or_for_fill() {
        let ask = |value| cells(&args(value)).map_err(|error| error.code);
        assert_eq!(ask(json!({ "cols": 120, "rows": 40 })), Ok(Some((120, 40))));
        assert_eq!(ask(json!({ "cols": 2, "rows": 1000 })), Ok(Some((2, 1000))));
        assert_eq!(ask(json!({ "fill": true })), Ok(None));
        let refused = Err(ErrorCode::InvalidArgument);
        assert_eq!(ask(json!({ "cols": 1, "rows": 24 })), refused);
        assert_eq!(ask(json!({ "cols": 80, "rows": 1001 })), refused);
        assert_eq!(ask(json!({ "cols": 80 })), refused);
        assert_eq!(ask(json!({})), refused);
        assert_eq!(ask(json!({ "fill": false })), refused);
        assert_eq!(
            ask(json!({ "fill": true, "cols": 80, "rows": 24 })),
            refused
        );
    }

    #[test]
    fn without_then_a_resize_waits_for_a_still_screen_and_reads_it() {
        let then = settle();
        let wait = then.wait.unwrap();
        assert_eq!(wait.until, WaitFor::Stable);
        assert_eq!((wait.stable_ms, wait.timeout_ms), (Some(300), Some(25_000)));
        assert!(then.read.is_some());
    }
}
