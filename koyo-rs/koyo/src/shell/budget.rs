//! What the shell's own triggers cost the window (`crate::budget`). Each test is a ratchet: it
//! holds a trigger at what it costs today, so it gets no worse unseen, and the budget wanted is
//! the shell rendering only when something it shows changed. A test fails as a regression when
//! its trigger costs more, and as better when a fix has lowered it, which takes the new number.

use std::cell::Cell;
use std::time::Duration;

use gpui_kit::{AppContext as _, TestAppContext, point, px};

use crate::budget::{
    Built, built_after, built_again, built_during, frame_of, ratchet, settle, within,
};
use crate::browser::profiles::{self, AgentPages};
use crate::changes::Touched;
use crate::files::tests::support::listed;
use crate::keyboard::{self, Holder};
use crate::popup::{self, Kind};
use crate::right_panel::ToggleFiles;
use super::Shell;
use crate::navigation::SettingsPage;
use crate::settings::{self, Layout};
use crate::test_support::{
    center_of, click, draw, hover, in_front, open_changes, open_page, open_page_view, scratch,
    shell_on, write,
};

/// A frame of the shown page's view, as each page-state update and cursor-shape change makes,
/// builds the view and leaves the shell alone.
#[gpui_kit::test]
fn a_frame_of_a_shown_pages_view_leaves_the_shell_alone(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("budget-page-shown", Layout::Orca, cx);
    let view = open_page_view(window, &shell, "https://example.com/", cx);

    let held = Built {
        shell: 0,
        views: 2,
        frames: 1,
    };
    let built = built_again(window, cx, frame_of(&view));
    ratchet(built, held, "a frame of the shown page's view");
}

/// A frame of the shown page's view while the page has the keyboard leaves the shell alone: it
/// reads the page's view to see whether GPUI's focus is still the page's as the window becomes
/// key and as GPUI's focus comes into it, never as it draws.
#[gpui_kit::test]
fn a_frame_of_a_page_with_the_keyboard_leaves_the_shell_alone(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("budget-page-keyboard", Layout::Orca, cx);
    (cx.update_window(window, |_, window, _| window.activate_window())).unwrap();
    let view = open_page_view(window, &shell, "https://example.com/", cx);
    draw(window, cx);
    let page_id = view.read_with(cx, |view, _| view.page_id());
    assert_eq!(
        keyboard::holder(window),
        Some(Holder::Page(page_id)),
        "the page has the keyboard"
    );

    let held = Built {
        shell: 0,
        views: 2,
        frames: 1,
    };
    let built = built_again(window, cx, frame_of(&view));
    ratchet(built, held, "a frame of the page with the keyboard");
}

/// A frame of a page's view whose tab is in the strip but not shown draws nothing.
#[gpui_kit::test]
fn a_frame_of_a_hidden_pages_view_draws_nothing(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("budget-page-hidden", Layout::Orca, cx);
    let hidden = open_page_view(window, &shell, "https://example.com/", cx);
    let shown = open_page(window, &shell, Some("https://example.org/"), cx);
    draw(window, cx);
    let tab = shell.read_with(cx, |shell, _| shell.shown_tab_key());
    assert_eq!(tab, Some(shown), "the second page's tab is shown");

    let held = Built::default();
    let built = built_again(window, cx, frame_of(&hidden));
    ratchet(built, held, "a frame of a hidden page's view");
}

/// In the Arc layout a frame of the shown page's view builds it and the address bar, which
/// reads it for its site buttons (`arc::Top`), and leaves the shell and the back, forward and
/// reload buttons alone.
#[gpui_kit::test]
fn a_frame_of_a_shown_pages_view_leaves_the_arc_shell_alone(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("budget-page-arc", Layout::Arc, cx);
    let view = open_page_view(window, &shell, "https://example.com/", cx);

    let held = Built {
        shell: 0,
        views: 2,
        frames: 1,
    };
    let built = built_again(window, cx, frame_of(&view));
    ratchet(built, held, "a frame of the shown page's view in Arc");
}

/// In the Arc layout showing another tab, as a press on its row does, renders the shell twice:
/// as the tab is shown, and as its page takes the keyboard (`keys_to_shown_tab`).
#[gpui_kit::test]
fn a_tab_switch_in_arc_renders_the_shell_twice(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("budget-arc-switch", Layout::Arc, cx);
    let first = open_page(window, &shell, Some("https://example.com/"), cx);
    let second = open_page(window, &shell, Some("https://example.org/"), cx);
    draw(window, cx);
    let switch = |cx: &mut TestAppContext| {
        shell.update(cx, |shell, cx| {
            let to = if shell.shown_tab_key() == Some(first) {
                second
            } else {
                first
            };
            assert!(shell.show_tab_holding(to, cx), "the other tab is open");
        });
    };

    let held = Built {
        shell: 2,
        views: 15,
        frames: 2,
    };
    let built = built_again(window, cx, switch);
    ratchet(built, held, "a tab switch in Arc");
}

