//! The device presets of responsive mode (`docs/responsive.md` 2.2): each one's size,
//! scale and user agent, and the limits every size keeps to.

use koyo_browser_ipc::Viewport;
use serde_json::{Value, json};

/// The fewest logical pixels a side has.
pub const MIN_SIDE: u32 = 50;

/// The most logical pixels a side has at `scale`, so its surface stays within the host's
/// largest ([`Viewport::max_side`]), and never fewer than [`MIN_SIDE`].
pub fn max_side(scale: f32) -> u32 {
    Viewport::max_side(scale).max(MIN_SIDE)
}

/// Whether `side` logical pixels are within the limits at `scale`.
pub fn within(side: u32, scale: f32) -> bool {
    (MIN_SIDE..=max_side(scale)).contains(&side)
}

/// `side` brought within the limits at `scale`, as a drag stops at them.
pub fn clamp(side: u32, scale: f32) -> u32 {
    side.clamp(MIN_SIDE, max_side(scale))
}

/// The browser a preset's pages think they're in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Agent {
    /// Safari on an iPhone.
    Iphone,
    /// Safari on an iPad.
    Ipad,
    /// Chrome on an Android phone, the model named.
    Android(&'static str),
}

/// A device preset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Device {
    /// Its name in the tools: `iphone-15`.
    pub id: &'static str,
    /// Its name in the device menu: `iPhone 15`.
    pub name: &'static str,
    /// Its portrait size, in logical pixels.
    pub width: u32,
    pub height: u32,
    /// Device pixels per logical pixel.
    pub scale: f32,
    pub agent: Agent,
}

/// Every preset, in the device menu's order. All are mobile and touch.
pub static DEVICES: [Device; 7] = [
    Device {
        id: "iphone-se",
        name: "iPhone SE",
        width: 375,
        height: 667,
        scale: 2.,
        agent: Agent::Iphone,
    },
    Device {
        id: "iphone-15",
        name: "iPhone 15",
        width: 393,
        height: 852,
        scale: 3.,
        agent: Agent::Iphone,
    },
    Device {
        id: "iphone-15-pro-max",
        name: "iPhone 15 Pro Max",
        width: 430,
        height: 932,
        scale: 3.,
        agent: Agent::Iphone,
    },
    Device {
        id: "pixel-8",
        name: "Pixel 8",
        width: 412,
        height: 915,
        scale: 2.625,
        agent: Agent::Android("Pixel 8"),
    },
    Device {
        id: "galaxy-s24",
        name: "Galaxy S24",
        width: 360,
        height: 780,
        scale: 3.,
        agent: Agent::Android("SM-S921B"),
    },
    Device {
        id: "ipad-mini",
        name: "iPad mini",
        width: 744,
        height: 1133,
        scale: 2.,
        agent: Agent::Ipad,
    },
    Device {
        id: "ipad-pro-11",
        name: "iPad Pro 11″",
        width: 834,
        height: 1194,
        scale: 2.,
        agent: Agent::Ipad,
    },
];

/// The preset named `id`.
pub fn find(id: &str) -> Option<&'static Device> {
    DEVICES.iter().find(|device| device.id == id)
}

/// A user agent override: the string, `navigator.platform`, and the client hints.
#[derive(Clone, Debug, PartialEq)]
pub struct Override {
    pub user_agent: String,
    pub platform: Option<&'static str>,
    pub metadata: Option<Value>,
}

impl Device {
    /// Its size turned `landscape` or not.
    pub fn size(&self, landscape: bool) -> (u32, u32) {
        if landscape {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        }
    }

