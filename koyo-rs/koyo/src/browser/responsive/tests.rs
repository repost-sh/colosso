//! Responsive mode's sums (fit, placement, the pointer's way into the page, grips), what an
//! agent's change resolves to, and a view in a window: the protocol commands its page's
//! session is sent on setting, clearing and reopening, and the lease, which the person's
//! changes leave alone and an agent's need.

use std::path::Path;

use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext, bounds, point, px, size};
use serde_json::{Value, json};

use super::devices::find;
use super::*;
use crate::browser::control::{Actor, Mode, Refused};
use crate::browser::host::Browsers;
use crate::browser::view::BrowserView;
use crate::responsive::GUTTER;
use crate::test_support::{draw, in_window};

mod redrawn;

fn iphone() -> &'static Device {
    find("iphone-15").unwrap()
}

#[test]
fn a_page_fits_its_room_at_most_at_its_true_size() {
    let mode = Responsive::free(400, 800, Zoom::Fit, SetBy::Person);
    // 432 × 432 leaves 400 × 400 inside the gutters: the height halves it.
    assert!((mode.fit(size(px(432.), px(432.))) - 0.5).abs() < 1e-6);
    assert!(
        (mode.fit(size(px(2000.), px(2000.))) - 1.).abs() < 1e-6,
        "never larger"
    );
    let half = Responsive {
        zoom: Zoom::Percent(50),
        ..mode
    };
    assert!(
        (half.fit(size(px(2000.), px(2000.))) - 0.5).abs() < 1e-6,
        "a zoom is a zoom"
    );
}

#[test]
fn a_page_shows_centred_at_the_top_past_the_gutter() {
    let area = bounds(point(px(100.), px(50.)), size(px(1000.), px(900.)));
    let shown = place(area, 400, 800, 0.5);
    assert_eq!(shown.size, size(px(200.), px(400.)));
    assert_eq!(shown.origin, point(px(100. + 400.), px(50. + GUTTER)));
    let wide = place(area, 3000, 800, 1.);
    assert_eq!(
        wide.origin.x,
        px(100. + GUTTER),
        "too wide, it keeps its left edge in sight"
    );
}

#[test]
fn pointer_positions_map_through_the_scale_both_ways() {
    let origin = point(px(500.), px(66.));
    let at = point(px(600.), px(166.));
    assert_eq!(
        to_page(at, origin, 1.),
        point(px(100.), px(100.)),
        "unscaled"
    );
    assert_eq!(
        to_page(at, origin, 0.5),
        point(px(200.), px(200.)),
        "shown at half"
    );
    assert_eq!(to_window(point(px(200.), px(200.)), origin, 0.5), at);
}

#[test]
fn a_preset_emulates_a_phone_and_leaving_it_clears_all_of_it() {
    let phone = Responsive::preset(iphone(), false, Zoom::Fit, SetBy::Person).emulation("146");
    let set = commands(&Emulation::default(), &phone);
    let methods: Vec<&str> = set.iter().map(|(method, _)| *method).collect();
    assert_eq!(
        methods,
        [
            "Emulation.setDeviceMetricsOverride",
            "Emulation.setTouchEmulationEnabled",
            "Emulation.setEmitTouchEventsForMouse",
            "Emulation.setUserAgentOverride",
        ]
    );
    assert_eq!(
        set[0].1,
        json!({ "width": 0, "height": 0, "deviceScaleFactor": 0, "mobile": true })
    );
    assert_eq!(set[1].1, json!({ "enabled": true, "maxTouchPoints": 5 }));
    assert_eq!(
        set[2].1,
        json!({ "enabled": true, "configuration": "mobile" })
    );
    assert_eq!(set[3].1["platform"], "iPhone");
    assert_eq!(set[3].1["userAgentMetadata"]["mobile"], true);
    assert!(
        set[3].1["userAgent"]
            .as_str()
            .unwrap()
            .contains("iPhone OS 18_0")
    );
    let cleared = commands(&phone, &Emulation::default());
    assert_eq!(
        cleared,
        [
            ("Emulation.clearDeviceMetricsOverride", json!({})),
            (
                "Emulation.setTouchEmulationEnabled",
                json!({ "enabled": false })
            ),
            (
                "Emulation.setEmitTouchEventsForMouse",
                json!({ "enabled": false })
            ),
            ("Emulation.setUserAgentOverride", json!({ "userAgent": "" })),
        ]
    );
    assert!(
        commands(&phone, &phone).is_empty(),
        "nothing new, nothing sent"
    );
    let free = Responsive::free(800, 600, Zoom::Fit, SetBy::Person).emulation("146");
    assert!(
        commands(&Emulation::default(), &free).is_empty(),
        "a free size is a desktop"
    );
}

