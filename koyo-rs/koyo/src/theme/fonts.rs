//! What text is in. Code is in [`MONO`] unless a person picks otherwise, embedded in the binary so
//! it looks the same on a Mac that never installed it. Everything else is in the kit's
//! `.SystemUIFont`, which is macOS's own San Francisco (SF Pro): Apple's licence has an app use
//! that one from the system rather than embed it.

use std::borrow::Cow;
use std::panic::Location;
use std::sync::OnceLock;

use gpui_kit::component::Theme;
use gpui_kit::{
    AnyElement, App, Bounds, Element, ElementId, Global, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, Window, px,
};

use crate::settings::{self, Fonts, SetCodeFont, SetInterfaceFont};

/// The family the app ships: what its faces, below, report to Core Text. Not the shorter
/// `JetBrainsMono NF` in their legacy name field, which macOS doesn't go by.
pub const MONO: &str = "JetBrainsMono Nerd Font";
/// The system's own family, which isn't among the ones that can be listed.
pub const SYSTEM: &str = ".SystemUIFont";

/// Every style code is drawn in, so none is synthesized. `fonts/README.md` says where they're
/// from.
const FACES: [&[u8]; 4] = [
    include_bytes!("../../fonts/JetBrainsMonoNerdFont-Regular.ttf"),
    include_bytes!("../../fonts/JetBrainsMonoNerdFont-Bold.ttf"),
    include_bytes!("../../fonts/JetBrainsMonoNerdFont-Italic.ttf"),
    include_bytes!("../../fonts/JetBrainsMonoNerdFont-BoldItalic.ttf"),
];

fn faces() -> Vec<Cow<'static, [u8]>> {
    Vec::from(FACES.map(Cow::Borrowed))
}

/// The families in use: what the settings name, where it's there to use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InUse {
    interface: &'static str,
    code: &'static str,
}

impl Global for InUse {}

impl InUse {
    /// What `fonts` names among `families`. A font since uninstalled, or misspelled in the file,
    /// goes back to the default rather than to GPUI's fallback, which isn't monospaced.
    fn of(fonts: &Fonts, families: &'static [String]) -> Self {
        let named = |chosen: Option<&str>, default: &'static str| {
            let chosen = chosen.and_then(|chosen| families.iter().find(|family| *family == chosen));
            chosen.map_or(default, String::as_str)
        };
        InUse {
            interface: named(fonts.interface.as_deref(), SYSTEM),
            code: named(fonts.code.as_deref(), MONO),
        }
    }
}

/// The family everything but code is in.
pub fn interface(cx: &App) -> &'static str {
    cx.try_global::<InUse>()
        .map_or(SYSTEM, |in_use| in_use.interface)
}

/// The family code is in.
pub fn code(cx: &App) -> &'static str {
    cx.try_global::<InUse>().map_or(MONO, |in_use| in_use.code)
}

/// The families there are to pick, sorted: the bundled one with what's installed, without the
/// ones macOS hides. Asked once, as a font installed later is only seen by the next launch.
pub fn families(cx: &App) -> &'static [String] {
    static FAMILIES: OnceLock<Vec<String>> = OnceLock::new();
    FAMILIES.get_or_init(|| {
        let mut families = cx.text_system().all_font_names();
        families.retain(|family| !family.starts_with('.'));
        families.sort_unstable();
        families.dedup();
        families
    })
}

/// Loads the faces, puts the settings' families to use, and keeps to them as they change.
pub(super) fn init(cx: &mut App) {
    if let Err(error) = cx.text_system().add_fonts(faces()) {
        // Code is then in GPUI's fallback font, which is legible, if not monospaced.
        log::error!("couldn't load the bundled code font: {error}");
    }
    cx.on_action(|picked: &SetInterfaceFont, cx| {
        settings::update(cx, |settings| {
            settings.fonts.interface.clone_from(&picked.0);
        });
    });
    cx.on_action(|picked: &SetCodeFont, cx| {
        settings::update(cx, |settings| settings.fonts.code.clone_from(&picked.0));
    });
    apply(cx);
    settings::observe_app(cx, apply).detach();
}

/// Names the families on the theme too, for what the kit draws: text views, code blocks, editors.
/// Koyo's themes name no font, so this lasts through every change of appearance. Settings change
/// for all sorts of reasons, and only a change of font is news to the theme's observers.
fn apply(cx: &mut App) {
    let in_use = InUse::of(&settings::get(cx).fonts, families(cx));
    if cx.try_global::<InUse>() == Some(&in_use) {
        return;
    }
    cx.set_global(in_use);
    Theme::update(cx, |theme| {
        theme.font_family = in_use.interface.into();
        theme.mono_font_family = in_use.code.into();
    });
}