/// In the Arc layout the pointer coming onto the address bar fades its site buttons in, and
/// each frame of the fade builds the bar alone, never the page's view it reads. The shell
/// renders once, as the pointer comes onto the bar.
#[gpui_kit::test]
fn the_arc_address_bars_hover_fade_leaves_the_page_alone(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("budget-address-hover", Layout::Arc, cx);
    open_page_view(window, &shell, "https://example.com/", cx);
    let bar = center_of(window, "arc-address", cx);
    let away = point(bar.x, bar.y + px(400.));
    hover(window, away, cx);
    settle(cx);

    let nothing = built_after(window, cx, |cx| {
        hover(window, away + point(px(4.), px(0.)), cx);
    });
    assert_eq!(nothing, Built::default(), "a move over nothing");
    let held = Built {
        shell: 1,
        views: 11,
        frames: 10,
    };
    let built = built_after(window, cx, |cx| hover(window, bar, cx));
    ratchet(built, held, "the pointer onto the Arc address bar");
}

/// While the sidebar's search field has the keyboard, each turn of its caret's 500 ms blink
/// builds the field, a view of its own, and leaves the shell alone.
#[gpui_kit::test]
fn the_search_fields_caret_leaves_the_shell_alone(cx: &mut TestAppContext) {
    let (window, _, _) = shell_on("budget-search-blink", Layout::Orca, cx);
    in_front(window, true, cx);
    settle(cx);
    click(window, "sidebar-search", cx);
    let two_seconds = Duration::from_secs(2);

    let held = Built {
        shell: 0,
        views: 8,
        frames: 4,
    };
    let built = built_during(window, cx, two_seconds);
    ratchet(built, held, "two seconds of the search field's caret");

    in_front(window, false, cx);
    let built = built_during(window, cx, two_seconds);
    ratchet(built, Built::default(), "two seconds of it behind another app's");
    in_front(window, true, cx);
    let again = Built { views: 9, ..held };
    let built = built_during(window, cx, two_seconds);
    ratchet(built, again, "two seconds of it in front again");
}

/// A notify of the selected checkout, as each git re-read and each batch of syntax colours
/// makes, leaves the shell alone while its totals stay as they were: the shell keeps them beside
/// the checkout and reads them, not the checkout. With its changes open, the panel's own view
/// builds, and the shell stays as it was around it, as it does around a terminal's frame beside
/// it (`terminals/budget.rs`).
#[gpui_kit::test]
fn a_notify_of_the_selected_checkout_leaves_the_shell_alone(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("budget-checkout", Layout::Orca, cx);
    settle(cx);
    let checkout = shell.read_with(cx, |shell, _| shell.right_panel.checkout().unwrap().clone());

    let built = built_again(window, cx, frame_of(&checkout));
    ratchet(built, Built::default(), "a notify of the checkout, the panel closed");

    open_changes(window, &shell, cx);
    settle(cx);

    let open = Built {
        shell: 0,
        views: 1,
        frames: 1,
    };
    let built = built_again(window, cx, frame_of(&checkout));
    ratchet(built, open, "a notify of the checkout, its changes open");
}

/// A read of the selected checkout that changes how many lines its changes add and remove renders
/// the shell, whose changes button says so.
#[gpui_kit::test]
fn the_selected_checkouts_totals_changed_render_the_shell(cx: &mut TestAppContext) {
    let (window, shell, root) = shell_on("budget-totals", Layout::Orca, cx);
    settle(cx);
    let checkout = shell.read_with(cx, |shell, _| shell.right_panel.checkout().unwrap().clone());
    let edited = |cx: &mut TestAppContext| {
        write(&root, "README.md", b"hi\nthere\n");
        checkout.update(cx, |checkout, cx| {
            checkout.touched(vec![Touched::File("README.md".into())], cx);
        });
    };

    let held = Built {
        shell: 1,
        views: 1,
        frames: 1,
    };
    ratchet(built_after(window, cx, edited), held, "the checkout's totals changed");
    let totals = shell.read_with(cx, |shell, _| shell.right_panel.totals_in(&root));
    assert_eq!(totals, (1, 0), "the button says a line was added");
}

