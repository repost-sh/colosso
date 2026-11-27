//! The app's folder, where its daemons, its relays and the app itself agree to find its data:
//! `~/Library/Application Support/codes.koyo.desktop` on macOS, or wherever `KOYO_DATA_DIR`
//! points.

use std::path::{Path, PathBuf};

/// The app's id, its folder under the platform's data folder.
pub const APP_ID: &str = "codes.koyo.desktop";
/// The app's id while the project's domain was koyo.dev, and its folder's name until a build
/// under [`APP_ID`] moves it (the app's `former_name`).
pub const FORMER_APP_ID: &str = "dev.koyo.desktop";

/// The app's data folder: `KOYO_DATA_DIR` when set, else its own ([`own_data_dir`]); `None`
/// when the platform has no data folder for this user.
pub fn data_dir() -> Option<PathBuf> {
    std::env::var_os("KOYO_DATA_DIR")
        .map(PathBuf::from)
        .or_else(own_data_dir)
}

/// The app's own data folder: [`app_folder`] in the platform's data folder (Application Support
/// on macOS), the [`data_dir`] of a run no `KOYO_DATA_DIR` points elsewhere. A folder
/// `KOYO_DATA_DIR` names is the app's own only when it is this one, as Chromium's own data
/// directory is the default one and no other (`chrome::IsUsingDefaultDataDirectory`). `None`
/// when the platform has no data folder for this user.
pub fn own_data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| app_folder(&dir))
}

/// The app's folder in the platform's data folder `base`: [`APP_ID`]'s, or [`FORMER_APP_ID`]'s
/// while only that one is there. The app moves the former folder once no older Koyo runs on
/// it; until then this build, its relays and its command line find the person's data, and
/// that Koyo, where that Koyo keeps it, rather than starting over in an empty folder the move
/// could no longer go to.
pub fn app_folder(base: &Path) -> PathBuf {
    let ours = base.join(APP_ID);
    let former = base.join(FORMER_APP_ID);
    if !ours.exists() && former.is_dir() {
        former
    } else {
        ours
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_former_folder_is_the_apps_only_while_it_alone_is_there() {
        let base = Path::new(env!("KOYO_TARGET_DIR"))
            .join("test-paths")
            .join(format!("app-folder-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        assert_eq!(app_folder(&base), base.join(APP_ID), "a first start's");
        std::fs::create_dir(base.join(FORMER_APP_ID)).unwrap();
        assert_eq!(app_folder(&base), base.join(FORMER_APP_ID), "not moved yet");
        std::fs::create_dir(base.join(APP_ID)).unwrap();
        assert_eq!(
            app_folder(&base),
            base.join(APP_ID),
            "its own, once it's there"
        );
        std::fs::remove_dir_all(&base).unwrap();
    }
}
