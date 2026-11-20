use super::parse;
use koyo_browser_ipc::Tone;

fn grid(cell: &str) -> String {
    vec![cell; Tone::ACROSS * Tone::DOWN].join(",")
}

#[test]
fn what_the_watcher_says_is_a_tone_only_of_its_shape() {
    assert_eq!(parse("1,0.5"), None);
    assert_eq!(parse(""), None);
    let dark = parse(&format!("{};18 18 18", grid("0.006"))).expect("a tone");
    assert_eq!(dark.canvas, [18, 18, 18]);
    assert!(dark.lightness.iter().all(|light| (light - 0.006).abs() < f32::EPSILON));
}

#[test]
fn a_canvas_of_another_shape_is_white() {
    assert_eq!(parse(&grid("1")).map(|tone| tone.canvas), Some([255; 3]));
    assert_eq!(parse(&format!("{};red", grid("1"))).map(|tone| tone.canvas), Some([255; 3]));
}
