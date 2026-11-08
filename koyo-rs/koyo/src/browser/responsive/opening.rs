//! The mode a page an agent opens starts in (`docs/responsive.md` 4.1): responsive mode at
//! the size the person set for pages agents open (Settings → MCP server → Agents), 1440 ×
//! 900 unless they changed it, or what the agent's call asks for instead. The pages the
//! person opens fill their pane. The mode is set before the page opens, so it opens at that
//! size whether it's drawn or out of sight; nothing of it outlives the app.

use super::pane::PANE_UNDRAWN;
use super::{Change, Responsive, SetBy, Zoom};
use crate::browser::view::BrowserView;

/// The mode a page `by` opens starts in, in a window at scale `window`: `asked`, the agent's
/// own, over the person's `default` size for pages agents open (`None`: filling the pane),
/// a side it leaves out being the default's; `None` fills the pane.
pub fn opening(
    asked: Option<&Change>,
    default: Option<(u32, u32)>,
    window: f32,
    by: &SetBy,
) -> Result<Option<Responsive>, String> {
    // The person's size, kept within the largest page this window shows, as a drag stops.
    let default = default.map(|(width, height)| {
        let free = Responsive::free(width, height, Zoom::Fit, by.clone());
        free.kept_within(window).unwrap_or(free)
    });
    let Some(asked) = asked else {
        return Ok(default);
    };
    // `resolve` refuses the rest, `fill` with anything else among it; this says it in the
    // words of an opening.
    if *asked == Change::default() {
        return Err(
            "`viewport` asks for nothing: give a `device`, a `width` or `height`, an \
             `orientation`, or `fill: true`; leave it out for the person's default."
                .into(),
        );
    }
    asked.resolve(default.as_ref(), PANE_UNDRAWN, window, by.clone())
}

impl BrowserView {
    /// Starts a page just made in `mode`, as an agent's page starts (`opening`): nothing goes
    /// to the host, as nothing has drawn or opened the page; it opens at the mode's size
    /// once drawn, or out of sight, and its session is told what the mode emulates as it
    /// opens.
    pub(crate) fn start_in(&mut self, mode: Responsive) {
        self.responsive.mode = Some(mode);
    }
}