/// A change to the shell with the right panel open, as each frame of a working chat's spinner
/// makes, builds the shell alone and not the panel: what the shell tells the panel's checkout
/// and file tree after each change, it tells them only when that's new, as an update of either
/// builds the panel again.
#[gpui_kit::test]
fn a_change_to_the_shell_leaves_the_open_right_panel_alone(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("budget-shell-panel", Layout::Orca, cx);
    let notified = |cx: &mut TestAppContext| shell.update(cx, |_, cx| cx.notify());
    let shell_alone = Built {
        shell: 1,
        views: 1,
        frames: 1,
    };

    open_changes(window, &shell, cx);
    settle(cx);
    let built = built_again(window, cx, notified);
    ratchet(
        built,
        shell_alone,
        "a change to the shell, its changes open",
    );

    cx.dispatch_action(window, ToggleFiles);
    settle(cx);
    let files = shell.read_with(cx, |shell, _| shell.right_panel.files().cloned());
    listed(&files.expect("the right panel shows the file tree"), cx);
    let built = built_again(window, cx, notified);
    ratchet(built, shell_alone, "a change to the shell, its files open");
}

/// In the Arc layout the pointer coming onto a worktree's header fades its pill in over 150 ms,
/// and each frame of the fade renders the shell, which runs the fade as it draws the header.
/// With the fade in a view of its own, the shell renders in none of its frames.
#[gpui_kit::test]
fn a_worktree_headers_hover_fade_renders_the_arc_shell_at_each_frame(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("budget-header-hover", Layout::Arc, cx);
    open_page_view(window, &shell, "https://example.com/", cx);
    let header = center_of(window, "arc-header-0", cx);
    let away = point(header.x, header.y + px(400.));
    hover(window, away, cx);
    settle(cx);

    let nothing = built_after(window, cx, |cx| {
        hover(window, away + point(px(4.), px(0.)), cx);
    });
    assert_eq!(nothing, Built::default(), "a move over nothing");
    let held = Built {
        shell: 10,
        views: 10,
        frames: 10,
    };
    let built = built_after(window, cx, |cx| hover(window, header, cx));
    ratchet(built, held, "the pointer onto a worktree's header");
}

/// ⌘B closing the sidebar renders the shell at each frame of the rails' spring as they fold
/// away, as the shell runs the spring as it renders. With the rails in a view of their own, the
/// shell would render once and the rails at each frame. The sidebar's search field, a view of
/// its own, is built again in the frames that move it. Orca's top rail stays, its tabs with it,
/// so only the sidebar and the status bar move.
#[gpui_kit::test]
fn closing_the_sidebar_renders_the_shell_at_each_frame_of_the_rails(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("budget-sidebar-close", Layout::Orca, cx);
    settle(cx);

    let held = Built {
        shell: 12,
        views: 32,
        frames: 12,
    };
    let close = |cx: &mut TestAppContext| {
        shell.update(cx, |shell, cx| shell.set_sidebar_open(false, cx));
    };
    let built = built_after(window, cx, close);
    ratchet(built, held, "the sidebar closed, rest to rest");
}

/// The sidebar's width set, as Reset in Settings sets it, renders the shell once, rightly, as it
/// draws the sidebar at that width, and the search field, a view of its own, at its new width.
/// The save 500 ms later writes the file and tells no one, so it builds nothing.
#[gpui_kit::test]
fn the_sidebar_resized_renders_the_shell_once_and_its_save_not_at_all(cx: &mut TestAppContext) {
    let (window, _, _) = shell_on("budget-sidebar-width", Layout::Orca, cx);
    let folder = scratch("budget-sidebar-width-settings");
    let file = cx.update(|cx| settings::tests::saved_in(&folder, cx));
    settle(cx);
    let wider = |cx: &mut TestAppContext| {
        cx.update(|cx| settings::update(cx, |settings| settings.sidebar_width += 10.));
    };

    let once = Built {
        shell: 1,
        views: 4,
        frames: 1,
    };
    ratchet(
        built_after(window, cx, wider),
        once,
        "the sidebar's width set",
    );
    ratchet(
        built_during(window, cx, settings::SAVE_DELAY),
        Built::default(),
        "the settings saved",
    );
    assert!(file.exists(), "the settings were saved");
}

/// A step of a drag in the theme picker, as each move of the pointer along its opacity slider
/// makes, changes the theme, and what's drawn in its colours is built again: in the shell's
/// window, and in the picker's own, which shows the theme it picks.
#[gpui_kit::test]
fn a_theme_picker_drag_step_builds_what_the_theme_colours(cx: &mut TestAppContext) {
    let (window, _, _) = shell_on("budget-theme", Layout::Orca, cx);
    settle(cx);
    let opened = cx.update_window(window, |_, window, cx| {
        crate::picker::open(point(px(40.), px(40.)), window, cx);
    });
    opened.unwrap();
    let picker = cx.read(|cx| popup::window(Kind::Picker, cx)).expect("the picker is up");
    settle(cx);
    let along = Cell::new(0.5);
    let step = |cx: &mut TestAppContext| {
        along.set(1.3 - along.get());
        let opacity = along.get();
        cx.update(|cx| crate::gradient::update(cx, |theme| theme.set_opacity(opacity)));
    };

    let shell = Built {
        shell: 1,
        views: 4,
        frames: 1,
    };
    ratchet(built_again(window, cx, step), shell, "a drag step, the shell's window");
    let own = Built {
        shell: 1,
        views: 1,
        frames: 1,
    };
    within(built_again(picker, cx, step), own, "a drag step, the picker's window");
}

