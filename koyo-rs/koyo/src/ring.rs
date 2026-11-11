//! A ring that fills as a share does: how full a chat's conversation is (`chat/composer`), and
//! how much of an account's week is used (`accounts`).

use gpui_kit::{Styled as _, Svg, px, svg};

/// How long a ring's stroke goes all the way around: its radius, by 2π.
const RING_LENGTH: f32 = 56.55;

/// A ring `percent` full, in the color it's given. A sprite, the same one until that changes: the
/// kit's ring is a path, and a path in view has every frame of a streaming answer draw the window
/// over to antialias it.
pub fn ring(percent: u8) -> Svg {
    let filled = RING_LENGTH * f32::from(percent.min(100)) / 100.;
    let drawn = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="3"><circle cx="12" cy="12" r="9" opacity="0.25"/><circle cx="12" cy="12" r="9" stroke-dasharray="{filled} {RING_LENGTH}" transform="rotate(-90 12 12)"/></svg>"#
    );
    svg().data(drawn.as_bytes()).size(px(20.)).flex_shrink_0()
}
