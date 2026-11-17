use std::path::Path;

use base64::Engine as _;
use plist::{Dictionary, Value};

use super::{Dynamic, Now, Place, Style, place_of, read_metadata, style_in, sun};

/// A value as a wallpaper's metadata holds it: a binary property list in Base64.
fn encoded(value: &Value) -> String {
    let mut bytes = Vec::new();
    plist::to_writer_binary(&mut bytes, value).unwrap();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn dict(entries: &[(&str, Value)]) -> Value {
    let mut dict = Dictionary::new();
    for (key, value) in entries {
        dict.insert((*key).to_owned(), value.clone());
    }
    Value::Dictionary(dict)
}

fn appearance(light: i64, dark: i64) -> Value {
    dict(&[("l", light.into()), ("d", dark.into())])
}

const BRUSSELS: Place = Place {
    latitude: 50.833,
    longitude: 4.333,
};

fn now(dark: bool, unix: f64, day: f64) -> Now {
    Now {
        dark,
        unix,
        day,
        place: Some(BRUSSELS),
    }
}

/// 2026-06-21 00:00 UTC.
const SOLSTICE: f64 = 1_782_000_000.;

#[test]
fn an_appearance_wallpaper_shows_its_light_or_dark_picture() {
    let sonoma_like = Dynamic::parse(Some(&encoded(&appearance(0, 1))), None, None).unwrap();
    assert_eq!(sonoma_like.pick(Style::Automatic, now(true, 0., 0.)), 1);
    assert_eq!(sonoma_like.pick(Style::Automatic, now(false, 0., 0.)), 0);
    assert_eq!(
        sonoma_like.pick(Style::Dynamic, now(true, 0., 0.)),
        1,
        "with no schedule, Dynamic follows the appearance"
    );
    assert_eq!(sonoma_like.pick(Style::Light, now(true, 0., 0.)), 0);
    assert_eq!(sonoma_like.pick(Style::Dark, now(false, 0., 0.)), 1);
    assert_eq!(Dynamic::parse(None, None, None), None, "not dynamic");
}

#[test]
fn a_time_of_day_wallpaper_shows_the_picture_whose_time_last_came() {
    let at = |t: f64, i: i64| dict(&[("t", t.into()), ("i", i.into())]);
    let h24 = dict(&[
        ("ap", appearance(1, 2)),
        (
            "ti",
            Value::Array(vec![at(0.75, 2), at(0.25, 0), at(0.5, 1)]),
        ),
    ]);
    let times = Dynamic::parse(None, Some(&encoded(&h24)), None).unwrap();
    let at_day = |day: f64| times.pick(Style::Dynamic, now(false, 0., day));
    assert_eq!(at_day(0.3), 0);
    assert_eq!(at_day(0.5), 1, "from its time on");
    assert_eq!(at_day(0.9), 2);
    assert_eq!(
        at_day(0.1),
        2,
        "before the first, the evening's still shows"
    );
    assert_eq!(
        times.pick(Style::Dark, now(false, 0., 0.3)),
        2,
        "Dark (Still) is its dark picture, whatever the time"
    );
    let real = |t: f64, i: f64| dict(&[("t", t.into()), ("i", i.into())]);
    let written_as_reals = dict(&[("ti", Value::Array(vec![real(0.25, 0.), real(0.5, 1.)]))]);
    let times = Dynamic::parse(None, Some(&encoded(&written_as_reals)), None).unwrap();
    assert_eq!(
        times.pick(Style::Dynamic, now(false, 0., 0.6)),
        1,
        "an index written as a whole real"
    );
    let halves = dict(&[("ti", Value::Array(vec![real(0.25, 0.5)]))]);
    assert_eq!(
        Dynamic::parse(None, Some(&encoded(&halves)), None),
        None,
        "no picture is half of one"
    );
}

#[test]
fn the_sun_is_where_the_almanac_has_it() {
    let day = (0..1440).map(|minute| SOLSTICE + f64::from(minute) * 60.);
    let (when, (altitude, azimuth)) = day
        .map(|unix| (unix, sun(unix, BRUSSELS)))
        .max_by(|a, b| a.1.0.total_cmp(&b.1.0))
        .unwrap();
    let expected = 90. - BRUSSELS.latitude + 23.44;
    assert!((altitude - expected).abs() < 0.3, "{altitude} at noon");
    assert!((azimuth - 180.).abs() < 1.5, "{azimuth} at noon");
    let noon = (when - SOLSTICE) / 3600.;
    assert!((11.5..12.).contains(&noon), "noon at {noon} h UTC");
    let (midnight, _) = sun(SOLSTICE, BRUSSELS);
    assert!(midnight < -10., "{midnight} at midnight");
    let (_, morning) = sun(SOLSTICE + 6. * 3600., BRUSSELS);
    assert!(
        (45.0..135.).contains(&morning),
        "east in the morning, {morning}"
    );
}

#[test]
fn a_solar_wallpaper_shows_the_picture_nearest_the_sun() {
    let at = |a: f64, z: f64, i: i64| dict(&[("a", a.into()), ("z", z.into()), ("i", i.into())]);
    let solar = dict(&[
        ("ap", appearance(0, 3)),
        (
            "si",
            Value::Array(vec![
                at(-30., 0., 3),
                at(10., 80., 1),
                at(60., 180., 0),
                at(10., 280., 2),
            ]),
        ),
    ]);
    let suns = Dynamic::parse(None, None, Some(&encoded(&solar))).unwrap();
    let at_hour = |hour: f64| suns.pick(Style::Dynamic, now(false, SOLSTICE + hour * 3600., 0.));
    assert_eq!(at_hour(0.), 3, "night");
    assert_eq!(at_hour(4.), 1, "sunrise, in the east");
    assert_eq!(at_hour(11.7), 0, "noon");
    assert_eq!(at_hour(19.5), 2, "sunset, in the west");
    assert_eq!(
        suns.pick(Style::Dark, now(false, SOLSTICE + 11.7 * 3600., 0.)),
        3,
        "Dark (Still)"
    );
    let nowhere = Now {
        place: None,
        ..now(true, SOLSTICE + 11.7 * 3600., 0.)
    };
    assert_eq!(
        suns.pick(Style::Dynamic, nowhere),
        3,
        "with no place, by the appearance"
    );
}

#[test]
fn the_choice_is_read_from_the_store_and_anything_else_is_none() {
    let store = |id: &str| {
        let options = dict(&[(
            "values",
            dict(&[(
                "style",
                dict(&[("picker", dict(&[("_0", dict(&[("id", id.into())]))]))]),
            )]),
        )]);
        let mut options_bytes = Vec::new();
        plist::to_writer_binary(&mut options_bytes, &options).unwrap();
        let content = dict(&[
            ("EncodedOptionValues", Value::Data(options_bytes)),
            (
                "Choices",
                Value::Array(vec![dict(&[(
                    "Provider",
                    "com.apple.wallpaper.choice.dynamic".into(),
                )])]),
            ),
        ]);
        let store = dict(&[(
            "AllSpacesAndDisplays",
            dict(&[("Desktop", dict(&[("Content", content)]))]),
        )]);
        let mut bytes = Vec::new();
        plist::to_writer_binary(&mut bytes, &store).unwrap();
        bytes
    };
    assert_eq!(style_in(&store("dark")), Some(Style::Dark));
    assert_eq!(style_in(&store("light")), Some(Style::Light));
    assert_eq!(style_in(&store("automatic")), Some(Style::Automatic));
    assert_eq!(style_in(&store("dynamic")), Some(Style::Dynamic));
    assert_eq!(style_in(&store("sepia")), None, "a style it doesn't know");
    assert_eq!(style_in(b"not a property list"), None);
}

#[test]
fn a_time_zone_is_placed_where_the_database_puts_it() {
    let table = "# comment\nBE\t+5050+00420\tEurope/Brussels\n\
                 US\t+404251-0740023\tAmerica/New_York\tEastern (most areas)\n";
    let brussels = place_of(table, "Europe/Brussels").unwrap();
    assert!((brussels.latitude - 50.833).abs() < 0.01 && (brussels.longitude - 4.333).abs() < 0.01);
    let new_york = place_of(table, "America/New_York").unwrap();
    assert!(
        (new_york.latitude - 40.714).abs() < 0.01 && (new_york.longitude + 74.006).abs() < 0.01
    );
    assert_eq!(place_of(table, "Mars/Olympus"), None);
}

/// Sonoma's own wallpaper, where macOS has it: light first, dark second.
#[test]
fn sonoma_is_an_appearance_wallpaper() {
    let path = Path::new("/System/Library/Desktop Pictures/Sonoma.heic");
    if !path.exists() {
        return;
    }
    let sonoma = read_metadata(path).expect("Sonoma's metadata");
    assert_eq!(sonoma.pick(Style::Automatic, now(true, 0., 0.)), 1);
    assert_eq!(sonoma.pick(Style::Automatic, now(false, 0., 0.)), 0);
}

/// What each wallpaper named in `KOYO_DYNAMIC` (paths split by `:`) shows through a day, for a
/// person to compare with what macOS shows:
/// `KOYO_DYNAMIC=a.heic:b.heic cargo test … dynamic_wallpapers_through_a_day -- --ignored --nocapture`.
#[test]
#[ignore = "reads wallpapers named in KOYO_DYNAMIC"]
fn dynamic_wallpapers_through_a_day() {
    let paths = std::env::var("KOYO_DYNAMIC").expect("KOYO_DYNAMIC");
    for path in paths.split(':') {
        let dynamic = read_metadata(Path::new(path)).expect(path);
        let styles = [Style::Automatic, Style::Dynamic, Style::Light, Style::Dark];
        println!("{path}");
        println!(
            "  now, with the store's choice and the system's appearance → {:?}",
            super::image(Path::new(path), None)
        );
        for dark in [false, true] {
            let still: Vec<_> = styles
                .iter()
                .map(|style| dynamic.pick(*style, now(dark, SOLSTICE, 0.)))
                .collect();
            println!("  dark {dark}: Automatic, Dynamic, Light, Dark at midnight → {still:?}");
        }
        let day: Vec<_> = (0..24)
            .map(|hour| {
                let hour = f64::from(hour);
                dynamic.pick(
                    Style::Dynamic,
                    now(false, SOLSTICE + hour * 3600., hour / 24.),
                )
            })
            .collect();
        println!("  Dynamic, each hour of 2026-06-21 UTC in Brussels: {day:?}");
    }
}