#[test]
fn a_device_lays_out_at_its_size_and_scale() {
    let phone = Responsive::preset(iphone(), true, Zoom::Fit, SetBy::Person);
    assert_eq!((phone.width, phone.height), (852, 393), "turned");
    let viewport = phone.viewport(2.);
    assert_eq!((viewport.width, viewport.height), (852, 393));
    assert!(
        (viewport.scale - 3.).abs() < f32::EPSILON,
        "the device's scale, not the window's"
    );
    let free = Responsive::free(800, 600, Zoom::Fit, SetBy::Person);
    assert!(
        (free.viewport(2.).scale - 2.).abs() < f32::EPSILON,
        "the window's"
    );
}

fn resolve(change: &Change, current: Option<&Responsive>) -> Result<Option<Responsive>, String> {
    change.resolve(
        current,
        (1000, 700),
        2.,
        SetBy::Agent("claude-code:7f3a".into()),
    )
}

#[test]
fn an_agents_change_resolves_against_what_shows() {
    let width = Change {
        width: Some(390),
        ..Change::default()
    };
    let free = resolve(&width, None).unwrap().unwrap();
    assert_eq!(
        (free.width, free.height, free.device),
        (390, 700, None),
        "the pane's height"
    );
    assert_eq!(free.set_by, SetBy::Agent("claude-code:7f3a".into()));
    let phone = Responsive::preset(iphone(), false, Zoom::Fit, SetBy::Person);
    let narrowed = resolve(&width, Some(&phone)).unwrap().unwrap();
    assert_eq!(
        (narrowed.width, narrowed.height),
        (390, 852),
        "the other side kept"
    );
    assert!(
        !narrowed.mobile && narrowed.device.is_none(),
        "a free size is a desktop"
    );
    let zoomed = Change {
        zoom: Some(Zoom::Percent(50)),
        ..Change::default()
    };
    let still = resolve(&zoomed, Some(&phone)).unwrap().unwrap();
    assert_eq!(
        (still.device, still.zoom),
        (Some(iphone()), Zoom::Percent(50)),
        "still the phone"
    );
    let turned = Change {
        landscape: Some(true),
        ..Change::default()
    };
    let turned = resolve(&turned, Some(&phone)).unwrap().unwrap();
    assert_eq!((turned.width, turned.height), (852, 393));
    let fill = Change {
        fill: true,
        ..Change::default()
    };
    assert_eq!(resolve(&fill, Some(&phone)), Ok(None));
}

#[test]
fn an_agents_change_is_refused_when_it_makes_no_sense() {
    let preset = Some(Choice::Preset(iphone()));
    for wrong in [
        Change::default(),
        Change {
            fill: true,
            width: Some(390),
            ..Change::default()
        },
        Change {
            device: preset,
            width: Some(390),
            ..Change::default()
        },
        Change {
            device: preset,
            user_agent: Some("Nokia".into()),
            ..Change::default()
        },
        Change {
            width: Some(49),
            ..Change::default()
        },
        Change {
            width: Some(5000),
            scale: Some(4.),
            ..Change::default()
        },
        Change {
            width: Some(390),
            scale: Some(9.),
            ..Change::default()
        },
        Change {
            user_agent: Some("  ".into()),
            ..Change::default()
        },
    ] {
        assert!(resolve(&wrong, None).is_err(), "{wrong:?}");
    }
    let custom = Change {
        width: Some(390),
        scale: Some(3.),
        mobile: Some(true),
        touch: Some(true),
        user_agent: Some("Nokia".into()),
        ..Change::default()
    };
    let custom = resolve(&custom, None).unwrap().unwrap();
    assert!(custom.mobile && custom.touch && custom.user_agent.as_deref() == Some("Nokia"));
    assert!((custom.scale_at(2.) - 3.).abs() < f32::EPSILON);
}

