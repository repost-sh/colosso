use super::{
    Camera, Fit, GAP, PAD, clamp, fit_zoom, layout, offset, page_in_sight, step, zoom_about,
};

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

#[test]
fn pages_are_laid_one_under_another_centred_on_the_widest() {
    let laid = layout(&[(100., 50.), (200., 100.)], 2.);
    assert_eq!(laid.size, (400. + 2. * PAD, 100. + 200. + GAP + 2. * PAD));
    assert!(close(laid.pages[0].x, PAD + 100.));
    assert!(close(laid.pages[1].x, PAD));
    assert!(close(laid.pages[1].y, PAD + 100. + GAP));
}

#[test]
fn a_picture_fits_whole_and_is_never_blown_up_past_its_size() {
    let fit = Fit::Whole { most: Some(1.) };
    let small = fit_zoom(&[(100., 100.)], (800., 600.), fit);
    assert!(close(small, 1.), "a small picture shows at its size");
    let wide = fit_zoom(&[(2000., 1000.)], (800. + 2. * PAD, 600.), fit);
    assert!(close(wide, 0.4), "a wide one fits the pane's width: {wide}");
    let drawing = fit_zoom(
        &[(24., 24.)],
        (300. + 2. * PAD, 600.),
        Fit::Whole { most: None },
    );
    assert!(close(drawing, 12.5), "a drawing fills the pane: {drawing}");
}

#[test]
fn a_document_fits_the_panes_width() {
    let zoom = fit_zoom(
        &[(612., 792.), (400., 500.)],
        (612. + 2. * PAD, 300.),
        Fit::Width,
    );
    assert!(close(zoom, 1.));
}

#[test]
fn pages_smaller_than_the_pane_are_centred_and_larger_ones_scroll() {
    assert_eq!(offset((100., 100.), (300., 200.), (50., 50.)), (100., 50.));
    assert_eq!(offset((1000., 100.), (300., 200.), (50., 9.)), (-50., 50.));
    assert_eq!(clamp((-5., 900.), (1000., 1000.), (300., 200.)), (0., 800.));
    assert_eq!(clamp((40., 40.), (100., 100.), (300., 200.)), (0., 0.));
}

#[test]
fn zooming_keeps_the_point_under_the_pointer_still() {
    let pages = [(1000., 1000.)];
    let viewport = (400., 300.);
    let camera = Camera {
        zoom: 1.,
        scroll: (200., 300.),
        fit: false,
    };
    let anchor = (100., 80.);
    let zoomed = zoom_about(&pages, viewport, camera, 2., anchor);
    // The page point under the anchor, before and after.
    let point = |camera: Camera| {
        let laid = layout(&pages, camera.zoom);
        let (left, top) = offset(laid.size, viewport, camera.scroll);
        let page = laid.pages[0];
        (
            (anchor.0 - left - page.x) / camera.zoom,
            (anchor.1 - top - page.y) / camera.zoom,
        )
    };
    let (before, after) = (point(camera), point(zoomed));
    assert!(
        close(before.0, after.0) && close(before.1, after.1),
        "{before:?} {after:?}"
    );
    assert!(!zoomed.fit, "a zoom leaves fitting");
}

#[test]
fn zooming_over_a_later_page_keeps_that_page_under_the_pointer() {
    let pages = [(600., 800.), (600., 800.), (600., 800.)];
    let viewport = (700., 500.);
    let camera = Camera {
        zoom: 1.,
        scroll: (0., 900.),
        fit: false,
    };
    let zoomed = zoom_about(&pages, viewport, camera, 1.5, (350., 250.));
    let laid = layout(&pages, zoomed.zoom);
    let (_, top) = offset(laid.size, viewport, zoomed.scroll);
    let under = 250. - top;
    assert!(laid.pages[1].y <= under && under <= laid.pages[1].bottom());
}

#[test]
fn the_zoom_steps_go_in_and_out_from_between_steps() {
    assert!(close(step(1., true), 1.1));
    assert!(close(step(1., false), 0.9));
    assert!(close(step(1.3, true), 1.5));
    assert!(close(step(64., true), 64.));
}

#[test]
fn the_page_in_sight_is_the_one_across_the_middle() {
    let pages = [(600., 800.), (600., 800.), (600., 800.)];
    let laid = layout(&pages, 1.);
    assert_eq!(page_in_sight(&laid, (700., 500.), (0., 0.)), 0);
    assert_eq!(page_in_sight(&laid, (700., 500.), (0., 1000.)), 1);
    assert_eq!(page_in_sight(&laid, (700., 500.), (0., 2000.)), 2);
}