/// The chat backdrop turned off or on in the settings builds what draws it, or the card in its
/// place, again: the panes' backdrops and what floats over a chat.
#[gpui_kit::test]
fn the_chat_backdrop_turned_off_builds_what_draws_it(cx: &mut TestAppContext) {
    let (window, _, _) = shell_on("budget-backdrop", Layout::Orca, cx);
    settle(cx);
    let turned = |cx: &mut TestAppContext| {
        cx.update(|cx| settings::update(cx, |settings| {
            settings.chat_backdrop.on = !settings.chat_backdrop.on;
        }));
    };

    let held = Built {
        shell: 1,
        views: 4,
        frames: 1,
    };
    ratchet(built_again(window, cx, turned), held, "the chat backdrop turned off or on");
}

/// A change of a setting the window doesn't draw from, as whether the sidebar opens with the app,
/// builds the shell and what draws in the theme's colours, which are read from the settings. The
/// agent desktop and the accounts, updated at each change of the settings, build nothing more:
/// what draws them draws in those colours.
#[gpui_kit::test]
fn a_setting_nothing_draws_from_builds_what_draws_in_the_themes_colours(cx: &mut TestAppContext) {
    let (window, _, _) = shell_on("budget-setting", Layout::Orca, cx);
    settle(cx);
    let turned = |cx: &mut TestAppContext| {
        cx.update(|cx| settings::update(cx, |settings| {
            settings.sidebar_open_on_launch = !settings.sidebar_open_on_launch;
        }));
    };

    let held = Built {
        shell: 1,
        views: 4,
        frames: 1,
    };
    ratchet(built_again(window, cx, turned), held, "a setting nothing draws from");
}

/// A change of the browser profiles, as picking where agents' pages open makes, builds the
/// shell while Settings → Browser, which lists them, is shown, and nothing while it isn't.
#[gpui_kit::test]
fn a_change_of_the_profiles_builds_the_shell_while_settings_lists_them(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("budget-profiles", Layout::Orca, cx);
    settle(cx);
    let changed = |cx: &mut TestAppContext| {
        cx.update(|cx| profiles::update(cx, |profiles| {
            profiles.agents = match profiles.agents {
                AgentPages::Shared => AgentPages::Separate,
                AgentPages::Separate => AgentPages::Shared,
            };
        }));
    };

    ratchet(built_again(window, cx, changed), Built::default(), "the profiles changed, unlisted");
    shell.update(cx, |shell, cx| shell.open_settings_page(Some(SettingsPage::Browser), cx));
    settle(cx);
    let listed = Built {
        shell: 1,
        views: 1,
        frames: 1,
    };
    ratchet(built_again(window, cx, changed), listed, "the profiles changed, listed");
}

/// A settings page, what changes what it draws from, and what that builds while it's hidden and
/// while it's shown.
type Page<'a> = (SettingsPage, &'a dyn Fn(&mut TestAppContext), Built, Built);

/// A change of what Settings → Agent desktop, Devices or Accounts draws from builds the shell
/// while that page is shown, as it reads it as it draws, and nothing of the shell while it isn't:
/// the accounts' own button builds, on the top rail.
#[gpui_kit::test]
fn a_change_of_what_a_settings_page_draws_from_builds_the_shell_while_it_is_shown(
    cx: &mut TestAppContext,
) {
    let (window, shell, _) = shell_on("budget-settings-pages", Layout::Orca, cx);
    settle(cx);
    let (desk, devices, accounts) = cx.update(|cx| {
        let desk = crate::agent_desktop::desk(cx);
        (desk, crate::devices::devices(cx), crate::accounts::accounts(cx))
    });
    let built = |shell, views| Built {
        shell,
        views,
        frames: u64::from(views > 0),
    };
    let pages: [Page; 3] = [
        (SettingsPage::AgentDesktop, &frame_of(&desk), built(0, 0), built(1, 1)),
        (SettingsPage::Devices, &frame_of(&devices), built(0, 0), built(1, 1)),
        (SettingsPage::Accounts, &frame_of(&accounts), built(0, 1), built(1, 2)),
    ];

    for (page, changed, hidden, shown) in pages {
        let what = |when: &str| format!("{page:?} changed, {when}");
        ratchet(built_again(window, cx, changed), hidden, &what("not shown"));
        shell.update(cx, |shell, cx| shell.open_settings_page(Some(page), cx));
        settle(cx);
        ratchet(built_again(window, cx, changed), shown, &what("shown"));
        shell.update(cx, Shell::close_settings);
        settle(cx);
    }
}
