//! A release's number and macOS's, compared part by part.

use std::fmt;
use std::str::FromStr;

use objc2_foundation::NSProcessInfo;

/// A release's number as `x.y.z`, or macOS's as `x.y` or `x.y.z`, a missing part being 0.
/// Compared part by part. Parsing refuses anything but dotted digits, so a pre-release or a
/// stray suffix in a feed is a malformed feed, not a version that sorts oddly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    major: u32,
    minor: u32,
    patch: u32,
}

impl Version {
    /// This build's, from the crate's version, read when it's compiled: nothing to fail as
    /// the app runs.
    pub const RUNNING: Version = Version {
        major: number(env!("CARGO_PKG_VERSION_MAJOR")),
        minor: number(env!("CARGO_PKG_VERSION_MINOR")),
        patch: number(env!("CARGO_PKG_VERSION_PATCH")),
    };

    /// This Mac's macOS, as the system says it of itself. No file is read.
    pub(super) fn macos() -> Version {
        let system = NSProcessInfo::processInfo().operatingSystemVersion();
        let part = |part: isize| u32::try_from(part).unwrap_or_default();
        Version {
            major: part(system.majorVersion),
            minor: part(system.minorVersion),
            patch: part(system.patchVersion),
        }
    }

    /// As macOS's versions are written: `13.0`, and `12.7.6` with a patch.
    pub(super) fn as_macos(self) -> String {
        if self.patch == 0 {
            format!("{}.{}", self.major, self.minor)
        } else {
            self.to_string()
        }
    }
}

/// The number `digits` write, which the compiler stops at anything but digits.
const fn number(digits: &str) -> u32 {
    let mut left = digits.as_bytes();
    let mut value = 0;
    while let [digit, rest @ ..] = left {
        assert!(
            digit.is_ascii_digit(),
            "a part of the crate's version isn't a number"
        );
        value = value * 10 + (*digit - b'0') as u32;
        left = rest;
    }
    value
}

impl FromStr for Version {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut parts = [0; 3];
        let mut written = text.split('.');
        for part in &mut parts {
            let Some(digits) = written.next() else {
                break;
            };
            let plain = !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit());
            *part = (digits.parse().ok())
                .filter(|_| plain)
                .ok_or_else(|| format!("{text:?} isn't a version: dotted numbers alone are"))?;
        }
        if written.next().is_some() {
            return Err(format!("{text:?} isn't a version: it has more than three parts"));
        }
        let [major, minor, patch] = parts;
        Ok(Version {
            major,
            minor,
            patch,
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::Version;

    fn version(text: &str) -> Version {
        text.parse().unwrap()
    }

    #[test]
    fn dotted_numbers_parse_and_a_missing_part_is_zero() {
        assert_eq!(version("0.2.0").to_string(), "0.2.0");
        assert_eq!(version("13.0"), version("13.0.0"));
        assert_eq!(version("14"), version("14.0.0"));
        assert_eq!(version("12.7.6").as_macos(), "12.7.6");
        assert_eq!(version("13.0").as_macos(), "13.0");
    }

    #[test]
    fn anything_but_dotted_numbers_is_refused() {
        for text in ["0.2.0-beta", "0.2.x", "", "1..2", "1.2.3.4", "+1.2", " 1.2", "v1.2"] {
            assert!(text.parse::<Version>().is_err(), "{text:?} parsed");
        }
    }

    #[test]
    fn versions_order_part_by_part() {
        assert!(version("0.10.0") > version("0.9.9"));
        assert!(version("1.0.0") > version("0.99.99"));
        assert!(version("0.2.1") > version("0.2.0"));
        assert_eq!(version("0.2"), version("0.2.0"));
    }

    #[test]
    fn the_running_version_is_the_crate_s() {
        assert_eq!(Version::RUNNING, version(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn this_mac_says_which_macos_it_runs() {
        assert!(Version::macos() >= version("11.0"));
    }
}