    /// The user agent its pages are given, Android's naming the engine's own Chromium
    /// `major` version.
    pub fn user_agent(&self, major: &str) -> Override {
        let ios = |device: &str, model: &str, platform: &'static str| Override {
            user_agent: format!(
                "Mozilla/5.0 ({device}) AppleWebKit/605.1.15 (KHTML, like Gecko) \
                 Version/18.0 Mobile/15E148 Safari/604.1"
            ),
            platform: Some(platform),
            metadata: Some(metadata(&[], "iOS", "18.0", model)),
        };
        match self.agent {
            Agent::Iphone => ios(
                "iPhone; CPU iPhone OS 18_0 like Mac OS X",
                "iPhone",
                "iPhone",
            ),
            Agent::Ipad => ios("iPad; CPU OS 18_0 like Mac OS X", "iPad", "iPad"),
            Agent::Android(model) => Override {
                user_agent: format!(
                    "Mozilla/5.0 (Linux; Android 14; {model}) AppleWebKit/537.36 \
                     (KHTML, like Gecko) Chrome/{major}.0.0.0 Mobile Safari/537.36"
                ),
                platform: Some("Linux armv81"),
                metadata: Some(metadata(
                    &[
                        ("Chromium", major),
                        ("Google Chrome", major),
                        ("Not-A.Brand", "99"),
                    ],
                    "Android",
                    "14.0.0",
                    model,
                )),
            },
        }
    }
}

/// The client hints of a mobile browser on `platform`.
fn metadata(brands: &[(&str, &str)], platform: &str, version: &str, model: &str) -> Value {
    let brands: Vec<Value> = (brands.iter())
        .map(|(brand, version)| json!({ "brand": brand, "version": version }))
        .collect();
    json!({
        "brands": brands,
        "platform": platform,
        "platformVersion": version,
        "architecture": "",
        "model": model,
        "mobile": true,
    })
}

#[cfg(test)]
mod tests {
    use koyo_app_control::ops::browser::DeviceName;

    use super::*;

    #[test]
    fn every_preset_is_one_the_tools_name_and_fits_the_limits() {
        let named: Vec<&str> = (DeviceName::ALL.iter())
            .filter(|name| **name != DeviceName::Responsive)
            .map(|name| name.id())
            .collect();
        let ours: Vec<&str> = DEVICES.iter().map(|device| device.id).collect();
        assert_eq!(named, ours, "the tools' names are the table's, in order");
        for device in &DEVICES {
            assert_eq!(find(device.id), Some(device));
            assert!(within(device.width, device.scale), "{}", device.id);
            assert!(within(device.height, device.scale), "{}", device.id);
            assert!(device.height > device.width, "{} is portrait", device.id);
        }
        assert_eq!(find("nokia-3310"), None);
    }

    #[test]
    fn sides_stay_within_16384_device_pixels() {
        assert_eq!(max_side(1.), 16384);
        assert_eq!(max_side(2.), 8192);
        assert_eq!(max_side(3.), 5461);
        assert_eq!(max_side(2.625), 6241);
        assert!(within(50, 3.) && within(5461, 3.));
        assert!(!within(49, 3.) && !within(5462, 3.));
        assert_eq!(clamp(10, 2.), 50);
        assert_eq!(clamp(9000, 2.), 8192);
        assert_eq!(clamp(800, 2.), 800);
    }

    #[test]
    fn rotating_swaps_the_sides() {
        let phone = find("iphone-15").unwrap();
        assert_eq!(phone.size(false), (393, 852));
        assert_eq!(phone.size(true), (852, 393));
    }

    #[test]
    fn phones_say_what_they_are_and_android_names_the_engines_chromium() {
        let iphone = find("iphone-se").unwrap().user_agent("146");
        assert!(
            iphone
                .user_agent
                .contains("iPhone; CPU iPhone OS 18_0 like Mac OS X")
        );
        assert!(iphone.user_agent.ends_with("Mobile/15E148 Safari/604.1"));
        assert_eq!(iphone.metadata.as_ref().unwrap()["platform"], "iOS");
        let ipad = find("ipad-mini").unwrap().user_agent("146");
        assert!(
            ipad.user_agent
                .contains("(iPad; CPU OS 18_0 like Mac OS X)")
        );
        let pixel = find("pixel-8").unwrap().user_agent("146");
        assert_eq!(
            pixel.user_agent,
            "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) \
             Chrome/146.0.0.0 Mobile Safari/537.36"
        );
        let hints = pixel.metadata.unwrap();
        assert_eq!(hints["platform"], "Android");
        assert_eq!(hints["mobile"], true);
        assert_eq!(hints["model"], "Pixel 8");
        assert_eq!(hints["brands"][0]["version"], "146");
    }
}