#[test]
fn a_page_has_settled_once_it_reports_the_size_and_shows_a_frame_of_it() {
    assert!(laid_out(393, 852, 100, false, (393., 852.)));
    assert!(
        !laid_out(393, 852, 100, false, (1200., 800.)),
        "still the old size"
    );
    assert!(
        laid_out(400, 800, 200, false, (200., 400.)),
        "zoomed, CSS pixels are larger"
    );
    // A phone page with no <meta viewport> lays out at 980 wide, the phone's shape.
    assert!(laid_out(375, 667, 100, true, (980., 1743.)));
    assert!(
        !laid_out(375, 667, 100, false, (980., 1743.)),
        "only on a phone"
    );
    let target = Viewport {
        width: 412,
        height: 915,
        scale: 2.625,
    };
    let fits = |width, height| target.pixels().near(PixelSize { width, height });
    assert!(fits(1082, 2402));
    assert!(fits(1081, 2401), "a device pixel either way");
    assert!(!fits(2400, 1600));
}

/// A page of `url` in a window of its own; a test names no engine, so its host fails to
/// start, and the page counts as open only when the test says so.
fn page(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<BrowserView>) {
    cx.update(crate::init);
    let (window, view) = in_window(size(px(1000.), px(800.)), cx, |window, cx| {
        let url = "https://example.com/".to_owned();
        cx.new(|cx| BrowserView::new(url, Path::new("/work/app"), window, cx))
    });
    draw(window, cx);
    (window, view)
}

/// The protocol commands asked of `view`'s page since the last look, by method.
fn asked(view: &Entity<BrowserView>, cx: &mut TestAppContext) -> Vec<(String, Value)> {
    let pane = view.read_with(cx, |view, _| view.pane);
    let browsers = cx.update(Browsers::global);
    browsers.update(cx, |browsers, _| {
        let asked = std::mem::take(&mut browsers.asked);
        asked
            .into_iter()
            .filter(|(of, ..)| *of == pane)
            .map(|(_, method, params)| (method, params))
            .collect()
    })
}

fn methods(asked: &[(String, Value)]) -> Vec<&str> {
    asked.iter().map(|(method, _)| method.as_str()).collect()
}

#[gpui_kit::test]
fn the_page_is_told_a_preset_on_setting_clearing_and_reopening(cx: &mut TestAppContext) {
    let (window, view) = page(cx);
    let filling = view.read_with(cx, |view, _| view.viewport);
    view.update(cx, BrowserView::pretend_opened);
    assert!(
        asked(&view, cx).is_empty(),
        "a page filling its pane emulates nothing"
    );
    view.update(cx, |view, cx| view.pick_device(Some(iphone()), cx));
    let set = asked(&view, cx);
    assert_eq!(methods(&set).len(), 4, "{set:?}");
    assert_eq!(set[0].1["mobile"], true);
    // The page is resized at once, at the phone's size and scale.
    view.read_with(cx, |view, _| {
        let viewport = view.viewport.unwrap();
        assert_eq!((viewport.width, viewport.height), (393, 852));
        assert!((viewport.scale - 3.).abs() < f32::EPSILON);
    });
    view.update(cx, BrowserView::pretend_opened);
    assert_eq!(
        methods(&asked(&view, cx)),
        methods(&set),
        "a new session is told it again"
    );
    view.update(cx, |view, cx| view.set_responsive(None, cx));
    let cleared = asked(&view, cx);
    assert_eq!(
        methods(&cleared),
        [
            "Emulation.clearDeviceMetricsOverride",
            "Emulation.setTouchEmulationEnabled",
            "Emulation.setEmitTouchEventsForMouse",
            "Emulation.setUserAgentOverride",
        ]
    );
    draw(window, cx);
    view.read_with(cx, |view, _| {
        assert_eq!(view.viewport, filling, "the pane's size again");
    });
}

