//! About: which build is running and where its update stands (`docs/updates.md` 1), where its
//! settings are kept and where it logs.

use std::path::Path;

use gpui_kit::{App, Styled as _, px};

use super::components::{PillButton, SettingRow, SettingsSection};
use crate::projects::display_path;
use crate::ring::ring;
use crate::updates::{self, Shown};
use crate::{logs, settings, theme};

pub fn sections(cx: &App) -> Vec<SettingsSection> {
    vec![
        SettingsSection::new("Koyo")
            .row(version_row(cx))
            .row(file_row(
                "Settings file",
                "reveal-settings-file",
                settings::path(cx),
                "No folder to save settings in; changes last until Koyo quits.",
            ))
            .row(file_row(
                "Log file",
                "reveal-log-file",
                logs::file(),
                "Logging to the terminal Koyo was started from.",
            )),
    ]
}

/// The running version and where its update stands, every stage of it, the quiet ones too,
/// with the one click the top bar's pill offers, or how far a download has come. It says only
/// what `updates::shown` does.
fn version_row(cx: &App) -> SettingRow {
    let shown = updates::shown(cx);
    let said = format!("{} · {}", env!("CARGO_PKG_VERSION"), shown.said());
    let row = SettingRow::new("Version").description(said);
    match shown {
        Shown::Quiet(_) => row,
        Shown::Offer { act, .. } => {
            row.control(PillButton::new("update-act", act).on_click(|_, _, cx| updates::act(cx)))
        }
        Shown::Working { percent, .. } => {
            let accent = theme::palette(cx).accent;
            row.control(ring(percent.unwrap_or(0)).size(px(18.)).text_color(accent))
        }
    }
}

/// A row saying where a file is kept, with a button to show it, or why there isn't one.
fn file_row(
    title: &'static str,
    id: &'static str,
    path: Option<&Path>,
    missing: &'static str,
) -> SettingRow {
    let path = path.map(Path::to_path_buf);
    SettingRow::new(title)
        .description(
            path.as_deref()
                .map_or_else(|| missing.to_owned(), display_path),
        )
        .control(
            PillButton::new(id, "Reveal in Finder")
                .disabled(path.is_none())
                .on_click(move |_, _, cx| {
                    if let Some(path) = &path {
                        cx.reveal_path(path);
                    }
                }),
        )
}

#[cfg(test)]
mod tests {
    use gpui_kit::{
        AnyWindowHandle, AppContext as _, Bounds, Context, IntoElement, Modifiers,
        ParentElement as _, Pixels, Render, Styled as _, TestAppContext, VisualTestContext, Window,
        div, px, size,
    };

    use super::sections;
    use crate::navigation::SettingsPage;
    use crate::test_support::{draw, in_window};
    use crate::updates::testing::{Koyo, koyo, until_it_says};

    /// The About page in a window of its own.
    struct About;

    impl Render for About {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(super::super::page(SettingsPage::About, cx))
        }
    }

    /// Whether the Version row says `words`, which must be lowercase.
    fn version_says(words: &str, cx: &mut TestAppContext) -> bool {
        cx.read(|cx| sections(cx)[0].matches(words))
    }

    fn button(window: AnyWindowHandle, cx: &mut TestAppContext) -> Option<Bounds<Pixels>> {
        draw(window, cx);
        VisualTestContext::from_window(window, cx).debug_bounds("update-act")
    }

    #[gpui_kit::test]
    fn the_version_row_says_where_the_update_stands_and_offers_its_click(cx: &mut TestAppContext) {
        cx.update(crate::test_support::init);
        let running = env!("CARGO_PKG_VERSION");
        let unreleased = format!("{running} · this build doesn't update itself");
        assert!(version_says(&unreleased, cx), "a build made here doesn't say so");

        let koyo = koyo("about-version", true, cx);
        let (window, _) = in_window(size(px(900.), px(700.)), cx, |_, cx| cx.new(|_| About));
        assert_eq!(button(window, cx), None, "a click is offered with nothing to install");
        assert!(version_says(&format!("{running} · couldn't check for updates"), cx));

        koyo.announce(running);
        Koyo::an_hour_on(cx);
        assert!(version_says(&format!("{running} · koyo is up to date. checked just now."), cx));
        assert_eq!(button(window, cx), None);

        koyo.announce("9.0.0");
        Koyo::an_hour_on(cx);
        assert!(version_says(&format!("{running} · koyo 9.0.0 is available."), cx));
        let offered = button(window, cx).expect("the page wasn't drawn again with the offer");

        let held = koyo.hold_download();
        VisualTestContext::from_window(window, cx).simulate_click(offered.center(), Modifiers::none());
        assert!(version_says(&format!("{running} · downloading koyo 9.0.0"), cx));
        assert_eq!(button(window, cx), None, "the click is still offered while it installs");

        drop(held);
        until_it_says("Retry Update", cx);
        assert!(version_says("couldn't update to koyo 9.0.0. the download didn't arrive", cx));
        assert!(button(window, cx).is_some(), "a failed update offers no retry");
    }
}
