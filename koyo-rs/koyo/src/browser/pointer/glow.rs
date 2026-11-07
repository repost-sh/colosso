//! The glow round a page an agent holds, as it comes and goes.

use std::time::Duration;

use crate::fade::Fade;

/// How long the glow takes to come and go.
const GLOW_FADE: Duration = Duration::from_millis(450);

/// The glow round a page an agent holds: how much of it shows, as it comes and goes.
pub type Glow = Fade;

/// The glow of a page no agent holds yet.
pub fn glow() -> Glow {
    Fade::new(GLOW_FADE, GLOW_FADE)
}
