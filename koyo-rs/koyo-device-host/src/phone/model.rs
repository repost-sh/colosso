//! A real device's screen corners (`docs/devices.md` 8): the device says its model at the
//! handshake (`iPhone15,2`), and Xcode keeps a sheet of every model's hardware beside its
//! simulators, the one a simulator's corners are read from.

use std::path::Path;

use idevice::rsd::RsdHandshake;

use crate::sim::core::corner_of;

/// Where Xcode installs the device types of every platform.
const TYPES: &str = "/Library/Developer/CoreSimulator/Profiles/DeviceTypes";

/// The model the device says it is at the handshake: `iPhone15,2`.
pub fn named(handshake: &RsdHandshake) -> Option<String> {
    handshake.properties.get("ProductType").and_then(plist::Value::as_string).map(str::to_owned)
}

/// The radius of the corners of the screen of `model`, in points; or why its picture stays
/// square: the device didn't say its model, or Xcode has no sheet for it (a model newer than
/// the Xcode here).
pub fn corner(model: Option<&str>) -> Result<f64, String> {
    let model = model.ok_or("its model isn't said: its picture's corners stay square")?;
    corner_among(Path::new(TYPES), model).ok_or_else(|| format!("Xcode has no sheet for its model {model}: its picture's corners stay square"))
}

/// The corner radius of the device type among `types` that stands for `model`: a sheet names
/// every identifier of its model, an iPad's with and without cellular.
fn corner_among(types: &Path, model: &str) -> Option<f64> {
    std::fs::read_dir(types).ok()?.flatten().find_map(|kind| {
        let bundle = kind.path();
        let profile = plist::Value::from_file(bundle.join("Contents/Resources/profile.plist")).ok()?;
        let named = profile.as_dictionary()?.get("representedModelIdentifiers")?.as_array()?;
        named.iter().any(|named| named.as_string() == Some(model)).then(|| corner_of(&bundle))?
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet(folder: &Path, name: &str, models: &[&str], radius: u32) {
        let sheets = folder.join(format!("{name}.simdevicetype/Contents/Resources"));
        std::fs::create_dir_all(&sheets).unwrap();
        let plist = |inner: String| format!("<plist version=\"1.0\"><dict>{inner}</dict></plist>");
        let all: String = models.iter().map(|model| ["<string>", model, "</string>"].concat()).collect();
        let profile = plist(format!("<key>representedModelIdentifiers</key><array>{all}</array>"));
        let capabilities = plist(format!("<key>capabilities</key><dict><key>DeviceCornerRadius</key><integer>{radius}</integer></dict>"));
        std::fs::write(sheets.join("profile.plist"), profile).unwrap();
        std::fs::write(sheets.join("capabilities.plist"), capabilities).unwrap();
    }

    #[test]
    fn a_models_corner_is_read_from_the_sheet_of_that_model() {
        let folder = std::env::temp_dir().join(format!("koyo-model-{}", std::process::id()));
        sheet(&folder, "A Phone", &["iPhone15,2"], 55);
        sheet(&folder, "A Pad", &["iPad13,18", "iPad13,19"], 25);
        assert_eq!(corner_among(&folder, "iPhone15,2"), Some(55.));
        assert_eq!(corner_among(&folder, "iPad13,18"), Some(25.));
        // The same model with cellular: no sheet of its own, named by its sibling's.
        assert_eq!(corner_among(&folder, "iPad13,19"), Some(25.));
        assert_eq!(corner_among(&folder, "iPhone99,9"), None);
        assert_eq!(corner_among(&folder.join("none"), "iPhone15,2"), None);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
