//! Which picture of a dynamic wallpaper macOS shows (`docs/chat-backdrop.md` 3.2): a HEIC
//! holding several, whose first image's `apple_desktop` metadata says which to show by the
//! appearance (`apr`), by the time of day (`h24`) or by where the sun is (`solar`), and the
//! person's choice of how to show it, Automatic, Dynamic, Light (Still) or Dark (Still).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, Once, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use chrono::Timelike as _;
use objc2_core_foundation::CFString;
use serde::Deserialize;

/// The pictures a dynamic wallpaper shows in light and dark.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
pub struct Appearance {
    #[serde(rename = "l", deserialize_with = "index")]
    light: usize,
    #[serde(rename = "d", deserialize_with = "index")]
    dark: usize,
}

/// A picture shown from a time of day on, `at` the fraction of the day gone.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
struct AtTime {
    #[serde(rename = "t")]
    at: f64,
    #[serde(rename = "i", deserialize_with = "index")]
    image: usize,
}

/// A picture shown while the sun is nearest `altitude` and `azimuth`, in degrees, the azimuth
/// from north towards east.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
struct AtSun {
    #[serde(rename = "a")]
    altitude: f64,
    #[serde(rename = "z")]
    azimuth: f64,
    #[serde(rename = "i", deserialize_with = "index")]
    image: usize,
}

/// A picture's index, written as an integer, or as a whole real by some tools.
fn index<'de, D: serde::Deserializer<'de>>(written: D) -> Result<usize, D::Error> {
    let index = f64::deserialize(written)?;
    if index >= 0. && index.fract() == 0. && index < 1e6 {
        Ok(index as usize)
    } else {
        Err(serde::de::Error::custom(format!(
            "{index} isn't a picture's index"
        )))
    }
}

/// What a dynamic wallpaper shows when.
#[derive(Clone, Debug, PartialEq)]
enum Schedule {
    Appearance,
    Time(Vec<AtTime>),
    Sun(Vec<AtSun>),
}

/// A dynamic wallpaper's metadata.
#[derive(Clone, Debug, PartialEq)]
pub struct Dynamic {
    appearance: Option<Appearance>,
    schedule: Schedule,
}

/// How the person chose to show a dynamic wallpaper, in System Settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// By the appearance.
    Automatic,
    /// Through the day.
    Dynamic,
    Light,
    Dark,
}

/// A place on Earth, in degrees, north and east positive.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    latitude: f64,
    longitude: f64,
}

/// What's needed to pick a picture now.
#[derive(Clone, Copy, Debug)]
pub struct Now {
    pub dark: bool,
    /// Seconds since 1970, UTC.
    pub unix: f64,
    /// The fraction of the local day gone.
    pub day: f64,
    pub place: Option<Place>,
}

impl Dynamic {
    /// From the `apple_desktop` values of a wallpaper's first image, each a binary property
    /// list in Base64; none when it has none, and isn't dynamic.
    fn parse(apr: Option<&str>, h24: Option<&str>, solar: Option<&str>) -> Option<Dynamic> {
        #[derive(Deserialize)]
        struct Times {
            ap: Option<Appearance>,
            ti: Vec<AtTime>,
        }
        #[derive(Deserialize)]
        struct Suns {
            ap: Option<Appearance>,
            si: Vec<AtSun>,
        }
        fn plist<T: serde::de::DeserializeOwned>(value: &str) -> Option<T> {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(value.trim())
                .ok()?;
            plist::from_bytes(&bytes).ok()
        }
        if let Some(Suns { ap, si }) = solar
            .and_then(plist)
            .filter(|suns: &Suns| !suns.si.is_empty())
        {
            return Some(Dynamic {
                appearance: ap,
                schedule: Schedule::Sun(si),
            });
        }
        if let Some(Times { ap, mut ti }) = h24
            .and_then(plist)
            .filter(|times: &Times| !times.ti.is_empty())
        {
            ti.sort_by(|a, b| a.at.total_cmp(&b.at));
            return Some(Dynamic {
                appearance: ap,
                schedule: Schedule::Time(ti),
            });
        }
        Some(Dynamic {
            appearance: Some(apr.and_then(plist)?),
            schedule: Schedule::Appearance,
        })
    }