/// `child` at the chat's text size: its rems are that size, and most of what's in a chat is sized
/// in rems, so its text and the room around it grow together, as iOS's text sizes do.
pub fn at_chat_size(child: impl IntoElement, cx: &App) -> AtRem {
    AtRem {
        rem: px(settings::get(cx).fonts.chat_size()),
        child: child.into_any_element(),
    }
}

/// An element whose rems are `rem`, for what's in it, however deep.
pub struct AtRem {
    rem: Pixels,
    child: AnyElement,
}

impl IntoElement for AtRem {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for AtRem {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let child = &mut self.child;
        (
            window.with_rem_size(Some(self.rem), |window| child.request_layout(window, cx)),
            (),
        )
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        (): &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let child = &mut self.child;
        window.with_rem_size(Some(self.rem), |window| child.prepaint(window, cx));
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        (): &mut (),
        (): &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let child = &mut self.child;
        window.with_rem_size(Some(self.rem), |window| child.paint(window, cx));
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::component::{ActiveTheme as _, ThemeMode};
    use gpui_kit::{TestAppContext, base};

    use super::*;

    /// The test platform's text system takes any name, so this asks the one the app draws with.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_real_text_system_finds_every_face_by_the_name_code_asks_for() {
        use gpui_apple::AppleTextSystem;
        use gpui_kit::{FontStyle, FontWeight, PlatformTextSystem as _, font};

        let system = AppleTextSystem::new();
        system.add_fonts(faces()).expect("the faces load");
        assert!(system.all_font_names().iter().any(|name| name == MONO));
        let bold = |font| gpui_kit::Font {
            weight: FontWeight::BOLD,
            ..font
        };
        let italic = |font| gpui_kit::Font {
            style: FontStyle::Italic,
            ..font
        };
        let ids = [
            font(MONO),
            bold(font(MONO)),
            italic(font(MONO)),
            bold(italic(font(MONO))),
        ]
        .map(|face| system.font_id(&face).expect("the family is there"));
        for (index, id) in ids.iter().enumerate() {
            assert!(
                !ids[..index].contains(id),
                "two styles drew in one face: {ids:?}"
            );
        }
    }

    fn listed() -> &'static [String] {
        static LISTED: OnceLock<Vec<String>> = OnceLock::new();
        LISTED.get_or_init(|| vec!["Fira Code".to_owned(), "Inter".to_owned()])
    }

    #[test]
    fn with_nothing_picked_it_is_the_system_font_and_the_bundled_one() {
        let in_use = InUse::of(&Fonts::default(), listed());
        assert_eq!(
            in_use,
            InUse {
                interface: SYSTEM,
                code: MONO
            }
        );
    }

    #[test]
    fn a_picked_family_is_used() {
        let fonts = Fonts {
            interface: Some("Inter".to_owned()),
            code: Some("Fira Code".to_owned()),
            ..Fonts::default()
        };
        assert_eq!(
            InUse::of(&fonts, listed()),
            InUse {
                interface: "Inter",
                code: "Fira Code"
            }
        );
    }

    #[test]
    fn a_family_no_longer_there_goes_back_to_the_default() {
        let fonts = Fonts {
            interface: Some("Gone".to_owned()),
            code: Some("Gone".to_owned()),
            ..Fonts::default()
        };
        assert_eq!(
            InUse::of(&fonts, listed()),
            InUse {
                interface: SYSTEM,
                code: MONO
            }
        );
    }

    #[gpui_kit::test]
    fn what_the_menus_pick_reaches_the_settings(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            settings::init(cx);
            init(cx);
            cx.dispatch_action(&SetInterfaceFont(Some("Inter".to_owned())));
            cx.dispatch_action(&SetCodeFont(Some("Fira Code".to_owned())));
        });
        cx.update(|cx| {
            let fonts = &settings::get(cx).fonts;
            assert_eq!(fonts.interface.as_deref(), Some("Inter"));
            assert_eq!(fonts.code.as_deref(), Some("Fira Code"));
            cx.dispatch_action(&SetCodeFont(None));
        });
        cx.read(|cx| assert_eq!(settings::get(cx).fonts.code, None));
    }

    #[gpui_kit::test]
    fn code_stays_in_the_font_through_a_change_of_appearance(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            settings::init(cx);
            init(cx);
            for mode in [ThemeMode::Dark, ThemeMode::Light, ThemeMode::Dark] {
                Theme::change(mode, None, cx);
                assert_eq!(cx.theme().mono_font_family.as_ref(), MONO);
                assert_eq!(
                    base::Theme::global(cx).tokens.typography.mono.as_ref(),
                    MONO
                );
            }
        });
    }
}
