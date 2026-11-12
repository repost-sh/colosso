//! What a real device says of itself over our tunnel (`docs/devices.md` 5), from its own
//! `deviceinfo` service, the one `devicectl` asks: how its display is turned, how it's held and
//! its point scale (`getdisplayinfo`, the `CoreDevice` feature a simulator answers too). No
//! subprocess. Its LCD's size in pixels too, which no picture of its stream says. Its lock
//! state isn't served there (`getlockstate`: "not implemented" on iPadOS
//! 27.0.1, by feature and by its action): `checks.rs` reads it.

use idevice::core_device::CoreDeviceServiceClient;
use idevice::rsd::RsdHandshake;
use idevice::tcp::handle::AdapterHandle;
use idevice::{IdeviceError, ReadWrite, RsdService};
use koyo_device_wire::{Orientation, PixelSize};

const DISPLAY: &str = "com.apple.coredevice.feature.getdisplayinfo";

/// The device's information service.
pub struct DeviceInfo(CoreDeviceServiceClient<Box<dyn ReadWrite>>);

impl RsdService for DeviceInfo {
    fn rsd_service_name() -> std::borrow::Cow<'static, str> {
        "com.apple.coredevice.deviceinfo".into()
    }

    async fn from_stream(stream: Box<dyn ReadWrite>) -> Result<Self, IdeviceError> {
        Ok(DeviceInfo(CoreDeviceServiceClient::new(stream).await?))
    }
}

/// How the device's picture is turned and held, and its points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Display {
    /// Quarter turns clockwise that put the stream's picture upright: its LCD's own turn.
    pub quarters: u8,
    pub held: Orientation,
    /// Pixels a point: an iPad's 2, an iPhone's 3, as the device says.
    pub scale: f64,
    /// The LCD's size in pixels, as it lies in the device: the screen itself, which the stream's
    /// pictures show in their top left, padded with black to a size their coder likes.
    pub pixels: Option<PixelSize>,
}

impl DeviceInfo {
    pub async fn connect(adapter: &mut AdapterHandle, handshake: &mut RsdHandshake) -> Result<DeviceInfo, String> {
        DeviceInfo::connect_rsd(adapter, handshake).await.map_err(|error| format!("no device info: {error}"))
    }

    pub async fn display(&mut self) -> Result<Display, String> {
        let said = (self.0.invoke(DISPLAY, None).await).map_err(|error| format!("the device's display info: {error}"))?;
        display(&said).ok_or_else(|| "the device's display info has no turn".to_owned())
    }
}

/// A word the device says how it's held in.
fn held(word: &str) -> Option<Orientation> {
    match word {
        "landscapeRight" => Some(Orientation::LandscapeRight),
        "portrait" => Some(Orientation::Portrait),
        "landscapeLeft" => Some(Orientation::LandscapeLeft),
        "portraitUpsideDown" => Some(Orientation::PortraitUpsideDown),
        _ => None,
    }
}

/// The LCD's turn and scale and how the device is held, from `getdisplayinfo`'s answer. Lying
/// flat it's held `faceUp`: then its last way of being held, `currentDeviceNonFlatOrientation`.
pub fn display(said: &plist::Value) -> Option<Display> {
    let said = said.as_dictionary()?;
    let displays = said.get("displays")?.as_array()?;
    let lcd = (displays.iter())
        .filter_map(plist::Value::as_dictionary)
        .find(|display| display.get("name").and_then(plist::Value::as_string) == Some("LCD"))
        .or_else(|| displays.first()?.as_dictionary())?;
    let quarters = crate::sim::display::quarters(lcd.get("currentOrientation")?.as_string()?)?;
    let scale = (lcd.get("pointScale").and_then(plist::Value::as_unsigned_integer))
        .and_then(|scale| u32::try_from(scale).ok())
        .map_or(2., f64::from);
    let orientation = said.get("orientation")?.as_dictionary()?;
    let word = |key: &str| orientation.get(key).and_then(plist::Value::as_string).and_then(held);
    let held = word("currentDeviceOrientation").or_else(|| word("currentDeviceNonFlatOrientation"))?;
    let side = |value: &plist::Value| Some(crate::sim::core::number(value)?.round() as u32).filter(|side| *side > 0);
    let pixels = lcd.get("nativeSize").and_then(plist::Value::as_array).and_then(|sides| match sides.as_slice() {
        [width, height] => Some(PixelSize {
            width: side(width)?,
            height: side(height)?,
        }),
        _ => None,
    });
    Some(Display { quarters, held, scale, pixels })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(current: &str, held: &str, non_flat: &str) -> plist::Value {
        let mut lcd = plist::Dictionary::new();
        lcd.insert("name".into(), "LCD".into());
        lcd.insert("currentOrientation".into(), current.into());
        lcd.insert("pointScale".into(), plist::Value::Integer(2.into()));
        let mut orientation = plist::Dictionary::new();
        orientation.insert("currentDeviceOrientation".into(), held.into());
        orientation.insert("currentDeviceNonFlatOrientation".into(), non_flat.into());
        let mut said = plist::Dictionary::new();
        said.insert("displays".into(), vec![plist::Value::Dictionary(lcd)].into());
        said.insert("orientation".into(), orientation.into());
        plist::Value::Dictionary(said)
    }

    #[test]
    fn the_turn_the_way_held_and_the_scale_are_read_and_flat_takes_the_last_way() {
        let held_up = display(&answer("rot180", "landscapeRight", "landscapeRight"));
        assert_eq!(held_up, Some(Display { quarters: 2, held: Orientation::LandscapeRight, scale: 2., pixels: None }));
        // As the iPad lay on the desk in M6: face up, last held landscape left, the LCD at rot0.
        let flat = display(&answer("rot0", "faceUp", "landscapeLeft"));
        assert_eq!(flat, Some(Display { quarters: 0, held: Orientation::LandscapeLeft, scale: 2., pixels: None }));
        assert_eq!(display(&plist::Value::Boolean(true)), None);
    }

    #[test]
    fn the_screens_size_is_the_lcds_own_and_no_other_displays() {
        let sized = |name: &str, width: f64, height: f64| {
            let mut display = plist::Dictionary::new();
            display.insert("name".into(), name.into());
            display.insert("currentOrientation".into(), "rot0".into());
            display.insert("nativeSize".into(), vec![plist::Value::Real(width), plist::Value::Real(height)].into());
            plist::Value::Dictionary(display)
        };
        let mut said = answer("rot0", "portrait", "portrait");
        let displays = said.as_dictionary_mut().and_then(|said| said.get_mut("displays")).and_then(plist::Value::as_array_mut).unwrap();
        // The display a stream is made on is larger than the screen, and isn't the screen.
        *displays = vec![sized("Wireless", 1184., 2560.), sized("LCD", 1179., 2556.)];
        let screen = PixelSize { width: 1179, height: 2556 };
        assert_eq!(display(&said).and_then(|display| display.pixels), Some(screen));
    }
}
