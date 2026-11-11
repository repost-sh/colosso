use super::{Element, settled};

fn element(frame: [f64; 4], children: Vec<Element>) -> Element {
    Element {
        frame,
        children,
        enabled: true,
        ..Element::default()
    }
}

#[test]
fn a_status_bar_alone_isnt_an_apps_tree() {
    let bar = element([0., 0., 402., 54.], vec![element([50., 20., 40., 20.], Vec::new())]);
    let screen = [0., 0., 402., 874.];
    assert!(!settled(&element(screen, vec![bar.clone()])));
    assert!(!settled(&element(screen, Vec::new())));
    let row = element([0., 300., 402., 44.], Vec::new());
    assert!(settled(&element(screen, vec![bar, row])));
}

#[test]
fn a_ref_touches_where_its_element_activates_inside_it_else_its_middle() {
    let mut switch = element([0., 300., 402., 52.], Vec::new());
    switch.role = "CheckBox".into();
    switch.label = Some("Sound".into());
    switch.activation = Some([350., 326.]);
    let mut outside = element([0., 400., 402., 52.], Vec::new());
    outside.activation = Some([900., 426.]);
    let root = element([0., 0., 402., 874.], vec![switch, outside]);
    let (lines, refs, truncated) = super::lines(&root, 1, 10);
    assert!(lines.contains("CheckBox \"Sound\" [ref=e2] @350,326"), "{lines}");
    assert_eq!((refs[2].1.x, refs[2].1.y), (201., 426.));
    assert!(!truncated);
}
