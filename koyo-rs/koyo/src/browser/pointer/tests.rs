use super::*;

const PAGE: Size<f32> = Size {
    width: 1200.,
    height: 800.,
};

fn distance(a: Point<f32>, b: Point<f32>) -> f32 {
    (a - b).x.hypot((a - b).y)
}

fn at(start: Instant, millis: u64) -> Instant {
    start + Duration::from_millis(millis)
}

/// How far a point lies off the arc's ellipse: 0 on it.
fn off_ellipse(arc: &Arc, p: Point<f32>) -> f32 {
    let d = p - arc.centre;
    let (along, across) = (d.x * arc.u.x + d.y * arc.u.y, d.x * arc.n.x + d.y * arc.n.y);
    ((along / arc.a).powi(2) + (across / arc.b).powi(2) - 1.).abs()
}

#[test]
fn an_arc_runs_from_where_the_pointer_is_to_where_it_clicks_on_one_ellipse() {
    let (from, to) = (point(100., 500.), point(700., 300.));
    let arc = Arc::new(from, to, None, PAGE).unwrap();
    assert!(distance(arc.at(0.), from) < 0.01);
    assert!(distance(arc.at(1.), to) < 0.01);
    assert!(arc.length() > distance(from, to), "it bows");
    for step in 0..=20 {
        let p = arc.at(step as f32 / 20.);
        assert!(off_ellipse(&arc, p) < 1e-3, "on the arc at {step}: {p:?}");
    }
    // It leaves its chord at the same angle it comes back to it.
    let chord = (to - from).y.atan2((to - from).x);
    let (leaves, arrives) = (turn(chord, arc.heading(0.)), turn(chord, arc.heading(1.)));
    assert!(
        (leaves.abs() - 38_f32.to_radians()).abs() < 2_f32.to_radians(),
        "{leaves}"
    );
    assert!((leaves + arrives).abs() < 0.01);
}

#[test]
fn with_no_heading_an_arc_bows_up_and_stays_in_the_page() {
    let arc = Arc::new(point(100., 400.), point(900., 400.), None, PAGE).unwrap();
    assert!(arc.at(0.5).y < 400., "up");
    // Along the top edge, bowing up would leave the page.
    let arc = Arc::new(point(100., 10.), point(900., 10.), None, PAGE).unwrap();
    assert!(arc.at(0.5).y > 10., "down, inside");
    assert!(
        Arc::new(point(5., 5.), point(6., 6.), None, PAGE).is_none(),
        "too short"
    );
}

#[test]
fn an_arc_leaves_on_the_side_nearest_the_heading_the_pointer_has() {
    let (from, to) = (point(400., 400.), point(800., 400.));
    let down = Arc::new(from, to, Some(30_f32.to_radians()), PAGE).unwrap();
    assert!(down.heading(0.) > 0. && down.at(0.5).y > 400.);
    let up = Arc::new(from, to, Some(-30_f32.to_radians()), PAGE).unwrap();
    assert!(up.heading(0.) < 0. && up.at(0.5).y < 400.);
}

#[test]
fn the_pointer_eases_in_and_out_along_its_arc_and_points_where_it_goes() {
    let start = Instant::now();
    let mut pointer = Pointer::default();
    pointer.glide_to(point(200., 200.), PAGE, false, start);
    let begun = at(start, 1000);
    let travel = pointer.glide_to(point(900., 600.), PAGE, false, begun);
    let millis = travel.as_millis() as u64;
    assert!((380..=900).contains(&millis), "{millis}");
    let tip = |t: u64| pointer.tip(at(begun, t));
    let step = millis / 50;
    let first = distance(tip(0), tip(step));
    let middle = distance(tip(millis / 2), tip(millis / 2 + step));
    let last = distance(tip(millis - step), tip(millis));
    assert!(
        first < middle / 10. && last < middle / 10.,
        "{first} {middle} {last}"
    );
    // Mid-way it heads along the arc, its tip on it.
    let glide = pointer.glide.clone().unwrap();
    let now = at(begun, millis / 2);
    let along = glide.along(now);
    assert!(off_ellipse(&glide.arc, pointer.tip(now)) < 1e-3);
    assert!(turn(pointer.heading(now), glide.arc.heading(along)).abs() < 1e-4);
    assert_eq!(pointer.tip(at(begun, millis + 1)), point(900., 600.));
    assert_eq!(pointer.next(now), Next::Frame);
}