    /// The picture macOS shows `now`, shown as `style` says.
    pub fn pick(&self, style: Style, now: Now) -> usize {
        let by_appearance = |dark: bool| {
            self.appearance
                .map(|ap| if dark { ap.dark } else { ap.light })
        };
        let through_the_day = || match &self.schedule {
            Schedule::Appearance => None,
            Schedule::Time(times) => {
                let past = times.iter().rev().find(|time| time.at <= now.day);
                past.or(times.last()).map(|time| time.image)
            }
            Schedule::Sun(suns) => {
                let (altitude, azimuth) = sun(now.unix, now.place?);
                (suns.iter())
                    .max_by(|a, b| {
                        let near = |at: &AtSun| closeness(at, altitude, azimuth);
                        near(a).total_cmp(&near(b))
                    })
                    .map(|at| at.image)
            }
        };
        let picked = match style {
            Style::Light => by_appearance(false),
            Style::Dark => by_appearance(true),
            Style::Automatic => by_appearance(now.dark),
            Style::Dynamic => through_the_day(),
        };
        picked
            .or_else(through_the_day)
            .or_else(|| by_appearance(now.dark))
            .unwrap_or(0)
    }
}

/// How near the sun at `altitude` and `azimuth` is to where `at` has it: the cosine of the
/// angle between them on the sky.
fn closeness(at: &AtSun, altitude: f64, azimuth: f64) -> f64 {
    let (a1, a2) = (at.altitude.to_radians(), altitude.to_radians());
    let dz = (at.azimuth - azimuth).to_radians();
    a1.sin() * a2.sin() + a1.cos() * a2.cos() * dz.cos()
}

/// Where the sun is at `unix` seen from `place`: its altitude and its azimuth from north towards
/// east, in degrees. The low-precision formulas of the Astronomical Almanac, good to about a
/// hundredth of a degree in this century, far finer than a wallpaper's steps.
fn sun(unix: f64, place: Place) -> (f64, f64) {
    let days = unix / 86_400. - 10_957.5;
    let anomaly = (357.529 + 0.985_600_28 * days).to_radians();
    let mean = 280.459 + 0.985_647_36 * days;
    let longitude = (mean + 1.915 * anomaly.sin() + 0.020 * (2. * anomaly).sin()).to_radians();
    let obliquity = (23.439 - 0.000_000_36 * days).to_radians();
    let ascension = (obliquity.cos() * longitude.sin()).atan2(longitude.cos());
    let declination = (obliquity.sin() * longitude.sin()).asin();
    let sidereal = (18.697_374_558 + 24.065_709_824_419_08 * days) * 15.;
    let hour = (sidereal + place.longitude).to_radians() - ascension;
    let latitude = place.latitude.to_radians();
    let altitude = (latitude.sin() * declination.sin()
        + latitude.cos() * declination.cos() * hour.cos())
    .asin();
    let azimuth = (-declination.cos() * hour.sin()).atan2(
        declination.sin() * latitude.cos() - declination.cos() * latitude.sin() * hour.cos(),
    );
    (altitude.to_degrees(), azimuth.to_degrees().rem_euclid(360.))
}

/// The picture to show of the wallpaper at `path`, last changed at `modified`: the one macOS
/// shows now, if it's dynamic; none if it isn't.
pub fn image(path: &Path, modified: Option<SystemTime>) -> Option<usize> {
    let dynamic = metadata(path, modified)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0., |since| since.as_secs_f64());
    let local = chrono::Local::now();
    let now = Now {
        dark: dark(),
        unix: now,
        day: f64::from(local.num_seconds_from_midnight()) / 86_400.,
        place: place(),
    };
    Some(dynamic.pick(style().unwrap_or(Style::Dynamic), now))
}

/// The dynamic metadata of the wallpaper at `path`, read once for each time it changes.
fn metadata(path: &Path, modified: Option<SystemTime>) -> Option<Dynamic> {
    type Read = HashMap<PathBuf, (Option<SystemTime>, Option<Dynamic>)>;
    static READ: LazyLock<Mutex<Read>> = LazyLock::new(Mutex::default);
    let mut read = READ.lock().ok()?;
    if let Some((when, dynamic)) = read.get(path)
        && *when == modified
    {
        return dynamic.clone();
    }
    let dynamic = read_metadata(path);
    read.insert(path.to_owned(), (modified, dynamic.clone()));
    dynamic
}

/// The `apple_desktop` values of the first image of the picture at `path`.
fn read_metadata(path: &Path) -> Option<Dynamic> {
    let source = crate::viewer::picture::source(path).ok()?;
    // SAFETY: a source just made, and a path in ImageIO's own prefix:name form.
    let metadata = unsafe { source.metadata_at_index(0, None) }?;
    let value = |name: &str| {
        let path = CFString::from_str(&format!("apple_desktop:{name}"));
        unsafe { metadata.string_value_with_path(None, &path) }.map(|value| value.to_string())
    };
    Dynamic::parse(
        value("apr").as_deref(),
        value("h24").as_deref(),
        value("solar").as_deref(),
    )
}

