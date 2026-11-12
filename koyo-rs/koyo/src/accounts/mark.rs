//! Each provider's mark, over its accounts in the panel: a vector drawn in one colour, as the
//! app's own marks are (`ring.rs`, `chat/subagents.rs`), so it's sharp at any scale and takes
//! the colour it's given.

use gpui_kit::{Styled as _, Svg, px, svg};

use super::model::Provider;

/// Anthropic's mark, the "A\" (as Simple Icons draw it), its view cut to the glyph's bounds.
const ANTHROPIC: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 3.541 24 16.918"><path d="M17.3041 3.541h-3.6718l6.696 16.918H24Zm-10.6082 0L0 20.459h3.7442l1.3693-3.5527h7.0052l1.3693 3.5528h3.7442L10.5363 3.5409Zm-.3712 10.2232 2.2914-5.9456 2.2914 5.9456Z"/></svg>"#;
/// Its width over its height: its view's.
const ANTHROPIC_ASPECT: f32 = 24. / 16.918;

/// `provider`'s mark, `height` high and as wide as its proportions have it: a sprite is scaled
/// by its width and drawn into its bounds, so they keep its shape.
pub fn mark(provider: Provider, height: f32) -> Svg {
    let (drawn, aspect) = match provider {
        Provider::Anthropic => (ANTHROPIC, ANTHROPIC_ASPECT),
    };
    svg()
        .data(drawn)
        .w(px(height * aspect))
        .h(px(height))
        .flex_shrink_0()
}