#[gpui_kit::test]
fn the_page_shows_scaled_and_the_pointer_reaches_it_through_the_scale(cx: &mut TestAppContext) {
    let (window, view) = page(cx);
    view.update(cx, BrowserView::toggle_responsive);
    draw(window, cx);
    let fitted = view.read_with(cx, |view, _| {
        let mode = view.responsive.mode.clone().unwrap();
        assert_eq!(mode.device, None, "a free size at first");
        (view.responsive.fit, view.origin)
    });
    assert!(
        (fitted.0 - 1.).abs() < f32::EPSILON,
        "the pane's size fits at its true size"
    );
    view.update(cx, |view, cx| {
        let change = Change {
            device: Some(Choice::Preset(find("ipad-pro-11").unwrap())),
            ..Change::default()
        };
        view.person_changes(&change, cx);
    });
    draw(window, cx);
    view.read_with(cx, |view, _| {
        let fit = view.responsive.fit;
        assert!(
            fit < 1.,
            "a tablet taller than the pane shows smaller: {fit}"
        );
        assert_eq!(view.shown_percent(), (fit * 100.).round() as u32);
        let inside = view.origin + point(px(100. * fit), px(50. * fit));
        let at = view.page_point(inside);
        assert!(
            (at.x.as_f32() - 100.).abs() < 0.01 && (at.y.as_f32() - 50.).abs() < 0.01,
            "{at:?}"
        );
        let back = view.window_point(koyo_browser_ipc::Point { x: 100., y: 50. });
        assert!((back - inside).x.abs() < px(0.01));
    });
}

#[gpui_kit::test]
fn the_persons_change_takes_nothing_and_an_agents_needs_the_lease(cx: &mut TestAppContext) {
    let (_, view) = page(cx);
    let agent = Actor {
        id: "a".into(),
        label: "claude-code:7f3a".into(),
    };
    view.update(cx, |view, cx| {
        view.agent_claims(agent.clone(), false, None, cx).unwrap();
    });
    view.update(cx, |view, cx| view.pick_device(Some(iphone()), cx));
    view.read_with(cx, |view, _| {
        assert_eq!(
            view.control.agent(),
            Some(&agent),
            "the agent still holds the page"
        );
        assert_eq!(view.responsive.mode.as_ref().unwrap().set_by, SetBy::Person);
    });
    let width = Change {
        width: Some(390),
        ..Change::default()
    };
    let target = view
        .update(cx, |view, cx| view.agent_viewport(&agent, &width, cx))
        .unwrap();
    assert_eq!(target.map(|target| target.width), Some(390));
    view.read_with(cx, |view, cx| {
        let facts = view.viewport_facts(cx);
        assert_eq!(
            facts.set_by.as_deref(),
            Some("claude-code:7f3a"),
            "the agent's mark"
        );
        assert_eq!((facts.width, facts.height), (390, 852));
    });
    view.update(cx, |view, cx| view.person_acts(Mode::Implicit, cx));
    let refused = view.update(cx, |view, cx| view.agent_viewport(&agent, &width, cx));
    assert!(matches!(
        refused,
        Err(ViewportRefused::Refused(Refused::PersonHasControl { .. }))
    ));
    let nonsense = Change::default();
    let invalid = view.update(cx, |view, cx| view.agent_viewport(&agent, &nonsense, cx));
    assert!(
        matches!(invalid, Err(ViewportRefused::Invalid(_))),
        "checked before the lease"
    );
}