/// Whether the system's appearance is dark, whatever Koyo's own theme is.
fn dark() -> bool {
    let defaults = objc2_foundation::NSUserDefaults::standardUserDefaults();
    let style = defaults.stringForKey(objc2_foundation::ns_string!("AppleInterfaceStyle"));
    style.is_some_and(|style| style.to_string() == "Dark")
}

/// Where the system's time zone is, from the zone's own entry in the time zone database: the
/// place a dynamic wallpaper follows the sun at, as Koyo has no location of its own.
fn place() -> Option<Place> {
    static PLACE: OnceLock<Option<Place>> = OnceLock::new();
    *PLACE.get_or_init(|| {
        let zone = std::fs::read_link("/etc/localtime").ok()?;
        let zone = zone.to_str()?.split_once("zoneinfo/")?.1.to_owned();
        let table = std::fs::read_to_string("/usr/share/zoneinfo/zone.tab").ok()?;
        place_of(&table, &zone)
    })
}

/// The place of `zone` in a `zone.tab`: its second column, `±DDMM[SS]±DDDMM[SS]`.
fn place_of(table: &str, zone: &str) -> Option<Place> {
    let line = table
        .lines()
        .filter(|line| !line.starts_with('#'))
        .find(|line| line.split('\t').nth(2) == Some(zone))?;
    let at = line.split('\t').nth(1)?;
    let split = at[1..].find(['+', '-'])? + 1;
    let angle = |text: &str, degrees: usize| {
        let sign = if text.starts_with('-') { -1. } else { 1. };
        let digits = &text[1..];
        let part =
            |from: usize, to: usize| digits.get(from..to).and_then(|d| d.parse::<f64>().ok());
        let whole = part(0, degrees)?;
        let minutes = part(degrees, degrees + 2)?;
        let seconds = part(degrees + 2, degrees + 4).unwrap_or(0.);
        Some(sign * (whole + minutes / 60. + seconds / 3600.))
    };
    Some(Place {
        latitude: angle(&at[..split], 2)?,
        longitude: angle(&at[split..], 3)?,
    })
}

/// How the person chose to show their dynamic wallpaper, from the store System Settings keeps it
/// in, a private format read as macOS 27.2 writes it (`docs/chat-backdrop.md` 3.2). The choice
/// for every screen and Space; one made for a single display isn't read. Read again only when
/// the store changes; none, said once, when it can't be read.
fn style() -> Option<Style> {
    type Kept = Option<(Option<SystemTime>, Option<Style>)>;
    static KEPT: Mutex<Kept> = Mutex::new(None);
    static SAID: Once = Once::new();
    let store = std::env::home_dir()?
        .join("Library/Application Support/com.apple.wallpaper/Store/Index.plist");
    let modified = std::fs::metadata(&store)
        .and_then(|file| file.modified())
        .ok();
    let mut kept = KEPT.lock().ok()?;
    if let Some((when, style)) = *kept
        && when == modified
    {
        return style;
    }
    let style = std::fs::read(&store)
        .ok()
        .and_then(|bytes| style_in(&bytes));
    if style.is_none() {
        SAID.call_once(|| {
            log::warn!(
                "how a dynamic wallpaper is shown couldn't be read from {}: shown as Dynamic, \
                 by the appearance if it has no schedule",
                store.display()
            );
        });
    }
    *kept = Some((modified, style));
    style
}

/// The style in a wallpaper store's bytes.
fn style_in(store: &[u8]) -> Option<Style> {
    let store: plist::Value = plist::from_bytes(store).ok()?;
    let options = store
        .as_dictionary()?
        .get("AllSpacesAndDisplays")?
        .as_dictionary()?
        .get("Desktop")?
        .as_dictionary()?
        .get("Content")?
        .as_dictionary()?
        .get("EncodedOptionValues")?
        .as_data()?;
    let options: plist::Value = plist::from_bytes(options).ok()?;
    let id = ["values", "style", "picker", "_0", "id"]
        .iter()
        .try_fold(&options, |value, key| value.as_dictionary()?.get(key))?
        .as_string()?;
    match id {
        "automatic" => Some(Style::Automatic),
        "dynamic" => Some(Style::Dynamic),
        "light" => Some(Style::Light),
        "dark" => Some(Style::Dark),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
