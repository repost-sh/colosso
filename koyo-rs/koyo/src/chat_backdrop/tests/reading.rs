//! A screen's wallpaper read off the main thread: none while its file is read, its picture made
//! once it's read, and nothing drawn of a file that can't be read.

use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::TestAppContext;

use super::{grey, picture_file, solid};
use crate::chat_backdrop::layout::Scaling;
use crate::chat_backdrop::make::Recipe;
use crate::chat_backdrop::screens;
use crate::chat_backdrop::wallpaper::{Named, Wallpaper};
use crate::settings::Quality;
use crate::test_support::scratch;

/// A 64 px grey picture saved in a folder of its own named `name`, with `mode` its permissions.
fn wallpaper_file(name: &str, mode: u32) -> PathBuf {
    let path = scratch(name).join("wallpaper.png");
    picture_file(&path, 64, 64, "public.png", grey);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
    path
}

/// Has screen 1's wallpaper read as `AppKit` would name the file at `path`.
fn read(path: PathBuf, cx: &mut TestAppContext) {
    let named = Named {
        path,
        scaling: Scaling::Fill,
        fill: [0., 0., 0.],
    };
    cx.update(|cx| screens::read(1, Some(named), cx));
}

fn wallpaper(cx: &mut TestAppContext) -> Option<Wallpaper> {
    cx.update(|cx| screens::wallpaper(1, cx))
}

/// Whether a chat on screen 1 draws a picture, asking for it as a chat does, from the wallpaper
/// as last read.
fn drawn(cx: &mut TestAppContext) -> bool {
    cx.update(|cx| {
        let recipe = screens::wallpaper(1, cx).map(|wallpaper| Recipe {
            wallpaper,
            screen: (64., 64.),
            scale: 2.,
            quality: Quality::Astc8Full,
        });
        screens::picture(1, recipe, solid(), cx).is_some()
    })
}

#[gpui_kit::test]
fn a_wallpaper_shows_once_its_file_is_read_and_goes_with_its_picture(
    cx: &mut TestAppContext,
) {
    cx.update(crate::chat_backdrop::init);
    let path = wallpaper_file("chat-backdrop-read", 0o644);
    let hold = cx.update(|cx| screens::hold(1, cx));
    read(path.clone(), cx);
    assert_eq!(wallpaper(cx), None, "its file not read yet");
    assert!(!drawn(cx), "nothing drawn while it's read");
    cx.run_until_parked();
    let first = wallpaper(cx).expect("read");
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    assert_eq!((first.path, first.modified), (path, Some(modified)));
    assert!(!drawn(cx), "being made");
    cx.run_until_parked();
    assert!(drawn(cx), "made");
    drop(hold);
    cx.executor().advance_clock(Duration::from_secs(30));
    cx.run_until_parked();
    let changed = wallpaper_file("chat-backdrop-read-changed", 0o644);
    read(changed.clone(), cx);
    assert_eq!(
        wallpaper(cx),
        None,
        "let go with its picture: nothing of it is drawn while the one named now is read"
    );
    cx.run_until_parked();
    assert_eq!(wallpaper(cx).map(|read| read.path), Some(changed));
}

#[gpui_kit::test]
fn a_wallpaper_whose_file_cant_be_read_draws_nothing(cx: &mut TestAppContext) {
    let path = wallpaper_file("chat-backdrop-unreadable", 0o000);
    let _hold = cx.update(|cx| screens::hold(1, cx));
    read(path.clone(), cx);
    cx.run_until_parked();
    assert_eq!(wallpaper(cx).map(|read| read.path), Some(path), "named, so read");
    assert!(!drawn(cx), "being made");
    cx.run_until_parked();
    assert!(!drawn(cx), "it can't be made, so nothing is drawn");
}