#[test]
fn a_pointer_never_shown_comes_in_from_below_left_and_fades_in() {
    let start = Instant::now();
    let mut pointer = Pointer::default();
    assert_eq!(pointer.shot(start), None);
    assert_eq!(pointer.next(start), Next::Never);
    pointer.glide_to(point(600., 300.), PAGE, false, start);
    let came_from = pointer.tip(start);
    assert!(
        distance(came_from, point(450., 420.)) < 0.01,
        "{came_from:?}"
    );
    assert!(pointer.opacity(start).abs() < 1e-6);
    assert!(pointer.opacity(at(start, 200)) > 0.99);
    // At the left edge, it comes from the right.
    let mut edge = Pointer::default();
    edge.glide_to(point(20., 300.), PAGE, false, start);
    assert!(edge.tip(start).x > 20.);
}

#[test]
fn it_fades_away_six_seconds_after_its_click_and_a_new_click_brings_it_back() {
    let start = Instant::now();
    let mut pointer = Pointer::default();
    let travel = pointer.glide_to(point(600., 300.), PAGE, false, start);
    let clicked = start + travel;
    pointer.click(clicked);
    let ring = pointer.shot(at(clicked, 100)).unwrap().ring.unwrap();
    assert!(ring.0 > 4. && ring.1 > 0.);
    assert_eq!(pointer.shot(at(clicked, 600)).unwrap().ring, None);
    assert_eq!(pointer.next(at(clicked, 1000)), Next::At(clicked + IDLE));
    assert!((pointer.opacity(at(clicked, 5_900)) - 1.).abs() < 1e-6);
    let fading = pointer.opacity(at(clicked, 6_450));
    assert!(fading > 0. && fading < 1., "{fading}");
    assert_eq!(pointer.next(at(clicked, 6_450)), Next::Frame);
    assert_eq!(pointer.shot(at(clicked, 7_000)), None);
    assert_eq!(pointer.next(at(clicked, 7_000)), Next::Never);
    // Back where it was, and on its way from there.
    let back = at(clicked, 20_000);
    pointer.glide_to(point(100., 100.), PAGE, false, back);
    assert_eq!(pointer.tip(back), point(600., 300.));
    assert!(pointer.opacity(at(back, 200)) > 0.99);
}

#[test]
fn less_motion_jumps_and_the_person_taking_the_page_hides_it() {
    let start = Instant::now();
    let mut pointer = Pointer::default();
    assert_eq!(
        pointer.glide_to(point(600., 300.), PAGE, true, start),
        Duration::ZERO
    );
    assert_eq!(pointer.tip(start), point(600., 300.));
    pointer.dismiss(at(start, 1000));
    assert!(pointer.opacity(at(start, 1100)) < 1.);
    assert_eq!(pointer.shot(at(start, 1300)), None);
}

#[test]
fn the_glow_comes_and_goes_from_where_it_was() {
    let start = Instant::now();
    let mut glow = glow();
    assert!(glow.level(start).abs() < 1e-6);
    glow.set(true, start);
    assert!(glow.changing(at(start, 100)));
    assert!((glow.level(at(start, 500)) - 1.).abs() < 1e-6);
    assert!(!glow.changing(at(start, 500)));
    glow.set(false, at(start, 1000));
    let midway = glow.level(at(start, 1200));
    assert!(midway > 0. && midway < 1.);
    glow.set(true, at(start, 1200));
    assert!(
        (glow.level(at(start, 1200)) - midway).abs() < 1e-6,
        "no jump"
    );
    assert!((glow.level(at(start, 2000)) - 1.).abs() < 1e-6);
}
